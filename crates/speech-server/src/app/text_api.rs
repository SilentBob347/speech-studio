//! Маршруты подготовки текста: глобальный словарь произношения, импорт словарей Balabolka, предпросмотр подготовки
//! блока с трассировкой слоёв, счётчики срабатываний словаря и слова проекта, которых нет ни в одном словаре.

use axum::extract::{Multipart, Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::text::pron_dict;
use crate::text::pron_dict::Entry;
use crate::speech::{self, PrepareText, SpeechPaths};
use crate::AppState;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/text/prepare", post(prepare))
        .route("/dictionary", get(get_global).put(put_global))
        .route("/dictionary/import", post(import))
        .route("/projects/{pid}/dictionary-counts", get(counts))
        .route("/projects/{pid}/unknown-words", get(unknown_words))
}

fn refuse(status: StatusCode, code: &str, detail: impl Into<String>) -> Response {
    (status, Json(json!({ "error": code, "detail": detail.into() }))).into_response()
}

fn joined<T>(r: Result<T, tokio::task::JoinError>) -> Result<T, Response> {
    r.map_err(|e| refuse(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string()))
}

fn paths(st: &AppState) -> SpeechPaths {
    SpeechPaths {
        repo_root: st.repo_root.clone(),
        workspace: st.workspace.clone(),
        voices_dir: st.voices_dir.clone(),
        models_root: st.models_root.clone(),
        higgs_dll: st.higgs_dll.clone(),
        threads: st.opts.num_threads,
    }
}

/// GET /dictionary — глобальные записи сверху вниз.
async fn get_global(State(st): State<AppState>) -> Json<Value> {
    Json(json!({ "entries": pron_dict::load_global(&st.repo_root) }))
}

#[derive(Deserialize)]
struct EntriesBody {
    entries: Vec<Entry>,
}

/// PUT /dictionary — заменить глобальный словарь целиком (порядок записей — порядок срабатывания).
async fn put_global(State(st): State<AppState>, Json(body): Json<EntriesBody>) -> Response {
    match pron_dict::save_global(&st.repo_root, &body.entries) {
        Ok(()) => Json(json!({ "entries": body.entries })).into_response(),
        Err(e) => refuse(StatusCode::BAD_REQUEST, "dict-invalid", e),
    }
}

/// POST /dictionary/import (multipart `file`) — разобрать словарь Balabolka; записи возвращаются, не сохраняются:
/// куда их положить (глобально или в проект) и в каком месте, решает окно.
async fn import(mut multipart: Multipart) -> Response {
    let mut got: Option<(String, Vec<u8>)> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(e) => return refuse(StatusCode::BAD_REQUEST, "bad_request", e.to_string()),
        };
        let name = field.file_name().unwrap_or("dictionary.dic").to_string();
        match field.bytes().await {
            Ok(b) => got = Some((name, b.to_vec())),
            Err(e) => return refuse(StatusCode::BAD_REQUEST, "bad_request", e.to_string()),
        }
    }
    let Some((name, bytes)) = got else {
        return refuse(StatusCode::BAD_REQUEST, "bad_request", "no file");
    };
    match pron_dict::import_balabolka(&name, &bytes) {
        Ok(entries) => Json(json!({ "entries": entries, "source": name })).into_response(),
        Err(e) => refuse(StatusCode::BAD_REQUEST, "dict-import-unknown", e),
    }
}

#[derive(Deserialize)]
struct PrepareBody {
    text: String,
    #[serde(default)]
    pid: Option<String>,
    #[serde(default)]
    block: Option<String>,
    #[serde(default)]
    lang: Option<String>,
}

