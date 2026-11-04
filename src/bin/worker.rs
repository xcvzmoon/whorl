#[path = "../config.rs"]
mod config;
#[path = "../telemetry.rs"]
mod telemetry;
use config::ServiceConfig;
use graphile_worker::WorkerOptions;
use sqlx::postgres::PgPoolOptions;
use std::{sync::Arc, time::Duration};
use whorl::{
    extraction::{ExtractionLimits, Extractor},
    queue::FileJobPayload,
    service::process_file::{ProcessFile, WorkerConfig},
    storage::{UploadStore, s3},
};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ServiceConfig::load_local()?;
    let logging = whorl::logging::init()?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(&logging))
}

async fn run(
    logging: &whorl::logging::Logging,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let workers = config::workers()?;
    let storage = ServiceConfig::storage()?;
    let objects = s3::connect(&storage.endpoint, &storage.bucket, &storage.region)?;
    let config = ServiceConfig::load()?;
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(config.database)
        .await?;
    let store = UploadStore::new(pool.clone(), objects.clone());
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
            retention: Duration::from_hours(24),
            tombstone_retention: Duration::from_hours(24),
        },
    )?;
    let restart = pipeline.clone();
    let worker = WorkerOptions::default()
        .pg_pool(pool.clone())
        .schema("whorl_queue")
        .concurrency(pipeline.concurrency())
        .heartbeat_interval(Duration::from_secs(30))
        .sweep_interval(Duration::from_secs(60))
        .sweep_threshold(Duration::from_secs(300))
        .recovery_delay(Duration::from_secs(30))
        .shutdown_grace_period(Duration::from_secs(5))
        .shutdown_signal(async move {
            restart.failed().await;
        })
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
                if let Ok(Ok(metrics)) = tokio::time::timeout(Duration::from_secs(5), telemetry::snapshot(&pool)).await { tracing::info!(event = "operational_snapshot", telemetry_dropped_lines = logging.dropped_lines(), queued_files = metrics["queued_files"].as_i64(), running_files = metrics["running_files"].as_i64(), oldest_pending_seconds = metrics["oldest_pending_seconds"].as_f64(), retry_attempts = metrics["retry_attempts"].as_i64(), failed_files = metrics["failed_files"].as_i64(), pending_bytes = metrics["pending_bytes"].as_i64(), cleanup_backlog = metrics["cleanup_backlog"].as_i64(), "Operational snapshot") } else { tracing::warn!(event = "operational_snapshot_failed", "Operational snapshot failed") }
                let mut cleanup = telemetry::StageMetrics::start("cleanup");
                let mut cleanup_ok = true;
                for _ in 0..32 {
                    match store.reconcile_one().await {
                        Ok(true) => {},
                        Ok(false) => break,
                        Err(_) => { cleanup_ok = false; tracing::warn!(event = "upload_cleanup_failed", "Upload cleanup failed"); break; }
                    }
                }
                if cleanup_ok { cleanup.complete(); }
            }
        } => unreachable!(),
    };
    pipeline.shutdown().await;
    if pipeline.has_failed() {
        return Err("CPU work panicked; worker restart required".into());
    }
    result?;
    Ok(())
}
