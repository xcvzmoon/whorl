use sqlx::PgPool;
use whorl::service::admission::{AdmissionError, AdmissionLease, ClientQuota};

fn quota(files: u32, bytes: u64) -> ClientQuota {
    ClientQuota {
        files,
        bytes,
        lease_seconds: 30,
        retry_after: 2,
    }
}

#[tokio::test]
async fn rejects_untrusted_identity_and_invalid_budgets_without_database() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://localhost/unused")
        .unwrap();
    assert!(matches!(
        AdmissionLease::reserve(&pool, "a", false, quota(1, 1)).await,
        Err(AdmissionError::Invalid)
    ));
    assert!(matches!(
        AdmissionLease::reserve(&pool, " ", true, quota(1, 1)).await,
        Err(AdmissionError::Identity)
    ));
    for budget in [quota(0, 1), quota(1, 0), quota(1, 104857601)] {
        assert!(matches!(
            AdmissionLease::reserve(&pool, "a", true, budget).await,
            Err(AdmissionError::Invalid)
        ));
    }
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL; WHORL_ADMISSION_TEST_URL"]
async fn races_isolation_expiry_and_idempotent_release() {
    let pool = PgPool::connect(&std::env::var("WHORL_ADMISSION_TEST_URL").unwrap())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../database/migrations/0001_application.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../database/migrations/0002_uploads.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(
        "INSERT INTO capacity_limits VALUES ('global',2,100),('client:a',1,60),('client:b',2,100)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let other = pool.clone();
    let (a, b) = tokio::join!(
        AdmissionLease::reserve(&pool, "a", true, quota(1, 60)),
        AdmissionLease::reserve(&other, "a", true, quota(1, 60))
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let lease = a.or(b).unwrap();
    assert!(matches!(
        AdmissionLease::reserve(&pool, "a", true, quota(1, 1)).await,
        Err(AdmissionError::ClientQuota { retry_after: 2 })
    ));
    assert!(matches!(
        AdmissionLease::reserve(&pool, "b", true, quota(1, 41)).await,
        Err(AdmissionError::Unavailable {
            code: "pending_bytes_exceeded",
            retry_after: 2
        })
    ));
    let b = AdmissionLease::reserve(&pool, "b", true, quota(1, 40))
        .await
        .unwrap();
    assert!(matches!(
        AdmissionLease::reserve(&pool, "b", true, quota(1, 1)).await,
        Err(AdmissionError::Unavailable {
            code: "queue_full",
            ..
        })
    ));
    lease.release(&pool).await.unwrap();
    lease.release(&pool).await.unwrap();
    b.release(&pool).await.unwrap();
    let (a, b) = tokio::join!(
        AdmissionLease::reserve(&pool, "a", true, quota(1, 60)),
        AdmissionLease::reserve(&other, "b", true, quota(1, 60))
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    a.or(b).unwrap().release(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO capacity_reservations(id,client_id,jobs,bytes,created_at,lease_until) VALUES (gen_random_uuid(),'a',1,60,clock_timestamp()-interval '2 minutes',clock_timestamp()-interval '1 minute')").execute(&pool).await.unwrap();
    let lease = AdmissionLease::reserve(&pool, "a", true, quota(1, 60))
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM capacity_reservations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    lease.release(&pool).await.unwrap();
}

#[tokio::test]
#[ignore = "requires PostgreSQL and MinIO; WHORL_ADMISSION_UPLOAD_TEST_URL and WHORL_STORAGE_S3_ENDPOINT"]
async fn upload_transfer_reconciles_bytes_and_bounds_file_attempts() {
    use object_store::aws::AmazonS3Builder;
    use whorl::storage::{StorageError, UploadStore};
    let pool = PgPool::connect(&std::env::var("WHORL_ADMISSION_UPLOAD_TEST_URL").unwrap())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../database/migrations/0001_application.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../database/migrations/0002_uploads.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO capacity_limits VALUES ('global',2,10),('client:a',2,10)")
        .execute(&pool)
        .await
        .unwrap();
    let s3 = AmazonS3Builder::new()
        .with_endpoint(std::env::var("WHORL_STORAGE_S3_ENDPOINT").unwrap())
        .with_bucket_name("uploads")
        .with_region("us-east-1")
        .with_access_key_id("minioadmin")
        .with_secret_access_key("minioadmin")
        .with_allow_http(true)
        .build()
        .unwrap();
    let store = UploadStore::new(pool.clone(), s3);
    let lease = AdmissionLease::reserve(&pool, "a", true, quota(2, 10))
        .await
        .unwrap();
    let expiry: i64 = sqlx::query_scalar("SELECT extract(epoch FROM clock_timestamp())::bigint+60")
        .fetch_one(&pool)
        .await
        .unwrap();
    let first = store
        .upload_admitted(&lease, expiry, &b"abc"[..])
        .await
        .unwrap();
    assert_eq!(first.bytes, 3);
    let second = store
        .upload_admitted(&lease, expiry, &b"1234567"[..])
        .await
        .unwrap();
    assert_eq!(second.bytes, 7);
    assert!(matches!(
        store
            .upload_admitted(&lease, expiry, tokio::io::repeat(0))
            .await,
        Err(StorageError::Capacity)
    ));
    lease.release(&pool).await.unwrap();
    lease.release(&pool).await.unwrap();
    assert_eq!(
        sqlx::query_as::<_, (i64, i64)>(
            "SELECT sum(jobs)::bigint,sum(bytes)::bigint FROM capacity_reservations"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        (2, 10)
    );
    sqlx::raw_sql("UPDATE stored_uploads SET expires_at=clock_timestamp()-interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(store.reconcile_one().await.unwrap());
    assert!(store.reconcile_one().await.unwrap());
    assert!(!store.reconcile_one().await.unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM capacity_reservations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    let lease = AdmissionLease::reserve(&pool, "a", true, quota(1, 3))
        .await
        .unwrap();
    assert!(matches!(
        store.upload_admitted(&lease, expiry, &b"toolong"[..]).await,
        Err(StorageError::Invalid)
    ));
    lease.release(&pool).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM capacity_reservations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}
