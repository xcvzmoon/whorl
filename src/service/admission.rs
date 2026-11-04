//! Durable request budgets for the trusted-proxy boundary.

use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Admission failure, mapped by HTTP before reading request content.
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
            .bind(id).bind(client).bind(i64::from(quota.files)).bind(quota.bytes as i64).bind(f64::from(quota.lease_seconds)).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Self {
            id,
            client: client.to_owned(),
        })
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
