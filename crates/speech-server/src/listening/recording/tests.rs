use super::*;
use axum::{body::to_bytes, http::StatusCode};

#[tokio::test]
async fn stopping_an_idle_microphone_does_not_create_a_project() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    assert!(matches!(stop_recording(&workspace).await, Err(StopError::NotRecording)));
    assert!(!workspace.exists());
}

#[tokio::test]
async fn stop_results_keep_the_original_http_contract() {
    for (error, status, code, cause) in [
        (StopError::NotRecording, StatusCode::CONFLICT, "not_recording", None),
        (
            StopError::Join("join failed".into()),
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            Some("join failed"),
        ),
        (
            StopError::Failed("capture failed".into()),
            StatusCode::INTERNAL_SERVER_ERROR,
            "listen_failed",
            Some("capture failed"),
        ),
        (
            StopError::Failed(format!("{}: stale", crate::app::PROJECT_CHANGED)),
            StatusCode::CONFLICT,
            crate::app::PROJECT_CHANGED,
            None,
        ),
    ] {
        let response = stop_response(Err(error));
        assert_eq!(response.status(), status);
        let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(value["error"], code);
        if let Some(cause) = cause {
            assert_eq!(value["detail"], cause);
        }
    }
    let value = json!({"pid":"abc","project":{"source":"source.wav"}});
    let response = stop_response(Ok(value.clone()));
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap(),
        value
    );
}
