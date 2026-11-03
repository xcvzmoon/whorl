//! Bounded candidate pages verified on dedicated CPU threads.

use crate::{
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
use tokio::sync::{Semaphore, oneshot};
use uuid::Uuid;

/// Failures while preparing, retrieving, verifying or ingesting a document.
#[derive(Debug, thiserror::Error)]
pub enum MatchingError {
    /// Metadata must be a JSON object (`invalid_metadata`).
    #[error("invalid_metadata: metadata must be an object")]
    InvalidMetadata,
    /// The upload is missing, terminal, foreign, check-mode, or version-incompatible (`invalid_upload`).
    #[error(
        "invalid_upload: upload must be a pending ingest file owned by this client and version"
    )]
    InvalidUpload,
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
    /// The dedicated CPU executor could not start.
    #[error(transparent)]
    CpuStart(#[from] rayon::ThreadPoolBuildError),
    /// A CPU result channel closed before delivery.
    #[error(transparent)]
    CpuDelivery(#[from] oneshot::error::RecvError),
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

    /// Stores canonical content and completes its ingest upload atomically.
    ///
    /// # Arguments
    /// * `client` - Shared database pool; the transaction holds one connection.
    /// * `client_id` - Authorized corpus owner, validated by the caller.
    /// * `document_id` - Proposed identity; duplicates return the original identity.
    /// * `file_id` - Existing queued or running file belonging to an ingest batch.
    /// * `file_type` - One of the schema's supported file formats.
    ///
    /// # Returns
    /// The canonical document UUID. First-ingest text, metadata, type and timestamps
    /// survive duplicate races. Upload metadata and source provenance remain on
    /// the file row. The signature version must already be registered.
    ///
    /// # Errors
    /// Returns [`MatchingError::Database`] for connection, SQL or decoding failures,
    /// or [`MatchingError::InvalidUpload`] (`invalid_upload`) for a missing,
    /// terminal, foreign, check-mode or version-incompatible upload.
    /// Cancellation rolls back the transaction unless its
    /// commit has reached PostgreSQL; reread the file state before retrying.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(client: &sqlx::PgPool, file: uuid::Uuid, proposed: uuid::Uuid) -> Result<(), whorl::service::matching::MatchingError> {
    /// use whorl::{service::matching::PreparedDocument, similarity::shingles::SourceProfile};
    /// let input = PreparedDocument::new("one two three", serde_json::json!({}), SourceProfile::NativeTextV1, None)?;
    /// let canonical = input.ingest(client, "authorized-client", proposed, file, "pdf").await?;
    /// # Ok(()) }
    /// ```
    pub async fn ingest(
        &self,
        client: &PgPool,
        client_id: &str,
        document_id: Uuid,
        file_id: Uuid,
        file_type: &str,
    ) -> Result<Uuid, MatchingError> {
        let mut transaction = client.begin().await?;
        let file = sqlx::query_scalar::<_, Uuid>(
            "SELECT f.id FROM batch_files f JOIN batches b ON b.id = f.batch_id AND b.client_id = f.client_id
             WHERE f.id = $1 AND f.client_id = $2 AND b.mode = 'ingest'
             AND b.signature_version = $3 AND f.state IN ('queued', 'running') FOR UPDATE OF f")
            .bind(file_id).bind(client_id).bind(&self.version).fetch_optional(&mut *transaction).await?;
        if file.is_none() {
            return Err(MatchingError::InvalidUpload);
        }
        let mut repository = CandidateRepository {
            connection: &mut transaction,
            client_id,
        };
        let canonical = repository.ingest(document_id, self, file_type).await?;
        sqlx::query("UPDATE batch_files SET document_id = $1, state = 'completed', stage = 'completed', next_retry_at = NULL WHERE id = $2 AND client_id = $3")
            .bind(canonical).bind(file_id).bind(client_id).execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(canonical)
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

/// Shared two-thread verifier with one admitted request and no waiting requests.
///
/// Share one instance per process. Pages contain at most 32 candidates; only
/// qualifying scores are retained. Phase 11 adds deadline and shutdown control.
pub struct Matcher {
    pool: rayon::ThreadPool,
    admission: Arc<Semaphore>,
}

impl Matcher {
    /// Starts the two dedicated verification threads.
    ///
    /// # Returns
    /// A process-shared matcher; dropping it releases the pool after running work.
    ///
    /// # Errors
    /// Returns [`MatchingError::CpuStart`] if the dedicated threads cannot start.
    ///
    /// # Examples
    /// ```
    /// let matcher = whorl::service::matching::Matcher::new()?;
    /// # Ok::<(), whorl::service::matching::MatchingError>(())
    /// ```
    pub fn new() -> Result<Self, MatchingError> {
        Ok(Self {
            pool: rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .thread_name(|i| format!("matching-{i}"))
                .build()?,
            admission: Arc::new(Semaphore::new(1)),
        })
    }

    /// Retrieves and exactly verifies a client's version-compatible candidates.
    ///
    /// # Arguments
    /// * `client` - Shared database pool; the operation holds one read-only snapshot connection.
    /// * `client_id` - Authorized corpus owner; callers must enforce authorization.
    /// * `input` - CPU-prepared query whose version is already registered.
    /// * `policy` - Immutable batch scoring settings.
    ///
    /// # Returns
    /// Qualifying results ordered by descending score, then ascending UUID. Exact
    /// content duplicates are checked first without suppressing other candidates.
    /// Corpus documents, signatures and upload provenance are never modified.
    ///
    /// # Errors
    /// Returns [`MatchingError::Database`] for SQLx failures,
    /// [`MatchingError::Overloaded`] (`overloaded`) when admission is occupied,
    /// [`MatchingError::CpuDelivery`] if a result channel closes,
    /// [`MatchingError::EmptyText`] (`no_extractable_text`) for invalid stored text,
    /// or [`MatchingError::IncompatibleProfile`] for mismatched text policies.
    /// Cancellation drops the read-only transaction;
    /// an already running CPU page finishes before releasing its input memory.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(client: &sqlx::PgPool) -> Result<(), whorl::service::matching::MatchingError> {
    /// use whorl::{service::matching::{Matcher, PreparedDocument}, similarity::{shingles::SourceProfile, verify::SimilarityPolicy}};
    /// let input = PreparedDocument::new("one two three", serde_json::json!({}), SourceProfile::NativeTextV1, None)?;
    /// let results = Matcher::new()?.check(client, "authorized-client", std::sync::Arc::new(input), SimilarityPolicy::default()).await?;
    /// # Ok(()) }
    /// ```
    pub async fn check(
        &self,
        client: &PgPool,
        client_id: &str,
        input: Arc<PreparedDocument>,
        policy: SimilarityPolicy,
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
                self.verify(input.clone(), vec![duplicate], policy, permit.clone())
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
                self.verify(input.clone(), page, policy, permit.clone())
                    .await?,
            );
        }
        transaction.commit().await?;
        let (send, receive) = oneshot::channel();
        self.pool.spawn(move || {
            matches.sort_by(|a, b| {
                b.score
                    .score
                    .total_cmp(&a.score.score)
                    .then_with(|| a.document_id.cmp(&b.document_id))
            });
            drop(permit);
            let _ = send.send(matches);
        });
        Ok(receive.await?)
    }

    async fn verify(
        &self,
        input: Arc<PreparedDocument>,
        page: Vec<Candidate>,
        policy: SimilarityPolicy,
        permit: Arc<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<Vec<DocumentMatch>, MatchingError> {
        let (send, receive) = oneshot::channel();
        self.pool.spawn(move || {
            let result = page
                .into_par_iter()
                .map(|candidate| {
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
                .map(|scores| scores.into_iter().flatten().collect());
            let _ = send.send(result);
            drop(permit);
        });
        receive.await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        sync::Barrier,
        task::{Context, Poll, Waker},
    };

    #[test]
    fn cancelled_cpu_page_keeps_admission_until_work_finishes() {
        let matcher = Matcher::new().unwrap();
        assert_eq!(matcher.pool.current_num_threads(), 2);
        let started = Arc::new(Barrier::new(3));
        let release = Arc::new(Barrier::new(3));
        for _ in 0..2 {
            let started = started.clone();
            let release = release.clone();
            matcher.pool.spawn(move || {
                started.wait();
                release.wait();
            });
        }
        started.wait();
        let input = Arc::new(
            PreparedDocument::new(
                "hello",
                serde_json::json!({}),
                SourceProfile::NativeTextV1,
                None,
            )
            .unwrap(),
        );
        let permit = Arc::new(matcher.admission.clone().try_acquire_owned().unwrap());
        let mut future = Box::pin(matcher.verify(
            input,
            Vec::new(),
            SimilarityPolicy::default(),
            permit.clone(),
        ));
        assert!(matches!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        drop(future);
        drop(permit);
        assert!(matcher.admission.clone().try_acquire_owned().is_err());
        release.wait();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let _permit = runtime.block_on(matcher.admission.acquire()).unwrap();
        assert_eq!(matcher.admission.available_permits(), 0);
    }
}
