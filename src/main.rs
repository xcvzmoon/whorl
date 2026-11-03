#[path = "config.rs"]
mod config;
use config::ServiceConfig;
mod http;
use actix_web::{App, HttpServer, web};
use sqlx::postgres::PgPoolOptions;
use std::num::NonZeroU16;
use whorl::{
    queue::JobQueue,
    storage::{UploadStore, s3},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    ServiceConfig::load_local()?;
    actix_web::rt::System::new().block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("WHORL_TRUSTED_PROXY_ONLY").as_deref() != Ok("true") {
        return Err(
            "configure the authenticated proxy and set WHORL_TRUSTED_PROXY_ONLY=true".into(),
        );
    }
    let listen = config::listen()?;
    let version = ServiceConfig::required("WHORL_SIGNATURE_VERSION")?;
    let storage = ServiceConfig::storage()?;
    let objects = s3::connect(&storage.endpoint, &storage.bucket, &storage.region)?;
    let config = ServiceConfig::load()?;
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(config.database)
        .await?;
    let state = web::Data::new(http::Submission {
        store: UploadStore::new(pool.clone(), objects),
        queue: JobQueue::new(pool.clone(), NonZeroU16::new(3).unwrap()),
        pool,
        version,
        trusted_proxy_only: true,
    });
    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .configure(http::configure)
    })
    .bind(listen)?
    .run()
    .await?;
    Ok(())
}
