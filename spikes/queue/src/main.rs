use graphile_worker::{
    IntoTaskHandlerResult, JobSpec, TaskHandler, WorkerContext, WorkerOptions, WorkerUtils,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Deserialize)]
struct Probe {
    label: String,
    seconds: u64,
    fail: bool,
}

impl TaskHandler for Probe {
    const IDENTIFIER: &'static str = "probe";

    async fn run(self, ctx: WorkerContext) -> impl IntoTaskHandlerResult {
        sqlx::query(
            "INSERT INTO public.queue_probe_events(label, event, job_id) VALUES ($1, 'start', $2)",
        )
        .bind(&self.label)
        .bind(*ctx.job().id())
        .execute(ctx.pg_pool())
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_secs(self.seconds)).await;
        sqlx::query(
            "INSERT INTO public.queue_probe_events(label, event, job_id) VALUES ($1, 'finish', $2)",
        )
        .bind(&self.label)
        .bind(*ctx.job().id())
        .execute(ctx.pg_pool())
        .await
        .unwrap();
        if self.fail {
            Err("intentional queue spike failure")
        } else {
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("WHORL_QUEUE_TEST_URL")?)
        .await?;
    if std::env::args().nth(1).as_deref() == Some("utility") {
        let mut transaction = pool.begin().await?;
        let utils = WorkerUtils::new(pool.clone(), "queue_spike");
        utils
            .with_executor(&mut *transaction)
            .add_job(
                Probe {
                    label: "utility-rollback".into(),
                    seconds: 0,
                    fail: false,
                },
                JobSpec::default(),
            )
            .await?;
        transaction.rollback().await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM queue_spike._private_jobs WHERE payload->>'label' = 'utility-rollback'")
            .fetch_one(&pool).await?;
        assert_eq!(count, 0);
        return Ok(());
    }
    let worker = WorkerOptions::default()
        .concurrency(2)
        .poll_interval(Duration::from_secs(2))
        .use_notification_delivery(std::env::var_os("QUEUE_POLL_ONLY").is_none())
        .heartbeat_interval(Duration::from_secs(1))
        .sweep_interval(Duration::from_secs(1))
        .sweep_threshold(Duration::from_secs(6))
        .recovery_delay(Duration::from_secs(1))
        .schema("queue_spike")
        .define_job::<Probe>()
        .pg_pool(pool)
        .init()
        .await?;
    match std::env::args().nth(1).as_deref() {
        Some("migrate") => (),
        Some("once") => worker.run_once().await?,
        Some("run") => worker.run().await?,
        _ => return Err("expected utility, migrate, once, or run".into()),
    }
    Ok(())
}
