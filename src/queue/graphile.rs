use super::{FileJobPayload, QueueError};
use graphile_worker::{JobKeyMode, JobSpec, WorkerUtils};
use sqlx::{PgPool, Postgres, Transaction};
use std::num::NonZeroU16;
use uuid::Uuid;

/// Enqueue-only adapter; construction never runs queue migrations.
///
/// Deploy library migrations separately under their owner role. Callers own
/// transaction commit and rollback. Workers must fence repeated execution.
pub struct JobQueue {
    utils: WorkerUtils,
    max_attempts: i16,
}
impl std::fmt::Debug for JobQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobQueue")
            .field("max_attempts", &self.max_attempts)
            .finish_non_exhaustive()
    }
}

impl JobQueue {
    /// Creates an adapter with an explicit delivery budget.
    ///
    /// # Arguments
    /// - `pool`: Connections to the application database.
    /// - `max_attempts`: Total attempts including the first, capped at 32767.
    ///   No default; calibrate before production deployment.
    ///
    /// # Returns
    /// An adapter using the fixed `whorl_queue` schema.
    ///
    /// # Examples
    /// ```
    /// use whorl::queue::JobQueue;
    /// # async fn example() -> Result<(), sqlx::Error> {
    /// let pool = sqlx::postgres::PgPoolOptions::new()
    ///     .connect_lazy("postgresql://localhost/whorl")?;
    /// let queue = JobQueue::new(pool, std::num::NonZeroU16::MIN);
    /// # Ok(()) }
    /// ```
    pub fn new(pool: PgPool, max_attempts: NonZeroU16) -> Self {
        Self {
            utils: WorkerUtils::new(pool, "whorl_queue"),
            max_attempts: max_attempts.get().min(i16::MAX as u16) as i16,
        }
    }

    /// Schedules one queued file backed by a complete stored upload.
    ///
    /// Locks file/upload records until transaction completion. Pending keys
    /// deduplicate submissions. Library retries use exponential delays of
    /// `exp(min(attempts, 10))` seconds up to the total attempt budget.
    ///
    /// # Arguments
    /// - `transaction`: Transaction containing batch/file writes.
    /// - `client_id`: Trusted tenant identity checked against file and upload.
    /// - `file_id`: Stable operation identity and job key.
    ///
    /// # Returns
    /// Job id; repeated pending submissions return the same id.
    ///
    /// # Errors
    /// Returns [`QueueError::Ineligible`] for invalid file/upload state,
    /// [`QueueError::Database`] for validation/locking errors, or
    /// [`QueueError::Queue`] for insertion errors. Roll back the entire transaction
    /// on error or cancellation; do not commit partial batches.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(pool: sqlx::PgPool, file: uuid::Uuid) -> Result<(), Box<dyn std::error::Error>> {
    /// use whorl::queue::JobQueue;
    /// let queue = JobQueue::new(pool.clone(), std::num::NonZeroU16::MIN);
    /// let mut tx = pool.begin().await?;
    /// queue.enqueue(&mut tx, "tenant", file).await?;
    /// tx.commit().await?;
    /// # Ok(()) }
    /// ```
    pub async fn enqueue(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        client_id: &str,
        file_id: Uuid,
    ) -> Result<i64, QueueError> {
        let batch_id = sqlx::query_scalar(
            "SELECT f.batch_id FROM batch_files f
             JOIN batches b ON b.id = f.batch_id AND b.client_id = f.client_id
             JOIN stored_uploads u ON u.object_key = f.source_object_key AND u.client_id = f.client_id
             WHERE f.id = $1 AND f.client_id = $2 AND f.state = 'queued'
               AND b.state IN ('queued', 'running') AND u.multipart_id IS NULL
               AND u.bytes = f.source_bytes AND u.bytes > 0
               AND u.expires_at > clock_timestamp()
               AND f.deadline_at > clock_timestamp() AND b.deadline_at > clock_timestamp()
             FOR UPDATE OF f, u")
            .bind(file_id).bind(client_id).fetch_optional(&mut **transaction).await?
            .ok_or(QueueError::Ineligible)?;
        let payload = FileJobPayload {
            client_id: client_id.to_owned(),
            batch_id,
            file_id,
        };
        let job = self
            .utils
            .clone()
            .with_executor(&mut **transaction)
            .add_raw_job(
                "process_file_v1",
                payload,
                JobSpec {
                    job_key: Some(format!("file:{file_id}")),
                    job_key_mode: Some(JobKeyMode::PreserveRunAt),
                    max_attempts: Some(self.max_attempts),
                    ..JobSpec::default()
                },
            )
            .await?;
        Ok(*job.id())
    }
}
