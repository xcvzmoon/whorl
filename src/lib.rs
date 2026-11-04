//! Durable document ingestion and exact verification of likely similarity matches.
//!
//! Use [`similarity`] for deterministic text preparation and scoring. Applications
//! use [`service::admission`] to reserve upload capacity, [`storage`] to retain
//! source bytes, and [`service::process_file`] to process durable file identities.
//! All corpus and batch access requires a client identity authorized by the caller.
//!
//! CPU-heavy preparation belongs on the shared [`cpu_pool::CpuPool`]. Native
//! extraction runs through [`extraction::Extractor`] in an isolated child process.
//! Callers own transaction commit/rollback and must schedule source reconciliation
//! and retention cleanup. Cancellation keeps resource charges until their owners
//! can safely release them; it does not make committed uploads unclaimed.
//!
//! # Examples
//!
//! Prepare canonical content without database or storage access:
//!
//! ```
//! use whorl::{service::matching::PreparedDocument, similarity::shingles::SourceProfile};
//! let document = PreparedDocument::new("one two three", serde_json::json!({}),
//!     SourceProfile::NativeTextV1, None)?;
//! assert!(document.signature_version().contains("word-3"));
//! # Ok::<(), whorl::service::matching::MatchingError>(())
//! ```

pub mod cpu_pool;
pub mod similarity;

#[path = "persistence/documents.rs"]
mod documents;
pub mod service;

pub mod storage;

pub mod extraction;

pub mod queue;

mod telemetry;

pub mod logging;

pub mod locks;

#[path = "persistence/results.rs"]
mod results;
