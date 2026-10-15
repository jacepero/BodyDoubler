// src/error.rs — the one error shape: an HTTP status plus a message, rendered
// as JSON {"error": ...}. Return Err(err(403, "...")) from any handler.

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

pub fn err(status: u16, message: impl Into<String>) -> ApiError {
    ApiError {
        status: StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        message: message.into(),
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            eprintln!("[kernel] {}", self.message);
        }
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

pub fn bearer(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(t) = v.strip_prefix("Bearer ") {
            return Some(t.to_string());
        }
    }
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

pub fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

// Constant-time compare over SHA-256 digests — no content/length timing leak.
pub fn timing_eq(a: &str, b: &str) -> bool {
    let da = Sha256::digest(a.as_bytes());
    let db = Sha256::digest(b.as_bytes());
    da.iter()
        .zip(db.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}
