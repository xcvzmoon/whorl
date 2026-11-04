use graphile_worker::WorkerUtils;
use object_store::{ObjectStoreExt, aws::AmazonS3Builder, path::Path};
use serde_json::json;
use sqlx::{PgPool, Row};
use std::num::NonZeroU16;
use uuid::Uuid;
use whorl::{
    queue::JobQueue,
    service::admission::{AdmissionError, AdmissionLease, ClientQuota, SubmittedFile},
    storage::{StoredObjectRef, UploadStore},
};

fn file(object: StoredObjectRef) -> SubmittedFile {
    SubmittedFile {
        id: Uuid::new_v4(),
        name: "document.pdf".into(),
        object: Some(object),
        error: None,
    }
}

async fn lease(pool: &PgPool) -> AdmissionLease {
    AdmissionLease::reserve(
        pool,
        "owner",
        true,
        ClientQuota {
            files: 1,
            bytes: 4,
            lease_seconds: 120,
            retry_after: 2,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires fresh WHORL_ATTACHMENT_TEST_URL and MinIO uploads bucket at WHORL_STORAGE_S3_ENDPOINT"]
#[expect(
    clippy::too_many_lines,
    reason = "One source ownership scenario verifies rollback and reconciliation races"
)]
async fn atomic_attachment_validates_sources_and_excludes_reconciliation() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(12)
        .connect(&std::env::var("WHORL_ATTACHMENT_TEST_URL").unwrap())
        .await
        .unwrap();
    for migration in [
        include_str!("../database/migrations/0001_application.sql"),
        include_str!("../database/migrations/0002_uploads.sql"),
    ] {
        sqlx::raw_sql(migration).execute(&pool).await.unwrap();
    }
    WorkerUtils::new(pool.clone(), "whorl_queue")
        .migrate()
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO capacity_limits VALUES('global',100,104857600),('client:owner',100,104857600); INSERT INTO signature_versions(version,configuration) VALUES('test','{}')")
        .execute(&pool).await.unwrap();
    let s3 = AmazonS3Builder::new()
        .with_endpoint(std::env::var("WHORL_STORAGE_S3_ENDPOINT").unwrap())
        .with_bucket_name("uploads")
        .with_region("us-east-1")
        .with_access_key_id("minioadmin")
        .with_secret_access_key("minioadmin")
        .with_allow_http(true)
        .build()
        .unwrap();
    let store = UploadStore::new(pool.clone(), s3.clone());
    let queue = JobQueue::new(pool.clone(), NonZeroU16::new(3).unwrap());
    let expiry: i64 =
        sqlx::query_scalar("SELECT extract(epoch FROM clock_timestamp())::bigint+3600")
            .fetch_one(&pool)
            .await
            .unwrap();
    let request = lease(&pool).await;
    let source = store
        .upload_admitted(&request, expiry, &b"text"[..])
        .await
        .unwrap();
    let balance: (i64, i64) =
        sqlx::query_as("SELECT jobs,bytes FROM capacity_reservations WHERE jobs=0 AND bytes=0")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(balance, (0, 0));
    assert!(matches!(
        request
            .admit(&pool, &queue, "unknown", "test", &[file(source.clone())])
            .await,
        Err(AdmissionError::Invalid)
    ));
    assert!(matches!(
        request.admit(&pool, &queue, "check", "test", &[]).await,
        Err(AdmissionError::Invalid)
    ));
    let mut contradictory = file(source.clone());
    contradictory.error = Some(json!({"code":"bad","message":"bad","retryable":false}));
    assert!(matches!(
        request
            .admit(&pool, &queue, "check", "test", &[contradictory])
            .await,
        Err(AdmissionError::Invalid)
    ));
    for change in 0..4 {
        let mut forged = source.clone();
        match change {
            0 => forged.id = Uuid::new_v4(),
            1 => forged.key.push_str("-foreign"),
            2 => forged.bytes += 1,
            _ => {
                sqlx::query("UPDATE stored_uploads SET client_id='foreign' WHERE id=$1")
                    .bind(source.id)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        }
        assert!(matches!(
            request
                .admit(&pool, &queue, "check", "test", &[file(forged)])
                .await,
            Err(AdmissionError::Invalid)
        ));
        sqlx::query("UPDATE stored_uploads SET client_id='owner' WHERE id=$1")
            .bind(source.id)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE stored_uploads SET multipart_id='unfinished' WHERE id=$1")
        .bind(source.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        request
            .admit(&pool, &queue, "check", "test", &[file(source.clone())])
            .await,
        Err(AdmissionError::Invalid)
    ));
    sqlx::query("UPDATE stored_uploads SET multipart_id=NULL WHERE id=$1")
        .bind(source.id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM batches")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    sqlx::raw_sql("CREATE FUNCTION reject_attachment_enqueue() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected queue outage'; END; $$; CREATE TRIGGER reject_attachment_enqueue BEFORE INSERT ON whorl_queue._private_jobs FOR EACH ROW EXECUTE FUNCTION reject_attachment_enqueue();")
        .execute(&pool).await.unwrap();
    assert!(matches!(
        request
            .admit(&pool, &queue, "check", "test", &[file(source.clone())])
            .await,
        Err(AdmissionError::Queue(_))
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM batches")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    sqlx::raw_sql("DROP TRIGGER reject_attachment_enqueue ON whorl_queue._private_jobs; DROP FUNCTION reject_attachment_enqueue();")
        .execute(&pool).await.unwrap();
    assert!(matches!(
        request
            .admit(
                &pool,
                &queue,
                "check",
                "test",
                &[file(source.clone()), file(source.clone())]
            )
            .await,
        Err(AdmissionError::Invalid)
    ));
    let admitted = request
        .admit(&pool, &queue, "ingest", "test", &[file(source.clone())])
        .await
        .unwrap();
    assert!(matches!(
        request
            .admit(&pool, &queue, "check", "test", &[file(source.clone())])
            .await,
        Err(AdmissionError::Invalid)
    ));
    request.release(&pool).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM whorl_queue._private_jobs")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    sqlx::query(
        "UPDATE stored_uploads SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(source.id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(!store.reconcile_one().await.unwrap());
    store.discard(&source).await.unwrap();
    assert_eq!(
        s3.head(&Path::from(source.key.clone())).await.unwrap().size,
        4
    );
    let retained: bool = sqlx::query_scalar("SELECT EXISTS(SELECT FROM capacity_reservations WHERE id=$1) AND EXISTS(SELECT FROM batch_files WHERE batch_id=$2 AND source_object_key=$3)")
        .bind(source.id).bind(admitted).bind(&source.key).fetch_one(&pool).await.unwrap();
    assert!(retained);
    let request = lease(&pool).await;
    let expiring = store
        .upload_admitted(&request, expiry, &b"text"[..])
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION gate_attachment_batch() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(746831910); RETURN NEW; END; $$; CREATE TRIGGER gate_attachment_batch BEFORE INSERT ON batches FOR EACH ROW EXECUTE FUNCTION gate_attachment_batch();")
        .execute(&pool).await.unwrap();
    let mut guard = pool.begin().await.unwrap();
    let guard_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *guard)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(746831910)")
        .execute(&mut *guard)
        .await
        .unwrap();
    let waiting_pool = pool.clone();
    let waiting_object = expiring.clone();
    let waiting = tokio::spawn(async move {
        let queue = JobQueue::new(waiting_pool.clone(), NonZeroU16::new(3).unwrap());
        let result = request
            .admit(
                &waiting_pool,
                &queue,
                "check",
                "test",
                &[file(waiting_object)],
            )
            .await;
        request.release(&waiting_pool).await.unwrap();
        result
    });
    tokio::time::timeout(std::time::Duration::from_secs(5),async {
        loop {
            let blocked: bool=sqlx::query_scalar("SELECT EXISTS(SELECT FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) AND query LIKE 'INSERT INTO batches%')")
                .bind(guard_pid).fetch_one(&pool).await.unwrap();
            if blocked { break; }
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    sqlx::query(
        "UPDATE stored_uploads SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(expiring.id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(!store.reconcile_one().await.unwrap());
    guard.commit().await.unwrap();
    assert!(matches!(
        waiting.await.unwrap(),
        Err(AdmissionError::Invalid)
    ));
    sqlx::raw_sql(
        "DROP TRIGGER gate_attachment_batch ON batches; DROP FUNCTION gate_attachment_batch();",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(store.reconcile_one().await.unwrap());
    assert!(s3.head(&Path::from(expiring.key.clone())).await.is_err());
    let row=sqlx::query("SELECT (SELECT count(*) FROM batch_files WHERE source_object_key=$1) AS files,(SELECT count(*) FROM capacity_reservations WHERE id=$2) AS charges,(SELECT count(*) FROM batches) AS batches")
        .bind(&expiring.key).bind(expiring.id).fetch_one(&pool).await.unwrap();
    assert_eq!(row.get::<i64, _>("files"), 0);
    assert_eq!(row.get::<i64, _>("charges"), 0);
    assert_eq!(row.get::<i64, _>("batches"), 1);
    let request = lease(&pool).await;
    let rejected = SubmittedFile {
        id: Uuid::new_v4(),
        name: "empty".into(),
        object: None,
        error: Some(json!({"code":"empty_file","message":"empty file","retryable":false})),
    };
    let terminal = request
        .admit(&pool, &queue, "check", "test", &[rejected])
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM batches WHERE id=$1")
            .bind(terminal)
            .fetch_one(&pool)
            .await
            .unwrap(),
        "failed"
    );
    request.release(&pool).await.unwrap();
}
