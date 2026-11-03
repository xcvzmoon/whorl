use super::{Submission, errors::ApiError};
use actix_web::{HttpResponse, web};
use std::time::Duration;

pub(super) async fn live() -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .finish()
}
pub(super) async fn ready(state: web::Data<Submission>) -> Result<HttpResponse, ApiError> {
    tokio::time::timeout(
        Duration::from_secs(2),
        sqlx::query("SELECT 1").execute(&state.pool),
    )
    .await
    .map_err(|_| ApiError::unavailable())?
    .map_err(|_| ApiError::unavailable())?;
    Ok(live().await)
}
