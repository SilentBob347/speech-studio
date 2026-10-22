use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub(crate) fn refuse(status: StatusCode, code: &str, detail: impl Into<String>) -> Response {
    (status, Json(json!({ "error": code, "detail": detail.into() }))).into_response()
}

pub(crate) fn joined<T>(r: Result<T, tokio::task::JoinError>) -> Result<T, Response> {
    r.map_err(|e| refuse(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string()))
}
