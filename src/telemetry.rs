use std::time::Instant;

#[allow(
    dead_code,
    reason = "stage timers are used by the library; snapshots by the worker"
)]
pub(crate) struct StageMetrics {
    stage: &'static str,
    started: Instant,
    completed: bool,
}

#[allow(
    dead_code,
    reason = "stage timers are used by the library; snapshots by the worker"
)]
impl StageMetrics {
    pub(crate) fn start(stage: &'static str) -> Self {
        Self {
            stage,
            started: Instant::now(),
            completed: false,
        }
    }

    pub(crate) fn complete(&mut self) {
        self.completed = true;
    }
}

impl Drop for StageMetrics {
    fn drop(&mut self) {
        eprintln!(
            "{}",
            serde_json::json!({
                "event": "stage_duration",
                "stage": self.stage,
                "seconds": self.started.elapsed().as_secs_f64(),
                "completed": self.completed,
            })
        );
    }
}

#[allow(
    dead_code,
    reason = "snapshot is used by the worker binary; stage timers by the library"
)]
pub(crate) async fn snapshot(pool: &sqlx::PgPool) -> Result<serde_json::Value, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *tx)
        .await?;
    let metrics = sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'event','operational_snapshot',
            'queued_files',count(*) FILTER (WHERE f.state='queued'),
            'running_files',count(*) FILTER (WHERE f.state='running'),
            'oldest_pending_seconds',coalesce(max(extract(epoch FROM (clock_timestamp()-b.created_at))) FILTER (WHERE f.state IN ('queued','running')),0),
            'retry_attempts',coalesce(sum(greatest(f.attempt-1,0)),0),
            'failed_files',count(*) FILTER (WHERE f.state='failed'),
            'pending_bytes',(SELECT coalesce(sum(bytes),0) FROM capacity_reservations),
            'cleanup_backlog',(SELECT count(*) FROM stored_uploads WHERE expires_at<=clock_timestamp())
        ) FROM batch_files f JOIN batches b ON b.id=f.batch_id",
    ).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(metrics)
}
