//! Durable per-file processing with transactional results and attempt fencing.

use crate::{
    cpu_pool::{CpuError, CpuPool, WorkDeadline},
    documents::CandidateRepository,
    extraction::{ExtractionError, Extractor},
    queue::FileJobPayload,
    service::matching::{Matcher, MatchingError, PreparedDocument},
    similarity::{shingles::SourceProfile, verify::SimilarityPolicy},
};
use graphile_worker::{IntoTaskHandlerResult, TaskHandler, WorkerContext};
use object_store::{ObjectStore, ObjectStoreExt, path::Path};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::{
    num::NonZeroUsize,
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// Worker bounds shared by queue handlers and CPU stages.
#[derive(Debug, Clone, Copy)]
pub struct WorkerConfig {
    /// One or two handlers and CPU threads; no default.
    pub workers: NonZeroUsize,
    /// Result retention after terminal completion; at least one whole second.
    pub retention: Duration,
    /// Tombstone lifetime after result expiry; at least one whole second.
    pub tombstone_retention: Duration,
}

/// Processing failures; only errors returned to the queue request a retry.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    /// Invalid worker bounds, policy snapshot, source, or version (`invalid_job`).
    #[error("invalid_job")]
    Invalid,
    /// Another attempt owns this file (`file_busy`).
    #[error("file_busy")]
    Busy,
    /// Database operation failed, including an unconfirmed commit.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    /// Shared object storage failed.
    #[error(transparent)]
    Storage(#[from] object_store::Error),
    /// Dedicated CPU admission, deadline, cancellation or execution failed.
    #[error(transparent)]
    Cpu(#[from] CpuError),
    /// Extraction rejected the file with its stable extraction code.
    #[error(transparent)]
    Extraction(#[from] ExtractionError),
    /// Candidate retrieval, validation or verification failed.
    #[error(transparent)]
    Matching(#[from] MatchingError),
}

/// Shared pipeline used by queue handlers and integration tests.
///
/// Every process uses a database advisory lock and an incrementing file attempt
/// fence. Results, canonical ingest and terminal state commit together. Dropping
/// processing releases the lock and leaves the file recoverable by queue retry.
/// Source uploads remain journaled for retention cleanup.
#[derive(Clone)]
pub struct ProcessFile {
    pool: PgPool,
    objects: Arc<dyn ObjectStore>,
    extractor: Arc<Extractor>,
    cpu: Arc<CpuPool>,
    config: WorkerConfig,
}

impl std::fmt::Debug for ProcessFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessFile")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl ProcessFile {
    /// Creates a shared pipeline with no waiting CPU submissions.
    ///
    /// # Arguments
    /// * `pool` - Application pool with at least four connections per handler.
    /// * `objects` - Shared service-owned source bucket; credentials stay here.
    /// * `extractor` - Trusted supervised extraction executable and limits.
    /// * `config` - Explicit concurrency and retention bounds; no defaults.
    ///   Fractional retention seconds are truncated; combined whole seconds
    ///   must not exceed `i32::MAX`.
    /// # Returns
    /// A cloneable pipeline owning a single dedicated CPU pool.
    /// # Errors
    /// [`ProcessError::Invalid`] for zero or overflowing retention, too few
    /// connections, or unsupported worker count; [`ProcessError::Cpu`] if the
    /// CPU pool cannot start or available parallelism is insufficient.
    /// # Examples
    /// ```no_run
    /// # fn example(pool: sqlx::PgPool, objects: std::sync::Arc<dyn object_store::ObjectStore>, extractor: whorl::extraction::Extractor) -> Result<(), whorl::service::process_file::ProcessError> {
    /// use whorl::service::process_file::{ProcessFile, WorkerConfig};
    /// let pipeline = ProcessFile::new(pool, objects, extractor, WorkerConfig {
    ///     workers: std::num::NonZeroUsize::MIN,
    ///     retention: std::time::Duration::from_secs(86400),
    ///     tombstone_retention: std::time::Duration::from_secs(86400),
    /// })?;
    /// # Ok(()) }
    /// ```
    pub fn new(
        pool: PgPool,
        objects: Arc<dyn ObjectStore>,
        extractor: Extractor,
        config: WorkerConfig,
    ) -> Result<Self, ProcessError> {
        if config.workers.get() > 2
            || pool.options().get_max_connections() < config.workers.get() as u32 * 4
            || config.retention.as_secs() == 0
            || config.tombstone_retention.as_secs() == 0
            || config
                .retention
                .as_secs()
                .checked_add(config.tombstone_retention.as_secs())
                .is_none_or(|n| n > i32::MAX as u64)
        {
            return Err(ProcessError::Invalid);
        }
        let cpu = Arc::new(CpuPool::new(
            config.workers,
            Duration::ZERO,
            Duration::from_secs(1),
        )?);
        Ok(Self {
            pool,
            objects,
            extractor: Arc::new(extractor),
            cpu,
            config,
        })
    }

    /// Returns the handler limit matching the dedicated CPU worker count.
    /// # Returns
    /// One or two queue handlers.
    /// # Examples
    /// ```no_run
    /// # fn example(pipeline: whorl::service::process_file::ProcessFile) {
    /// assert!(pipeline.concurrency() <= 2);
    /// # }
    /// ```
    pub fn concurrency(&self) -> usize {
        self.config.workers.get()
    }

    /// Stops CPU admission and drains admitted work after the queue stops.
    /// # Returns
    /// Nothing; all admitted CPU work has released its resources.
    /// # Examples
    /// ```no_run
    /// # async fn example(pipeline: whorl::service::process_file::ProcessFile) {
    /// pipeline.shutdown().await;
    /// # }
    /// ```
    pub async fn shutdown(&self) {
        self.cpu.shutdown().await;
    }

    /// Waits for a CPU panic that requires stopping and restarting this worker.
    /// # Returns
    /// Nothing once the CPU pool is poisoned. The queue host must stop admission,
    /// drain unrelated handlers, call [`Self::shutdown`], and exit with failure.
    /// Cancelling the wait does not clear the failure.
    /// # Examples
    /// ```no_run
    /// # async fn example(pipeline: &whorl::service::process_file::ProcessFile) {
    /// pipeline.failed().await;
    /// pipeline.shutdown().await;
    /// # }
    /// ```
    pub async fn failed(&self) {
        self.cpu.failed().await;
    }

    /// Reports whether this pipeline requires a process restart after a CPU panic.
    /// # Returns
    /// `true` when the shared CPU pool is poisoned; normal file failures return `false`.
    /// # Examples
    /// ```no_run
    /// # fn example(pipeline: &whorl::service::process_file::ProcessFile) {
    /// assert!(!pipeline.has_failed());
    /// # }
    /// ```
    #[must_use]
    pub fn has_failed(&self) -> bool {
        self.cpu.has_failed()
    }

    /// Processes one durable identity and commits results before acknowledgment.
    ///
    /// # Arguments
    /// * `payload` - Tenant, batch and file identity; all are checked in SQL.
    /// * `last_attempt` - True when the queue has exhausted its delivery budget.
    /// # Returns
    /// Nothing after durable success, permanent failure, exhaustion, or an
    /// already terminal file. Unknown identities are acknowledged without work.
    /// # Errors
    /// [`ProcessError::Busy`] requests retry when another handler owns the file.
    /// [`ProcessError::Database`], [`ProcessError::Storage`], [`ProcessError::Cpu`]
    /// and retryable [`ProcessError::Matching`] failures request retry until the
    /// last attempt. A database failure while persisting a terminal result always
    /// requests retry; operations must reconcile exhausted queue rows after an outage.
    /// Cancellation releases the advisory lock; generation checks prevent stale
    /// completion after a connection loss. Extraction subprocesses stop when the
    /// CPU work observes cancellation, within a 50 ms polling interval.
    /// # Examples
    /// ```no_run
    /// # async fn example(pipeline: whorl::service::process_file::ProcessFile, payload: whorl::queue::FileJobPayload) -> Result<(), whorl::service::process_file::ProcessError> {
    /// pipeline.process(&payload, false).await?;
    /// # Ok(()) }
    /// ```
    pub async fn process(
        &self,
        payload: &FileJobPayload,
        last_attempt: bool,
    ) -> Result<(), ProcessError> {
        let mut guard = self.pool.begin().await?;
        let locked: bool =
            sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,$2))")
                .bind(payload.file_id.to_string())
                .bind(crate::locks::ATTEMPT)
                .fetch_one(&mut *guard)
                .await?;
        if !locked {
            return Err(ProcessError::Busy);
        }
        let row = sqlx::query("UPDATE batch_files f SET state='running',stage='extracting',attempt=attempt+1,next_retry_at=NULL FROM batches b WHERE f.id=$1 AND f.batch_id=$2 AND f.client_id=$3 AND b.id=f.batch_id AND b.client_id=f.client_id AND b.state IN ('queued','running') AND f.state IN ('queued','running') RETURNING f.attempt,f.source_object_key,f.source_bytes,f.upload_metadata,b.mode,b.policy,b.signature_version,extract(epoch FROM (least(f.deadline_at,b.deadline_at)-clock_timestamp()))::double precision AS remaining")
            .bind(payload.file_id).bind(payload.batch_id).bind(&payload.client_id).fetch_optional(&self.pool).await?;
        let Some(row) = row else {
            return Ok(());
        };
        let attempt: i32 = row.try_get("attempt")?;
        sqlx::query(
            "UPDATE batches SET state='running' WHERE id=$1 AND client_id=$2 AND state='queued'",
        )
        .bind(payload.batch_id)
        .bind(&payload.client_id)
        .execute(&self.pool)
        .await?;
        let remaining: f64 = row.try_get("remaining")?;
        let result = if remaining <= 0.0 {
            Err(ProcessError::Extraction(ExtractionError::Timeout))
        } else {
            let duration =
                Duration::try_from_secs_f64(remaining).map_err(|_| ProcessError::Invalid)?;
            let deadline = Instant::now()
                .checked_add(duration)
                .ok_or(ProcessError::Invalid)?;
            match tokio::time::timeout(
                duration,
                self.execute(payload, attempt, &row, WorkDeadline::new(deadline)),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(ProcessError::Extraction(ExtractionError::Timeout)),
            }
        };
        match result {
            Ok(()) => {}
            Err(error) => {
                if self.cpu.has_failed() {
                    guard.rollback().await?;
                    return Err(error);
                }
                let permanent = matches!(
                    error,
                    ProcessError::Invalid
                        | ProcessError::Extraction(_)
                        | ProcessError::Matching(
                            MatchingError::EmptyText(_)
                                | MatchingError::InvalidMetadata
                                | MatchingError::IncompatibleProfile(_)
                        )
                );
                if permanent || last_attempt {
                    self.fail(
                        payload,
                        attempt,
                        if last_attempt && !permanent {
                            "retry_exhausted"
                        } else {
                            code(&error)
                        },
                    )
                    .await?;
                } else {
                    sqlx::query("UPDATE batch_files SET state='queued',stage=NULL,next_retry_at=clock_timestamp()+interval '1 second' * exp(least(attempt,10)) WHERE id=$1 AND client_id=$2 AND attempt=$3 AND state='running'")
                        .bind(payload.file_id).bind(&payload.client_id).bind(attempt).execute(&self.pool).await?;
                    guard.rollback().await?;
                    return Err(error);
                }
            }
        }
        guard.commit().await?;
        Ok(())
    }

    async fn stage(
        &self,
        payload: &FileJobPayload,
        attempt: i32,
        stage: &str,
    ) -> Result<(), ProcessError> {
        let updated = sqlx::query("UPDATE batch_files SET stage=$4 WHERE id=$1 AND client_id=$2 AND attempt=$3 AND state='running'")
            .bind(payload.file_id).bind(&payload.client_id).bind(attempt).bind(stage).execute(&self.pool).await?;
        if updated.rows_affected() == 0 {
            return Err(ProcessError::Busy);
        }
        Ok(())
    }

    async fn execute(
        &self,
        payload: &FileJobPayload,
        attempt: i32,
        row: &sqlx::postgres::PgRow,
        deadline: WorkDeadline,
    ) -> Result<(), ProcessError> {
        let mut metrics = crate::telemetry::StageMetrics::start("extracting");
        let key: Option<String> = row.try_get("source_object_key")?;
        let key = key.ok_or(ProcessError::Invalid)?;
        let expected: i64 = row.try_get("source_bytes")?;
        if !(1..=20 * 1024 * 1024).contains(&expected) {
            return Err(ProcessError::Invalid);
        }
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT FROM stored_uploads WHERE object_key=$1 AND client_id=$2 AND multipart_id IS NULL AND bytes=$3)")
            .bind(&key).bind(&payload.client_id).bind(expected).fetch_one(&self.pool).await?;
        if !valid {
            return Err(ProcessError::Invalid);
        }
        let path = Path::from(key);
        let head = self.objects.head(&path).await?;
        if head.size != expected as u64 {
            return Err(ProcessError::Invalid);
        }
        let bytes = self.objects.get_range(&path, 0..expected as u64).await?;
        if bytes.len() != expected as usize {
            return Err(ProcessError::Invalid);
        }
        let extractor = self.extractor.clone();
        let runtime = tokio::runtime::Handle::current();
        let extracted = self
            .cpu
            .run(deadline, move |context| {
                runtime.block_on(async move {
                    let extraction = extractor.extract(&bytes);
                    tokio::pin!(extraction);
                    loop {
                        tokio::select! {
                            result = &mut extraction => return result,
                            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                                if context.should_stop() { return Err(ExtractionError::Timeout); }
                            }
                        }
                    }
                })
            })
            .await??;
        metrics.complete();
        drop(metrics);
        self.stage(payload, attempt, "signing").await?;
        let mut metrics = crate::telemetry::StageMetrics::start("signing");
        let file_type = extracted.file_type.clone();
        let upload_metadata: Value = row.try_get("upload_metadata")?;
        let prepared = Arc::new(
            self.cpu
                .run(deadline, move |_| {
                    let mut metadata = serde_json::to_value(extracted.metadata)
                        .map_err(|_| MatchingError::InvalidMetadata)?;
                    metadata
                        .as_object_mut()
                        .ok_or(MatchingError::InvalidMetadata)?
                        .extend(
                            upload_metadata
                                .as_object()
                                .ok_or(MatchingError::InvalidMetadata)?
                                .clone(),
                        );
                    PreparedDocument::new(
                        &extracted.text,
                        metadata,
                        if extracted.ocr {
                            SourceProfile::OcrVerbatimV1
                        } else {
                            SourceProfile::NativeTextV1
                        },
                        None,
                    )
                })
                .await??,
        );
        let snapshot: String = row.try_get("signature_version")?;
        if !compatible_version(&snapshot, prepared.signature_version()) {
            return Err(ProcessError::Invalid);
        }
        let registered: bool = sqlx::query_scalar("SELECT EXISTS(SELECT FROM signature_versions actual JOIN signature_versions snapshot ON snapshot.version=$1 WHERE actual.version=$2 AND actual.configuration=snapshot.configuration)")
            .bind(&snapshot).bind(prepared.signature_version()).fetch_one(&self.pool).await?;
        if !registered {
            return Err(ProcessError::Invalid);
        }
        let policy = policy(row.try_get("policy")?)?;
        metrics.complete();
        drop(metrics);
        self.stage(payload, attempt, "candidate_lookup").await?;
        let mut metrics = crate::telemetry::StageMetrics::start("matching");
        self.stage(payload, attempt, "verifying").await?;
        let matches = Matcher::new(self.cpu.clone())
            .check(
                &self.pool,
                &payload.client_id,
                prepared.clone(),
                policy,
                deadline,
            )
            .await?;
        metrics.complete();
        drop(metrics);
        self.stage(payload, attempt, "persisting").await?;
        let mut metrics = crate::telemetry::StageMetrics::start("persisting");
        let mut tx = self.pool.begin().await?;
        let eligible: Option<bool> = sqlx::query_scalar("SELECT least(f.deadline_at,b.deadline_at)>clock_timestamp() FROM batch_files f JOIN batches b ON b.id=f.batch_id AND b.client_id=f.client_id WHERE f.id=$1 AND f.client_id=$2 AND f.attempt=$3 AND f.state='running' AND b.state IN ('queued','running') FOR UPDATE OF f")
            .bind(payload.file_id).bind(&payload.client_id).bind(attempt).fetch_optional(&mut *tx).await?;
        match eligible {
            None => return Err(ProcessError::Busy),
            Some(false) => return Err(ExtractionError::Timeout.into()),
            Some(true) => {}
        }
        for found in matches {
            sqlx::query("INSERT INTO match_results(client_id,file_id,document_id,score,text_similarity,metadata_similarity) VALUES($1,$2,$3,$4,$5,$6)")
                .bind(&payload.client_id).bind(payload.file_id).bind(found.document_id).bind(found.score.score).bind(found.score.text_similarity).bind(found.score.metadata_similarity)
                .execute(&mut *tx).await?;
        }
        let document = if row.try_get::<String, _>("mode")? == "ingest" {
            Some(
                CandidateRepository {
                    connection: &mut tx,
                    client_id: &payload.client_id,
                }
                .ingest(Uuid::new_v4(), &prepared, &file_type)
                .await?,
            )
        } else {
            None
        };
        sqlx::query(
            "UPDATE batch_files SET state='completed',stage='completed',document_id=$2 WHERE id=$1",
        )
        .bind(payload.file_id)
        .bind(document)
        .execute(&mut *tx)
        .await?;
        self.finish_batch(&mut tx, payload.batch_id).await?;
        tx.commit().await?;
        metrics.complete();
        Ok(())
    }

    async fn fail(
        &self,
        payload: &FileJobPayload,
        attempt: i32,
        code: &str,
    ) -> Result<(), ProcessError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE batch_files SET state='failed',stage=NULL,next_retry_at=NULL,error=$4 WHERE id=$1 AND client_id=$2 AND attempt=$3 AND state='running'")
            .bind(payload.file_id).bind(&payload.client_id).bind(attempt).bind(json!({"code":code,"message":code,"retryable":false})).execute(&mut *tx).await?;
        self.finish_batch(&mut tx, payload.batch_id).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn finish_batch(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        batch: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT id FROM batches WHERE id=$1 FOR UPDATE")
            .bind(batch)
            .execute(&mut **tx)
            .await?;
        sqlx::query("UPDATE batches SET state=CASE WHEN NOT EXISTS(SELECT FROM batch_files WHERE batch_id=$1 AND state='failed') THEN 'completed' WHEN EXISTS(SELECT FROM batch_files WHERE batch_id=$1 AND state='completed') THEN 'partially_failed' ELSE 'failed' END,completed_at=clock_timestamp(),expires_at=clock_timestamp()+make_interval(secs=>$2),tombstone_until=clock_timestamp()+make_interval(secs=>$3) WHERE id=$1 AND completed_at IS NULL AND NOT EXISTS(SELECT FROM batch_files WHERE batch_id=$1 AND state IN ('queued','running'))")
            .bind(batch).bind(self.config.retention.as_secs() as f64).bind((self.config.retention+self.config.tombstone_retention).as_secs() as f64).execute(&mut **tx).await?;
        Ok(())
    }
}

