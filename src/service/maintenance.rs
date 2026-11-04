use sqlx::{PgPool, Row};
use std::num::NonZeroUsize;
use uuid::Uuid;
pub(crate) use whorl::service::lifecycle::RetentionPolicy;
use whorl::similarity::{
    normalize::NormalizationVersion,
    shingles::{ShingleSet, SourceProfile},
    signature::{SignatureIndex, SignatureVersion},
};

type Error = Box<dyn std::error::Error + Send + Sync>;

pub(crate) async fn sweep(pool: &PgPool, policy: RetentionPolicy) -> Result<(), Error> {
    let mut tx = pool.begin().await?;
    whorl::service::lifecycle::expire_pending(&mut tx, policy).await?;
    sqlx::query("DELETE FROM batch_files WHERE batch_id IN (SELECT id FROM batches WHERE expires_at<=clock_timestamp() AND EXISTS(SELECT FROM batch_files WHERE batch_id=batches.id) ORDER BY expires_at LIMIT 32 FOR UPDATE SKIP LOCKED)")
        .execute(&mut *tx).await?;
    sqlx::query("DELETE FROM batches WHERE id IN (SELECT id FROM batches WHERE tombstone_until<=clock_timestamp() ORDER BY tombstone_until LIMIT 32 FOR UPDATE SKIP LOCKED)")
        .execute(&mut *tx).await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(whorl::locks::CAPACITY)
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
    pub fn new(source: SourceProfile, k: NonZeroUsize) -> Self {
        Self {
            version: SignatureVersion::new(source, k).as_str().to_owned(),
            source,
            k,
        }
    }

    pub async fn page(&self, pool: &PgPool) -> Result<usize, Error> {
        let source = match self.source {
            SourceProfile::NativeTextV1 => "native-text-v1",
            SourceProfile::OcrVerbatimV1 => "ocr-verbatim-v1",
        };
        let config = SignatureVersion::new(self.source, self.k).configuration();
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
                let index = SignatureIndex::new(&shingles);
                Ok((index.values, index.bands))
            })
            .await??;
            sqlx::query("INSERT INTO signatures(document_id,version,values,band_hashes) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING")
                .bind(id).bind(&self.version).bind(values).bind(bands).execute(pool).await?;
        }
        Ok(count)
    }
}