/// POST /text/prepare — что синтез сделает с текстом: итог, шаги по слоям, нераспознанное, что движок проигнорирует.
async fn prepare(State(st): State<AppState>, Json(body): Json<PrepareBody>) -> Response {
    let p = paths(&st);
    let done = joined(
        tokio::task::spawn_blocking(move || {
            speech::prepare_text(
                &p,
                PrepareText {
                    text: body.text,
                    pid: body.pid,
                    block: body.block,
                    lang: body.lang,
                },
            )
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
async fn counts(State(st): State<AppState>, AxPath(pid): AxPath<String>) -> Response {
    let p = paths(&st);
    let done = joined(tokio::task::spawn_blocking(move || speech::dictionary_counts(&p, &pid)).await);
    match done {
        Ok(Ok(counts)) => Json(json!({ "counts": counts })).into_response(),
        Ok(Err(e)) => refuse(StatusCode::NOT_FOUND, "project_missing", e),
        Err(r) => r,
    }
}

/// GET /projects/{pid}/unknown-words — слова проекта, которых нет в словаре ударений и в словаре произношения, и
/// то, что нормализатор не смог прочесть (латиница, цифры): с числом вхождений, частые сверху.
async fn unknown_words(State(st): State<AppState>, AxPath(pid): AxPath<String>) -> Response {
    let p = paths(&st);
    let done = joined(tokio::task::spawn_blocking(move || speech::unknown_words(&p, &pid)).await);
    match done {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => refuse(StatusCode::BAD_REQUEST, "unknown-words-failed", e),
        Err(r) => r,
    }
}

#[cfg(test)]
mod tests {
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Request};
    use tower::ServiceExt;

    use super::*;

    async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    fn request(method: &str, uri: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn preview_preserves_the_response_shape_and_missing_project_errors() {
        let root = tempfile::tempdir().unwrap();
        let app = router().with_state(AppState::new(root.path()));
        let (status, preview) = send(&app, request("POST", "/text/prepare", json!({ "text": "Hello" }))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            preview,
            json!({
                "prepared": {
                    "text": "Hello", "engine_text": "Hello", "steps": [], "unknown": [],
                    "ignored": { "accent": false, "phonemes": false, "stress": false, "unknown_tags": [] },
                    "ignored_phonemes": [], "broken": []
                },
                "stress_applied": false, "stress_missing": false
            })
        );
        for (method, uri, body, expected_status, code) in [
            (
                "POST",
                "/text/prepare",
                json!({ "text": "Hello", "pid": "absent" }),
                StatusCode::BAD_REQUEST,
                "prepare-failed",
            ),
            (
                "GET",
                "/projects/absent/dictionary-counts",
                Value::Null,
                StatusCode::NOT_FOUND,
                "project_missing",
            ),
            (
                "GET",
                "/projects/absent/unknown-words",
                Value::Null,
                StatusCode::BAD_REQUEST,
                "unknown-words-failed",
            ),
        ] {
            let (status, error) = send(&app, request(method, uri, body)).await;
            assert_eq!(status, expected_status, "{uri}");
            assert_eq!(error["error"], code);
            assert!(!error["detail"].as_str().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn dictionary_replacement_validates_before_saving_and_keeps_entry_order() {
        let root = tempfile::tempdir().unwrap();
        let app = router().with_state(AppState::new(root.path()));
        let entries = vec![
            Entry {
                id: "second".into(),
                find: "cat".into(),
                say: "kitten".into(),
                ..Default::default()
            },
            Entry {
                id: "first".into(),
                find: "dog".into(),
                say: "puppy".into(),
                ..Default::default()
            },
        ];
        let expected = json!({ "entries": entries });
        let (status, saved) = send(&app, request("PUT", "/dictionary", expected.clone())).await;
        assert_eq!((status, saved), (StatusCode::OK, expected.clone()));
        let before = std::fs::read(pron_dict::global_path(root.path())).unwrap();
        let (status, error) = send(&app, request("PUT", "/dictionary", json!({ "entries": [entries[0], entries[0]] }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error["error"], "dict-invalid");
        assert_eq!(std::fs::read(pron_dict::global_path(root.path())).unwrap(), before);
        assert_eq!(
            send(&app, request("GET", "/dictionary", Value::Null)).await,
            (StatusCode::OK, expected)
        );
    }

    #[tokio::test]
    async fn project_reports_keep_their_json_envelopes() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::new(root.path());
        let mut project = crate::speech::from_text("Report", "iPhone iPhone", "Narrator");
        project.settings.lang = "ru".into();
        let pid = crate::platform::ids::new_id();
        let path = crate::platform::workspace::project_file(&state.workspace, &pid);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec(&project).unwrap()).unwrap();
        let app = router().with_state(state);
        let (status, counts) = send(&app, request("GET", &format!("/projects/{pid}/dictionary-counts"), Value::Null)).await;
        assert_eq!((status, counts), (StatusCode::OK, json!({ "counts": {} })));
        let (status, words) = send(&app, request("GET", &format!("/projects/{pid}/unknown-words"), Value::Null)).await;
        assert_eq!(
            (status, words),
            (
                StatusCode::OK,
                json!({
                    "words": [{ "word": "iPhone", "count": 2, "why": "unread" }], "stress_checked": false
                })
            )
        );
    }

    async fn import_body(app: &Router, body: String) -> (StatusCode, Value) {
        send(
            app,
            Request::builder()
                .method("POST")
                .uri("/dictionary/import")
                .header(header::CONTENT_TYPE, "multipart/form-data; boundary=import-boundary")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
    }

    const PART: &str = "--import-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"my.dic\"\r\n\r\ncat=kitten\r\n";

    #[tokio::test]
    async fn dictionary_import_returns_entries_without_saving_them() {
        let root = tempfile::tempdir().unwrap();
        let app = router().with_state(AppState::new(root.path()));
        let (status, imported) = import_body(&app, format!("{PART}--import-boundary--\r\n")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(imported["source"], "my.dic");
        assert_eq!(imported["entries"][0]["find"], "cat");
        assert_eq!(imported["entries"][0]["say"], "kitten");
        assert!(!pron_dict::global_path(root.path()).exists());
        let (status, error) = import_body(&app, "--import-boundary--\r\n".into()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error, json!({ "error": "bad_request", "detail": "no file" }));
    }

    #[tokio::test]
    async fn a_malformed_part_after_a_valid_dictionary_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let app = router().with_state(AppState::new(root.path()));
        let (status, error) = import_body(
            &app,
            format!("{PART}--import-boundary\r\ninvalid header\r\n\r\nignored\r\n--import-boundary--\r\n"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
        assert_eq!(error["error"], "bad_request");
        assert_ne!(error["detail"], "no file");
        assert!(error["detail"].as_str().is_some_and(|detail| !detail.is_empty()));
        assert!(!pron_dict::global_path(root.path()).exists());
    }
}
