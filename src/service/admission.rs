//! Durable request budgets for the trusted-proxy boundary.

use crate::{
    queue::{JobQueue, QueueError},
    service::lifecycle::{RetentionPolicy, finish_batch},
    storage::StoredObjectRef,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Request reservation or durable file admission failed.
#[derive(Debug, thiserror::Error)]
pub enum AdmissionError {
    /// Missing trusted identity; HTTP 401, `client_identity_required`.
    #[error("trusted client identity required")]
    Identity,
    /// Invalid budget, forwarding contract, or expired lease; HTTP 400.
    #[error("invalid admission request or expired lease")]
    Invalid,
    /// Client quota exhausted; HTTP 429, `client_quota_exceeded`.
    #[error("client quota exhausted; retry after {retry_after} seconds")]
    ClientQuota {
        /// Required Retry-After seconds.
        retry_after: u32,
    },
    /// Global quota exhausted; HTTP 503.
    #[error("{code}; retry after {retry_after} seconds")]
    Unavailable {
        /// `queue_full` or `pending_bytes_exceeded`.
        code: &'static str,
        /// Required Retry-After seconds.
        retry_after: u32,
    },
    /// Database failure, including missing quota configuration.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    /// Stored-file validation or transactional job insertion failed.
    #[error(transparent)]
    Queue(#[from] QueueError),
}

/// Request budget configuration, calibrated by deployment load tests.
#[derive(Debug, Clone, Copy)]
pub struct ClientQuota {
    /// Maximum file attempts reserved before reading content; must be positive.
    pub files: u32,
    /// Content budget, between 1 and 100 MiB inclusive.
    pub bytes: u64,
    /// Upload lease lifetime in seconds; must be positive.
    pub lease_seconds: u32,
    /// Retry-After for capacity rejection, in seconds; must be positive.
    pub retry_after: u32,
}

/// One received file awaiting durable admission into a batch.
///
/// Exactly one of `object` and `error` must be present. The admission module
/// validates stored references under the same lock used by source reconciliation.
#[derive(Debug)]
pub struct SubmittedFile {
    /// Stable file identity, including for individually rejected content.
    pub id: Uuid,
    /// Display filename; never used as a storage path.
    pub name: String,
    /// Completed staged source for a valid file, or `None` for a rejected file.
    pub object: Option<StoredObjectRef>,
    /// Per-file rejection with string `code`/`message` and boolean `retryable` fields.
    /// Must be `None` when a completed source is present.
    pub error: Option<Value>,
}

/// Durable upload budget belonging to one authenticated upstream client.
///
/// The proxy must strip and replace X-Client-Id and network policy must block
/// direct API access. This type does not authenticate public headers. Call
/// [`Self::release`] on every completed or failed request. Cancellation and process
/// death leave a bounded charge until the next reservation expires abandoned
/// leases. Stored uploads keep their own charges until storage reconciliation.
#[derive(Debug)]
pub struct AdmissionLease {
    pub(crate) id: Uuid,
    pub(crate) client: String,
}

impl AdmissionLease {
    /// Reserves request capacity before the caller reads any content.
    ///
    /// # Arguments
    /// * `pool` - Shared database with application and upload migrations applied.
    /// * `client` - Identity supplied by the authenticated upstream proxy.
    /// * `trusted_proxy_only` - Deployment confirms header replacement and blocked direct access.
    /// * `quota` - Bounded upload budget and retry timing; no defaults are assumed.
    ///
    /// # Returns
    /// A durable lease. No input stream is read. Cancellation before commit rolls
    /// back; cancellation after commit leaves a lease that expires automatically.
    ///
    /// # Errors
    /// Returns [`AdmissionError::Identity`] for missing identity, `Invalid` for
    /// invalid configuration, `ClientQuota` or `Unavailable` for capacity, and
    /// `Database` for SQL failures or absent quota rows.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(pool: &sqlx::PgPool) -> Result<(), whorl::service::admission::AdmissionError> {
    /// use whorl::service::admission::{AdmissionLease, ClientQuota};
    /// let lease = AdmissionLease::reserve(pool, "client-a", true, ClientQuota {
    ///     files: 5, bytes: 100 * 1024 * 1024, lease_seconds: 120, retry_after: 2,
    /// }).await?;
    /// lease.release(pool).await?;
    /// # Ok(()) }
    /// ```
    pub async fn reserve(
        pool: &PgPool,
        client: &str,
        trusted_proxy_only: bool,
        quota: ClientQuota,
    ) -> Result<Self, AdmissionError> {
        if !trusted_proxy_only {
            return Err(AdmissionError::Invalid);
        }
        if client.trim().is_empty() {
            return Err(AdmissionError::Identity);
        }
        if quota.files == 0
            || quota.bytes == 0
            || quota.bytes > 100 * 1024 * 1024
            || quota.lease_seconds == 0
            || quota.retry_after == 0
        {
            return Err(AdmissionError::Invalid);
        }
        let mut tx = pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(crate::locks::CAPACITY)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM capacity_reservations r WHERE lease_until <= clock_timestamp() AND NOT EXISTS (SELECT 1 FROM stored_uploads u WHERE u.id=r.id)").execute(&mut *tx).await?;
        for scope in ["global".to_owned(), format!("client:{client}")] {
            let row = sqlx::query(
                "SELECT max_jobs, max_bytes FROM capacity_limits WHERE scope=$1 FOR UPDATE",
            )
            .bind(&scope)
            .fetch_one(&mut *tx)
            .await?;
            let (jobs, bytes): (i64, i64) = sqlx::query_as("SELECT COALESCE(sum(jobs),0)::bigint, COALESCE(sum(bytes),0)::bigint FROM capacity_reservations WHERE $1='global' OR client_id=$2").bind(&scope).bind(client).fetch_one(&mut *tx).await?;
            let jobs_full = i128::from(jobs) + i128::from(quota.files)
                > i128::from(row.get::<i64, _>("max_jobs"));
            let bytes_full = i128::from(bytes) + i128::from(quota.bytes)
                > i128::from(row.get::<i64, _>("max_bytes"));
            if jobs_full || bytes_full {
                return Err(if scope == "global" {
                    AdmissionError::Unavailable {
                        code: if jobs_full {
                            "queue_full"
                        } else {
                            "pending_bytes_exceeded"
                        },
                        retry_after: quota.retry_after,
                    }
                } else {
                    AdmissionError::ClientQuota {
                        retry_after: quota.retry_after,
                    }
                });
            }
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO capacity_reservations(id,client_id,jobs,bytes,lease_until) VALUES ($1,$2,$3,$4,clock_timestamp()+make_interval(secs => $5::double precision))")
            .bind(id).bind(client).bind(i64::from(quota.files)).bind(i64::try_from(quota.bytes).map_err(|_| AdmissionError::Invalid)?).bind(f64::from(quota.lease_seconds)).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Self {
            id,
            client: client.to_owned(),
        })
    }

    /// Commits received files, their batch, and durable jobs in one transaction.
    ///
    /// Source-key locks serialize attachment with discard and reconciliation.
    /// Stored ownership, identity, bytes, multipart completion, and expiry are
    /// checked inside that transaction. A source may be claimed by only one file.
    /// Rejected files remain ordered siblings;
    /// an all-rejected batch receives terminal retention without queue work.
    ///
    /// # Arguments
    /// * `pool` - Database used to reserve this still-active lease.
    /// * `queue` - Queue adapter using this database and its deployed queue schema.
    /// * `mode` - `check` or `ingest`; no default or other mode is accepted.
    /// * `version` - Registered signature version snapshotted on the batch.
    /// * `files` - One through five files, each with a source or an individual error.
    ///
    /// # Returns
    /// The new batch ID after commit. The batch snapshots text-only scoring at
    /// 0.75, a one-hour processing deadline, and default terminal retention.
    /// Call [`Self::release`] afterward to release unused request capacity.
    /// This operation does not delete sources on failure; callers must discard
    /// staged sources or leave them for journaled reconciliation.
    /// Cancellation rolls back uncommitted database writes. If commit succeeded
    /// before the connection failed, its outcome is ambiguous; discard checks
    /// committed file references so cleanup cannot delete a claimed source.
    /// No request replay key is provided, and a repeated call can create a batch.
    ///
    /// # Errors
    /// Returns [`AdmissionError::Invalid`] for an unsupported mode, file count,
    /// error/source combination, invalid/repeated/already-claimed stored source,
    /// or missing/expired lease.
    /// [`AdmissionError::Database`] covers database and commit failures, including
    /// an unregistered version or duplicate file identity. [`AdmissionError::Queue`]
    /// covers stored-file eligibility and queue insertion failures. All uncommitted
    /// batch/file/job writes roll back together on error.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(pool: &sqlx::PgPool, lease: &whorl::service::admission::AdmissionLease, queue: &whorl::queue::JobQueue, version: &str, files: &[whorl::service::admission::SubmittedFile]) -> Result<(), whorl::service::admission::AdmissionError> {
    /// let batch = lease.admit(pool, queue, "ingest", version, files).await?;
    /// lease.release(pool).await?;
    /// # Ok(()) }
    /// ```
    pub async fn admit(
        &self,
        pool: &PgPool,
        queue: &JobQueue,
        mode: &str,
        version: &str,
        files: &[SubmittedFile],
    ) -> Result<Uuid, AdmissionError> {
        if !matches!(mode, "check" | "ingest")
            || !(1..=5).contains(&files.len())
            || files.iter().any(|file| {
                file.object.is_some() == file.error.is_some()
                    || file.error.as_ref().is_some_and(|error| {
                        !error.is_object()
                            || error.get("code").and_then(Value::as_str).is_none()
                            || error.get("message").and_then(Value::as_str).is_none()
                            || error.get("retryable").and_then(Value::as_bool).is_none()
                    })
            })
        {
            return Err(AdmissionError::Invalid);
        }
        let mut tx = pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await?;
        let mut keys: Vec<_> = files
            .iter()
            .filter_map(|file| file.object.as_ref().map(|object| &object.key))
            .collect();
        keys.sort_unstable();
        if keys.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(AdmissionError::Invalid);
        }
        for key in keys {
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,$2))")
                .bind(key)
                .bind(crate::locks::UPLOAD)
                .execute(&mut *tx)
                .await?;
        }
        let lease: Option<Uuid> = sqlx::query_scalar("SELECT id FROM capacity_reservations WHERE id=$1 AND client_id=$2 AND lease_until>clock_timestamp() AND NOT EXISTS(SELECT FROM stored_uploads WHERE id=$1) FOR UPDATE")
            .bind(self.id).bind(&self.client).fetch_optional(&mut *tx).await?;
        if lease.is_none() {
            return Err(AdmissionError::Invalid);
        }
        let batch = Uuid::new_v4();
        sqlx::query("INSERT INTO batches(id,client_id,mode,signature_version,policy,deadline_at) VALUES ($1,$2,$3,$4,$5,clock_timestamp()+interval '1 hour')")
            .bind(batch).bind(&self.client).bind(mode).bind(version).bind(json!({"kind":"text_only","threshold":0.75})).execute(&mut *tx).await?;
        for (ordinal, file) in files.iter().enumerate() {
            if let Some(object) = &file.object {
                let valid: Option<Uuid> = sqlx::query_scalar("SELECT id FROM stored_uploads WHERE id=$1 AND object_key=$2 AND client_id=$3 AND bytes=$4 AND bytes>0 AND multipart_id IS NULL AND expires_at>clock_timestamp() AND NOT EXISTS(SELECT FROM batch_files WHERE source_object_key=$2) FOR UPDATE")
                    .bind(object.id).bind(&object.key).bind(&self.client).bind(object.bytes).fetch_optional(&mut *tx).await?;
                if valid.is_none() {
                    return Err(AdmissionError::Invalid);
                }
            }
            sqlx::query("INSERT INTO batch_files(id,batch_id,client_id,ordinal,name,state,error,source_object_key,source_bytes,deadline_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,clock_timestamp()+interval '1 hour')")
                .bind(file.id).bind(batch).bind(&self.client).bind(i16::try_from(ordinal).map_err(|_| AdmissionError::Invalid)?).bind(&file.name).bind(if file.error.is_some() { "failed" } else { "queued" }).bind(&file.error).bind(file.object.as_ref().map(|object| &object.key)).bind(file.object.as_ref().map_or(0, |object| object.bytes)).execute(&mut *tx).await?;
            if file.object.is_some() {
                queue.enqueue(&mut tx, &self.client, file.id).await?;
            }
        }
        finish_batch(&mut tx, batch, RetentionPolicy::default()).await?;
        tx.commit().await?;
        Ok(batch)
    }

    /// Releases unused request capacity; repeated calls succeed.
    ///
    /// # Arguments
    /// * `pool` - The database used to reserve this lease.
    ///
    /// # Returns
    /// Unit after removing the unused budget. Already transferred upload charges remain.
    /// Cancellation rolls back an uncommitted release; expiry handles abandoned leases.
    ///
    /// # Errors
    /// Returns [`AdmissionError::Database`] on SQL failure.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(lease: &whorl::service::admission::AdmissionLease, pool: &sqlx::PgPool) -> Result<(), whorl::service::admission::AdmissionError> {
    /// lease.release(pool).await?;
    /// lease.release(pool).await?;
    /// # Ok(()) }
    /// ```
    pub async fn release(&self, pool: &PgPool) -> Result<(), AdmissionError> {
        sqlx::query("DELETE FROM capacity_reservations WHERE id=$1 AND client_id=$2 AND NOT EXISTS (SELECT 1 FROM stored_uploads WHERE id=$1)")
            .bind(self.id).bind(&self.client).execute(pool).await?;
        Ok(())
    }
}
