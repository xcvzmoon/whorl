use super::errors::ApiError;
use actix_multipart::Multipart;
use actix_web::{HttpRequest, HttpResponse, http::StatusCode, web};
use futures_util::{StreamExt, TryStreamExt};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio_util::io::StreamReader;
use uuid::Uuid;
use whorl::{
    queue::JobQueue,
    service::admission::{AdmissionError, AdmissionLease, ClientQuota},
    storage::{StoredObjectRef, UploadStore},
};

const FILE: u64 = 20 * 1024 * 1024;
const TOTAL: u64 = 5 * FILE;
const RAW: u64 = TOTAL + 1024 * 1024;

pub(crate) struct Submission {
    pub(crate) pool: PgPool,
    pub(crate) store: UploadStore,
    pub(crate) queue: JobQueue,
    pub(crate) version: String,
    pub(crate) trusted_proxy_only: bool,
}
pub(crate) fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/v1/checks", web::post().to(check))
        .route("/v1/ingests", web::post().to(ingest))
        .route("/v1/batches/{id}", web::get().to(super::status::status))
        .route("/health/live", web::get().to(super::health::live))
        .route("/health/ready", web::get().to(super::health::ready));
}
async fn check(
    req: HttpRequest,
    body: web::Payload,
    state: web::Data<Submission>,
) -> Result<HttpResponse, ApiError> {
    submit(req, body, state, "check").await
}
async fn ingest(
    req: HttpRequest,
    body: web::Payload,
    state: web::Data<Submission>,
) -> Result<HttpResponse, ApiError> {
    submit(req, body, state, "ingest").await
}
fn bad(code: &'static str) -> ApiError {
    ApiError::new(code, StatusCode::BAD_REQUEST)
}
fn large(code: &'static str) -> ApiError {
    ApiError::new(code, StatusCode::PAYLOAD_TOO_LARGE)
}

struct Structure {
    marker: Vec<u8>,
    matched: usize,
    headers: bool,
    started: bool,
    ended: bool,
    count: usize,
    tail: u32,
}
impl Structure {
    fn new(boundary: &str) -> Self {
        Self {
            marker: format!("\r\n--{boundary}").into_bytes(),
            matched: 2,
            headers: false,
            started: false,
            ended: false,
            count: 0,
            tail: 0,
        }
    }
    fn check(&mut self, chunk: &[u8]) -> Result<(), actix_web::error::PayloadError> {
        for &byte in chunk {
            if self.ended {
                continue;
            }
            if self.headers {
                self.count += 1;
                self.tail = (self.tail << 8) | u32::from(byte);
                if self.count == 2 && self.tail == 0x2d2d {
                    self.ended = true;
                }
                if self.count > 8192 {
                    return Err(actix_web::error::PayloadError::Overflow);
                }
                if self.tail == 0x0d0a0d0a {
                    self.headers = false;
                    self.count = 0;
                }
            } else {
                if !self.started {
                    self.count += 1;
                    if self.count > 8192 {
                        return Err(actix_web::error::PayloadError::Overflow);
                    }
                }
                if byte == self.marker[self.matched] {
                    self.matched += 1;
                } else {
                    self.matched = usize::from(byte == b'\r');
                }
                if self.matched == self.marker.len() {
                    self.started = true;
                    self.headers = true;
                    self.count = 0;
                    self.tail = 0;
                    self.matched = 0;
                }
            }
        }
        Ok(())
    }
}

struct Cleanup {
    lease: Option<AdmissionLease>,
    pool: PgPool,
    store: UploadStore,
    objects: RefCell<Vec<StoredObjectRef>>,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(lease) = self.lease.take() {
            let pool = self.pool.clone();
            let store = self.store.clone();
            let objects = self.objects.take();
            tokio::spawn(async move {
                for object in objects {
                    let _ = store.discard(&object).await;
                }
                let _ = lease.release(&pool).await;
            });
        }
    }
}

