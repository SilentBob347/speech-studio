use super::files::open_folder;
use crate::{app, setup, AppState};
use crate::integrations::ytdlp;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use serde_json::json;
use serde_json::Value;
use std::path::Path;

#[cfg(test)]
mod tests;

/// Ответ на отказ закачки/удаления: HTTP-статус по коду + {code, detail}.
pub(super) fn dl_error_response(e: setup::DlError) -> Response {
    let status = match e.code {
        "busy" => StatusCode::CONFLICT,
        "disk_space" => StatusCode::INSUFFICIENT_STORAGE,
        "nothing_to_download" | "unknown_component" | "not_removable" | "no_ids" => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({ "code": e.code, "detail": e.detail }))).into_response()
}

pub(super) fn join_failure(e: tokio::task::JoinError) -> Response {
    internal_error(e.to_string())
}

pub(super) fn body_ids(body: &Value) -> Vec<String> {
    body.get("ids")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

/// Полный статус «Первого запуска» с фоновой закачкой (ФС-обход и первая проба драйвера — в блокирующем пуле).
/// Ошибка — текст для ответа 500 (internal_error).
pub(super) async fn full_setup_status(root: std::path::PathBuf, downloads: &super::Downloads) -> Result<Value, String> {
    let mut status = tokio::task::spawn_blocking(move || app::engine_setup::setup_status(&root))
        .await
        .map_err(|e| e.to_string())?;
    status.active = downloads.active();
    serde_json::to_value(status).map_err(|e| e.to_string())
}

pub(super) fn internal_error(detail: String) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "code": "internal", "detail": detail })),
    )
        .into_response()
}

/// GET /setup/status — статус каждого компонента (installed/missing/размер/место), папка моделей и свободное
/// место, видеокарта против CUDA 13 и фоновая закачка (active). Читает только диск — безопасно до закачки.
pub(super) async fn setup_status(State(st): State<AppState>) -> Response {
    match full_setup_status(st.repo_root.clone(), &st.downloads).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal_error(e),
    }
}

/// POST /setup/download {ids:[...]} — запустить фоновую закачку (мимо GPU-очереди): {download}. Идущая
/// закачка — 409 busy, нехватка места — 507 disk_space. Прогресс — в GET /setup/status (active).
pub(super) async fn setup_download(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let ids = body_ids(&body);
    if ids.is_empty() {
        return dl_error_response(setup::DlError::new("no_ids", t!("setup-no-ids")));
    }
    let dl = st.downloads.clone();
    match tokio::task::spawn_blocking(move || dl.start(ids)).await {
        Ok(Ok(job)) => Json(json!({ "download": job })).into_response(),
        Ok(Err(e)) => dl_error_response(e),
        Err(e) => join_failure(e),
    }
}

/// POST /setup/cancel — пауза фоновой закачки: скачанное остаётся и докачивается следующим /setup/download.
pub(super) async fn setup_cancel(State(st): State<AppState>) -> Json<Value> {
    Json(json!({ "paused": st.downloads.pause() }))
}

/// POST /setup/remove {ids:[...]} — удалить скачанные компоненты: {removed, freedBytes, errors, status}.
pub(super) async fn setup_remove(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let ids = body_ids(&body);
    if ids.is_empty() {
        return dl_error_response(setup::DlError::new("no_ids", t!("setup-no-ids")));
    }
    let root = st.repo_root.clone();
    let dl = st.downloads.clone();
    let res = tokio::task::spawn_blocking(move || {
        let mut report = app::engine_setup::remove_components(&root, &ids)?;
        dl.forget_covering(&ids);
        if ids.iter().any(|id| id == ytdlp::COMPONENT) {
            report.errors.extend(ytdlp::remove_updates(&root));
        }
        Ok::<_, setup::DlError>(report)
    })
    .await;
    let report = match res {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return dl_error_response(e),
        Err(e) => return join_failure(e),
    };
    let status = match full_setup_status(st.repo_root.clone(), &st.downloads).await {
        Ok(v) => v,
        Err(e) => return internal_error(e),
    };
    Json(json!({ "removed": report.removed, "freedBytes": report.freed_bytes, "errors": report.errors, "status": status })).into_response()
}

/// POST /setup/open-models — открыть папку моделей в проводнике: {path}.
pub(super) async fn setup_open_models(State(st): State<AppState>) -> Response {
    let dir = st.repo_root.join("models");
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| open_folder(&dir)) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "open_failed", "detail": format!("{}: {e}", dir.display()) })),
        )
            .into_response();
    }
    Json(json!({ "path": dir.to_string_lossy() })).into_response()
}

/// POST /setup/browse — открыть нативный диалог выбора папки и импортировать оттуда готовые веса (без
/// докачки): {picked, imported, files, errors, status}.
pub(super) async fn setup_browse(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let root = st.repo_root.clone();
    let only = body.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
    let res = tokio::task::spawn_blocking(move || {
        let title = if only.is_some() {
            t!("setup-pick-model-files")
        } else {
            t!("setup-pick-models-folder")
        };
        rfd::FileDialog::new()
            .set_title(title)
            .pick_folder()
            .map(|d| setup::import_from_dir(&root, &d, only.as_deref(), true, false))
    })
    .await;
    let report = match res {
        Ok(r) => r,
        Err(e) => return join_failure(e),
    };
    let status = match full_setup_status(st.repo_root.clone(), &st.downloads).await {
        Ok(v) => v,
        Err(e) => return internal_error(e),
    };
    let picked = report.is_some();
    let report = report.unwrap_or_default();
    Json(json!({ "picked": picked, "imported": report.imported, "files": report.files, "errors": report.errors, "status": status }))
        .into_response()
}

/// POST /setup/import {path, id?} — импортировать готовые веса из заданной папки (без диалога):
/// {imported, files, errors, status}.
pub(super) async fn setup_import(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let path = body.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if path.is_empty() || !Path::new(&path).is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "code": "bad_path", "detail": t!("setup-no-folder", path = path.clone()) })),
        )
            .into_response();
    }
    let only = body.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
    let root = st.repo_root.clone();
    let report =
        match tokio::task::spawn_blocking(move || setup::import_from_dir(&root, Path::new(&path), only.as_deref(), true, false)).await {
            Ok(r) => r,
            Err(e) => return join_failure(e),
        };
    let status = match full_setup_status(st.repo_root.clone(), &st.downloads).await {
        Ok(v) => v,
        Err(e) => return internal_error(e),
    };
    Json(json!({ "imported": report.imported, "files": report.files, "errors": report.errors, "status": status })).into_response()
}
