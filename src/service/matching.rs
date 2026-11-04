//! Bounded candidate pages verified on dedicated CPU threads.

use crate::{
    cpu_pool::{CpuError, CpuPool, WorkDeadline},
    documents::{Candidate, CandidateRepository},
    similarity::{
        normalize::NormalizationVersion,
        shingles::{ShingleSet, SourceProfile},
        signature::MinHashSignature,
        verify::{MatchScore, MetadataTokens, SimilarityEngine, SimilarityPolicy},
    },
};
use rayon::prelude::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{num::NonZeroUsize, sync::Arc};
use tokio::sync::Semaphore;
use uuid::Uuid;

/// Failures while preparing, retrieving, verifying or ingesting a document.
#[derive(Debug, thiserror::Error)]
pub enum MatchingError {
    /// Metadata must be a JSON object (`invalid_metadata`).
    #[error("invalid_metadata: metadata must be an object")]
    InvalidMetadata,
    /// Canonical text has no words (`no_extractable_text`).
    #[error(transparent)]
    EmptyText(#[from] crate::similarity::shingles::NoExtractableText),
    /// Stored text has an incompatible comparison policy.
    #[error(transparent)]
    IncompatibleProfile(#[from] crate::similarity::shingles::IncompatibleProfile),
    /// A SQLx connection, transaction, query or decoding operation failed.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    /// The matcher already owns work; callers must retry through admission (`overloaded`).
    #[error("overloaded: {0}")]
    Overloaded(#[from] tokio::sync::TryAcquireError),
    /// Shared CPU admission, deadline, panic or shutdown failure.
    #[error(transparent)]
    Cpu(#[from] CpuError),
}

/// Owned canonical input and its versioned retrieval indexes.
///
/// Construct on a CPU worker. Metadata is uploaded metadata; duplicate matching
/// always scores against the stored first-ingest metadata.
pub struct PreparedDocument {
    pub(crate) text: String,
    pub(crate) hash: [u8; 32],
    pub(crate) metadata: Value,
    pub(crate) tokens: MetadataTokens,
    pub(crate) version: String,
    pub(crate) bands: Vec<i32>,
    pub(crate) values: Option<Vec<i32>>,
    source: SourceProfile,
    k: Option<NonZeroUsize>,
    shingles: ShingleSet,
}

impl PreparedDocument {
    /// Normalizes and indexes extracted text under normalization v1.
    ///
    /// # Arguments
    /// * `text` - Extracted text; punctuation and whitespace follow normalization v1.
    /// * `metadata` - Object containing this upload's metadata.
    /// * `source` - Native or OCR comparison namespace.
    /// * `k` - Word-shingle length; `None` selects three.
    ///
    /// # Returns
    /// Owned canonical text, SHA-256 content identity, tokens and optional MinHash.
    ///
    /// # Errors
    /// Returns [`MatchingError::EmptyText`] (`no_extractable_text`) for empty
    /// canonical text, or [`MatchingError::InvalidMetadata`] (`invalid_metadata`)
    /// if metadata is not an object. No external side effects occur.
    ///
    /// # Examples
    /// ```
    /// use whorl::{service::matching::PreparedDocument, similarity::shingles::SourceProfile};
    /// let input = PreparedDocument::new("Hello", serde_json::json!({}), SourceProfile::NativeTextV1, None)?;
    /// # Ok::<(), whorl::service::matching::MatchingError>(())
    /// ```
    pub fn new(
        text: &str,
        metadata: Value,
        source: SourceProfile,
        k: Option<NonZeroUsize>,
    ) -> Result<Self, MatchingError> {
        if !metadata.is_object() {
            return Err(MatchingError::InvalidMetadata);
        }
        let text = NormalizationVersion::V1.normalize(text);
        let shingles = ShingleSet::new(&text, NormalizationVersion::V1, source, k)?;
        let signature = MinHashSignature::new(&shingles);
        let version = format!("{}/xxh3-low32-le-v1/125/25x5", shingles.version_key());
        let values = signature.as_ref().map(|s| {
            s.values()
                .iter()
                .map(|&v| i32::from_ne_bytes(v.to_ne_bytes()))
                .collect()
        });
        let bands = signature
            .as_ref()
            .map(|s| {
                s.bands()
                    .values()
                    .iter()
                    .map(|&v| i32::from_ne_bytes(v.to_ne_bytes()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            hash: Sha256::digest(text.as_bytes()).into(),
            tokens: MetadataTokens::new(&metadata),
            text,
            metadata,
            version,
            bands,
            values,
            source,
            k,
            shingles,
        })
    }

    /// Returns the complete comparison-policy key, including for short text.
    ///
    /// # Returns
    /// The key to register in `signature_versions` before ingest or batch admission.
    ///
    /// # Examples
    /// ```
    /// use whorl::{service::matching::PreparedDocument, similarity::shingles::SourceProfile};
    /// let input = PreparedDocument::new("hello", serde_json::json!({}), SourceProfile::NativeTextV1, None)?;
    /// assert!(input.signature_version().ends_with("125/25x5"));
    /// # Ok::<(), whorl::service::matching::MatchingError>(())
    /// ```
    pub fn signature_version(&self) -> &str {
        &self.version
    }

    /// Stores canonical content and version membership in the caller's transaction.
    ///
    /// First-ingest metadata and text survive duplicate races. This method does
    /// not change file or batch state; the durable worker owns those transitions.
    ///
    /// # Arguments
    /// * `transaction` - Application transaction; the caller owns commit and rollback.
    /// * `client_id` - Corpus owner authorized by the caller.
    /// * `document_id` - Proposed identity; duplicate content reuses its existing identity.
    /// * `file_type` - Content-detected format accepted by the document schema.
    ///
    /// # Returns
    /// The canonical identity, with this version's signature stored if absent.
    /// The version must already be registered. Cancellation requires rollback;
    /// an ambiguous commit requires rereading durable state before retrying.
    ///
    /// # Errors
    /// Returns [`MatchingError::Database`] for connection, query, decoding, or
    /// schema constraint failures. This method does not authenticate identities.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(pool: &sqlx::PgPool) -> Result<(), Box<dyn std::error::Error>> {
    /// use whorl::{service::matching::PreparedDocument, similarity::shingles::SourceProfile};
    /// let input = PreparedDocument::new("one two three", serde_json::json!({}), SourceProfile::NativeTextV1, None)?;
    /// let mut transaction = pool.begin().await?;
    /// let id = input.ingest(&mut transaction, "authorized-client", uuid::Uuid::new_v4(), "pdf").await?;
    /// transaction.commit().await?;
    /// # Ok(()) }
    /// ```
    pub async fn ingest(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        client_id: &str,
        document_id: Uuid,
        file_type: &str,
    ) -> Result<Uuid, MatchingError> {
        Ok(CandidateRepository {
            connection: transaction,
            client_id,
        }
        .ingest(document_id, self, file_type)
        .await?)
    }

    pub(crate) fn optional_bands(&self) -> Option<&Vec<i32>> {
        self.values.as_ref().map(|_| &self.bands)
    }
}

/// Canonical document identity and exact policy scores.
#[derive(Debug)]
pub struct DocumentMatch {
    /// Stable canonical corpus identity.
    pub document_id: Uuid,
    /// Exact text and metadata scores with strict threshold qualification.
    pub score: MatchScore,
}

/// Candidate verifier using the process-shared pipeline CPU pool.
///
/// Pages contain at most 32 candidates. One matching request is admitted at once.
pub struct Matcher {
    pool: Arc<CpuPool>,
    admission: Arc<Semaphore>,
}

impl Matcher {
    /// Uses the existing process-wide CPU pool without creating threads.
    ///
    /// # Arguments
    /// * `pool` - The same pool used for extraction and signing.
    /// # Returns
    /// A matcher with one admitted request and no waiting matching requests.
    /// # Examples
    /// ```no_run
    /// # fn example(pool: std::sync::Arc<whorl::cpu_pool::CpuPool>) {
    /// let matcher = whorl::service::matching::Matcher::new(pool);
    /// # }
    /// ```
    pub fn new(pool: Arc<CpuPool>) -> Self {
        Self {
            pool,
            admission: Arc::new(Semaphore::new(1)),
        }
    }

    /// Retrieves and exactly verifies a client's version-compatible candidates.
    ///
    /// # Arguments
    /// * `client` - Shared database pool; the operation holds one read-only snapshot connection.
    /// * `client_id` - Authorized corpus owner; callers must enforce authorization.
    /// * `input` - CPU-prepared query whose version is already registered.
    /// * `policy` - Immutable batch scoring settings.
    /// * `deadline` - Absolute file deadline shared across all CPU stages.
    ///
    /// # Returns
    /// Qualifying results ordered by descending score, then ascending UUID. Exact
    /// content duplicates are checked first without suppressing other candidates.
    /// Corpus documents, signatures and upload provenance are never modified.
    ///
    /// # Errors
    /// Returns [`MatchingError::Database`] for SQLx failures,
    /// [`MatchingError::Overloaded`] (`overloaded`) when admission is occupied,
    /// [`MatchingError::Cpu`] for pool overload, expiration, shutdown, panic or delivery failure,
    /// [`MatchingError::EmptyText`] (`no_extractable_text`) for invalid stored text,
    /// or [`MatchingError::IncompatibleProfile`] for mismatched text policies.
    /// Cancellation drops the read-only transaction;
    /// an already running CPU page finishes before releasing its input memory.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(client: &sqlx::PgPool, pool: std::sync::Arc<whorl::cpu_pool::CpuPool>, deadline: whorl::cpu_pool::WorkDeadline) -> Result<(), whorl::service::matching::MatchingError> {
    /// use whorl::{service::matching::{Matcher, PreparedDocument}, similarity::{shingles::SourceProfile, verify::SimilarityPolicy}};
    /// let input = PreparedDocument::new("one two three", serde_json::json!({}), SourceProfile::NativeTextV1, None)?;
    /// let results = Matcher::new(pool).check(client, "authorized-client", std::sync::Arc::new(input), SimilarityPolicy::default(), deadline).await?;
    /// # Ok(()) }
    /// ```
    pub async fn check(
        &self,
        client: &PgPool,
        client_id: &str,
        input: Arc<PreparedDocument>,
        policy: SimilarityPolicy,
        deadline: WorkDeadline,
    ) -> Result<Vec<DocumentMatch>, MatchingError> {
        let permit = Arc::new(self.admission.clone().try_acquire_owned()?);
        let mut transaction = client
            .begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .await?;
        let mut repository = CandidateRepository {
            connection: &mut transaction,
            client_id,
        };
        let mut matches = Vec::new();
        if let Some(duplicate) = repository.duplicate(&input).await? {
            matches.extend(
                self.verify(
                    input.clone(),
                    vec![duplicate],
                    policy,
                    permit.clone(),
                    deadline,
                )
                .await?,
            );
        }
        let mut after = None;
        loop {
            let page = repository
                .page(
                    &input,
                    after,
                    policy.text_weight().is_some_and(|weight| weight < 1.0),
                )
                .await?;
            if page.is_empty() {
                break;
            }
            after = page.last().map(|candidate| candidate.id);
            matches.extend(
                self.verify(input.clone(), page, policy, permit.clone(), deadline)
                    .await?,
            );
        }
        transaction.commit().await?;
        Ok(self
            .pool
            .run(deadline, move |_| {
                let _permit = permit;
                matches.sort_by(|a, b| {
                    b.score
                        .score
                        .total_cmp(&a.score.score)
                        .then_with(|| a.document_id.cmp(&b.document_id))
                });
                matches
            })
            .await?)
    }

    async fn verify(
        &self,
        input: Arc<PreparedDocument>,
        page: Vec<Candidate>,
        policy: SimilarityPolicy,
        permit: Arc<tokio::sync::OwnedSemaphorePermit>,
        deadline: WorkDeadline,
    ) -> Result<Vec<DocumentMatch>, MatchingError> {
        self.pool
            .run(deadline, move |context| {
                let _permit = permit;
                page.into_par_iter()
                    .map(|candidate| {
                        if context.should_stop() {
                            return Err(MatchingError::Cpu(if deadline.is_expired() {
                                CpuError::Expired
                            } else {
                                CpuError::Shutdown
                            }));
                        }
                        let text = ShingleSet::new(
                            &candidate.text,
                            NormalizationVersion::V1,
                            input.source,
                            input.k,
                        )?;
                        let tokens = MetadataTokens::new(&candidate.metadata);
                        let score = SimilarityEngine::new(policy).verify(
                            &input.shingles,
                            &text,
                            Some((&input.tokens, &tokens)),
                        )?;
                        Ok(score.qualifies.then_some(DocumentMatch {
                            document_id: candidate.id,
                            score,
                        }))
                    })
                    .collect::<Result<Vec<_>, MatchingError>>()
                    .map(|scores| scores.into_iter().flatten().collect())
            })
            .await?
    }
}
