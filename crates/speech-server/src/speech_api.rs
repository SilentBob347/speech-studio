//! Маршруты проектов: создать (Speech из текста), список, проект со статусами блоков, запись целиком (ревизии Dub:
//! 409 на чужую правку), удаление в корзину системы, синтез и экспорт задачами, звук дублей и файлов экспорта.

use std::path::Path;

use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::speech_jobs::{self, ExportOpts, Scope, SpeechPaths};
use crate::speech_project::{self as sp, SpeechProject};
use crate::{jobs, AppState};

fn refuse(status: StatusCode, code: &str, detail: impl Into<String>) -> Response {
    (status, Json(json!({ "error": code, "detail": detail.into() }))).into_response()
}

fn joined<T>(r: Result<T, tokio::task::JoinError>) -> Result<T, Response> {
    r.map_err(|e| refuse(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string()))
}

pub fn paths(st: &AppState) -> SpeechPaths {
    SpeechPaths {
        repo_root: st.repo_root.clone(),
        workspace: st.workspace.clone(),
        voices_dir: st.voices_dir.clone(),
        models_root: st.models_root.clone(),
        higgs_dll: st.higgs_dll.clone(),
        threads: st.opts.num_threads,
    }
}

fn shown(p: &SpeechPaths, project: &SpeechProject) -> Value {
    sp::with_status(project, &speech_jobs::keys(p, project))
}

/// Карточка проекта для списка: вид, имя, время, число блоков и готовых.
fn summary(p: &SpeechPaths, dir: &Path) -> Option<Value> {
    let pid = dir.file_name()?.to_str()?.to_string();
    let text = std::fs::read_to_string(dir.join("project.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let mtime = std::fs::metadata(dir.join("project.json")).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let kind = v.get("kind").and_then(Value::as_str).unwrap_or("").to_string();
    let mut card = json!({ "pid": pid, "kind": kind, "mtime": mtime });
    if kind == sp::KIND {
        let project: SpeechProject = serde_json::from_value(v).ok()?;
        let keys = speech_jobs::keys(p, &project);
        let total = project.blocks().filter(|b| !b.hidden && !b.text.trim().is_empty()).count();
        let ready = project.blocks().filter(|b| sp::status(b, keys.get(&b.id).map(String::as_str).unwrap_or("")) == "ready").count();
        card["name"] = json!(project.name);
        card["created"] = json!(project.created);
        card["blocks"] = json!(total);
        card["ready"] = json!(ready);
    } else {
        card["name"] = json!(std::fs::read_to_string(dir.join("name.txt")).unwrap_or_default());
    }
    Some(card)
}

/// GET /projects — проекты Speech и Listening, новые первыми.
pub async fn list(State(st): State<AppState>) -> Response {
    let p = paths(&st);
    let res = joined(tokio::task::spawn_blocking(move || {
        let mut out: Vec<Value> = std::fs::read_dir(&p.workspace)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|d| d.join("project.json").is_file()).filter_map(|d| summary(&p, &d)).collect())
            .unwrap_or_default();
        out.sort_by_key(|v| std::cmp::Reverse(v["mtime"].as_u64().unwrap_or(0)));
        out
    })
    .await);
    match res {
        Ok(v) => Json(json!({ "projects": v })).into_response(),
        Err(r) => r,
    }
}

/// POST /projects {name, text?, voice?, lang?} — новый проект Speech: текст режется на блоки (абзацы или диалог).
pub async fn create(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let name = body.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| t!("speech-untitled"));
    let text = body.get("text").and_then(Value::as_str).unwrap_or("").to_string();
    let voice = body.get("voice").and_then(Value::as_str).map(str::to_string).filter(|v| !v.is_empty());
    let lang = body.get("lang").and_then(Value::as_str).map(str::to_string);
    let p = paths(&st);
    let res = joined(tokio::task::spawn_blocking(crate::mcp::carry(move || -> Result<(String, Value), String> {
        let mut project = sp::from_text(&name, &text, &t!("speech-narrator"));
        if project.chapters[0].blocks.is_empty() {
            project.chapters[0].blocks.push(sp::Block { id: sp::new_id(), speaker: t!("speech-narrator"), ..Default::default() });
        }
        project.settings.voice = voice;
        if let Some(l) = lang {
            project.settings.lang = l;
        }
        let pid = sp::new_id();
        sp::save(&p.workspace, &pid, &project)?;
        Ok((pid, shown(&p, &project)))
    }))
    .await);
    match res {
        Ok(Ok((pid, project))) => Json(json!({ "project_id": pid, "project": project })).into_response(),
        Ok(Err(e)) => refuse(StatusCode::BAD_REQUEST, "bad_project", e),
        Err(r) => r,
    }
}

