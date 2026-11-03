use sqlx::{PgPool, Row};
use std::num::NonZeroUsize;
use uuid::Uuid;
use whorl::similarity::{
    normalize::NormalizationVersion,
    shingles::{ShingleSet, SourceProfile},
    signature::MinHashSignature,
};

type Error = Box<dyn std::error::Error + Send + Sync>;

pub(crate) struct RetentionPolicy {
    pub seconds: i32,
    pub tombstone_seconds: i32,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            seconds: 86400,
            tombstone_seconds: 86400,
        }
    }
}

pub(crate) async fn sweep(pool: &PgPool, policy: RetentionPolicy) -> Result<(), Error> {
    if policy.seconds <= 0 || policy.tombstone_seconds <= 0 {
        return Err("retention must be positive".into());
    }
    let mut tx = pool.begin().await?;
    let files: Vec<(Uuid, Uuid)> = sqlx::query_as("SELECT f.id,f.batch_id FROM batch_files f JOIN batches b ON b.id=f.batch_id WHERE f.state IN ('queued','running') AND least(f.deadline_at,b.deadline_at)<=clock_timestamp() ORDER BY f.deadline_at,f.id LIMIT 32")
        .fetch_all(&mut *tx).await?;
    for (file, _) in files {
        let locked: bool =
            sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,15))")
                .bind(file.to_string())
                .fetch_one(&mut *tx)
                .await?;
        if locked {
            sqlx::query("UPDATE batch_files SET state='failed',stage=NULL,next_retry_at=NULL,attempt=attempt+1,error='{\"code\":\"deadline_exceeded\",\"message\":\"deadline_exceeded\",\"retryable\":false}' WHERE id=$1 AND state IN ('queued','running')")
                .bind(file).execute(&mut *tx).await?;
        }
    }
    sqlx::query("UPDATE batches SET state=CASE WHEN NOT EXISTS(SELECT FROM batch_files WHERE batch_id=batches.id AND state='failed') THEN 'completed' WHEN EXISTS(SELECT FROM batch_files WHERE batch_id=batches.id AND state='completed') THEN 'partially_failed' ELSE 'failed' END,completed_at=clock_timestamp(),expires_at=clock_timestamp()+make_interval(secs=>$1::double precision),tombstone_until=clock_timestamp()+make_interval(secs=>$1::double precision)+make_interval(secs=>$2::double precision) WHERE id IN (SELECT b.id FROM batches b WHERE completed_at IS NULL AND NOT EXISTS(SELECT FROM batch_files f WHERE f.batch_id=b.id AND f.state IN ('queued','running')) ORDER BY b.id LIMIT 32 FOR UPDATE SKIP LOCKED)")
        .bind(policy.seconds).bind(policy.tombstone_seconds).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM batch_files WHERE batch_id IN (SELECT id FROM batches WHERE expires_at<=clock_timestamp() AND EXISTS(SELECT FROM batch_files WHERE batch_id=batches.id) ORDER BY expires_at LIMIT 32 FOR UPDATE SKIP LOCKED)")
        .execute(&mut *tx).await?;
    sqlx::query("DELETE FROM batches WHERE id IN (SELECT id FROM batches WHERE tombstone_until<=clock_timestamp() ORDER BY tombstone_until LIMIT 32 FOR UPDATE SKIP LOCKED)")
        .execute(&mut *tx).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(746831904)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM capacity_reservations WHERE id IN (SELECT r.id FROM capacity_reservations r WHERE lease_until<=clock_timestamp() AND NOT EXISTS(SELECT FROM stored_uploads u WHERE u.id=r.id) ORDER BY lease_until LIMIT 32)")
        .execute(&mut *tx).await?;
    tx.commit().await?;
    let ids: Vec<i64> = sqlx::query_scalar("SELECT j.id FROM whorl_queue.jobs j JOIN whorl_queue._private_jobs p ON p.id=j.id WHERE j.task_identifier='process_file_v1' AND j.locked_at IS NULL AND j.updated_at<clock_timestamp()-interval '24 hours' AND NOT EXISTS(SELECT FROM batch_files f WHERE f.id::text=p.payload->>'file_id' AND f.state IN ('queued','running')) ORDER BY j.id LIMIT 32")
        .fetch_all(pool).await?;
    graphile_worker::WorkerUtils::new(pool.clone(), "whorl_queue")
        .complete_jobs(&ids)
        .await?;
    Ok(())
}

pub(crate) struct ReindexCursor {
    pub version: String,
    pub source: SourceProfile,
    pub k: NonZeroUsize,
}

impl ReindexCursor {
    pub fn new(source: SourceProfile, k: NonZeroUsize) -> Result<Self, Error> {
        let input = ShingleSet::new("version", NormalizationVersion::V1, source, Some(k))?;
        Ok(Self {
            version: format!("{}/xxh3-low32-le-v1/125/25x5", input.version_key()),
            source,
            k,
        })
    }

    pub async fn page(&self, pool: &PgPool) -> Result<usize, Error> {
        let source = match self.source {
            SourceProfile::NativeTextV1 => "native-text-v1",
            SourceProfile::OcrVerbatimV1 => "ocr-verbatim-v1",
        };
        let config =
            serde_json::json!({"normalization":"nfkc-v1", "source":source,"k":self.k.get()});
        sqlx::query("INSERT INTO signature_versions(version,configuration) VALUES ($1,$2) ON CONFLICT DO NOTHING")
            .bind(&self.version).bind(config).execute(pool).await?;
        let rows = sqlx::query("SELECT d.id FROM documents d WHERE EXISTS(SELECT FROM signatures s WHERE s.document_id=d.id AND split_part(s.version,'/',2)=$2) AND NOT EXISTS(SELECT FROM signatures s WHERE s.document_id=d.id AND s.version=$1) ORDER BY d.id LIMIT 32")
            .bind(&self.version).bind(source).fetch_all(pool).await?;
        let count = rows.len();
        for row in rows {
            let id: Uuid = row.try_get("id")?;
            let text: String = sqlx::query_scalar("SELECT text_content FROM documents WHERE id=$1")
                .bind(id)
                .fetch_one(pool)
                .await?;
            let source = self.source;
            let k = self.k;
            let (values, bands) = tokio::task::spawn_blocking(move || -> Result<_, Error> {
                let shingles = ShingleSet::new(&text, NormalizationVersion::V1, source, Some(k))?;
                let signature = MinHashSignature::new(&shingles);
                Ok((
                    signature
                        .as_ref()
                        .map(|s| s.values().iter().map(|v| *v as i32).collect::<Vec<_>>()),
                    signature.as_ref().map(|s| {
                        s.bands()
                            .values()
                            .iter()
                            .map(|v| *v as i32)
                            .collect::<Vec<_>>()
                    }),
                ))
            })
            .await??;
            sqlx::query("INSERT INTO signatures(document_id,version,values,band_hashes) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING")
                .bind(id).bind(&self.version).bind(values).bind(bands).execute(pool).await?;
        }
        Ok(count)
    }
}
