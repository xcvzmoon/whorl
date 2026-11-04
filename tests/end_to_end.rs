#[path = "../src/http/mod.rs"]
mod http;

use actix_web::{App, http::StatusCode, test, web};
use object_store::aws::AmazonS3Builder;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::{
    num::{NonZeroU16, NonZeroUsize},
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;
use whorl::{
    extraction::{ExtractionLimits, Extractor},
    queue::{FileJobPayload, JobQueue},
    service::{
        matching::PreparedDocument,
        process_file::{ProcessFile, WorkerConfig},
    },
    similarity::shingles::SourceProfile,
    storage::UploadStore,
};

#[actix_web::test]
#[ignore = "requires Linux, fresh WHORL_E2E_TEST_URL, and MinIO uploads bucket"]
async fn admission_storage_processing_and_polling() {
    assert_eq!(
        std::env::consts::OS,
        "linux",
        "successful extraction requires Linux"
    );
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&std::env::var("WHORL_E2E_TEST_URL").unwrap())
        .await
        .unwrap();
    for migration in [
        include_str!("../database/migrations/0001_application.sql"),
        include_str!("../database/migrations/0002_uploads.sql"),
    ] {
        sqlx::raw_sql(migration).execute(&pool).await.unwrap();
    }
    graphile_worker::WorkerUtils::new(pool.clone(), "whorl_queue")
        .migrate()
        .await
        .unwrap();
    let version =
        PreparedDocument::new("hello world", json!({}), SourceProfile::NativeTextV1, None)
            .unwrap()
            .signature_version()
            .to_owned();
    for source in ["native", "ocr"] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_maintenance"))
            .args(["reindex", source, "3"])
            .env("DATABASE_URL", std::env::var("WHORL_E2E_TEST_URL").unwrap())
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    sqlx::raw_sql("INSERT INTO capacity_limits VALUES ('global',100,1073741824),('client:acceptance',100,1073741824)").execute(&pool).await.unwrap();
    let objects = || {
        AmazonS3Builder::new()
            .with_endpoint(std::env::var("WHORL_E2E_S3_ENDPOINT").unwrap())
            .with_allow_http(true)
            .with_bucket_name("uploads")
            .with_region("us-east-1")
            .with_access_key_id("minioadmin")
            .with_secret_access_key("minioadmin")
            .build()
            .unwrap()
    };
    let pipeline = ProcessFile::new(
        pool.clone(),
        Arc::new(objects()),
        Extractor::new(
            env!("CARGO_BIN_EXE_whorl-extract").into(),
            ExtractionLimits::default(),
            Duration::from_secs(60),
        )
        .unwrap(),
        WorkerConfig {
            workers: NonZeroUsize::new(2).unwrap(),
            retention: Duration::from_secs(3600),
            tombstone_retention: Duration::from_secs(3600),
        },
    )
    .unwrap();
    let worker = graphile_worker::WorkerOptions::default()
        .pg_pool(pool.clone())
        .schema("whorl_queue")
        .concurrency(pipeline.concurrency())
        .add_extension(pipeline.clone())
        .define_job::<FileJobPayload>()
        .init()
        .await
        .unwrap();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(http::Submission {
                pool: pool.clone(),
                store: UploadStore::new(pool.clone(), objects()),
                queue: JobQueue::new(pool.clone(), NonZeroU16::new(3).unwrap()),
                version,
                trusted_proxy_only: true,
            }))
            .configure(http::configure),
    )
    .await;
    let document = std::fs::read("spikes/extraction/fixtures/table.docx").unwrap();
    for (mode, count, invalid, mismatch) in [
        ("ingests", 1, false, false),
        ("ingests", 5, false, false),
        ("checks", 5, false, false),
        ("checks", 2, true, false),
        ("checks", 1, false, true),
    ] {
        let started = Instant::now();
        let mut body = Vec::new();
        for i in 0..count {
            body.extend_from_slice(b"--acceptance\r\nContent-Disposition: form-data; name=\"files\"; filename=\"sample.docx\"\r\n\r\n");
            body.extend_from_slice(if invalid && i == 0 {
                b"invalid"
            } else {
                &document
            });
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(b"--acceptance--\r\n");
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(&format!("/v1/{mode}"))
                .insert_header(("X-Client-Id", "acceptance"))
                .insert_header(("Content-Type", "multipart/form-data; boundary=acceptance"))
                .set_payload(body)
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let admitted: Value = test::read_body_json(response).await;
        let admission = started.elapsed();
        let batch_id = Uuid::parse_str(admitted["batch_id"].as_str().unwrap()).unwrap();
        let uri = admitted["status_url"].as_str().unwrap();
        let before = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(uri)
                .insert_header(("X-Client-Id", "acceptance"))
                .to_request(),
        )
        .await;
        let etag = before.headers().get("ETag").unwrap().clone();
        let _ = test::read_body(before).await;
        if mismatch {
            sqlx::raw_sql(
                "INSERT INTO signature_versions(version,configuration) VALUES('unsupported','{}')",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query("UPDATE batches SET signature_version='unsupported' WHERE id=$1")
                .bind(batch_id)
                .execute(&pool)
                .await
                .unwrap();
        }
        worker.run_once().await.unwrap();
        for file in admitted["files"].as_array().unwrap() {
            let payload = FileJobPayload {
                client_id: "acceptance".into(),
                batch_id,
                file_id: Uuid::parse_str(file["file_id"].as_str().unwrap()).unwrap(),
            };
            pipeline.process(&payload, false).await.unwrap();
            pipeline.process(&payload, false).await.unwrap();
        }
        let poll_started = Instant::now();
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(uri)
                .insert_header(("X-Client-Id", "acceptance"))
                .insert_header(("If-None-Match", etag))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let result: Value = test::read_body_json(response).await;
        let poll = poll_started.elapsed();
        for (i, file) in result["files"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                file["state"],
                if mismatch || (invalid && i == 0) {
                    "failed"
                } else {
                    "completed"
                },
                "{result}"
            );
            if mode == "checks" && !mismatch && !(invalid && i == 0) {
                assert_eq!(file["matches"].as_array().unwrap().len(), 1, "{result}");
            }
        }
        let canonical: i64 = sqlx::query_scalar("SELECT count(*) FROM documents")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(canonical, 1);
        println!(
            "acceptance mode={mode} files={count} partial_failure={invalid} admission_us={} poll_us={} total_us={}",
            admission.as_micros(),
            poll.as_micros(),
            started.elapsed().as_micros()
        );
        let health = test::call_service(
            &app,
            test::TestRequest::get().uri("/health/ready").to_request(),
        )
        .await;
        assert_eq!(health.status(), StatusCode::OK);
        let foreign = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(uri)
                .insert_header(("X-Client-Id", "foreign"))
                .to_request(),
        )
        .await;
        assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    }
    pipeline.shutdown().await;
}