/// GET /projects/{pid} — проект Speech со статусами блоков (Listening — как записан).
pub async fn get(State(st): State<AppState>, AxPath(pid): AxPath<String>) -> Response {
    let p = paths(&st);
    let res = joined(tokio::task::spawn_blocking(move || -> Result<Value, (StatusCode, String)> {
        if !sp::is_pid(&pid) {
            return Err((StatusCode::BAD_REQUEST, format!("bad project id {pid}")));
        }
        let file = sp::path(&p.workspace, &pid);
        let text = std::fs::read_to_string(&file).map_err(|_| (StatusCode::NOT_FOUND, t!("speech-no-project", pid = pid.clone())))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}: {e}", file.display())))?;
        if v.get("kind").and_then(Value::as_str) == Some(sp::KIND) {
            let project: SpeechProject = serde_json::from_value(v).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            return Ok(shown(&p, &project));
        }
        Ok(v)
    })
    .await);
    match res {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err((status, e))) => refuse(status, "no_project", e),
        Err(r) => r,
    }
}

/// PUT /projects/{pid} — проект Speech целиком (окно, undo, агент). Состояние, снятое до чужого сохранения, — 409.
pub async fn put(State(st): State<AppState>, AxPath(pid): AxPath<String>, Json(body): Json<Value>) -> Response {
    let p = paths(&st);
    let res = joined(tokio::task::spawn_blocking(crate::mcp::carry(move || -> Result<Value, String> {
        let mut project: SpeechProject = serde_json::from_value(body).map_err(|e| e.to_string())?;
        sp::load(&p.workspace, &pid)?;
        project.kind = sp::KIND.into();
        sp::save(&p.workspace, &pid, &project)?;
        Ok(shown(&p, &project))
    }))
    .await);
    match res {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) if e.starts_with(crate::mcp::PROJECT_CHANGED) => refuse(StatusCode::CONFLICT, crate::mcp::PROJECT_CHANGED, e),
        Ok(Err(e)) => refuse(StatusCode::BAD_REQUEST, "bad_project", e),
        Err(r) => r,
    }
}

/// DELETE /projects/{pid} — проект в корзину системы (восстанавливается оттуда).
pub async fn delete(State(st): State<AppState>, AxPath(pid): AxPath<String>) -> Response {
    if !sp::is_pid(&pid) {
        return refuse(StatusCode::BAD_REQUEST, "bad_request", format!("bad project id {pid}"));
    }
    let dir = st.workspace.join(&pid);
    if !dir.join("project.json").is_file() {
        return refuse(StatusCode::NOT_FOUND, "no_project", t!("speech-no-project", pid = pid));
    }
    match joined(tokio::task::spawn_blocking(move || trash::delete(&dir).map_err(|e| e.to_string())).await) {
        Ok(Ok(())) => Json(json!({ "deleted": pid })).into_response(),
        Ok(Err(e)) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "delete_failed", e),
        Err(r) => r,
    }
}

/// POST /projects/{pid}/synth {ids?, from?, force?} — озвучить блоки задачей.
pub async fn synth(State(st): State<AppState>, AxPath(pid): AxPath<String>, Json(body): Json<Value>) -> Response {
    if let Err(e) = sp::load(&st.workspace, &pid) {
        return refuse(StatusCode::NOT_FOUND, "no_project", e);
    }
    let scope = Scope {
        ids: body.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()),
        from: body.get("from").and_then(Value::as_str).map(str::to_string),
        force: body.get("force").and_then(Value::as_bool).unwrap_or(false),
    };
    let p = paths(&st);
    let pid2 = pid.clone();
    let job: jobs::JobFn = Box::new(move |progress: jobs::ProgressFn| {
        let cb = |ev: Value| progress(ev);
        speech_jobs::synth(&p, &pid2, &scope, &cb)
    });
    match st.jobs.enqueue(jobs::JobMeta::new(jobs::JobKind::Synth, Some(&pid)), job).await {
        Ok(job_id) => Json(json!({ "job_id": job_id })).into_response(),
        Err(e) => crate::enqueue_error(e),
    }
}

