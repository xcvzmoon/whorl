#[path = "../src/http/mod.rs"]
mod http;

use actix_web::{App, http::StatusCode, test, web};
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::num::NonZeroU16;
use uuid::Uuid;
use whorl::{queue::JobQueue, storage::UploadStore};

fn state(pool: PgPool) -> web::Data<http::Submission> {
    web::Data::new(http::Submission {
        store: UploadStore::new(
            pool.clone(),
            object_store::aws::AmazonS3Builder::new()
                .with_bucket_name("unused")
                .with_region("us-east-1")
                .with_access_key_id("unused")
                .with_secret_access_key("unused")
                .build()
                .unwrap(),
        ),
        queue: JobQueue::new(pool.clone(), NonZeroU16::new(3).unwrap()),
        pool,
        version: "test".into(),
        trusted_proxy_only: true,
    })
}

#[actix_web::test]
async fn health_does_not_require_processing_or_identity() {
    let pool = PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(20))
        .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/invalid")
        .unwrap();
    let app = test::init_service(App::new().app_data(state(pool)).configure(http::configure)).await;
    let live = test::call_service(
        &app,
        test::TestRequest::get().uri("/health/live").to_request(),
    )
    .await;
    assert_eq!(live.status(), StatusCode::OK);
    let ready = test::call_service(
        &app,
        test::TestRequest::get().uri("/health/ready").to_request(),
    )
    .await;
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    let missing = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/v1/batches/{}", Uuid::new_v4()))
            .to_request(),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
