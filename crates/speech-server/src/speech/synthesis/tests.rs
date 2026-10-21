use super::*;
use axum::body::to_bytes;

#[tokio::test]
async fn a_missing_project_is_refused_before_aloud_or_queue_work() {
    let root = tempfile::tempdir().unwrap();
    let paths = SpeechPaths {
        repo_root: root.path().into(),
        workspace: root.path().join("workspace"),
        voices_dir: root.path().join("voices"),
        models_root: root.path().join("models"),
        higgs_dll: root.path().join("missing.dll"),
        threads: 1,
    };
    let queue = jobs::JobQueue::new();
    let response = enqueue(&paths, &queue, "absent".into(), json!({"aloud":true,"ids":["block"]})).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(value["error"], "no_project");
    assert!(!value["detail"].as_str().unwrap().is_empty());
    assert!(queue.list(None).await.is_empty());
    assert!(!paths.workspace.exists());
}
