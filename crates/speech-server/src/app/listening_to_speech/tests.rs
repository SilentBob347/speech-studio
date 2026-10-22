use std::fs;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::{routing::post, Router};
use serde_json::{json, Value};
use tower::ServiceExt;

use super::*;
use crate::app::context::RequestContext;
use crate::listening::{Line, ListenProject};
use crate::AppState;

fn fixture(workspace: &Path, lines: Vec<Line>) -> String {
    let pid = crate::platform::ids::new_id();
    let mut project = ListenProject::new("Запись", "source.wav", "original");
    project.speakers = [
        ("a".into(), "Alice".into()),
        ("b".into(), "Bob".into()),
        ("unused".into(), "Unused".into()),
    ]
    .into();
    project.lines = lines;
    crate::listening::save(workspace, &pid, &project).unwrap();
    pid
}

fn line(speaker: &str, text: &str, start: f64, hidden: bool) -> Line {
    Line {
        id: crate::platform::ids::new_id(),
        speaker: speaker.into(),
        text: text.into(),
        start,
        end: start + 1.25,
        hidden,
        ..Default::default()
    }
}

#[test]
fn conversion_keeps_verbatim_text_order_speaker_names_times_and_existing_voice_mappings() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let voices = root.path().join("voices");
    fs::create_dir(&voices).unwrap();
    fs::write(voices.join("Alice.wav"), b"voice exists").unwrap();
    fs::write(voices.join("Unused.mp3"), b"voice exists").unwrap();
    let pid = fixture(
        &workspace,
        vec![
            line("a", "  Первая реплика.  ", 4.0, false),
            line("b", "Скрытая", 2.0, true),
            line("b", " \t ", 2.5, false),
            line("missing", "Без имени", 1.0, false),
            line("b", "Вторая реплика", 8.0, false),
        ],
    );
    let path = workspace.join(&pid).join("project.json");
    let before = fs::read(&path).unwrap();
    let revision = crate::app::revisions::revision(&pid);
    let new_pid = convert(&workspace, &voices, &pid).unwrap();
    assert_ne!(new_pid, pid);
    let project = crate::speech::load(&workspace, &new_pid).unwrap();
    assert_eq!(project.name, "Запись");
    assert!(project.settings.timeline);
    let blocks: Vec<_> = project.blocks().collect();
    assert_eq!(
        blocks
            .iter()
            .map(|b| (b.text.as_str(), b.speaker.as_str(), b.start, b.end))
            .collect::<Vec<_>>(),
        [
            ("  Первая реплика.  ", "Alice", Some(4.0), Some(5.25)),
            ("Без имени", "missing", Some(1.0), Some(2.25)),
            ("Вторая реплика", "Bob", Some(8.0), Some(9.25))
        ]
    );
    assert_eq!(
        project.cast,
        [("Alice".into(), "Alice".into()), ("Unused".into(), "Unused".into())].into()
    );
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(crate::app::revisions::revision(&pid), revision);
    assert_eq!(crate::app::revisions::revision(&new_pid), 1);
}

#[test]
fn empty_and_unreadable_sources_do_not_create_a_speech_project() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    let pid = fixture(root.path(), vec![line("a", "hidden", 0.0, true), line("b", " \n ", 1.5, false)]);
    let before = fs::read(root.path().join(&pid).join("project.json")).unwrap();
    assert_eq!(
        convert(root.path(), &root.path().join("voices"), &pid).unwrap_err(),
        t!("listen-empty")
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(fs::read(root.path().join(&pid).join("project.json")).unwrap(), before);
    assert_eq!(
        convert(root.path(), root.path(), "../outside").unwrap_err(),
        "bad project id ../outside"
    );
    assert_eq!(
        convert(root.path(), root.path(), "Missing").unwrap_err(),
        t!("project-missing", pid = "Missing".to_string())
    );
}

#[test]
fn speech_creation_reports_real_save_failures() {
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("workspace-is-a-file");
    fs::write(&blocked, b"keep").unwrap();
    let cues = [crate::text::subimport::Cue {
        start: 0.0,
        end: 1.0,
        text: "Hello".into(),
        speaker: Some("Alice".into()),
    }];
    let project = speech::from_transcript("Test", &cues, "Narrator");
    let error = project.create(&blocked).unwrap_err();
    assert!(error.contains(&blocked.display().to_string()));
    assert_eq!(fs::read(blocked).unwrap(), b"keep");
}

#[tokio::test]
async fn http_conversion_keeps_the_pid_envelope_errors_and_request_context() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    let state = AppState::new(root.path());
    let workspace = state.workspace.clone();
    let pid = fixture(&workspace, vec![line("a", "Привет", 0.0, false)]);
    let empty = fixture(&workspace, Vec::new());
    let router = Router::new()
        .route("/listen/{pid}/to-speech", post(crate::listening::http::to_speech))
        .with_state(state);
    let request = RequestContext::new("agent:b05".into(), None);
    let outcome = request.outcome();
    let response = request
        .run(
            router
                .clone()
                .oneshot(Request::post(format!("/listen/{pid}/to-speech")).body(Body::empty()).unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 1);
    let target = value["pid"].as_str().unwrap();
    assert_eq!(target.len(), 10);
    assert_eq!(outcome.saved_revision(), Some(1));
    assert!(!outcome.conflicted());
    assert_eq!(crate::speech::load(&workspace, target).unwrap().blocks().count(), 1);
    for (source, detail) in [
        (empty.as_str(), t!("listen-empty")),
        ("Missing", t!("project-missing", pid = "Missing".to_string())),
    ] {
        let request = RequestContext::new("agent:b05:error".into(), None);
        let outcome = request.outcome();
        let response = request
            .run(
                router
                    .clone()
                    .oneshot(Request::post(format!("/listen/{source}/to-speech")).body(Body::empty()).unwrap()),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(value, json!({"error":"listen_failed","detail":detail}));
        assert_eq!(outcome.saved_revision(), None);
    }
}