#[ignore = "requires fresh WHORL_POLL_TEST_URL PostgreSQL database"]
async fn persisted_states_revisions_streaming_and_expiration() {
    let pool = PgPool::connect(&std::env::var("WHORL_POLL_TEST_URL").unwrap())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0001_application.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO signature_versions(version,configuration) VALUES ('test','{}')")
        .execute(&pool)
        .await
        .unwrap();
    let id = Uuid::new_v4();
    let file = Uuid::new_v4();
    sqlx::query("INSERT INTO batches(id,client_id,mode,signature_version,policy,deadline_at) VALUES ($1,'owner','check','test','{\"kind\":\"text_only\",\"threshold\":0.75}',now()+interval '1 hour')").bind(id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO batch_files(id,batch_id,client_id,ordinal,name,deadline_at) VALUES ($1,$2,'owner',0,'example.pdf',now()+interval '1 hour')").bind(file).bind(id).execute(&pool).await.unwrap();
    let app = test::init_service(
        App::new()
            .app_data(state(pool.clone()))
            .configure(http::configure),
    )
    .await;
    assert_eq!(
        test::call_service(
            &app,
            test::TestRequest::get().uri("/health/ready").to_request()
        )
        .await
        .status(),
        StatusCode::OK
    );
    let uri = format!("/v1/batches/{id}");
    let request = || {
        test::TestRequest::get()
            .uri(&uri)
            .insert_header(("X-Client-Id", "owner"))
    };
    let response = test::call_service(&app, request().to_request()).await;
    let mut etag = response
        .headers()
        .get("ETag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(response.headers().get("Retry-After").unwrap(), "2");
    let value: Value = test::read_body_json(response).await;
    assert_eq!(value["state"], "queued");
    assert_eq!(value["expires_at"], Value::Null);
    let response = test::call_service(
        &app,
        request()
            .insert_header(("If-None-Match", format!("\"other\", W/{etag}")))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert!(test::read_body(response).await.is_empty());
    for stage in [
        "extracting",
        "signing",
        "candidate_lookup",
        "verifying",
        "persisting",
    ] {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("UPDATE batch_files SET state='running',stage=$2,attempt=1 WHERE id=$1")
            .bind(file)
            .bind(stage)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE batches SET state='running' WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let response = test::call_service(
            &app,
            request()
                .insert_header(("If-None-Match", etag.clone()))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let next = response
            .headers()
            .get("ETag")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert_ne!(etag, next);
        etag = next;
        let value: Value = test::read_body_json(response).await;
        assert_eq!(value["files"][0]["progress"]["stage"], stage);
    }
    sqlx::query("UPDATE batch_files SET state='queued',stage=NULL,next_retry_at=now()+interval '1 minute' WHERE id=$1").bind(file).execute(&pool).await.unwrap();
    let value: Value =
        test::read_body_json(test::call_service(&app, request().to_request()).await).await;
    assert_eq!(value["state"], "running");
    assert!(value["files"][0]["next_retry_at"].is_string());
    sqlx::query(
        "UPDATE batch_files SET state='completed',stage='completed',next_retry_at=NULL WHERE id=$1",
    )
    .bind(file)
    .execute(&pool)
    .await
    .unwrap();
    let empty: Value =
        test::read_body_json(test::call_service(&app, request().to_request()).await).await;
    assert_eq!(empty["files"][0]["matches"], serde_json::json!([]));
    sqlx::query("INSERT INTO documents(id,client_id,content_hash,text_content,file_type,metadata) SELECT md5(i::text)::uuid,'owner',sha256(i::text::bytea),'text','pdf','{}' FROM generate_series(1,300) i").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO match_results SELECT 'owner',$1,id,0.9,0.9,NULL FROM documents")
        .bind(file)
        .execute(&pool)
        .await
        .unwrap();
    let snapshot = test::call_service(&app, request().to_request()).await;
    sqlx::query("UPDATE match_results SET score=0.95 WHERE file_id=$1")
        .bind(file)
        .execute(&pool)
        .await
        .unwrap();
    let snapshot: Value = test::read_body_json(snapshot).await;
    assert_eq!(snapshot["files"][0]["matches"][0]["score"], 0.9);
    for aggregate in ["completed", "partially_failed", "failed"] {
        if aggregate == "partially_failed" {
            sqlx::query("INSERT INTO batch_files(id,batch_id,client_id,ordinal,name,state,error,deadline_at) VALUES ($1,$2,'owner',1,'empty.pdf','failed','{\"code\":\"empty_file\",\"message\":\"File is empty.\",\"retryable\":false}',now()+interval '1 hour')").bind(Uuid::new_v4()).bind(id).execute(&pool).await.unwrap();
        }
        if aggregate == "failed" {
            sqlx::query("UPDATE batch_files SET state='failed',stage=NULL,error='{\"code\":\"malformed_file\",\"message\":\"Malformed file.\",\"retryable\":false}' WHERE id=$1").bind(file).execute(&pool).await.unwrap();
        }
        sqlx::query("UPDATE batches SET state=$2,completed_at=now(),expires_at=now()+interval '24 hours',tombstone_until=now()+interval '48 hours' WHERE id=$1").bind(id).bind(aggregate).execute(&pool).await.unwrap();
        let response = test::call_service(&app, request().to_request()).await;
        assert!(!response.headers().contains_key("Retry-After"));
        etag = response
            .headers()
            .get("ETag")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let value: Value = test::read_body_json(response).await;
        assert_eq!(value["state"], aggregate);
        if aggregate == "failed" {
            assert!(value["files"][0].get("matches").is_none());
            assert_eq!(value["files"][0]["error"]["code"], "malformed_file");
            continue;
        }
        let matches = value["files"][0]["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 300);
        assert!(
            matches
                .windows(2)
                .all(|w| w[0]["document_id"].as_str() < w[1]["document_id"].as_str())
        );
        assert_eq!(matches[0]["text_score_method"], "exact");
    }
    let response = test::call_service(
        &app,
        request()
            .insert_header(("If-None-Match", etag.clone()))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert!(!response.headers().contains_key("Retry-After"));
    sqlx::query("UPDATE batches SET completed_at=now()-interval '25 hours',expires_at=now()-interval '1 hour',tombstone_until=now()+interval '23 hours' WHERE id=$1").bind(id).execute(&pool).await.unwrap();
    let expired = test::call_service(
        &app,
        request().insert_header(("If-None-Match", "*")).to_request(),
    )
    .await;
    assert_eq!(expired.status(), StatusCode::GONE);
    assert_eq!(expired.headers().get("Cache-Control").unwrap(), "no-store");
    let other = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&uri)
            .insert_header(("X-Client-Id", "other"))
            .to_request(),
    )
    .await;
    assert_eq!(other.status(), StatusCode::NOT_FOUND);
    sqlx::query("UPDATE batches SET completed_at=now()-interval '49 hours',expires_at=now()-interval '25 hours',tombstone_until=now()-interval '1 hour' WHERE id=$1").bind(id).execute(&pool).await.unwrap();
    assert_eq!(
        test::call_service(&app, request().to_request())
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}
