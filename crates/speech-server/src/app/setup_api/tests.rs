use super::*;
use axum::body::to_bytes;

#[tokio::test]
async fn component_errors_keep_their_status_code_and_cause() {
    for (code, status) in [
        ("busy", StatusCode::CONFLICT),
        ("disk_space", StatusCode::INSUFFICIENT_STORAGE),
        ("nothing_to_download", StatusCode::BAD_REQUEST),
        ("unknown_component", StatusCode::BAD_REQUEST),
        ("not_removable", StatusCode::BAD_REQUEST),
        ("no_ids", StatusCode::BAD_REQUEST),
        ("download_failed", StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        let response = dl_error_response(setup::DlError::new(code, "the original cause"));
        assert_eq!(response.status(), status);
        let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(value, json!({"code":code,"detail":"the original cause"}));
    }
}
