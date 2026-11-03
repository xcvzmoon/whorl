use graphile_worker::WorkerUtils;
use sqlx::{PgPool, Row};
use std::num::NonZeroU16;
use uuid::Uuid;
use whorl::queue::{FileFailure, FileJobPayload, JobQueue, QueueError};

#[test]
fn stable_payload_and_failure_policy() {
    let payload = FileJobPayload {
        client_id: "tenant".into(),
        batch_id: Uuid::nil(),
        file_id: Uuid::nil(),
    };
    let value = serde_json::to_value(&payload).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 3);
    assert_eq!(
        serde_json::from_value::<FileJobPayload>(value).unwrap(),
        payload
    );
    assert!(!FileFailure::Extraction.retryable());
    assert!(FileFailure::Storage.retryable());
    assert!(FileFailure::Database.retryable());
}

async fn seed(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> (Uuid, Uuid) {
    let batch = Uuid::new_v4();
    let file = Uuid::new_v4();
    let upload = Uuid::new_v4();
    let key = format!("upload/{upload}");
    sqlx::query("INSERT INTO capacity_reservations(id,client_id,jobs,bytes,lease_until) VALUES ($1,'a',1,4,clock_timestamp()+interval '1 hour')")
        .bind(upload).execute(&mut **tx).await.unwrap();
    sqlx::query("INSERT INTO stored_uploads(id,client_id,object_key,bytes,expires_at) VALUES ($1,'a',$2,4,clock_timestamp()+interval '1 hour')")
        .bind(upload).bind(&key).execute(&mut **tx).await.unwrap();
    sqlx::query("INSERT INTO batches(id,client_id,mode,signature_version,policy,deadline_at) VALUES ($1,'a','check','test','{}',clock_timestamp()+interval '1 hour')")
        .bind(batch).execute(&mut **tx).await.unwrap();
    sqlx::query("INSERT INTO batch_files(id,batch_id,client_id,ordinal,name,deadline_at,source_object_key,source_bytes) VALUES ($1,$2,'a',0,'test',clock_timestamp()+interval '1 hour',$3,4)")
        .bind(file).bind(batch).bind(key).execute(&mut **tx).await.unwrap();
    (batch, file)
}

#[tokio::test]
#[ignore = "requires fresh PostgreSQL database in WHORL_ENQUEUE_TEST_URL; see docs/queue-adapter.md"]
async fn atomic_enqueue_and_concurrent_submissions() {
    let pool = PgPool::connect(&std::env::var("WHORL_ENQUEUE_TEST_URL").unwrap())
        .await
        .unwrap();
    for migration in [
        include_str!("../migrations/0001_application.sql"),
        include_str!("../migrations/0002_uploads.sql"),
    ] {
        sqlx::raw_sql(migration).execute(&pool).await.unwrap();
    }
    WorkerUtils::new(pool.clone(), "whorl_queue")
        .migrate()
        .await
        .unwrap();
    sqlx::query("INSERT INTO signature_versions(version,configuration) VALUES ('test','{}')")
        .execute(&pool)
        .await
        .unwrap();
    let queue = JobQueue::new(pool.clone(), NonZeroU16::new(3).unwrap());
    let mut tx = pool.begin().await.unwrap();
    let (batch, file) = seed(&mut tx).await;
    queue.enqueue(&mut tx, "a", file).await.unwrap();
    assert!(matches!(
        queue.enqueue(&mut tx, "a", Uuid::new_v4()).await,
        Err(QueueError::Ineligible)
    ));
    tx.rollback().await.unwrap();
    let absent: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT FROM batches WHERE id=$1) AND NOT EXISTS(SELECT FROM whorl_queue._private_jobs)").bind(batch).fetch_one(&pool).await.unwrap();
    assert!(absent);
    let mut tx = pool.begin().await.unwrap();
    let (batch, file) = seed(&mut tx).await;
    tx.commit().await.unwrap();
    let submit = || async {
        let mut tx = pool.begin().await.unwrap();
        let id = queue.enqueue(&mut tx, "a", file).await.unwrap();
        tx.commit().await.unwrap();
        id
    };
    let (first, second) = tokio::join!(submit(), submit());
    assert_eq!(first, second);
    let row = sqlx::query(
        "SELECT payload,max_attempts,attempts FROM whorl_queue._private_jobs WHERE id=$1",
    )
    .bind(first)
    .fetch_one(&pool)
    .await
    .unwrap();
    let payload: serde_json::Value = row.get("payload");
    assert_eq!(
        serde_json::from_value::<FileJobPayload>(payload).unwrap(),
        FileJobPayload {
            client_id: "a".into(),
            batch_id: batch,
            file_id: file
        }
    );
    assert_eq!(row.get::<i16, _>("max_attempts"), 3);
    let mut tx = pool.begin().await.unwrap();
    assert!(matches!(
        queue.enqueue(&mut tx, "b", file).await,
        Err(QueueError::Ineligible)
    ));
    sqlx::query("UPDATE stored_uploads SET multipart_id='unfinished'")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(matches!(
        queue.enqueue(&mut tx, "a", file).await,
        Err(QueueError::Ineligible)
    ));
    tx.rollback().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE batch_files SET state='running',stage='extracting' WHERE id=$1")
        .bind(file)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(matches!(
        queue.enqueue(&mut tx, "a", file).await,
        Err(QueueError::Ineligible)
    ));
    tx.rollback().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let (failed_batch, failed_file) = seed(&mut tx).await;
    sqlx::raw_sql("CREATE FUNCTION reject_enqueue() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected queue outage'; END; $$; CREATE TRIGGER reject_enqueue BEFORE INSERT ON whorl_queue._private_jobs FOR EACH ROW EXECUTE FUNCTION reject_enqueue();")
        .execute(&mut *tx).await.unwrap();
    assert!(matches!(
        queue.enqueue(&mut tx, "a", failed_file).await,
        Err(QueueError::Queue(_))
    ));
    tx.rollback().await.unwrap();
    let absent: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT FROM batches WHERE id=$1)")
        .bind(failed_batch)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(absent);
}
