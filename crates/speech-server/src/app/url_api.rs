use super::url_import::FetchRequest;
use crate::integrations::url_import::{self as download, Cookies};
use crate::integrations::ytdlp;
use crate::integrations::ytdlp::{Quality, UrlError};
use crate::AppState;
use axum::extract::{Path as AxPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
fn status_of(code: &str) -> StatusCode {
    match code {
        "bad_url" | "bad_quality" | "cookies_invalid" => StatusCode::BAD_REQUEST,
        "not_found" => StatusCode::NOT_FOUND,
        "tool_missing" | "ffmpeg_missing" | "busy" | "running" => StatusCode::CONFLICT,
        "network" | "proxy" | "rate_limited" => StatusCode::BAD_GATEWAY,
        "disk_space" => StatusCode::INSUFFICIENT_STORAGE,
        "io" => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    }
}

fn refuse(e: UrlError) -> Response {
    (status_of(e.code), Json(e.to_json())).into_response()
}

fn joined<T>(r: Result<Result<T, UrlError>, tokio::task::JoinError>) -> Result<T, UrlError> {
    r.map_err(|e| UrlError::new("io", e.to_string()))?
}

fn body_cookies(body: &Value) -> Option<Cookies> {
    let text = |k: &str| {
        body.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    match (
        text("cookies"),
        body.get("cookies_text").and_then(Value::as_str).filter(|s| !s.trim().is_empty()),
    ) {
        (Some(path), _) => Some(Cookies::Path(PathBuf::from(path))),
        (None, Some(t)) => Some(Cookies::Text(t.to_string())),
        (None, None) => None,
    }
}

/// GET /url/tool — версия yt-dlp, закреплённая и последняя на GitHub, идёт ли обновление.
pub async fn tool_status(State(st): State<AppState>) -> Response {
    let root = st.repo_root.clone();
    match tokio::task::spawn_blocking(move || ytdlp::status(&root)).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => refuse(UrlError::new("io", e.to_string())),
    }
}

/// POST /url/tool/update — проверить релизы сейчас и поставить новый yt-dlp рядом (в фоне): {started, tool}.
pub async fn tool_update(State(st): State<AppState>) -> Response {
    let root = st.repo_root.clone();
    let res = tokio::task::spawn_blocking(move || {
        let before = ytdlp::status(&root);
        if !before.installed {
            return Err(UrlError::new("tool_missing", t!("url-ytdlp-missing")));
        }
        let started = ytdlp::start_update(&root);
        Ok(json!({ "started": started, "tool": ytdlp::status(&root) }))
    })
    .await;
    match joined(res) {
        Ok(v) => Json(v).into_response(),
        Err(e) => refuse(e),
    }
}

#[derive(Deserialize)]
pub struct ProbeQuery {
    url: Option<String>,
    cookies: Option<String>,
}

/// GET /url/probe?url=&cookies= — что за видео по ссылке (cookies — путь к cookies.txt).
pub async fn probe_get(State(st): State<AppState>, Query(q): Query<ProbeQuery>) -> Response {
    let cookies = q
        .cookies
        .filter(|c| !c.trim().is_empty())
        .map(|c| Cookies::Path(PathBuf::from(c.trim())));
    run_probe(st, q.url.unwrap_or_default(), cookies).await
}

/// POST /url/probe {url, cookies?, cookies_text?} — то же, cookies.txt содержимым (окно не знает путей файлов).
pub async fn probe_post(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let url = body.get("url").and_then(Value::as_str).unwrap_or_default().to_string();
    run_probe(st, url, body_cookies(&body)).await
}

async fn run_probe(st: AppState, url: String, cookies: Option<Cookies>) -> Response {
    let root = st.repo_root.clone();
    let fetch_root = st.workspace.join(".fetch");
    match joined(tokio::task::spawn_blocking(move || download::probe(&root, &fetch_root, &url, cookies)).await) {
        Ok(v) => Json(v).into_response(),
        Err(e) => refuse(e),
    }
}

/// POST /projects/from_url {url, quality, subs_lang?, cookies?, cookies_text?} — начать загрузку в новый проект: {fetch}.
/// Идёт в фоне мимо очереди джоб; готовый проект — fetch.pid в GET /url/fetches/{id}.
pub async fn create_from_url(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let quality_text = body.get("quality").and_then(Value::as_str).unwrap_or("best");
    let Some(quality) = Quality::parse(quality_text) else {
        return refuse(UrlError::new(
            "bad_quality",
            t!("url-bad-quality", quality = quality_text.to_string()),
        ));
    };
    let req = FetchRequest {
        url: body.get("url").and_then(Value::as_str).unwrap_or_default().to_string(),
        quality,
        subs_lang: body
            .get("subs_lang")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        cookies: body_cookies(&body),
    };
    let fetches = st.fetches.clone();
    match joined(tokio::task::spawn_blocking(move || fetches.start(req)).await) {
        Ok(f) => Json(json!({ "fetch": f })).into_response(),
        Err(e) => refuse(e),
    }
}

/// GET /url/fetches — загрузки по ссылке, новые первыми.
pub async fn fetches_list(State(st): State<AppState>) -> Json<Value> {
    Json(json!({ "fetches": st.fetches.list() }))
}

/// GET /url/fetches/{id} — одна загрузка.
pub async fn fetch_get(State(st): State<AppState>, AxPath(id): AxPath<String>) -> Response {
    match st.fetches.get(&id) {
        Some(f) => Json(f).into_response(),
        None => refuse(UrlError::new("not_found", t!("url-no-fetch", id = id.to_string()))),
    }
}

/// POST /url/fetches/{id}/cancel — остановить загрузку (недокачанное удаляется).
pub async fn fetch_cancel(State(st): State<AppState>, AxPath(id): AxPath<String>) -> Response {
    match st.fetches.cancel(&id) {
        Ok(f) => Json(json!({ "fetch": f })).into_response(),
        Err(e) => refuse(e),
    }
}

/// POST /url/fetches/{id}/resume — продолжить прерванную или упавшую загрузку с места.
pub async fn fetch_resume(State(st): State<AppState>, AxPath(id): AxPath<String>) -> Response {
    let fetches = st.fetches.clone();
    match joined(tokio::task::spawn_blocking(move || fetches.resume(&id)).await) {
        Ok(f) => Json(json!({ "fetch": f })).into_response(),
        Err(e) => refuse(e),
    }
}

/// DELETE /url/fetches/{id} — убрать загрузку из списка вместе с недокачанным.
pub async fn fetch_forget(State(st): State<AppState>, AxPath(id): AxPath<String>) -> Response {
    let fetches = st.fetches.clone();
    match joined(tokio::task::spawn_blocking(move || fetches.forget(&id).map(|_| ())).await) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => refuse(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Request};
    use tower::ServiceExt;

    async fn send(app: &axum::Router, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "127.0.0.1:8797")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn routes_preserve_saved_fetch_json_and_error_codes() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("workspace/.fetch");
        std::fs::create_dir_all(&dir).unwrap();
        let legacy = json!({
            "id": "url1", "url": "https://example.com/v", "quality": "720", "subsLang": "en", "cookies": false,
            "status": "completed", "phase": "done", "title": "clip", "duration": 12.5,
            "downloaded": 10, "total": 10, "speedBps": 0, "etaS": null,
            "pid": "project1", "subsImported": false, "warning": "subs_failed", "warningDetail": "rate_limited",
            "errorCode": null, "error": null, "hint": null, "toolVersion": "2026.08.19", "startedAt": 1, "updatedAt": 2
        });
        std::fs::write(dir.join("fetches.json"), json!({ "fetches": [legacy] }).to_string()).unwrap();
        let app = crate::build_router(AppState::new(root.path()));
        assert_eq!(
            send(&app, "GET", "/url/fetches", Value::Null).await,
            (StatusCode::OK, json!({ "fetches": [legacy] }))
        );
        assert_eq!(
            send(&app, "GET", "/url/fetches/url1", Value::Null).await,
            (StatusCode::OK, legacy.clone())
        );
        assert_eq!(
            send(&app, "POST", "/url/fetches/url1/cancel", json!({})).await,
            (StatusCode::OK, json!({ "fetch": legacy }))
        );
        assert_eq!(
            send(&app, "DELETE", "/url/fetches/url1", Value::Null).await,
            (StatusCode::OK, json!({ "ok": true }))
        );
        for (method, uri, body, expected, code) in [
            ("GET", "/url/fetches/missing", Value::Null, StatusCode::NOT_FOUND, "not_found"),
            (
                "POST",
                "/projects/from_url",
                json!({ "url": "https://example.com/v", "quality": "invalid" }),
                StatusCode::BAD_REQUEST,
                "bad_quality",
            ),
            (
                "POST",
                "/projects/from_url",
                json!({ "url": "ftp://example.com/v" }),
                StatusCode::BAD_REQUEST,
                "bad_url",
            ),
            (
                "POST",
                "/projects/from_url",
                json!({ "url": "https://example.com/v" }),
                StatusCode::CONFLICT,
                "tool_missing",
            ),
            ("POST", "/url/probe", json!({ "url": "" }), StatusCode::BAD_REQUEST, "bad_url"),
        ] {
            let (status, error) = send(&app, method, uri, body).await;
            assert_eq!(status, expected, "{uri}: {error}");
            assert_eq!(error["error"], code);
            assert!(error["detail"].as_str().is_some_and(|d| !d.is_empty()));
            assert!(error["hint"].is_string());
        }
        assert_eq!(
            send(&app, "GET", "/url/fetches", Value::Null).await,
            (StatusCode::OK, json!({ "fetches": [] }))
        );
    }

    #[test]
    fn errors_answer_with_the_right_status() {
        assert_eq!(status_of("bad_url"), StatusCode::BAD_REQUEST);
        assert_eq!(status_of("tool_missing"), StatusCode::CONFLICT);
        assert_eq!(status_of("geo_blocked"), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(status_of("network"), StatusCode::BAD_GATEWAY);
        let v = UrlError::new("login_required", "Sign in").to_json();
        assert_eq!(
            (v["error"].as_str(), v["detail"].as_str()),
            (Some("login_required"), Some("Sign in"))
        );
        assert!(v["hint"].as_str().unwrap().contains("cookies.txt"));
    }
}
