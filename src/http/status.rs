use super::{Submission, errors::ApiError};
use actix_web::{HttpRequest, HttpResponse, http::StatusCode, web};
use bytes::Bytes;
use futures_util::stream;
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

struct BatchRevision(i64);
impl BatchRevision {
    fn etag(&self, id: Uuid) -> String {
        format!("\"batch-{id}-r{}\"", self.0)
    }
}
#[expect(
    clippy::struct_excessive_bools,
    reason = "Cursor emission tracks independent JSON delimiter and terminal states"
)]
struct PollResponse {
    tx: Transaction<'static, Postgres>,
    files: std::collections::VecDeque<Value>,
    prefix: Option<String>,
    current: bool,
    first_file: bool,
    first_match: bool,
    finished: bool,
}

#[expect(
    clippy::too_many_lines,
    reason = "Snapshot transaction and streaming cursor share one response lifetime"
)]
pub(super) async fn status(
    req: HttpRequest,
    id: web::Path<Uuid>,
    state: web::Data<Submission>,
) -> Result<HttpResponse, ApiError> {
    let client = req
        .headers()
        .get("X-Client-Id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.trim().is_empty() && state.trusted_proxy_only)
        .ok_or_else(|| ApiError::new("client_identity_required", StatusCode::UNAUTHORIZED))?;
    let id = id.into_inner();
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|_| ApiError::unavailable())?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::unavailable())?;
    let batch = sqlx::query("SELECT revision, state, expires_at <= CURRENT_TIMESTAMP AS expired, tombstone_until <= CURRENT_TIMESTAMP AS forgotten, jsonb_build_object('batch_id',id,'mode',mode,'state',state,'signature_version',signature_version,'policy',policy,'expires_at',expires_at) AS body FROM batches WHERE id=$1 AND client_id=$2")
        .bind(id).bind(client).fetch_optional(&mut *tx).await.map_err(|_| ApiError::unavailable())?;
    let batch = batch.ok_or_else(|| ApiError::new("batch_not_found", StatusCode::NOT_FOUND))?;
    if batch.get::<Option<bool>, _>("forgotten") == Some(true) {
        return Err(ApiError::new("batch_not_found", StatusCode::NOT_FOUND));
    }
    if batch.get::<Option<bool>, _>("expired") == Some(true) {
        return Err(ApiError::new("batch_expired", StatusCode::GONE));
    }
    let etag = BatchRevision(batch.get("revision")).etag(id);
    let unchanged = req
        .headers()
        .get_all("If-None-Match")
        .filter_map(|h| h.to_str().ok())
        .any(|h| {
            h.split(',').any(|v| {
                let v = v.trim();
                v == "*" || v.strip_prefix("W/").unwrap_or(v) == etag
            })
        });
    let mut response = HttpResponse::build(if unchanged {
        StatusCode::NOT_MODIFIED
    } else {
        StatusCode::OK
    });
    response
        .insert_header(("ETag", etag))
        .insert_header(("Cache-Control", "no-store"))
        .insert_header(("Location", format!("/v1/batches/{id}")));
    if matches!(batch.get::<&str, _>("state"), "queued" | "running") {
        response.insert_header(("Retry-After", "2"));
    }
    if unchanged {
        return Ok(response.finish());
    }
    let files: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('file_id',id,'name',name,'state',state,'attempt',attempt) || CASE WHEN stage IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('progress',jsonb_build_object('stage',stage)) END || CASE WHEN next_retry_at IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('next_retry_at',next_retry_at) END || CASE WHEN state='completed' THEN jsonb_build_object('document_id',document_id) WHEN state='failed' THEN jsonb_build_object('error',error) ELSE '{}'::jsonb END FROM batch_files WHERE batch_id=$1 ORDER BY ordinal")
        .bind(id).fetch_all(&mut *tx).await.map_err(|_| ApiError::unavailable())?;
    let mut prefix = batch.get::<Value, _>("body").to_string();
    prefix.pop();
    prefix.push_str(",\"files\":[");
    let body = stream::try_unfold(
        PollResponse {
            tx,
            files: files.into(),
            prefix: Some(prefix),
            current: false,
            first_file: true,
            first_match: true,
            finished: false,
        },
        |mut state| async move {
            if let Some(prefix) = state.prefix.take() {
                return Ok(Some((Bytes::from(prefix), state)));
            }
            if state.current {
                let rows: Vec<Value> = sqlx::query_scalar("FETCH FORWARD 128 FROM polling_matches")
                    .fetch_all(&mut *state.tx)
                    .await
                    .map_err(actix_web::error::ErrorInternalServerError)?;
                if !rows.is_empty() {
                    let mut chunk = String::new();
                    for row in rows {
                        if !state.first_match {
                            chunk.push(',');
                        }
                        state.first_match = false;
                        chunk.push_str(&row.to_string());
                    }
                    return Ok(Some((Bytes::from(chunk), state)));
                }
                sqlx::query("CLOSE polling_matches")
                    .execute(&mut *state.tx)
                    .await
                    .map_err(actix_web::error::ErrorInternalServerError)?;
                state.current = false;
                return Ok(Some((Bytes::from_static(b"]}"), state)));
            }
            if let Some(file) = state.files.pop_front() {
                let mut chunk = if state.first_file {
                    String::new()
                } else {
                    ",".into()
                };
                state.first_file = false;
                let completed = file["state"] == "completed";
                if completed {
                    let file_id = file["file_id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .ok_or_else(|| {
                            actix_web::error::ErrorInternalServerError("invalid persisted file id")
                        })?;
                    sqlx::query("DECLARE polling_matches NO SCROLL CURSOR FOR SELECT jsonb_build_object('document_id',document_id,'score',score,'text_similarity',text_similarity,'text_score_method','exact','metadata_similarity',metadata_similarity) FROM match_results WHERE file_id=$1 ORDER BY score DESC, document_id")
                    .bind(file_id).execute(&mut *state.tx).await.map_err(actix_web::error::ErrorInternalServerError)?;
                    let mut value = file.to_string();
                    value.pop();
                    chunk.push_str(&value);
                    chunk.push_str(",\"matches\":[");
                    state.current = true;
                    state.first_match = true;
                } else {
                    chunk.push_str(&file.to_string());
                }
                return Ok(Some((Bytes::from(chunk), state)));
            }
            if !state.finished {
                state.finished = true;
                return Ok(Some((Bytes::from_static(b"]}"), state)));
            }
            Ok::<_, actix_web::Error>(None)
        },
    );
    Ok(response.content_type("application/json").streaming(body))
}
