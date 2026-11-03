#[path = "../src/telemetry.rs"]
mod telemetry;

use serde_json::json;
use sqlx::Row;
use std::process::Command;

#[test]
fn startup_rejects_invalid_config_and_loads_local_env() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".env"), "WHORL_CPU_WORKERS=3\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_worker"))
        .current_dir(dir.path())
        .env_remove("WHORL_CPU_WORKERS")
        .env_remove("DATABASE_URL")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("WHORL_CPU_WORKERS"));
    let result = Command::new(env!("CARGO_BIN_EXE_worker"))
        .current_dir(dir.path())
        .env("WHORL_CPU_WORKERS", "1")
        .env_remove("WHORL_S3_ENDPOINT")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&result.stderr).contains("WHORL_S3_ENDPOINT"));
    std::fs::write(dir.path().join(".env"), "INVALID='unterminated\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_worker"))
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains(".env"));
}

#[tokio::test]
#[ignore = "requires fresh PostgreSQL in WHORL_OPERATIONS_TEST_URL and MinIO in WHORL_OPERATIONS_S3_ENDPOINT"]
async fn partial_failure_and_cleanup_metrics() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect(&std::env::var("WHORL_OPERATIONS_TEST_URL").unwrap())
        .await
        .unwrap();
    for migration in [
        include_str!("../migrations/0001_application.sql"),
        include_str!("../migrations/0002_uploads.sql"),
    ] {
        sqlx::raw_sql(migration).execute(&pool).await.unwrap();
    }
    let batch = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO signature_versions VALUES ('test','{}',clock_timestamp())")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO batches(id,client_id,mode,signature_version,policy,deadline_at) VALUES($1,'test','check','test','{}',clock_timestamp()+interval '1 hour')")
        .bind(batch).execute(&pool).await.unwrap();
    for (ordinal, state, error, stage) in [
        (0i16, "completed", None, Some("completed")),
        (
            1,
            "failed",
            Some(
                json!({"code":"extraction_failed","message":"extraction_failed","retryable":false}),
            ),
            None,
        ),
        (2, "queued", None, None),
    ] {
        sqlx::query("INSERT INTO batch_files(id,batch_id,client_id,ordinal,name,deadline_at,state,error,stage,attempt) VALUES($1,$2,'test',$3,'private-name',clock_timestamp()+interval '1 hour',$4,$5,$6,2)")
            .bind(uuid::Uuid::new_v4()).bind(batch).bind(ordinal).bind(state).bind(error).bind(stage).execute(&pool).await.unwrap();
    }
    let objects = whorl::storage::s3::connect(
        &std::env::var("WHORL_OPERATIONS_S3_ENDPOINT").unwrap(),
        "uploads",
        "us-east-1",
    )
    .unwrap();
    use object_store::ObjectStoreExt;
    objects
        .put(
            &object_store::path::Path::from("uploads/orphan"),
            vec![0; 42].into(),
        )
        .await
        .unwrap();
    let id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO capacity_reservations VALUES($1,'test',1,42,clock_timestamp()+interval '1 hour',clock_timestamp())")
        .bind(id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO stored_uploads(id,client_id,object_key,bytes,expires_at) VALUES($1,'test','uploads/orphan',42,clock_timestamp()-interval '1 second')")
        .bind(id).execute(&pool).await.unwrap();
    let before = telemetry::snapshot(&pool).await.unwrap();
    assert_eq!(before["queued_files"], 1);
    assert_eq!(before["failed_files"], 1);
    assert_eq!(before["retry_attempts"], 3);
    assert_eq!(before["pending_bytes"], 42);
    assert_eq!(before["cleanup_backlog"], 1);
    assert!(before["oldest_pending_seconds"].as_f64().unwrap() >= 0.0);
    assert!(!before.to_string().contains("private-name"));
    let store = whorl::storage::UploadStore::new(pool.clone(), objects);
    assert!(store.reconcile_one().await.unwrap());
    let after = telemetry::snapshot(&pool).await.unwrap();
    assert_eq!(after["pending_bytes"], 0);
    assert_eq!(after["cleanup_backlog"], 0);
    assert_eq!(after["failed_files"], 1);
    let row = sqlx::query("SELECT count(*) AS n FROM stored_uploads")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<i64, _>("n"), 0);
}
