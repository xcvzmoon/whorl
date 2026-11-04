//! Versioned lexical comparison shared by document ingestion and checking.
//!
//! [`normalize`] defines canonical Unicode text. [`shingles::ShingleSet`] records
//! word shingles and normalized equality for short text. [`signature`] produces
//! deterministic `MinHash` values and locality-sensitive bands for candidate lookup.
//! Bands retrieve likely candidates; [`verify::SimilarityEngine`] computes their
//! exact qualification score. Retrieved candidates must share the query's source
//! profile, normalization, and shingle length.
//!
//! These operations perform CPU work without I/O. Run larger inputs through the
//! shared CPU pool. Changing comparison rules requires a new registered signature
//! version and reindexing; existing arrays must not be relabeled.
//!
//! # Examples
//!
//! ```
//! use whorl::similarity::{normalize::NormalizationVersion,
//!     shingles::{ShingleSet, SourceProfile}, verify::{SimilarityEngine, SimilarityPolicy}};
//! let text = ShingleSet::new("one two three", NormalizationVersion::V1,
//!     SourceProfile::NativeTextV1, None)?;
//! let score = SimilarityEngine::new(SimilarityPolicy::default()).verify(&text, &text, None)?;
//! assert_eq!(score.text_similarity, 1.0);
//! assert!(score.qualifies);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod normalize;
pub mod shingles;
pub mod signature;
pub mod verify;