/// GET /projects/{pid}/export-check — что мешает экспорту.
pub async fn export_check(State(st): State<AppState>, AxPath(pid): AxPath<String>) -> Response {
    let p = paths(&st);
    let res = joined(tokio::task::spawn_blocking(move || sp::load(&p.workspace, &pid).map(|project| speech_jobs::export_check(&p, &pid, &project))).await);
    match res {
        Ok(Ok(problems)) => Json(json!({ "problems": problems })).into_response(),
        Ok(Err(e)) => refuse(StatusCode::NOT_FOUND, "no_project", e),
        Err(r) => r,
    }
}

/// POST /projects/{pid}/export {format, chapter?} — файл экспорта задачей.
pub async fn export(State(st): State<AppState>, AxPath(pid): AxPath<String>, Json(body): Json<Value>) -> Response {
    if let Err(e) = sp::load(&st.workspace, &pid) {
        return refuse(StatusCode::NOT_FOUND, "no_project", e);
    }
    let opts = ExportOpts {
        format: body.get("format").and_then(Value::as_str).unwrap_or("wav").to_string(),
        chapter: body.get("chapter").and_then(Value::as_str).map(str::to_string),
    };
    if !crate::media::EXPORT_FORMATS.contains(&opts.format.as_str()) {
        return refuse(StatusCode::BAD_REQUEST, "bad_format", t!("media-bad-format", format = opts.format.clone()));
    }
    let p = paths(&st);
    let pid2 = pid.clone();
    let job: jobs::JobFn = Box::new(move |progress: jobs::ProgressFn| {
        let cb = |ev: Value| progress(ev);
        speech_jobs::export(&p, &pid2, &opts, &cb)
    });
    match st.jobs.enqueue(jobs::JobMeta::new(jobs::JobKind::Export, Some(&pid)), job).await {
        Ok(job_id) => Json(json!({ "job_id": job_id })).into_response(),
        Err(e) => crate::enqueue_error(e),
    }
}

fn safe_file(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && !name.contains("..")
}

/// GET /projects/{pid}/takes/{file} — звук дубля с Range.
pub async fn take_file(State(st): State<AppState>, AxPath((pid, file)): AxPath<(String, String)>, req: axum::http::Request<axum::body::Body>) -> Response {
    if !sp::is_pid(&pid) || !safe_file(&file) {
        return refuse(StatusCode::BAD_REQUEST, "bad_request", file);
    }
    let path = st.workspace.join(&pid).join("takes").join(&file);
    if !path.is_file() {
        return refuse(StatusCode::NOT_FOUND, "no_file", file);
    }
    crate::serve_file_range(&path, req, None).await
}

/// GET /projects/{pid}/exports/{file} — файл экспорта (скачивание).
pub async fn export_file(State(st): State<AppState>, AxPath((pid, file)): AxPath<(String, String)>, req: axum::http::Request<axum::body::Body>) -> Response {
    if !sp::is_pid(&pid) || !safe_file(&file) {
        return refuse(StatusCode::BAD_REQUEST, "bad_request", file);
    }
    let path = st.workspace.join(&pid).join("exports").join(&file);
    if !path.is_file() {
        return refuse(StatusCode::NOT_FOUND, "no_file", file);
    }
    crate::serve_file_range(&path, req, Some(file)).await
}

/// POST /projects/{pid}/save-export {file, path} — копия файла экспорта туда, куда сказали (агент, «Сохранить как»).
pub async fn save_export(State(st): State<AppState>, AxPath(pid): AxPath<String>, Json(body): Json<Value>) -> Response {
    let file = body.get("file").and_then(Value::as_str).unwrap_or("").to_string();
    let target = body.get("path").and_then(Value::as_str).map(str::trim).unwrap_or("").to_string();
    if !sp::is_pid(&pid) || !safe_file(&file) || target.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "bad_request", "file and path are needed");
    }
    let src = st.workspace.join(&pid).join("exports").join(&file);
    let mut dst = std::path::PathBuf::from(&target);
    if dst.is_dir() {
        dst = dst.join(&file);
    }
    match std::fs::copy(&src, &dst) {
        Ok(bytes) => Json(json!({ "path": dst.to_string_lossy(), "bytes": bytes })).into_response(),
        Err(e) => refuse(StatusCode::BAD_REQUEST, "save_failed", format!("{} -> {}: {e}", src.display(), dst.display())),
    }
}
