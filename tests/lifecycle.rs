use sqlx::{PgPool, Row};
use uuid::Uuid;
use whorl::service::lifecycle::{RetentionPolicy, expire_pending, finish_batch};

async fn batch(pool: &PgPool, states: &[&str], expired: bool) -> (Uuid, Vec<Uuid>) {
    let batch = Uuid::new_v4();
    sqlx::query("INSERT INTO batches(id,client_id,mode,signature_version,policy,created_at,deadline_at) VALUES($1,'owner','check','test','{}',clock_timestamp()-interval '2 hours',clock_timestamp()+CASE WHEN $2 THEN interval '-1 hour' ELSE interval '1 hour' END)")
        .bind(batch).bind(expired).execute(pool).await.unwrap();
    let mut files = Vec::new();
    for (ordinal, state) in states.iter().enumerate() {
        let file = Uuid::new_v4();
        sqlx::query("INSERT INTO batch_files(id,batch_id,client_id,ordinal,name,state,stage,error,deadline_at) VALUES($1,$2,'owner',$3,'file',$4,CASE WHEN $4='completed' THEN 'completed' WHEN $4='running' THEN 'extracting' ELSE NULL END,CASE WHEN $4='failed' THEN '{\"code\":\"test\",\"message\":\"test\",\"retryable\":false}'::jsonb ELSE NULL END,clock_timestamp()+CASE WHEN $5 THEN interval '-1 hour' ELSE interval '1 hour' END)")
            .bind(file).bind(batch).bind(i16::try_from(ordinal).unwrap()).bind(state).bind(expired).execute(pool).await.unwrap();
        files.push(file);
    }
    (batch, files)
}

async fn state(pool: &PgPool, batch: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM batches WHERE id=$1")
        .bind(batch)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn validates_retention() {
    assert!(RetentionPolicy::new(0, 1).is_err());
    assert!(RetentionPolicy::new(1, -1).is_err());
    assert!(RetentionPolicy::new(i32::MAX, 1).is_err());
    assert!(RetentionPolicy::new(1, 1).is_ok());
}

#[tokio::test]
#[ignore = "requires fresh WHORL_LIFECYCLE_TEST_URL PostgreSQL database"]
async fn terminal_aggregation_expiry_and_concurrent_completion() {
    let pool = PgPool::connect(&std::env::var("WHORL_LIFECYCLE_TEST_URL").unwrap())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../database/migrations/0001_application.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO signature_versions(version,configuration) VALUES('test','{}')")
        .execute(&pool)
        .await
        .unwrap();
    let policy = RetentionPolicy::new(10, 20).unwrap();
    for (states, expected) in [
        (vec!["failed", "failed"], "failed"),
        (vec!["completed", "completed"], "completed"),
        (vec!["completed", "failed"], "partially_failed"),
        (vec!["completed", "queued"], "queued"),
    ] {
        let (id, _) = batch(&pool, &states, false).await;
        let mut tx = pool.begin().await.unwrap();
        assert_eq!(
            finish_batch(&mut tx, id, policy).await.unwrap(),
            expected != "queued"
        );
        tx.commit().await.unwrap();
        assert_eq!(state(&pool, id).await, expected);
        if expected != "queued" {
            let before: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_array(completed_at,expires_at,tombstone_until) FROM batches WHERE id=$1")
                .bind(id).fetch_one(&pool).await.unwrap();
            let row = sqlx::query("SELECT extract(epoch FROM expires_at-completed_at)::bigint AS result_seconds,extract(epoch FROM tombstone_until-expires_at)::bigint AS tombstone_seconds FROM batches WHERE id=$1")
                .bind(id).fetch_one(&pool).await.unwrap();
            assert_eq!(row.get::<i64, _>("result_seconds"), 10);
            assert_eq!(row.get::<i64, _>("tombstone_seconds"), 20);
            let mut tx = pool.begin().await.unwrap();
            assert!(
                !finish_batch(&mut tx, id, RetentionPolicy::default())
                    .await
                    .unwrap()
            );
            tx.commit().await.unwrap();
            let after: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_array(completed_at,expires_at,tombstone_until) FROM batches WHERE id=$1")
                .bind(id).fetch_one(&pool).await.unwrap();
            assert_eq!(before, after);
        }
    }
    let (expired, files) = batch(&pool, &["running"], true).await;
    let mut guard = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,15))")
        .bind(files[0].to_string())
        .execute(&mut *guard)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert_eq!(expire_pending(&mut tx, policy).await.unwrap(), 0);
    tx.commit().await.unwrap();
    assert_eq!(state(&pool, expired).await, "queued");
    guard.rollback().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert_eq!(expire_pending(&mut tx, policy).await.unwrap(), 1);
    tx.commit().await.unwrap();
    assert_eq!(state(&pool, expired).await, "failed");
    let row = sqlx::query("SELECT attempt,error->>'code' AS code FROM batch_files WHERE id=$1")
        .bind(files[0])
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<i32, _>("attempt"), 1);
    assert_eq!(row.get::<String, _>("code"), "deadline_exceeded");
    let (concurrent, files) = batch(&pool, &["running", "running"], false).await;
    let finish = |file| {
        let pool = pool.clone();
        async move {
            let mut tx = pool.begin().await.unwrap();
            sqlx::query("UPDATE batch_files SET state='completed',stage='completed' WHERE id=$1")
                .bind(file)
                .execute(&mut *tx)
                .await
                .unwrap();
            finish_batch(&mut tx, concurrent, policy).await.unwrap();
            tx.commit().await.unwrap();
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(finish(files[0]), finish(files[1]));
    })
    .await
    .unwrap();
    assert_eq!(state(&pool, concurrent).await, "completed");
}