struct File {
    id: Uuid,
    name: String,
    object: Option<StoredObjectRef>,
    error: Option<Value>,
}
async fn submit(
    req: HttpRequest,
    body: web::Payload,
    state: web::Data<Submission>,
    mode: &str,
) -> Result<HttpResponse, ApiError> {
    let client = req
        .headers()
        .get("X-Client-Id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let lease = AdmissionLease::reserve(
        &state.pool,
        client,
        state.trusted_proxy_only,
        ClientQuota {
            files: 5,
            bytes: TOTAL,
            lease_seconds: 120,
            retry_after: 2,
        },
    )
    .await
    .map_err(|error| match error {
        AdmissionError::Identity => {
            ApiError::new("client_identity_required", StatusCode::UNAUTHORIZED)
        }
        AdmissionError::ClientQuota { retry_after } => ApiError {
            code: "client_quota_exceeded",
            status: StatusCode::TOO_MANY_REQUESTS,
            retry: Some(retry_after),
        },
        AdmissionError::Unavailable { code, retry_after } => ApiError {
            code,
            status: StatusCode::SERVICE_UNAVAILABLE,
            retry: Some(retry_after),
        },
        _ => ApiError::unavailable(),
    })?;
    let mut cleanup = Cleanup {
        lease: Some(lease),
        pool: state.pool.clone(),
        store: state.store.clone(),
        objects: RefCell::new(Vec::new()),
    };
    let mut files = Vec::new();
    let result = tokio::time::timeout(
        Duration::from_secs(110),
        receive(&req, body, &state, &cleanup, client, mode, &mut files),
    )
    .await
    .unwrap_or_else(|_| Err(bad("malformed_multipart")));
    if result.is_err() {
        let objects = cleanup.objects.take();
        for object in &objects {
            let _ = state.store.discard(object).await;
        }
    }
    if let Some(lease) = cleanup.lease.as_ref() {
        let _ = lease.release(&state.pool).await;
    }
    cleanup.lease.take();
    result
}

async fn receive(
    req: &HttpRequest,
    body: web::Payload,
    state: &Submission,
    cleanup: &Cleanup,
    client: &str,
    mode: &str,
    files: &mut Vec<File>,
) -> Result<HttpResponse, ApiError> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| bad("malformed_multipart"))?;
    let mime: mime::Mime = content_type
        .parse()
        .map_err(|_| bad("malformed_multipart"))?;
    let boundary = mime
        .get_param("boundary")
        .ok_or_else(|| bad("malformed_multipart"))?
        .as_str();
    if mime.essence_str() != "multipart/form-data" || boundary.is_empty() || boundary.len() > 70 {
        return Err(bad("malformed_multipart"));
    }
    let wire_error = Rc::new(Cell::new(None));
    let error = wire_error.clone();
    let body = Rc::new(RefCell::new(body));
    let source = body.clone();
    let raw = Rc::new(Cell::new(0u64));
    let counted = raw.clone();
    let mut structure = Structure::new(boundary);
    let stream = futures_util::stream::poll_fn(move |cx| source.borrow_mut().poll_next_unpin(cx))
        .map(move |chunk| {
            let chunk = chunk?;
            counted.set(counted.get() + chunk.len() as u64);
            if counted.get() > RAW {
                error.set(Some("body_too_large"));
                return Err(actix_web::error::PayloadError::Overflow);
            }
            structure.check(&chunk)?;
            Ok(chunk)
        });
    let mut multipart = Multipart::new(req.headers(), stream);
    let total = Rc::new(Cell::new(0u64));
    while let Some(part) = multipart.next().await {
        let field = part.map_err(|_| {
            wire_error.get().map_or_else(
                || bad("malformed_multipart"),
                |code| {
                    if code == "body_too_large" {
                        large(code)
                    } else {
                        bad(code)
                    }
                },
            )
        })?;
        if field.name() != Some("files") {
            return Err(bad("unknown_part"));
        }
        if files.len() == 5 {
            return Err(bad("too_many_files"));
        }
        let name = field
            .content_disposition()
            .and_then(|d| d.get_filename())
            .unwrap_or("")
            .to_owned();
        let mut field = field.fuse();
        let bytes = Rc::new(Cell::new(0u64));
        let file_bytes = bytes.clone();
        let combined = total.clone();
        let malformed = Rc::new(Cell::new(false));
        let malformed_stream = malformed.clone();
        let mapped = (&mut field).map(move |chunk| {
            let chunk = chunk.map_err(|_| {
                malformed_stream.set(true);
                std::io::Error::other("multipart")
            })?;
            file_bytes.set(file_bytes.get() + chunk.len() as u64);
            combined.set(combined.get() + chunk.len() as u64);
            if file_bytes.get() > FILE || combined.get() > TOTAL {
                return Err(std::io::Error::other("limit"));
            }
            Ok(chunk)
        });
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ApiError::unavailable())?
            .as_secs() as i64
            + 3600;
        let result = state
            .store
            .upload_admitted(
                cleanup.lease.as_ref().expect("active request lease"),
                expiry,
                StreamReader::new(mapped),
            )
            .await;
        if let Ok(object) = &result {
            cleanup.objects.borrow_mut().push(object.clone());
        }
        let mut file = File {
            id: Uuid::new_v4(),
            name,
            object: result.as_ref().ok().cloned(),
            error: None,
        };
        if malformed.get() {
            return Err(if wire_error.get() == Some("body_too_large") {
                large("body_too_large")
            } else {
                bad("malformed_multipart")
            });
        }
        if result.is_err()
            && !(bytes.get() == 0 && matches!(result, Err(whorl::storage::StorageError::Invalid)))
            && bytes.get() <= FILE
            && total.get() <= TOTAL
        {
            return Err(ApiError::unavailable());
        }
        while let Some(chunk) = field.try_next().await.map_err(|_| {
            if wire_error.get() == Some("body_too_large") {
                large("body_too_large")
            } else {
                bad("malformed_multipart")
            }
        })? {
            bytes.set(bytes.get() + chunk.len() as u64);
            total.set(total.get() + chunk.len() as u64);
            if total.get() > TOTAL {
                return Err(large("total_too_large"));
            }
        }
        if total.get() > TOTAL {
            return Err(large("total_too_large"));
        }
        if bytes.get() == 0 || bytes.get() > FILE {
            let code = if bytes.get() == 0 {
                "empty_file"
            } else {
                "file_too_large"
            };
            file.error =
                Some(json!({"code":code,"message":code.replace('_', " "),"retryable":false}));
        }
        files.push(file);
    }
    drop(multipart);
    let mut rest = futures_util::stream::poll_fn(move |cx| body.borrow_mut().poll_next_unpin(cx));
    while let Some(chunk) = rest.next().await {
        let chunk = chunk.map_err(|_| bad("malformed_multipart"))?;
        raw.set(raw.get() + chunk.len() as u64);
        if raw.get() > RAW {
            return Err(large("body_too_large"));
        }
    }
    if files.is_empty() {
        return Err(bad("no_files"));
    }
    persist(state, client, mode, files).await
}

