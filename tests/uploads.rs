#[path = "../src/http/mod.rs"]
mod http;

use actix_web::{App, http::StatusCode, test, web};
use bytes::Bytes;
use futures_util::stream;
use graphile_worker::WorkerUtils;
use object_store::aws::AmazonS3Builder;
use serde_json::Value;
use sqlx::PgPool;
use std::num::NonZeroU16;
use whorl::{queue::JobQueue, storage::UploadStore};

#[actix_web::test]
#[ignore = "requires fresh WHORL_UPLOAD_TEST_URL and local MinIO"]
async fn streaming_boundaries_and_atomic_admission() {
    let pool = PgPool::connect(&std::env::var("WHORL_UPLOAD_TEST_URL").unwrap())
        .await
        .unwrap();
    for sql in [
        include_str!("../database/migrations/0001_application.sql"),
        include_str!("../database/migrations/0002_uploads.sql"),
    ] {
        sqlx::raw_sql(sql).execute(&pool).await.unwrap();
    }
    WorkerUtils::new(pool.clone(), "whorl_queue")
        .migrate()
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO signature_versions(version,configuration) VALUES ('test','{}'); INSERT INTO capacity_limits VALUES ('global',100,10737418240),('client:a',100,10737418240)").execute(&pool).await.unwrap();
    let s3 = AmazonS3Builder::new()
        .with_endpoint(std::env::var("WHORL_STORAGE_S3_ENDPOINT").unwrap())
        .with_allow_http(true)
        .with_bucket_name("uploads")
        .with_region("us-east-1")
        .with_access_key_id("minioadmin")
        .with_secret_access_key("minioadmin")
        .build()
        .unwrap();
    let state = web::Data::new(http::Submission {
        pool: pool.clone(),
        store: UploadStore::new(pool.clone(), s3),
        queue: JobQueue::new(pool.clone(), NonZeroU16::new(3).unwrap()),
        version: "test".into(),
        trusted_proxy_only: true,
    });
    let app = test::init_service(App::new().app_data(state).configure(http::configure)).await;
    let cap = 20_971_520usize;
    let data = Bytes::from(vec![b'x'; 16_384]);
    for (sizes, expected, codes) in [
        (vec![], StatusCode::BAD_REQUEST, vec![]),
        (vec![1], StatusCode::ACCEPTED, vec![None]),
        (vec![cap], StatusCode::ACCEPTED, vec![None]),
        (
            vec![cap + 1, 1],
            StatusCode::ACCEPTED,
            vec![Some("file_too_large"), None],
        ),
        (vec![0], StatusCode::ACCEPTED, vec![Some("empty_file")]),
        (
            vec![0, 1],
            StatusCode::ACCEPTED,
            vec![Some("empty_file"), None],
        ),
        (vec![cap; 5], StatusCode::ACCEPTED, vec![None; 5]),
        (
            vec![cap + 1, cap, cap, cap, cap],
            StatusCode::PAYLOAD_TOO_LARGE,
            vec![],
        ),
        (vec![1; 6], StatusCode::BAD_REQUEST, vec![]),
    ] {
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM batches")
            .fetch_one(&pool)
            .await
            .unwrap();
        let mut chunks = Vec::new();
        for size in sizes {
            chunks.push(Ok::<_,actix_web::error::PayloadError>(Bytes::from_static(b"--bound\r\nContent-Disposition: form-data; name=\"files\"; filename=\"x.pdf\"\r\n\r\n")));
            for offset in (0..size).step_by(16_384) {
                chunks.push(Ok(data.slice(..(size - offset).min(16_384))));
            }
            chunks.push(Ok(Bytes::from_static(b"\r\n")));
        }
        chunks.push(Ok(Bytes::from_static(b"--bound--\r\n")));
        let mut req = test::TestRequest::post()
            .uri(if codes.len() == 1 {
                "/v1/ingests"
            } else {
                "/v1/checks"
            })
            .insert_header(("X-Client-Id", "a"))
            .insert_header(("Content-Type", "multipart/form-data; boundary=bound"))
            .to_request();
        *req.payload() = actix_web::dev::Payload::from(Box::pin(stream::iter(chunks))
            as std::pin::Pin<
                Box<dyn futures_util::Stream<Item = Result<Bytes, actix_web::error::PayloadError>>>,
            >);
        let response = test::call_service(&app, req).await;
        let status = response.status();
        let value: Value = test::read_body_json(response).await;
        assert_eq!(status, expected, "{value}");
        if status == StatusCode::ACCEPTED {
            for (file, code) in value["files"].as_array().unwrap().iter().zip(codes) {
                assert_eq!(file["error"]["code"].as_str(), code);
            }
        } else {
            let after: i64 = sqlx::query_scalar("SELECT count(*) FROM batches")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(before, after);
            let unclaimed: i64 = sqlx::query_scalar("SELECT count(*) FROM stored_uploads u WHERE NOT EXISTS(SELECT FROM batch_files f WHERE f.source_object_key=u.object_key)").fetch_one(&pool).await.unwrap();
            assert_eq!(unclaimed, 0);
        }
    }
    for (body, code) in [
        (
            b"--bound\r\nContent-Disposition: form-data; name=\"other\"\r\n\r\nx\r\n--bound--\r\n"
                .to_vec(),
            "unknown_part",
        ),
        (
            [b"--bound\r\nX-Long: ".as_slice(), &vec![b'x'; 8193]].concat(),
            "malformed_multipart",
        ),
        (
            b"--bound\r\nContent-Disposition: form-data; name=\"files\"\r\n\r\npartial".to_vec(),
            "malformed_multipart",
        ),
    ] {
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/v1/ingests")
                .insert_header(("X-Client-Id", "a"))
                .insert_header(("Content-Type", "multipart/form-data; boundary=bound"))
                .set_payload(body)
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let value: Value = test::read_body_json(response).await;
        assert_eq!(value["error"]["code"], code);
    }
    sqlx::raw_sql("CREATE FUNCTION reject_http_enqueue() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected queue failure'; END; $$; CREATE TRIGGER reject_http_enqueue BEFORE INSERT ON whorl_queue._private_jobs FOR EACH ROW EXECUTE FUNCTION reject_http_enqueue();").execute(&pool).await.unwrap();
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM batches")
        .fetch_one(&pool)
        .await
        .unwrap();
    let response = test::call_service(&app, test::TestRequest::post().uri("/v1/ingests")
        .insert_header(("X-Client-Id", "a")).insert_header(("Content-Type", "multipart/form-data; boundary=bound"))
        .set_payload(b"--bound\r\nContent-Disposition: form-data; name=\"files\"\r\n\r\nx\r\n--bound--\r\n".as_slice()).to_request()).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers().get("Retry-After").unwrap(), "2");
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM batches")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    let unclaimed: i64 = sqlx::query_scalar("SELECT count(*) FROM stored_uploads u WHERE NOT EXISTS(SELECT FROM batch_files f WHERE f.source_object_key=u.object_key)").fetch_one(&pool).await.unwrap();
    assert_eq!(unclaimed, 0);
    sqlx::raw_sql("DROP TRIGGER reject_http_enqueue ON whorl_queue._private_jobs; DROP FUNCTION reject_http_enqueue();").execute(&pool).await.unwrap();
    let mut chunks = vec![Ok::<_, actix_web::error::PayloadError>(Bytes::from_static(
        b"--bound\r\nContent-Disposition: form-data; name=\"files\"\r\n\r\nx\r\n--bound--\r\n",
    ))];
    let padding = Bytes::from(vec![b'x'; 1024 * 1024]);
    chunks.extend((0..102).map(|_| Ok(padding.clone())));
    let mut request = test::TestRequest::post()
        .uri("/v1/ingests")
        .insert_header(("X-Client-Id", "a"))
        .insert_header(("Content-Type", "multipart/form-data; boundary=bound"))
        .to_request();
    *request.payload() = actix_web::dev::Payload::from(Box::pin(stream::iter(chunks))
        as std::pin::Pin<
            Box<dyn futures_util::Stream<Item = Result<Bytes, actix_web::error::PayloadError>>>,
        >);
    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let value: Value = test::read_body_json(response).await;
    assert_eq!(value["error"]["code"], "body_too_large");
    let response = test::call_service(
        &app,
        test::TestRequest::post().uri("/v1/ingests").to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
