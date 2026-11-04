#[path = "../src/service/maintenance.rs"]
mod maintenance;

use maintenance::{ReindexCursor, RetentionPolicy};
use std::num::NonZeroUsize;
use whorl::similarity::shingles::SourceProfile;

#[tokio::test]
#[ignore = "requires fresh WHORL_MAINTENANCE_TEST_URL database"]
async fn restartable_cleanup_and_version_isolation() {
    let pool = sqlx::PgPool::connect(&std::env::var("WHORL_MAINTENANCE_TEST_URL").unwrap())
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
    let old =
        ReindexCursor::new(SourceProfile::NativeTextV1, NonZeroUsize::new(3).unwrap()).unwrap();
    old.page(&pool).await.unwrap();
    let id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO documents(id,client_id,content_hash,text_content,file_type,metadata) VALUES ($1,'a',decode(repeat('00',32),'hex'),'one two three four','pdf','{}')").bind(id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO signatures(document_id,version) VALUES ($1,$2)")
        .bind(id)
        .bind(&old.version)
        .execute(&pool)
        .await
        .unwrap();
    let new =
        ReindexCursor::new(SourceProfile::NativeTextV1, NonZeroUsize::new(4).unwrap()).unwrap();
    assert_eq!(new.page(&pool).await.unwrap(), 1);
    assert_eq!(new.page(&pool).await.unwrap(), 0);
    let versions: i64 =
        sqlx::query_scalar("SELECT count(DISTINCT version) FROM signatures WHERE document_id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(versions, 2);
    let ocr =
        ReindexCursor::new(SourceProfile::OcrVerbatimV1, NonZeroUsize::new(4).unwrap()).unwrap();
    assert_eq!(ocr.page(&pool).await.unwrap(), 0);
    for n in 1..=33i32 {
        let extra = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO documents(id,client_id,content_hash,text_content,file_type,metadata) VALUES ($1,'a',decode(lpad(to_hex($2),64,'0'),'hex'),'short','pdf','{}')").bind(extra).bind(n).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO signatures(document_id,version) VALUES ($1,$2)")
            .bind(extra)
            .bind(&old.version)
            .execute(&pool)
            .await
            .unwrap();
    }
    assert_eq!(new.page(&pool).await.unwrap(), 32);
    assert_eq!(new.page(&pool).await.unwrap(), 1);
    assert_eq!(new.page(&pool).await.unwrap(), 0);
    let short: i64 = sqlx::query_scalar("SELECT count(*) FROM signatures WHERE version=$1 AND values IS NULL AND band_hashes IS NULL").bind(&new.version).fetch_one(&pool).await.unwrap();
    assert_eq!(short, 33);
    let batch = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO batches(id,client_id,mode,signature_version,policy,created_at,deadline_at) VALUES ($1,'a','check',$2,'{}',now()-interval '2 hours',now()-interval '1 hour')").bind(batch).bind(&old.version).execute(&pool).await.unwrap();
    let file = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO batch_files(id,batch_id,client_id,ordinal,name,deadline_at) VALUES ($1,$2,'a',0,'x',now()-interval '1 hour')").bind(file).bind(batch).execute(&pool).await.unwrap();
    let job = graphile_worker::WorkerUtils::new(pool.clone(), "whorl_queue")
        .add_raw_job(
            "process_file_v1",
            serde_json::json!({"file_id":file}),
            graphile_worker::JobSpec::default(),
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE whorl_queue._private_jobs SET updated_at=now()-interval '2 days' WHERE id=$1",
    )
    .bind(*job.id())
    .execute(&pool)
    .await
    .unwrap();
    let reservation = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO capacity_reservations(id,client_id,jobs,bytes,created_at,lease_until) VALUES ($1,'a',1,20,now()-interval '2 hours',now()-interval '1 hour')").bind(reservation).execute(&pool).await.unwrap();
    let mut guard = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,15))")
        .bind(file.to_string())
        .execute(&mut *guard)
        .await
        .unwrap();
    maintenance::sweep(&pool, RetentionPolicy::default())
        .await
        .unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM batch_files WHERE id=$1")
        .bind(file)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "queued");
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM whorl_queue.jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(jobs, 1);
    let reservations: i64 = sqlx::query_scalar("SELECT count(*) FROM capacity_reservations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(reservations, 0);
    guard.rollback().await.unwrap();
    maintenance::sweep(&pool, RetentionPolicy::default())
        .await
        .unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM batches WHERE id=$1")
        .bind(batch)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "failed");
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM whorl_queue.jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(jobs, 0);

    sqlx::query("UPDATE batches SET completed_at=now()-interval '3 days',expires_at=now()-interval '2 days',tombstone_until=now()+interval '1 day' WHERE id=$1").bind(batch).execute(&pool).await.unwrap();
    maintenance::sweep(&pool, RetentionPolicy::default())
        .await
        .unwrap();
    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM batch_files")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(files, 0);
    sqlx::query("UPDATE batches SET tombstone_until=now()-interval '1 day' WHERE id=$1")
        .bind(batch)
        .execute(&pool)
        .await
        .unwrap();
    maintenance::sweep(&pool, RetentionPolicy::default())
        .await
        .unwrap();
    maintenance::sweep(&pool, RetentionPolicy::default())
        .await
        .unwrap();
    let batches: i64 = sqlx::query_scalar("SELECT count(*) FROM batches")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(batches, 0);
}