async fn persist(
    state: &Submission,
    client: &str,
    mode: &str,
    files: &[File],
) -> Result<HttpResponse, ApiError> {
    let batch = Uuid::new_v4();
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|_| ApiError::unavailable())?;
    sqlx::query("INSERT INTO batches(id,client_id,mode,signature_version,policy,deadline_at) VALUES ($1,$2,$3,$4,$5,clock_timestamp()+interval '1 hour')")
        .bind(batch).bind(client).bind(mode).bind(&state.version).bind(json!({"kind":"text_only","threshold":0.75})).execute(&mut *tx).await.map_err(|_| ApiError::unavailable())?;
    for (ordinal, file) in files.iter().enumerate() {
        if let Some(object) = &file.object {
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
                .bind(&object.key)
                .execute(&mut *tx)
                .await
                .map_err(|_| ApiError::unavailable())?;
        }
        sqlx::query("INSERT INTO batch_files(id,batch_id,client_id,ordinal,name,state,error,source_object_key,source_bytes,deadline_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,clock_timestamp()+interval '1 hour')")
            .bind(file.id).bind(batch).bind(client).bind(ordinal as i16).bind(&file.name).bind(if file.error.is_some() { "failed" } else { "queued" }).bind(&file.error).bind(file.object.as_ref().map(|o| &o.key)).bind(file.object.as_ref().map_or(0, |o| o.bytes)).execute(&mut *tx).await.map_err(|_| ApiError::unavailable())?;
        if file.object.is_some() {
            state
                .queue
                .enqueue(&mut tx, client, file.id)
                .await
                .map_err(|_| ApiError::unavailable())?;
        }
    }
    let failed = files.iter().all(|f| f.error.is_some());
    if failed {
        sqlx::query("UPDATE batches SET state='failed',completed_at=clock_timestamp(),expires_at=clock_timestamp()+interval '24 hours',tombstone_until=clock_timestamp()+interval '48 hours' WHERE id=$1").bind(batch).execute(&mut *tx).await.map_err(|_| ApiError::unavailable())?;
    }
    tx.commit().await.map_err(|_| ApiError::unavailable())?;
    let url = format!("/v1/batches/{batch}");
    let files: Vec<_> = files.iter().map(|f| {
        let mut value = json!({"file_id":f.id,"name":f.name,"state":if f.error.is_some() { "failed" } else { "queued" }});
        if let Some(error) = &f.error { value["error"] = error.clone(); } value
    }).collect();
    Ok(HttpResponse::Accepted().insert_header(("Location", url.clone())).insert_header(("Retry-After", "2")).json(json!({"batch_id":batch,"mode":mode,"state":if failed { "failed" } else { "queued" },"status_url":url,"files":files})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_structure_is_bounded_across_chunk_splits() {
        let mut structure = Structure::new("b");
        for byte in
            b"--b\r\nContent-Disposition: form-data; name=\"files\"\r\n\r\ncontent\r\n--b--\r\n"
        {
            structure.check(&[*byte]).unwrap();
        }
        assert!(structure.ended);
        let mut structure = Structure::new("b");
        structure.check(b"--b\r\n").unwrap();
        assert!(structure.check(&vec![b'x'; 8192]).is_err());
        let mut structure = Structure::new("b");
        assert!(structure.check(&vec![b'x'; 8193]).is_err());
    }
}
