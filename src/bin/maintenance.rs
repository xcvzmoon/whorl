use sqlx::postgres::PgPoolOptions;

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
        _ => return Err("usage: maintenance migrate".into()),
    }
    Ok(())
}
