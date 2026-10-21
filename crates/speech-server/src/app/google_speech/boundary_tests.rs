use std::fs;

use base64::Engine;
use dub_llm::test_http::{serve, Reply};

use super::*;
use crate::app::spending;
use crate::google_tts::Client;

fn setup() -> Setup {
    Setup {
        model: "gemini-3.8-flash-lite-tts".into(),
        voice: "Kore".into(),
        mode: Mode::Batch,
    }
}

fn want() -> Want {
    Want {
        text: "Hello".into(),
        style: "Quietly".into(),
        n: 1,
    }
}

fn response() -> Value {
    json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"inlineData":{"mimeType":"audio/L16;rate=24000","data":base64::engine::general_purpose::STANDARD.encode([0,0,0,64,0,128].repeat(50))}}]}}],
        "usageMetadata":{"promptTokenCount":8,"candidatesTokenCount":85,"candidatesTokensDetails":[{"modality":"AUDIO","tokenCount":85}]}})
}

fn completed(key: &str) -> Reply {
    Reply::json(
        200,
        &json!({"name":"batches/paid","done":true,"metadata":{"state":"BATCH_STATE_SUCCEEDED"},
        "response":{"inlinedResponses":{"inlinedResponses":[{"metadata":{"key":key},"response":response()}]}}})
        .to_string(),
    )
}

fn journal(project: &Path, s: &Setup, w: &Want, extra: Value) -> String {
    fs::create_dir_all(cloud_dir(project)).unwrap();
    let key = key_of(s, &w.text, &w.style, w.n);
    let mut j =
        json!({"model":s.model,"voice":s.voice,"display_name":"recover-me","keys":[key],"texts":[w.text],"created_at":"2026-10-21"});
    j.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    save(&journal_path(project), &j).unwrap();
    key
}

#[test]
fn a_named_batch_resumes_reads_audio_and_keeps_history_without_a_new_submission() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let s = setup();
    let w = want();
    let key = journal(&project, &s, &w, json!({"name":"batches/paid"}));
    let server = serve(vec![completed(&key)]);
    let client = Client::with_base("test-key".into(), &server.base()).unwrap();
    let result = batch_run(&client, root.path(), root.path(), &project, "p", &s, &[(key.clone(), &w)], &|_| {}).unwrap();
    assert!(result.0.is_empty());
    assert_eq!(result.2.as_deref(), Some("batches/paid"));
    assert!(result.1.unwrap() > 0.0);
    assert_eq!(server.count(), 1);
    assert!(server.request(0).starts_with("GET /v1beta/batches/paid"));
    assert!(!journal_path(&project).exists());
    assert_eq!(
        crate::wavio::read_mono_f32(&cache(&project, &key)).unwrap().0,
        [0.0, 0.5, -1.0].repeat(50)
    );
    assert_eq!(report(&project)["batches"][0]["state"], "downloaded");
    let rows = spending::spent(root.path());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider, "google-batch");
    assert_eq!(rows[0].estimate, result.1);
}

#[test]
fn a_lost_creation_answer_keeps_the_marker_then_finds_the_paid_batch_by_display_name() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let s = setup();
    let w = want();
    let key = journal(&project, &s, &w, json!({"input_file":"files/uploaded"}));
    let server = serve(vec![
        Reply::json(503, r#"{"error":{"message":"answer lost"}}"#),
        Reply::json(
            200,
            r#"{"operations":[{"name":"batches/paid","metadata":{"displayName":"recover-me"}}]}"#,
        ),
        completed(&key),
    ]);
    let client = Client::with_base("test-key".into(), &server.base()).unwrap();
    assert!(batch_run(&client, root.path(), root.path(), &project, "p", &s, &[(key.clone(), &w)], &|_| {}).is_err());
    let saved: Value = serde_json::from_slice(&fs::read(journal_path(&project)).unwrap()).unwrap();
    assert_eq!(saved["submitted"], true);
    assert_eq!(saved["input_file"], "files/uploaded");
    assert!(saved["submitted_at"].is_i64());
    assert_eq!(server.count(), 1, "creating a paid batch is attempted only once");
    let result = batch_run(&client, root.path(), root.path(), &project, "p", &s, &[(key, &w)], &|_| {}).unwrap();
    assert_eq!(result.2.as_deref(), Some("batches/paid"));
    assert_eq!(server.count(), 3);
    assert!(server.request(0).starts_with("POST "));
    assert!(server.request(1).starts_with("GET /v1beta/batches?"));
    assert!(server.request(2).starts_with("GET /v1beta/batches/paid"));
}

#[test]
fn a_recent_unknown_or_ambiguous_batch_is_not_submitted_again() {
    let _language = crate::i18n::test_language("en");
    for operations in [
        json!([]),
        json!([
            {"name":"batches/a","metadata":{"displayName":"recover-me"}},
            {"name":"batches/b","metadata":{"displayName":"recover-me"}}
        ]),
    ] {
        let root = tempfile::tempdir().unwrap();
        let s = setup();
        let w = want();
        let key = journal(
            root.path(),
            &s,
            &w,
            json!({"submitted":true,"submitted_at":chrono::Utc::now().timestamp()}),
        );
        let before = fs::read(journal_path(root.path())).unwrap();
        let server = serve(vec![Reply::json(200, &json!({"operations":operations}).to_string())]);
        let client = Client::with_base("test-key".into(), &server.base()).unwrap();
        assert!(batch_run(&client, root.path(), root.path(), root.path(), "p", &s, &[(key, &w)], &|_| {}).is_err());
        assert_eq!(server.count(), 1);
        assert!(server.request(0).starts_with("GET "));
        assert_eq!(fs::read(journal_path(root.path())).unwrap(), before);
    }
}

