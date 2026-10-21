//! Маршруты подготовки текста: глобальный словарь произношения, импорт словарей Balabolka, предпросмотр подготовки
//! блока с трассировкой слоёв, счётчики срабатываний словаря и слова проекта, которых нет ни в одном словаре.

use std::collections::BTreeMap;

use axum::extract::{Multipart, Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::pron_dict::{self, Entry};
use crate::speech_api::paths;
use crate::speech_jobs;
use crate::speech_project as sp;
use crate::AppState;

fn refuse(status: StatusCode, code: &str, detail: impl Into<String>) -> Response {
    (status, Json(json!({ "error": code, "detail": detail.into() }))).into_response()
}

fn joined<T>(r: Result<T, tokio::task::JoinError>) -> Result<T, Response> {
    r.map_err(|e| refuse(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string()))
}

/// GET /dictionary — глобальные записи сверху вниз.
pub async fn get_global(State(st): State<AppState>) -> Json<Value> {
    Json(json!({ "entries": pron_dict::load_global(&st.repo_root) }))
}

#[derive(Deserialize)]
pub struct EntriesBody {
    entries: Vec<Entry>,
}

/// PUT /dictionary — заменить глобальный словарь целиком (порядок записей — порядок срабатывания).
pub async fn put_global(State(st): State<AppState>, Json(body): Json<EntriesBody>) -> Response {
    match pron_dict::save_global(&st.repo_root, &body.entries) {
        Ok(()) => Json(json!({ "entries": body.entries })).into_response(),
        Err(e) => refuse(StatusCode::BAD_REQUEST, "dict-invalid", e),
    }
}

/// POST /dictionary/import (multipart `file`) — разобрать словарь Balabolka; записи возвращаются, не сохраняются:
/// куда их положить (глобально или в проект) и в каком месте, решает окно.
pub async fn import(mut multipart: Multipart) -> Response {
    let mut got: Option<(String, Vec<u8>)> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.file_name().unwrap_or("dictionary.dic").to_string();
        match field.bytes().await {
            Ok(b) => got = Some((name, b.to_vec())),
            Err(e) => return refuse(StatusCode::BAD_REQUEST, "bad_request", e.to_string()),
        }
    }
    let Some((name, bytes)) = got else { return refuse(StatusCode::BAD_REQUEST, "bad_request", "no file") };
    match pron_dict::import_balabolka(&name, &bytes) {
        Ok(entries) => Json(json!({ "entries": entries, "source": name })).into_response(),
        Err(e) => refuse(StatusCode::BAD_REQUEST, "dict-import-unknown", e),
    }
}

#[derive(Deserialize)]
pub struct PrepareBody {
    text: String,
    #[serde(default)]
    pid: Option<String>,
    #[serde(default)]
    block: Option<String>,
    #[serde(default)]
    lang: Option<String>,
}

/// POST /text/prepare — что синтез сделает с текстом: итог, шаги по слоям, нераспознанное, что движок проигнорирует.
pub async fn prepare(State(st): State<AppState>, Json(body): Json<PrepareBody>) -> Response {
    let p = paths(&st);
    let done = joined(
        tokio::task::spawn_blocking(move || -> Result<Value, String> {
            let project = match &body.pid {
                Some(pid) => sp::load(&p.workspace, pid)?,
                None => sp::SpeechProject::default(),
            };
            let mut project = project;
            if let Some(lang) = &body.lang {
                project.settings.lang = lang.clone();
            }
            let voice = body.block.as_ref().and_then(|id| project.blocks().find(|b| &b.id == id)).and_then(|b| project.voice_of(b));
            let entries = speech_jobs::dictionary(&p, &project);
            let russian = crate::stress::is_russian(&body.text, &project.settings.lang);
            let stress = project.settings.auto_stress && russian && crate::stress::installed(&p.repo_root);
            let lang = body.block.as_ref().and_then(|id| project.blocks().find(|b| &b.id == id)).map_or(project.settings.lang.clone(), |b| project.lang_of(b).to_string());
            let prepared = crate::prep::prepare(&body.text, &speech_jobs::prep_opts(&p, &project, &lang, &project.settings.engine, voice.as_deref(), &entries, stress))?;
            Ok(json!({ "prepared": prepared, "stress_applied": stress, "stress_missing": project.settings.auto_stress && russian && !stress }))
        })
        .await,
    );
    match done {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => refuse(StatusCode::BAD_REQUEST, "prepare-failed", e),
        Err(r) => r,
    }
}

/// GET /projects/{pid}/dictionary-counts — сколько раз каждая запись (проекта и глобальная) встречается в тексте.
pub async fn counts(State(st): State<AppState>, AxPath(pid): AxPath<String>) -> Response {
    let p = paths(&st);
    let done = joined(
        tokio::task::spawn_blocking(move || -> Result<Value, String> {
            let project = sp::load(&p.workspace, &pid)?;
            let entries = speech_jobs::dictionary(&p, &project);
            let text: String = project.blocks().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n");
            let counts: BTreeMap<String, usize> = pron_dict::counts(&text, &entries).into_iter().collect();
            Ok(json!({ "counts": counts }))
        })
        .await,
    );
    match done {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => refuse(StatusCode::NOT_FOUND, "project_missing", e),
        Err(r) => r,
    }
}

/// GET /projects/{pid}/unknown-words — слова проекта, которых нет в словаре ударений и в словаре произношения, и
/// то, что нормализатор не смог прочесть (латиница, цифры): с числом вхождений, частые сверху.
pub async fn unknown_words(State(st): State<AppState>, AxPath(pid): AxPath<String>) -> Response {
    let p = paths(&st);
    let done = joined(
        tokio::task::spawn_blocking(move || -> Result<Value, String> {
            let project = sp::load(&p.workspace, &pid)?;
            let entries = speech_jobs::dictionary(&p, &project);
            let stresser = if crate::stress::installed(&p.repo_root) { Some(crate::stress::shared(&p.models_root)?) } else { None };
            let mut words: BTreeMap<String, (usize, &'static str)> = BTreeMap::new();
            for b in project.blocks().filter(|b| !b.hidden) {
                let voice = project.voice_of(b);
                let prepared = crate::prep::prepare(&b.text, &speech_jobs::prep_opts(&p, &project, project.lang_of(b), project.engine_of(b), voice.as_deref(), &entries, false))?;
                for w in &prepared.unknown {
                    words.entry(w.clone()).or_insert((0, "unread")).0 += 1;
                }
                if let Some(s) = &stresser {
                    for w in crate::stress::cyrillic_words(&prepared.text) {
                        if !s.knows(&w) {
                            words.entry(w).or_insert((0, "no_stress")).0 += 1;
                        }
                    }
                }
            }
            let mut list: Vec<Value> = words.into_iter().map(|(w, (n, why))| json!({ "word": w, "count": n, "why": why })).collect();
            list.sort_by(|a, b| b["count"].as_u64().cmp(&a["count"].as_u64()));
            Ok(json!({ "words": list, "stress_checked": stresser.is_some() }))
        })
        .await,
    );
    match done {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => refuse(StatusCode::BAD_REQUEST, "unknown-words-failed", e),
        Err(r) => r,
    }
}