fn compatible_version(snapshot: &str, actual: &str) -> bool {
    let snapshot: Vec<_> = snapshot.split('/').collect();
    let actual: Vec<_> = actual.split('/').collect();
    snapshot.len() == 7
        && actual.len() == 7
        && snapshot
            .iter()
            .zip(&actual)
            .enumerate()
            .all(|(index, (a, b))| {
                if index == 1 {
                    matches!(*a, "native-text-v1" | "ocr-verbatim-v1")
                        && matches!(*b, "native-text-v1" | "ocr-verbatim-v1")
                } else {
                    a == b
                }
            })
}

fn policy(value: Value) -> Result<SimilarityPolicy, ProcessError> {
    let threshold = value
        .get("threshold")
        .and_then(Value::as_f64)
        .ok_or(ProcessError::Invalid)?;
    let weight = match value.get("kind").and_then(Value::as_str) {
        Some("text_only") => None,
        Some("weighted") => Some(
            value
                .get("text_weight")
                .and_then(Value::as_f64)
                .ok_or(ProcessError::Invalid)?,
        ),
        _ => return Err(ProcessError::Invalid),
    };
    SimilarityPolicy::new(threshold, weight).map_err(|_| ProcessError::Invalid)
}

fn code(error: &ProcessError) -> &'static str {
    match error {
        ProcessError::Extraction(error) => match error {
            ExtractionError::EmptyFile => "empty_file",
            ExtractionError::FileTooLarge => "file_too_large",
            ExtractionError::UnsupportedMediaType => "unsupported_type",
            ExtractionError::MalformedFile => "malformed_file",
            ExtractionError::LimitExceeded => "extraction_limit",
            ExtractionError::Timeout => "extraction_timeout",
            ExtractionError::NoExtractableText => "no_extractable_text",
            ExtractionError::ExtractionFailed => "extraction_failed",
        },
        ProcessError::Matching(MatchingError::EmptyText(_)) => "no_extractable_text",
        _ => "invalid_job",
    }
}

