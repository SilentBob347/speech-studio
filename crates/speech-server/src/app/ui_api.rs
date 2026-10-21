use crate::app::spa;
use crate::{i18n, AppState};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use serde_json::json;
use serde_json::Value;

use super::runtime::UI_LANG_FILE;

#[cfg(test)]
mod tests;

pub(super) async fn spa_fallback(State(st): State<AppState>, uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    spa::serve_spa(st.web_root.as_deref(), path).await
}

/// The language the window shows, reported by the window so the server speaks it too, now and after a restart.
pub(super) async fn set_ui_lang(State(st): State<AppState>, Json(body): Json<Value>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let lang = body.get("lang").and_then(Value::as_str).unwrap_or_default();
    i18n::set_language(lang).map_err(|detail| (StatusCode::BAD_REQUEST, Json(json!({ "code": "bad_lang", "detail": detail }))))?;
    let file = st.repo_root.join(UI_LANG_FILE);
    std::fs::write(&file, lang).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "write_failed", "detail": format!("{}: {e}", file.display()) })),
        )
    })?;
    Ok(Json(json!({ "lang": lang })))
}
