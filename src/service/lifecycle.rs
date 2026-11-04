//! Durable terminal-state aggregation and deadline recovery.
//!
//! Callers retain transaction ownership so results, file transitions, and batch
//! completion can commit together. Recovery respects live worker advisory locks.

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

/// Result and tombstone lifetimes fixed when a batch first becomes terminal.
///
/// The default keeps results for 24 hours and their tombstone for another 24 hours.
/// Construction validates positive lifetimes whose sum fits a PostgreSQL integer.
#[derive(Debug, Clone, Copy)]
pub struct RetentionPolicy {
    seconds: i32,
    tombstone_seconds: i32,
}

/// A retention lifetime is nonpositive or their combined duration overflows.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("retention lifetimes must be positive and their sum must fit i32")]
pub struct InvalidRetention;

impl RetentionPolicy {
    /// Validates explicit result and tombstone lifetimes.
    ///
    /// # Arguments
    /// * `seconds` - Seconds to retain results after terminal completion.
    /// * `tombstone_seconds` - Additional seconds to retain the owner-visible tombstone.
    ///
    /// # Returns
    /// A policy with no side effects; neither lifetime has an implicit default.
    ///
    /// # Errors
    /// Returns [`InvalidRetention`] for a nonpositive lifetime or an overflowing sum.
    ///
    /// # Examples
    /// ```
    /// use whorl::service::lifecycle::RetentionPolicy;
    /// let policy = RetentionPolicy::new(3600, 86400)?;
    /// # Ok::<(), whorl::service::lifecycle::InvalidRetention>(())
    /// ```
    pub fn new(seconds: i32, tombstone_seconds: i32) -> Result<Self, InvalidRetention> {
        if seconds <= 0
            || tombstone_seconds <= 0
            || seconds.checked_add(tombstone_seconds).is_none()
        {
            return Err(InvalidRetention);
        }
        Ok(Self {
            seconds,
            tombstone_seconds,
        })
    }
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            seconds: 86400,
            tombstone_seconds: 86400,
        }
    }
}

/// Completes a batch once every file is terminal, preserving existing expiry.
///
/// Locks the batch before reading its file outcomes so concurrent final siblings
/// cannot leave it unfinished. All-success batches become completed, mixed
/// outcomes become partially failed, and batches with no successes become failed.
/// File/result writes in the caller's transaction are included in aggregation.
///
/// # Arguments
/// * `transaction` - READ COMMITTED application transaction containing file changes.
/// * `batch` - Batch already authorized by the caller; this function does not authenticate.
/// * `policy` - Retention applied only to the first terminal transition.
///
/// # Returns
/// `true` when completion was recorded, or `false` for a missing, already terminal,
/// or unfinished batch. One database timestamp anchors all three retention dates.
/// The caller must commit. Cancellation or an error requires rolling back the
/// transaction; uncommitted changes are not durable.
///
/// # Errors
/// Returns [`sqlx::Error`] for database access, locking, or statement failures.
///
/// # Examples
/// ```no_run
/// # async fn example(pool: &sqlx::PgPool, batch: uuid::Uuid) -> Result<(), sqlx::Error> {
/// use whorl::service::lifecycle::{RetentionPolicy, finish_batch};
/// let mut transaction = pool.begin().await?;
/// finish_batch(&mut transaction, batch, RetentionPolicy::default()).await?;
/// transaction.commit().await?;
/// # Ok(()) }
/// ```
pub async fn finish_batch(
    transaction: &mut Transaction<'_, Postgres>,
    batch: Uuid,
    policy: RetentionPolicy,
) -> Result<bool, sqlx::Error> {
    sqlx::query("SELECT id FROM batches WHERE id=$1 FOR UPDATE")
        .bind(batch)
        .execute(&mut **transaction)
        .await?;
    let changed = sqlx::query("WITH completed AS MATERIALIZED (SELECT clock_timestamp() AS at) UPDATE batches SET state=CASE WHEN NOT EXISTS(SELECT FROM batch_files WHERE batch_id=$1 AND state='failed') THEN 'completed' WHEN EXISTS(SELECT FROM batch_files WHERE batch_id=$1 AND state='completed') THEN 'partially_failed' ELSE 'failed' END,completed_at=completed.at,expires_at=completed.at+make_interval(secs=>$2::double precision),tombstone_until=completed.at+make_interval(secs=>$3::double precision) FROM completed WHERE id=$1 AND completed_at IS NULL AND NOT EXISTS(SELECT FROM batch_files WHERE batch_id=$1 AND state IN ('queued','running'))")
        .bind(batch)
        .bind(policy.seconds)
        .bind(policy.seconds + policy.tombstone_seconds)
        .execute(&mut **transaction)
        .await?;
    Ok(changed.rows_affected() != 0)
}

