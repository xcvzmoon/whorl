use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use serde_json::json;
use std::fmt;
use uuid::Uuid;

#[derive(Debug)]
pub(crate) struct ApiError {
    pub(crate) code: &'static str,
    pub(crate) status: StatusCode,
    pub(crate) retry: Option<u32>,
}
impl ApiError {
    pub(crate) fn new(code: &'static str, status: StatusCode) -> Self {
        Self {
            code,
            status,
            retry: None,
        }
    }
    pub(crate) fn unavailable() -> Self {
        Self {
            code: "storage_unavailable",
            status: StatusCode::SERVICE_UNAVAILABLE,
            retry: Some(2),
        }
    }
}
impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code)
    }
}
impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        self.status
    }
    fn error_response(&self) -> HttpResponse {
        let mut response = HttpResponse::build(self.status);
        if let Some(retry) = self.retry {
            response.insert_header(("Retry-After", retry.to_string()));
        }
        response.json(json!({"error":{"code":self.code,"message":self.code.replace('_', " "),"retryable":self.retry.is_some(),"request_id":Uuid::new_v4()}}))
    }
}
