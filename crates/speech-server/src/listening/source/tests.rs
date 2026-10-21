use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::{routing::post, Router};
use serde_json::{json, Value};
use tower::ServiceExt;

use super::*;
use crate::app::{context::RequestContext, revisions};

fn audio(path: &Path) {
    crate::wavio::write_mono_f32(path, &[0.25; 4_000], 16_000).unwrap();
}

fn modified(path: &Path, seconds: u64) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(seconds)))
        .unwrap();
}

#[test]
fn file_keys_keep_the_namespace_path_size_and_whole_second_timestamp() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("Record.WAV");
    fs::write(&path, b"abc").unwrap();
    modified(&path, 1_700_000_000);
    let expected = blake3::hash(format!("listen|{}|3|1700000000", path.display()).as_bytes()).to_hex()[..16].to_string();
    assert_eq!(file_key(&path, "listen").unwrap(), expected);
    assert_ne!(file_key(&path, "separate").unwrap(), expected);
    let other = root.path().join("Other.wav");
    fs::copy(&path, &other).unwrap();
    modified(&other, 1_700_000_000);
    assert_ne!(file_key(&other, "listen").unwrap(), expected);
    fs::write(&path, b"abcd").unwrap();
    modified(&path, 1_700_000_000);
    let larger = file_key(&path, "listen").unwrap();
    assert_ne!(larger, expected);
    modified(&path, 1_700_000_001);
    assert_ne!(file_key(&path, "listen").unwrap(), larger);
    let missing = root.path().join("missing.wav");
    assert!(file_key(&missing, "listen")
        .unwrap_err()
        .starts_with(&format!("{}:", missing.display())));
}

#[test]
fn importing_and_reusing_a_recording_preserves_sources_edits_keys_and_revisions() {
    let root = tempfile::tempdir().unwrap();
    let ws = root.path().join("workspace");
    fs::create_dir(&ws).unwrap();
    let source = root.path().join("Моя запись.WAV");
    audio(&source);
    modified(&source, 1_700_000_000);
    let (pid, existing) = project_of_file(&ws, &source).unwrap();
    assert!(!existing);
    assert_eq!(pid.len(), 10);
    let mut project = lp::load(&ws, &pid).unwrap();
    assert_eq!(project.source, "source.wav");
    assert_eq!(project.name, "Моя запись");
    assert_eq!(project.origin, source.to_string_lossy());
    assert_eq!(project.key, file_key(&source, "listen").unwrap());
    assert_eq!(revisions::revision(&pid), 2);
    assert_eq!(fs::read(ws.join(&pid).join("source.wav")).unwrap(), fs::read(&source).unwrap());
    project.name = "Edited".into();
    project.lines.push(lp::Line {
        id: "line".into(),
        text: "Keep me".into(),
        ..Default::default()
    });
    lp::save(&ws, &pid, &project).unwrap();
    let path = ws.join(&pid).join("project.json");
    let before = fs::read(&path).unwrap();
    let revision = revisions::revision(&pid);
    assert_eq!(project_of_file(&ws, &source).unwrap(), (pid.clone(), true));
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(revisions::revision(&pid), revision);
    modified(&source, 1_700_000_001);
    let (changed, existing) = project_of_file(&ws, &source).unwrap();
    assert!(!existing);
    assert_ne!(changed, pid);
    assert_eq!(lp::load(&ws, &pid).unwrap().name, "Edited");
}

#[test]
fn non_webview_media_prepares_the_same_aac_preview_before_creating_the_project() {
    let root = tempfile::tempdir().unwrap();
    let ws = root.path().join("workspace");
    fs::create_dir(&ws).unwrap();
    let source = root.path().join("record.wma");
    audio(&source);
    let (pid, _) = project_of_file(&ws, &source).unwrap();
    assert_eq!(lp::load(&ws, &pid).unwrap().source, "source.wma");
    let preview = ws.join(&pid).join("work/play.m4a");
    assert!(preview.is_file());
    assert!(crate::media::probe(&preview).unwrap().duration > 0.0);
}

#[test]
fn invalid_inputs_and_preview_failure_keep_the_existing_cleanup_and_errors() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    let ws = root.path().join("workspace");
    fs::create_dir(&ws).unwrap();
    for name in ["missing.wav", "notes.txt"] {
        let path = root.path().join(name);
        if name.ends_with("txt") {
            fs::write(&path, b"text").unwrap();
        }
        assert_eq!(
            project_of_file(&ws, &path).unwrap_err(),
            t!("listen-not-media", file = path.display().to_string())
        );
    }
    let broken = root.path().join("broken.mkv");
    fs::write(&broken, b"invalid media").unwrap();
    assert!(project_of_file(&ws, &broken).is_err());
    assert_eq!(fs::read_dir(&ws).unwrap().count(), 0);
    assert_eq!(fs::read(&broken).unwrap(), b"invalid media");
    let valid = root.path().join("ok.wav");
    audio(&valid);
    assert!(project_of_file(&root.path().join("missing-workspace"), &valid).is_err());
    assert!(!root.path().join("missing-workspace").exists());
}

#[tokio::test]
async fn http_import_keeps_the_response_error_and_blocking_request_context() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("record.wav");
    audio(&source);
    let state = crate::AppState::new(root.path());
    let ws = state.workspace.clone();
    let router = Router::new()
        .route("/listen/from-path", post(crate::listening::api::from_path))
        .with_state(state);
    let mut target = String::new();
    for existing in [false, true] {
        let context = RequestContext::new("agent:b06".into(), None);
        let outcome = context.outcome();
        let response = context
            .run(
                router.clone().oneshot(
                    Request::post("/listen/from-path")
                        .header("content-type", "application/json")
                        .body(Body::from(json!({"path":source}).to_string()))
                        .unwrap(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2);
        assert_eq!(value["existing"], existing);
        if existing {
            assert_eq!(value["pid"], target);
            assert_eq!(outcome.saved_revision(), None);
        } else {
            target = value["pid"].as_str().unwrap().into();
            assert_eq!(outcome.saved_revision(), Some(2));
        }
    }
    assert_eq!(lp::load(&ws, &target).unwrap().name, "record");
    let missing = root.path().join("missing.wav");
    let response = router
        .oneshot(
            Request::post("/listen/from-path")
                .header("content-type", "application/json")
                .body(Body::from(json!({"path":missing}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(
        value,
        json!({"error":"listen_failed","detail":t!("listen-not-media", file = missing.display().to_string())})
    );
}
