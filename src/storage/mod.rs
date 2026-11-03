//! Quota-reserved uploads and durable orphan recovery for shared S3 storage.

pub mod s3;

use bytes::Bytes;
use object_store::{ObjectStoreExt, aws::AmazonS3, multipart::MultipartStore, path::Path};
use sqlx::{PgPool, Row};
use tokio::io::{AsyncRead, AsyncReadExt};
use uuid::Uuid;

const PART_BYTES: usize = 5 * 1024 * 1024;
const FILE_BYTES: i64 = 20 * 1024 * 1024;

/// Storage failures; failed cleanup retains its durable reservation for retry.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Global or client capacity is exhausted.
    #[error("upload capacity exhausted")]
    Capacity,
    /// Client, lease, or content length is invalid.
    #[error("invalid upload or expired lease")]
    Invalid,
    /// PostgreSQL failed, including missing quota configuration.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    /// S3 failed; an ambiguous completion is recovered by reconciliation.
    #[error(transparent)]
    Object(#[from] object_store::Error),
    /// The input stream failed.
    #[error(transparent)]
    Input(#[from] std::io::Error),
}

/// Durable identity suitable for job payloads, without endpoint or credentials.
#[derive(Debug, Clone)]
pub struct StoredObjectRef {
    /// Reservation and journal identity.
    pub id: Uuid,
    /// Random key in the service-owned bucket.
    pub key: String,
    /// Actual content length in bytes.
    pub bytes: i64,
    /// Unix seconds after which unclaimed objects may be reclaimed.
    pub expires_at: i64,
}

/// Shared S3 storage backed by PostgreSQL quota accounting.
///
/// Create one store per pool and clone it for callers. Use a pool with at least two connections. The bucket must abort incomplete
/// multipart uploads through a lifecycle rule to cover crashes during creation.
/// Callers must enforce request deadlines and invoke reconciliation regularly.
/// Completed objects retain their reservation until deletion; admission must
/// attach the key to a same-client `batch_files` row before its lease expires.
#[derive(Clone)]
pub struct UploadStore {
    pool: PgPool,
    s3: AmazonS3,
    admission: std::sync::Arc<tokio::sync::Semaphore>,
}

impl UploadStore {
    /// Creates a store sharing the application's database pool.
    ///
    /// # Arguments
    /// * `pool` - Pool with application and upload migrations applied.
    /// * `s3` - Client for the same bucket on every instance.
    ///
    /// # Returns
    /// A store; construction performs no I/O.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(pool: sqlx::PgPool) -> Result<(), Box<dyn std::error::Error>> {
    /// let s3 = whorl::storage::s3::connect("https://minio.example.com", "uploads", "us-east-1")?;
    /// let store = whorl::storage::UploadStore::new(pool, s3);
    /// # Ok(()) }
    /// ```
    pub fn new(pool: PgPool, s3: AmazonS3) -> Self {
        let slots = (pool.options().get_max_connections() / 2).max(1) as usize;
        Self {
            pool,
            s3,
            admission: std::sync::Arc::new(tokio::sync::Semaphore::new(slots)),
        }
    }

    /// Reserves a full file allowance before reading and streams sequential parts.
    ///
    /// Cancellation leaves a journal entry and charged bytes for reconciliation.
    /// No more than one 5 MiB part is buffered by this layer. Empty and oversized
    /// streams are rejected. Errors trigger cleanup; cleanup failures retain the
    /// journal. This method stops reading on rejection; HTTP owns bounded draining.
    ///
    /// # Arguments
    /// * `client` - Trusted, nonempty owner identity with configured quotas.
    /// * `expires_at` - Future retention deadline in Unix seconds.
    /// * `input` - Content stream; read only after durable capacity reservation.
    ///
    /// # Returns
    /// A completed durable reference. Bytes remain charged until reconciliation.
    ///
    /// # Errors
    /// Returns [`StorageError::Invalid`] for invalid input or lease,
    /// [`StorageError::Capacity`] for exhausted quotas, or `Database`, `Object`,
    /// or `Input` for backend or stream failures.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(store: &whorl::storage::UploadStore, expiry: i64) -> Result<(), whorl::storage::StorageError> {
    /// let object = store.upload("client-a", expiry, &b"document"[..]).await?;
    /// assert_eq!(object.bytes, 8);
    /// # Ok(()) }
    /// ```
    pub async fn upload<R: AsyncRead + Unpin>(
        &self,
        client: &str,
        expires_at: i64,
        input: R,
    ) -> Result<StoredObjectRef, StorageError> {
        self.upload_inner(client, expires_at, input, None).await
    }

    /// Streams a file using capacity already reserved before request content arrived.
    ///
    /// # Arguments
    /// * `lease` - Active request lease for the authenticated client.
    /// * `expires_at` - Future object retention deadline in Unix seconds.
    /// * `input` - File content; read only after transferring capacity.
    ///
    /// # Returns
    /// A durable object reference. One file attempt and its actual bytes consume
    /// the request budget. Release the lease after the request. Cancellation leaves
    /// journaled uploads for storage reconciliation.
    ///
    /// # Errors
    /// Returns [`StorageError::Capacity`] for insufficient remaining budget,
    /// `Invalid` for expired leases or invalid content, and `Database`, `Object`,
    /// or `Input` for backend or stream failures. Failed attempts consume their
    /// transferred allowance until cleanup; callers must count drained content.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(store: &whorl::storage::UploadStore, lease: &whorl::service::admission::AdmissionLease, expiry: i64) -> Result<(), whorl::storage::StorageError> {
    /// let object = store.upload_admitted(lease, expiry, &b"text"[..]).await?;
    /// assert_eq!(object.bytes, 4);
    /// # Ok(()) }
    /// ```
    pub async fn upload_admitted<R: AsyncRead + Unpin>(
        &self,
        lease: &crate::service::admission::AdmissionLease,
        expires_at: i64,
        input: R,
    ) -> Result<StoredObjectRef, StorageError> {
        self.upload_inner(&lease.client, expires_at, input, Some(lease.id))
            .await
    }

    async fn upload_inner<R: AsyncRead + Unpin>(
        &self,
        client: &str,
        expires_at: i64,
        mut input: R,
        lease: Option<Uuid>,
    ) -> Result<StoredObjectRef, StorageError> {
        let _permit = self
            .admission
            .acquire()
            .await
            .map_err(|_| StorageError::Capacity)?;
        if self.pool.options().get_max_connections() < 2 || client.is_empty() {
            return Err(StorageError::Invalid);
        }
        let id = Uuid::new_v4();
        let key = format!("uploads/{id}");
        let mut guard = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&key)
            .execute(&mut *guard)
            .await?;
        let mut tx = self.pool.begin().await?;
        let valid: bool =
            sqlx::query_scalar("SELECT to_timestamp($1::double precision) > clock_timestamp()")
                .bind(expires_at)
                .fetch_one(&mut *tx)
                .await?;
        if !valid {
            return Err(StorageError::Invalid);
        }
        let allowance = if let Some(lease) = lease {
            let remaining: Option<i64> = sqlx::query_scalar("SELECT bytes FROM capacity_reservations WHERE id=$1 AND client_id=$2 AND jobs>0 AND bytes>0 AND lease_until>clock_timestamp() FOR UPDATE")
                .bind(lease).bind(client).fetch_optional(&mut *tx).await?;
            let allowance = remaining.ok_or(StorageError::Capacity)?.min(FILE_BYTES);
            sqlx::query("UPDATE capacity_reservations SET jobs=jobs-1, bytes=bytes-$2 WHERE id=$1")
                .bind(lease)
                .bind(allowance)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO capacity_reservations(id,client_id,jobs,bytes,lease_until) VALUES ($1,$2,1,$3,to_timestamp($4::double precision))")
                .bind(id).bind(client).bind(allowance).bind(expires_at).execute(&mut *tx).await?;
            allowance
        } else {
            let reserved: bool = sqlx::query_scalar(
                "SELECT reserve_capacity($1, $2, 1, $3, to_timestamp($4::double precision))",
            )
            .bind(id)
            .bind(client)
            .bind(FILE_BYTES)
            .bind(expires_at)
            .fetch_one(&mut *tx)
            .await?;
            if !reserved {
                return Err(StorageError::Capacity);
            }
            FILE_BYTES
        };
        sqlx::query("INSERT INTO stored_uploads(id, client_id, object_key, expires_at) VALUES ($1,$2,$3,to_timestamp($4::double precision))")
            .bind(id).bind(client).bind(&key).bind(expires_at).execute(&mut *tx).await?;
        tx.commit().await?;
        let path = Path::from(key.clone());
        let result = self.stream(id, &path, &mut input, allowance, lease).await;
        if result.is_err() {
            let _ = self.cleanup(id, &path).await;
        }
        guard.commit().await?;
        let bytes = result?;
        Ok(StoredObjectRef {
            id,
            key,
            bytes,
            expires_at,
        })
    }

    async fn stream<R: AsyncRead + Unpin>(
        &self,
        id: Uuid,
        path: &Path,
        input: &mut R,
        allowance: i64,
        lease: Option<Uuid>,
    ) -> Result<i64, StorageError> {
        let multipart = self.s3.create_multipart(path).await?;
        sqlx::query("UPDATE stored_uploads SET multipart_id=$2 WHERE id=$1")
            .bind(id)
            .bind(&multipart)
            .execute(&self.pool)
            .await?;
        let mut parts = Vec::new();
        let mut total = 0i64;
        loop {
            let mut buffer = vec![0; PART_BYTES];
            let mut filled = 0;
            while filled < PART_BYTES {
                let count = input.read(&mut buffer[filled..]).await?;
                if count == 0 {
                    break;
                }
                filled += count;
                total += count as i64;
                if total > allowance {
                    return Err(StorageError::Invalid);
                }
            }
            if filled == 0 {
                break;
            }
            buffer.truncate(filled);
            parts.push(
                self.s3
                    .put_part(path, &multipart, parts.len(), Bytes::from(buffer).into())
                    .await?,
            );
        }
        if total == 0 {
            return Err(StorageError::Invalid);
        }
        self.s3.complete_multipart(path, &multipart, parts).await?;
        let updated = sqlx::query(
            "UPDATE stored_uploads SET bytes=$2, multipart_id=NULL WHERE id=$1 AND expires_at > clock_timestamp()",
        )
        .bind(id)
        .bind(total)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::Invalid);
        }
        if let Some(lease) = lease {
            let mut tx = self.pool.begin().await?;
            sqlx::query("UPDATE capacity_reservations SET bytes=bytes+$2 WHERE id=$1 AND lease_until>clock_timestamp()")
                .bind(lease).bind(allowance-total).execute(&mut *tx).await?;
            sqlx::query("UPDATE capacity_reservations SET bytes=$2 WHERE id=$1")
                .bind(id)
                .bind(total)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        Ok(total)
    }

    async fn cleanup(&self, id: Uuid, path: &Path) -> Result<(), StorageError> {
        let multipart: Option<String> =
            sqlx::query_scalar("SELECT multipart_id FROM stored_uploads WHERE id=$1")
                .bind(id)
                .fetch_one(&self.pool)
                .await?;
        if let Some(multipart) = multipart {
            match self.s3.abort_multipart(path, &multipart).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
        match self.s3.delete(path).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => {}
            Err(error) => return Err(error.into()),
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM stored_uploads WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM capacity_reservations WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Reclaims one expired upload with no active file references.
    ///
    /// Run repeatedly from a bounded maintenance loop. Admission must hold the
    /// same advisory transaction lock (hash of the object key with seed zero)
    /// while validating owner, completed bytes and lease and attaching the key.
    /// Cancellation retains the journal; deletion and abort are retried safely.
    ///
    /// # Returns
    /// `true` after cleanup, or `false` when the oldest 32 eligible objects
    /// contain no unlocked entry. A later scheduled pass retries active entries.
    ///
    /// # Errors
    /// Returns [`StorageError::Invalid`] for a pool smaller than two connections,
    /// [`StorageError::Capacity`] if admission closes, or `Database` or `Object` on backend failure.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(store: &whorl::storage::UploadStore) -> Result<(), whorl::storage::StorageError> {
    /// while store.reconcile_one().await? {}
    /// # Ok(()) }
    /// ```
    pub async fn reconcile_one(&self) -> Result<bool, StorageError> {
        let _permit = self
            .admission
            .acquire()
            .await
            .map_err(|_| StorageError::Capacity)?;
        if self.pool.options().get_max_connections() < 2 {
            return Err(StorageError::Invalid);
        }
        let mut guard = self.pool.begin().await?;
        let row = sqlx::query("WITH candidates AS MATERIALIZED (SELECT id, object_key FROM stored_uploads u WHERE expires_at <= clock_timestamp() AND NOT EXISTS (SELECT 1 FROM batch_files f WHERE f.source_object_key=u.object_key AND f.state IN ('queued','running')) ORDER BY expires_at LIMIT 32) SELECT id, object_key FROM candidates WHERE pg_try_advisory_xact_lock(hashtextextended(object_key,0)) LIMIT 1")
            .fetch_optional(&mut *guard).await?;
        let Some(row) = row else {
            return Ok(false);
        };
        let eligible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stored_uploads u WHERE id=$1 AND expires_at <= clock_timestamp() AND NOT EXISTS(SELECT 1 FROM batch_files f WHERE f.source_object_key=u.object_key AND f.state IN ('queued','running')))")
            .bind(row.get::<Uuid,_>("id")).fetch_one(&mut *guard).await?;
        if !eligible {
            return Ok(false);
        }
        self.cleanup(
            row.get("id"),
            &Path::from(row.get::<String, _>("object_key")),
        )
        .await?;
        guard.commit().await?;
        Ok(true)
    }
}
