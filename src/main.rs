mod http;
use actix_web::{App, HttpServer, web};
use sqlx::postgres::PgPoolOptions;
use std::num::NonZeroU16;
use whorl::{
    queue::JobQueue,
    storage::{UploadStore, s3},
};

#[actix_web::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("WHORL_TRUSTED_PROXY_ONLY").as_deref() != Ok("true") {
        return Err(
            "configure the authenticated proxy and set WHORL_TRUSTED_PROXY_ONLY=true".into(),
        );
    }
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let objects = s3::connect(
        &std::env::var("WHORL_S3_ENDPOINT")?,
        &std::env::var("WHORL_S3_BUCKET")?,
        &std::env::var("WHORL_S3_REGION")?,
    )?;
    let state = web::Data::new(http::Submission {
        store: UploadStore::new(pool.clone(), objects),
        queue: JobQueue::new(pool.clone(), NonZeroU16::new(3).unwrap()),
        pool,
        version: std::env::var("WHORL_SIGNATURE_VERSION")?,
        trusted_proxy_only: true,
    });
    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .configure(http::configure)
    })
    .bind(std::env::var("WHORL_LISTEN").unwrap_or_else(|_| "127.0.0.1:8080".into()))?
    .run()
    .await?;
    Ok(())
}
