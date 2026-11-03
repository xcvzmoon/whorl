use object_store::{ObjectStoreExt, aws::AmazonS3Builder, path::Path};
use sqlx::{PgPool, Row};
use whorl::storage::{StorageError, UploadStore};

fn client() -> object_store::aws::AmazonS3 {
    AmazonS3Builder::new()
        .with_endpoint(std::env::var("WHORL_STORAGE_S3_ENDPOINT").unwrap())
        .with_bucket_name("uploads")
        .with_region("us-east-1")
        .with_access_key_id("minioadmin")
        .with_secret_access_key("minioadmin")
        .with_allow_http(true)
        .build()
        .unwrap()
}

async fn counts(pool: &PgPool) -> (i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM stored_uploads), (SELECT count(*) FROM capacity_reservations)")
        .fetch_one(pool).await.unwrap()
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL and local MinIO; see docs/storage.md"]
async fn durable_uploads_and_recovery() {
    let pool = PgPool::connect(&std::env::var("WHORL_STORAGE_TEST_URL").unwrap())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0001_application.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0002_uploads.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO capacity_limits VALUES ('global',2,41943040), ('client:a',1,20971520), ('client:b',1,20971520)").execute(&pool).await.unwrap();
    let store = UploadStore::new(pool.clone(), client());
    let expiry: i64 =
        sqlx::query_scalar("SELECT extract(epoch FROM clock_timestamp())::bigint + 3600")
            .fetch_one(&pool)
            .await
            .unwrap();
    let object = store.upload("a", expiry, &b"document"[..]).await.unwrap();
    assert_eq!(object.bytes, 8);
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT multipart_id IS NULL AND bytes=8 FROM stored_uploads WHERE id=$1"
        )
        .bind(object.id)
        .fetch_one(&pool)
        .await
        .unwrap()
    );
    assert_eq!(object.expires_at, expiry);
    let other_instance = client();
    assert_eq!(
        other_instance
            .get(&Path::from(object.key.clone()))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
        "document"
    );
    let mut unread = tokio::io::repeat(1);
    assert!(matches!(
        store.upload("a", expiry, &mut unread).await,
        Err(StorageError::Capacity)
    ));
    assert!(matches!(
        store.upload("b", expiry, &b""[..]).await,
        Err(StorageError::Invalid)
    ));
    assert_eq!(counts(&pool).await, (1, 1));
    assert!(matches!(
        store.upload("b", expiry, tokio::io::repeat(0)).await,
        Err(StorageError::Invalid)
    ));
    assert_eq!(counts(&pool).await, (1, 1));
    assert!(matches!(
        store.upload("b", expiry, FailingReader).await,
        Err(StorageError::Input(_))
    ));
    assert_eq!(counts(&pool).await, (1, 1));
    assert!(matches!(
        store.upload("b", 0, &b"expired"[..]).await,
        Err(StorageError::Invalid)
    ));
    assert_eq!(counts(&pool).await, (1, 1));
    let big = tokio::io::repeat(7).take(20 * 1024 * 1024);
    let full = store.upload("b", expiry, big).await.unwrap();
    assert_eq!(full.bytes, 20 * 1024 * 1024);
    assert_eq!(
        other_instance
            .head(&Path::from(full.key.clone()))
            .await
            .unwrap()
            .size,
        full.bytes as u64
    );
    sqlx::query("UPDATE stored_uploads SET expires_at=clock_timestamp()-interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO signature_versions(version, configuration) VALUES ('storage-test','{}'); INSERT INTO batches(id,client_id,mode,signature_version,policy,deadline_at) VALUES ('00000000-0000-0000-0000-000000000001','a','check','storage-test','{}',clock_timestamp()+interval '1 hour');").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO batch_files(id,batch_id,client_id,ordinal,name,deadline_at,source_object_key) VALUES ($1,'00000000-0000-0000-0000-000000000001','a',0,'source',clock_timestamp()+interval '1 hour',$2)")
        .bind(object.id).bind(&object.key).execute(&pool).await.unwrap();
    sqlx::query("UPDATE stored_uploads SET bytes=NULL WHERE id=$1")
        .bind(full.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(store.reconcile_one().await.unwrap());
    assert!(!store.reconcile_one().await.unwrap());
    assert_eq!(counts(&pool).await, (1, 1));
    sqlx::query("UPDATE batch_files SET state='running',stage='extracting'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(!store.reconcile_one().await.unwrap());
    assert_eq!(counts(&pool).await, (1, 1));
    sqlx::query("UPDATE batch_files SET state='completed',stage='completed'")
        .execute(&pool)
        .await
        .unwrap();
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(&object.key)
        .execute(&mut *lock)
        .await
        .unwrap();
    assert!(!store.reconcile_one().await.unwrap());
    lock.rollback().await.unwrap();
    assert!(store.reconcile_one().await.unwrap());
    assert_eq!(counts(&pool).await, (0, 0));
    assert!(other_instance.head(&Path::from(object.key)).await.is_err());

    let interrupted = store.clone();
    let (writer, reader) = tokio::io::duplex(64);
    let handle = tokio::spawn(async move { interrupted.upload("a", expiry, reader).await });
    for _ in 0..200 {
        let ready: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM stored_uploads WHERE multipart_id IS NOT NULL)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        if ready {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let row = sqlx::query("SELECT object_key, multipart_id FROM stored_uploads")
        .fetch_one(&pool)
        .await
        .unwrap();
    let key: String = row.get("object_key");
    let multipart: String = row.get("multipart_id");
    handle.abort();
    assert!(handle.await.unwrap_err().is_cancelled());
    drop(writer);
    sqlx::query("UPDATE stored_uploads SET expires_at=clock_timestamp()-interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let unavailable = AmazonS3Builder::new()
        .with_endpoint("http://127.0.0.1:1")
        .with_allow_http(true)
        .with_bucket_name("uploads")
        .with_region("us-east-1")
        .with_access_key_id("test")
        .with_secret_access_key("test")
        .with_retry(object_store::RetryConfig {
            max_retries: 0,
            ..Default::default()
        })
        .build()
        .unwrap();
    let broken = UploadStore::new(pool.clone(), unavailable);
    assert!(broken.reconcile_one().await.is_err());
    assert_eq!(counts(&pool).await, (1, 1));
    assert!(store.reconcile_one().await.unwrap());
    assert_eq!(counts(&pool).await, (0, 0));
    use object_store::multipart::MultipartStore;
    assert!(
        other_instance
            .put_part(
                &Path::from(key),
                &multipart,
                0,
                bytes::Bytes::from_static(b"gone").into()
            )
            .await
            .is_err()
    );
    assert!(!store.reconcile_one().await.unwrap());
}

use tokio::io::AsyncReadExt;

struct FailingReader;

impl tokio::io::AsyncRead for FailingReader {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        _: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Err(std::io::Error::other("interrupted input")))
    }
}