#[test]
fn definite_creation_refusals_clear_the_marker_and_keep_input_only_for_a_full_queue() {
    for code in [429, 400] {
        let root = tempfile::tempdir().unwrap();
        let s = setup();
        let w = want();
        let key = journal(root.path(), &s, &w, json!({"input_file":"files/uploaded"}));
        let server = serve(vec![Reply::json(code, r#"{"error":{"message":"refused"}}"#)]);
        let client = Client::with_base("test-key".into(), &server.base()).unwrap();
        assert!(batch_run(&client, root.path(), root.path(), root.path(), "p", &s, &[(key, &w)], &|_| {}).is_err());
        assert_eq!(server.count(), 1);
        let saved: Value = serde_json::from_slice(&fs::read(journal_path(root.path())).unwrap()).unwrap();
        assert!(saved.get("submitted").is_none());
        assert_eq!(saved.get("input_file").is_some(), code == 429);
        assert!(saved["rejected"].as_str().unwrap().contains("refused"));
        assert!(spending::spent(root.path()).is_empty());
    }
}

#[test]
fn direct_account_refusals_are_not_retried_and_errors_do_not_leak_the_key() {
    for code in [402, 403] {
        let server = serve(vec![Reply::json(code, r#"{"error":{"message":"test-key refused"}}"#)]);
        let client = Client::with_base("test-key".into(), &server.base()).unwrap();
        let error = direct(&client, &setup(), "Hello", "").unwrap_err();
        assert!(!error.contains("test-key"));
        assert!(error.contains("redacted"));
        assert_eq!(server.count(), 1);
    }
}

#[test]
fn cached_audio_and_fully_cached_prefetch_need_no_client_credentials_or_spending() {
    let root = tempfile::tempdir().unwrap();
    let s = setup();
    let w = want();
    let key = key_of(&s, &w.text, &w.style, w.n);
    fs::create_dir_all(cache(root.path(), &key).parent().unwrap()).unwrap();
    crate::wavio::write_mono_f32(&cache(root.path(), &key), &[0.0, 0.5, -1.0], SR).unwrap();
    let (samples, cost) = phrase(root.path(), root.path(), root.path(), "p", &s, &w.text, &w.style, w.n).unwrap();
    assert_eq!(samples, [0.0, 0.5, -1.0]);
    assert_eq!(cost, None);
    assert_eq!(
        prefetch(root.path(), root.path(), root.path(), "p", &s, &[w], &|_| panic!()).unwrap(),
        json!({"phrases":1,"cached":1})
    );
    assert!(spending::spent(root.path()).is_empty());
}

#[test]
fn missing_batch_rows_remain_failed_and_a_finished_operation_is_archived() {
    let root = tempfile::tempdir().unwrap();
    let s = setup();
    let w = want();
    let key = journal(root.path(), &s, &w, json!({"name":"batches/paid"}));
    let server = serve(vec![Reply::json(
        200,
        r#"{"name":"batches/paid","done":true,"metadata":{"state":"BATCH_STATE_SUCCEEDED"},"response":{"inlinedResponses":{"inlinedResponses":[]}}}"#,
    )]);
    let client = Client::with_base("test-key".into(), &server.base()).unwrap();
    let result = batch_run(
        &client,
        root.path(),
        root.path(),
        root.path(),
        "p",
        &s,
        &[(key.clone(), &w)],
        &|_| {},
    )
    .unwrap();
    assert!(result.0.contains_key(&key));
    assert!(!cache(root.path(), &key).exists());
    assert!(!journal_path(root.path()).exists());
    assert_eq!(report(root.path())["batches"][0]["failed"], 1);
    assert!(spending::spent(root.path()).is_empty());
}

#[tokio::test]
async fn cancelling_before_batch_polling_keeps_the_paid_operation_for_the_next_run() {
    let root = tempfile::tempdir().unwrap();
    let s = setup();
    let w = want();
    let key = journal(root.path(), &s, &w, json!({"name":"batches/paid"}));
    let before = fs::read(journal_path(root.path())).unwrap();
    let path = root.path().to_path_buf();
    let server = serve(vec![]);
    let base = server.base();
    let (started, received) = tokio::sync::oneshot::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let queue = crate::jobs::JobQueue::new();
    let (id, done) = queue
        .enqueue_awaitable(
            crate::jobs::JobMeta::new(crate::jobs::JobKind::Synth, None),
            Box::new(move |_| {
                let client = Client::with_base("test-key".into(), &base)?;
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                batch_run(&client, &path, &path, &path, "p", &s, &[(key, &w)], &|_| {}).map(|_| json!(null))
            }),
        )
        .await
        .unwrap();
    received.await.unwrap();
    queue.cancel(&id).await.unwrap();
    release.send(()).unwrap();
    assert_eq!(done.await.unwrap().unwrap_err(), crate::jobs::CANCELLED);
    assert_eq!(server.count(), 0);
    assert_eq!(fs::read(journal_path(root.path())).unwrap(), before);
    assert!(spending::spent(root.path()).is_empty());
}
