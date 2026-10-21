use std::fs;

use super::*;
use crate::app::{context::RequestContext, spending};
use dub_llm::test_http::{serve, Reply};

fn form() -> ModelForm {
    serde_json::from_value(json!({"provider":"fal","id":"model","fields":[],"delivery":"async",
        "price":{"rates":[{"amount":0.25,"unit":"output"}]}}))
    .unwrap()
}

fn request(root: &Path) -> Run<'_> {
    Run {
        repo_root: root,
        models_root: root,
        project: "p",
        task: Task::Speech,
        handle_file: root,
    }
}

#[test]
fn a_paid_result_is_recorded_before_a_later_audio_decode_error() {
    let root = tempfile::tempdir().unwrap();
    let handle = root.path().join("pending.json");
    let run = Run {
        handle_file: &handle,
        ..request(root.path())
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let done = rt.block_on(RequestContext::new("api".into(), None).run(async {
        run_with(
            Provider::Fal,
            "model",
            &form(),
            &Map::new(),
            &run,
            || {
                Ok(Submitted::Done {
                    outputs: vec![Output::Data {
                        data_base64: "broken base64".into(),
                        mime: "audio/wav".into(),
                    }],
                    cost: Some(0.12),
                })
            },
            |_| panic!("synchronous results are not polled"),
        )
        .unwrap()
    }));
    assert_eq!(done.cost, Some(0.12));
    assert_eq!(done.estimate, Some(0.25));
    assert!(!handle.exists());
    assert!(Client::new().unwrap().audio(&done.outputs).is_err());
    let rows = spending::spent(root.path());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].origin, "api");
    assert_eq!(rows[0].cost, Some(0.12));
    assert_eq!(rows[0].cost_source, "provider");
    assert_eq!(rows[0].project, "p");
}

#[test]
fn pending_handles_are_saved_before_polling_and_a_network_error_can_resume_without_submission() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("pending.json");
    let run = Run {
        handle_file: &path,
        ..request(root.path())
    };
    let server = serve(vec![
        Reply::json(200, r#"{"status":"COMPLETED"}"#),
        Reply::json(200, r#"{"text":"Hello"}"#),
    ]);
    let handle =
        json!({"request_id":"paid","status_url":format!("{}/status",server.base()),"response_url":format!("{}/result",server.base())});
    let error = run_with(
        Provider::Fal,
        "model",
        &form(),
        &Map::new(),
        &run,
        || Ok(Submitted::Pending { handle: handle.clone() }),
        |seen| {
            assert_eq!(seen, &handle);
            assert_eq!(serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(), handle);
            Err("connection lost".into())
        },
    )
    .err()
    .unwrap();
    assert!(error.contains("connection lost"));
    assert!(spending::spent(root.path()).is_empty());
    let client = Client::new().unwrap();
    let done = run_with(
        Provider::Fal,
        "model",
        &form(),
        &Map::new(),
        &run,
        || panic!("an existing provider operation must not be submitted again"),
        |handle| block(client.poll(Provider::Fal, "test-key", handle)).map_err(|e| e.to_string()),
    )
    .unwrap();
    assert_eq!(done.outputs, [Output::Text { text: "Hello".into() }]);
    assert_eq!(server.count(), 2);
    assert!(server.request(0).starts_with("GET /status"));
    assert!(server.request(0).to_lowercase().contains("authorization: key test-key"));
    assert!(!path.exists());
    assert_eq!(spending::spent(root.path())[0].cost_source, "estimate");
}

#[test]
fn the_daily_limit_refuses_before_submitting_but_does_not_block_an_already_paid_operation() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    crate::models::set_selection(root.path(), "cloud_daily_limit_usd", "0.1").unwrap();
    let handle = root.path().join("pending.json");
    let run = Run {
        handle_file: &handle,
        ..request(root.path())
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(RequestContext::new("agent".into(), None).run(async {
        let denied = run_with(
            Provider::Fal,
            "model",
            &form(),
            &Map::new(),
            &run,
            || panic!("budget must be checked before a paid submission"),
            |_| panic!(),
        )
        .err()
        .unwrap();
        assert!(denied.contains(&t!("cloud-limit-reached", used = "0.00", limit = "0.10")));
        fs::write(&handle, b"{\"id\":\"already-paid\"}").unwrap();
        run_with(
            Provider::Fal,
            "model",
            &form(),
            &Map::new(),
            &run,
            || panic!("must resume"),
            |_| {
                Ok(Polled::Done {
                    outputs: vec![],
                    cost: Some(0.3),
                })
            },
        )
        .unwrap();
    }));
    assert_eq!(spending::spent(root.path())[0].cost, Some(0.3));
}

#[test]
fn a_definitive_failure_removes_the_handle_and_submission_errors_are_not_retried() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("pending.json");
    let run = Run {
        handle_file: &path,
        ..request(root.path())
    };
    fs::write(&path, b"{\"id\":\"failed\"}").unwrap();
    let error = run_with(
        Provider::Fal,
        "model",
        &form(),
        &Map::new(),
        &run,
        || panic!(),
        |_| {
            Ok(Polled::Failed {
                message: "provider refused".into(),
            })
        },
    )
    .err()
    .unwrap();
    assert!(error.contains("provider refused"));
    assert!(!path.exists());
    let calls = std::cell::Cell::new(0);
    let error = run_with(
        Provider::Fal,
        "model",
        &form(),
        &Map::new(),
        &run,
        || {
            calls.set(calls.get() + 1);
            Err("submission answer lost".into())
        },
        |_| panic!(),
    )
    .err()
    .unwrap();
    assert!(error.contains("submission answer lost"));
    assert_eq!(calls.get(), 1);
    assert!(spending::spent(root.path()).is_empty());
}

#[tokio::test]
async fn cancellation_keeps_the_provider_handle_without_polling_or_recording_a_result() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("pending.json");
    fs::write(&path, b"{\"id\":\"paid\"}").unwrap();
    let owned = root.path().to_path_buf();
    let (started, received) = tokio::sync::oneshot::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let queue = crate::jobs::JobQueue::new();
    let (id, done) = queue
        .enqueue_awaitable(
            crate::jobs::JobMeta::new(crate::jobs::JobKind::Synth, None),
            Box::new(move |_| {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                let path = owned.join("pending.json");
                let run = Run {
                    handle_file: &path,
                    ..request(&owned)
                };
                run_with(
                    Provider::Fal,
                    "model",
                    &form(),
                    &Map::new(),
                    &run,
                    || panic!(),
                    |_| panic!("cancelled operation must not poll"),
                )
                .map(|_| json!(null))
            }),
        )
        .await
        .unwrap();
    received.await.unwrap();
    queue.cancel(&id).await.unwrap();
    release.send(()).unwrap();
    assert_eq!(done.await.unwrap().unwrap_err(), crate::jobs::CANCELLED);
    assert!(path.exists());
    assert!(spending::spent(root.path()).is_empty());
}