impl TaskHandler for FileJobPayload {
    const IDENTIFIER: &'static str = "process_file_v1";
    async fn run(self, ctx: WorkerContext) -> impl IntoTaskHandlerResult {
        let pipeline = ctx
            .get_ext::<ProcessFile>()
            .ok_or_else(|| "missing pipeline".to_owned())?;
        pipeline
            .process(&self, ctx.job().attempts() >= ctx.job().max_attempts())
            .await
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_derivation_preserves_algorithm_parameters() {
        let native = "nfkc-v1/native-text-v1/word-3/short-equality-v1/xxh3-low32-le-v1/125/25x5";
        let ocr = native.replace("native-text-v1", "ocr-verbatim-v1");
        assert!(compatible_version(native, &ocr));
        assert!(!compatible_version(
            native,
            &ocr.replace("word-3", "word-4")
        ));
        assert!(!compatible_version(
            native,
            &ocr.replace("ocr-verbatim-v1", "unknown")
        ));
    }

    #[test]
    fn policy_requires_valid_immutable_snapshot() {
        assert!(policy(json!({})).is_err());
        assert!(policy(json!({"kind":"weighted","threshold":0.75})).is_err());
        assert!(policy(json!({"kind":"text_only","threshold":2})).is_err());
        assert_eq!(
            policy(json!({"kind":"weighted","threshold":0.75,"text_weight":0.8}))
                .unwrap()
                .text_weight(),
            Some(0.8)
        );
    }
}
