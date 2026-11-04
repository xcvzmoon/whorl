//! Durable admission, processing, and terminal-state ownership for document batches.
//!
//! [`admission::AdmissionLease`] reserves request capacity and commits received
//! files with their durable jobs. [`process_file::ProcessFile`] owns attempt-fenced
//! extraction, matching, canonical ingestion, and result persistence. [`lifecycle`]
//! shares terminal aggregation, retention, and abandoned-deadline recovery.
//!
//! [`matching::PreparedDocument`] prepares a query and writes canonical corpus
//! content. Its ingestion method does not complete files or batches. The worker
//! performs those transitions in the same transaction as results and ingestion.
//! Callers authorize the client, register signature versions before admission,
//! release unused leases, and retain journaled source charges until cleanup succeeds.
//!
//! # Examples
//!
//! Write canonical content within a transaction owned by the application:
//!
//! ```no_run
//! # async fn example(pool: &sqlx::PgPool) -> Result<(), Box<dyn std::error::Error>> {
//! use whorl::{service::matching::PreparedDocument, similarity::shingles::SourceProfile};
//! let document = PreparedDocument::new("one two three", serde_json::json!({}),
//!     SourceProfile::NativeTextV1, None)?;
//! let mut transaction = pool.begin().await?;
//! let id = document.ingest(&mut transaction, "authorized-client", uuid::Uuid::new_v4(), "pdf").await?;
//! transaction.commit().await?;
//! # Ok(()) }
//! ```

pub mod matching;

pub mod process_file;

pub mod admission;

pub mod lifecycle;
