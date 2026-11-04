//! Transactional scheduling of stored files with at-least-once delivery.
mod graphile;
pub use graphile::JobQueue;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Durable identity of a file; workers load bytes and policy from stored records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileJobPayload {
    /// Tenant owning the operation.
    pub client_id: String,
    /// Batch containing the operation.
    pub batch_id: Uuid,
    /// Stable operation identity across retries.
    pub file_id: Uuid,
}

/// Worker failure categories controlling queue retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFailure {
    /// Extraction rejection; persist a terminal file error.
    Extraction,
    /// Temporary shared storage failure.
    Storage,
    /// Database failure, including an unconfirmed commit.
    Database,
}
impl FileFailure {
    /// Reports whether infrastructure recovery can justify another attempt.
    ///
    /// # Returns
    /// True for storage and database failures; false for extraction failures.
    ///
    /// # Examples
    /// ```
    /// use whorl::queue::FileFailure;
    /// assert!(!FileFailure::Extraction.retryable());
    /// assert!(FileFailure::Storage.retryable());
    /// ```
    #[must_use]
    pub const fn retryable(self) -> bool {
        matches!(self, Self::Storage | Self::Database)
    }
}

/// Scheduling failed before the caller committed its transaction.
#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    /// Missing, expired, wrong-tenant, running or terminal file, or incomplete upload.
    #[error("file is not eligible for scheduling")]
    Ineligible,
    /// File validation or locking failed.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    /// Library job insertion failed.
    #[error(transparent)]
    Queue(#[from] graphile_worker::errors::GraphileWorkerError),
}