/// Fails expired unowned files and completes ready batches in bounded pages.
///
/// Live workers keep their file advisory locks and are skipped. Recovered files
/// increment their attempt generation so abandoned work cannot commit later.
/// At most 32 expired files and 32 ready batches are considered per call. Repeated
/// calls safely continue recovery; result/tombstone deletion belongs to maintenance.
///
/// # Arguments
/// * `transaction` - READ COMMITTED maintenance transaction; caller owns commit/rollback.
/// * `policy` - Retention for newly terminal batches, including recovered deadlines.
///
/// # Returns
/// Number of expired files changed to failed. Ready batches may complete even
/// when this count is zero. Cancellation requires rollback; no uncommitted
/// transition survives. This privileged operation spans clients and must only
/// run through trusted maintenance code.
///
/// # Errors
/// Returns [`sqlx::Error`] for selection, advisory locking, transitions, or aggregation.
///
/// # Examples
/// ```no_run
/// # async fn example(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
/// use whorl::service::lifecycle::{RetentionPolicy, expire_pending};
/// let mut transaction = pool.begin().await?;
/// let expired_files = expire_pending(&mut transaction, RetentionPolicy::default()).await?;
/// transaction.commit().await?;
/// # Ok(()) }
/// ```
pub async fn expire_pending(
    transaction: &mut Transaction<'_, Postgres>,
    policy: RetentionPolicy,
) -> Result<usize, sqlx::Error> {
    let files: Vec<Uuid> = sqlx::query_scalar("SELECT f.id FROM batch_files f JOIN batches b ON b.id=f.batch_id WHERE f.state IN ('queued','running') AND least(f.deadline_at,b.deadline_at)<=clock_timestamp() ORDER BY f.deadline_at,f.id LIMIT 32")
        .fetch_all(&mut **transaction).await?;
    let mut expired = 0;
    for file in files {
        let locked: bool =
            sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,$2))")
                .bind(file.to_string())
                .bind(crate::locks::ATTEMPT)
                .fetch_one(&mut **transaction)
                .await?;
        if locked {
            expired += usize::from(sqlx::query("UPDATE batch_files SET state='failed',stage=NULL,next_retry_at=NULL,attempt=attempt+1,error='{\"code\":\"deadline_exceeded\",\"message\":\"deadline_exceeded\",\"retryable\":false}' WHERE id=$1 AND state IN ('queued','running')")
                .bind(file).execute(&mut **transaction).await?.rows_affected() != 0);
        }
    }
    let batches: Vec<Uuid> = sqlx::query_scalar("SELECT b.id FROM batches b WHERE completed_at IS NULL AND NOT EXISTS(SELECT FROM batch_files f WHERE f.batch_id=b.id AND f.state IN ('queued','running')) ORDER BY b.id LIMIT 32 FOR UPDATE SKIP LOCKED")
        .fetch_all(&mut **transaction).await?;
    for batch in batches {
        finish_batch(transaction, batch, policy).await?;
    }
    Ok(expired)
}
