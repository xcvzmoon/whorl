#[path = "../service/maintenance.rs"]
mod maintenance;

use maintenance::{ReindexCursor, RetentionPolicy};
use sqlx::postgres::PgPoolOptions;
use std::num::NonZeroUsize;
use whorl::{
    similarity::shingles::SourceProfile,
    storage::{UploadStore, s3},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    match args.as_slice() {
        [command] if command == "migrate" => {
            let mut tx = pool.begin().await?;
            sqlx::query("SELECT pg_advisory_xact_lock(746831905)")
                .execute(&mut *tx)
                .await?;
            sqlx::query("CREATE TABLE IF NOT EXISTS whorl_migrations(version integer PRIMARY KEY)")
                .execute(&mut *tx)
                .await?;
            for (version, migration) in [
                (1i32, include_str!("../../migrations/0001_application.sql")),
                (2, include_str!("../../migrations/0002_uploads.sql")),
            ] {
                let applied: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT FROM whorl_migrations WHERE version=$1)",
                )
                .bind(version)
                .fetch_one(&mut *tx)
                .await?;
                if !applied {
                    sqlx::raw_sql(
                        migration
                            .trim()
                            .strip_prefix("BEGIN;")
                            .ok_or("missing BEGIN")?
                            .trim()
                            .strip_suffix("COMMIT;")
                            .ok_or("missing COMMIT")?,
                    )
                    .execute(&mut *tx)
                    .await?;
                    sqlx::query("INSERT INTO whorl_migrations VALUES ($1)")
                        .bind(version)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            tx.commit().await?;
            graphile_worker::WorkerUtils::new(pool, "whorl_queue")
                .migrate()
                .await?;
        }
        [command] if command == "sweep" => {
            maintenance::sweep(&pool, RetentionPolicy::default()).await?;
            let store = UploadStore::new(
                pool,
                s3::connect(
                    &std::env::var("WHORL_S3_ENDPOINT")?,
                    &std::env::var("WHORL_S3_BUCKET")?,
                    &std::env::var("WHORL_S3_REGION")?,
                )?,
            );
            for _ in 0..32 {
                if !store.reconcile_one().await? {
                    break;
                }
            }
        }
        [command, source, k] if command == "reindex" => {
            let source = match source.as_str() {
                "native" => SourceProfile::NativeTextV1,
                "ocr" => SourceProfile::OcrVerbatimV1,
                _ => return Err("source must be native or ocr".into()),
            };
            let cursor = ReindexCursor::new(source, k.parse::<NonZeroUsize>()?)?;
            let count = cursor.page(&pool).await?;
            println!("version={} processed={count}", cursor.version);
        }
        _ => return Err("usage: maintenance migrate | sweep | reindex <native|ocr> <k>".into()),
    }
    Ok(())
}
