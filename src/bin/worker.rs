use graphile_worker::WorkerOptions;
use sqlx::postgres::PgPoolOptions;
use std::{num::NonZeroUsize, sync::Arc, time::Duration};
use whorl::{
    extraction::{ExtractionLimits, Extractor},
    queue::FileJobPayload,
    service::process_file::{ProcessFile, WorkerConfig},
    storage::{UploadStore, s3},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let objects = s3::connect(
        &std::env::var("WHORL_S3_ENDPOINT")?,
        &std::env::var("WHORL_S3_BUCKET")?,
        &std::env::var("WHORL_S3_REGION")?,
    )?;
    let store = UploadStore::new(pool.clone(), objects.clone());
    let workers = std::env::var("WHORL_CPU_WORKERS")
        .unwrap_or_else(|_| "2".into())
        .parse::<NonZeroUsize>()?;
    let executable = std::env::current_exe()?.with_file_name("whorl-extract");
    let extractor = Extractor::new(
        executable,
        ExtractionLimits::default(),
        Duration::from_secs(60),
    )?;
    let pipeline = ProcessFile::new(
        pool.clone(),
        Arc::new(objects),
        extractor,
        WorkerConfig {
            workers,
            retention: Duration::from_secs(86400),
            tombstone_retention: Duration::from_secs(86400),
        },
    )?;
    let worker = WorkerOptions::default()
        .pg_pool(pool)
        .schema("whorl_queue")
        .concurrency(pipeline.concurrency())
        .heartbeat_interval(Duration::from_secs(30))
        .sweep_interval(Duration::from_secs(60))
        .sweep_threshold(Duration::from_secs(300))
        .recovery_delay(Duration::from_secs(30))
        .shutdown_grace_period(Duration::from_secs(5))
        .add_extension(pipeline.clone())
        .define_job::<FileJobPayload>()
        .init()
        .await?;
    let result = tokio::select! {
        result = worker.run() => result,
        () = async {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                for _ in 0..32 {
                    match store.reconcile_one().await {
                        Ok(true) => {},
                        Ok(false) => break,
                        Err(error) => { eprintln!("upload cleanup failed: {error}"); break; }
                    }
                }
            }
        } => unreachable!(),
    };
    pipeline.shutdown().await;
    result?;
    Ok(())
}
