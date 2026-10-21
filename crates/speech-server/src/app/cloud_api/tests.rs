use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
    routing::{get, post},
    Router,
};
use tower::ServiceExt;

#[tokio::test]
async fn cloud_routes_keep_validation_errors_and_the_spending_response() {
    let root = tempfile::tempdir().unwrap();
    let state = AppState::new(root.path());
    crate::models::set_selection(&state.models_root, "cloud_daily_limit_usd", "2").unwrap();
    super::super::spending::record(
        &state.repo_root,
        super::super::spending::Spent {
            project: "p".into(),
            cost: Some(0.1),
            ..Default::default()
        },
    );
    let router = Router::new()
        .route("/cloud/form", get(form))
        .route("/cloud/estimate", post(estimate))
        .route("/cloud/spend", get(spend))
        .with_state(state);
    for request in [
        Request::get("/cloud/form?provider=unknown&id=m").body(Body::empty()).unwrap(),
        Request::post("/cloud/estimate")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"provider":"unknown","id":"m"}"#))
            .unwrap(),
    ] {
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(value["error"], "unknown_provider");
        assert!(value["detail"].as_str().unwrap().contains("unknown"));
    }
    let response = router
        .oneshot(Request::get("/cloud/spend?project=p").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(value["total"], json!(0.1));
    assert_eq!(value["daily_limit_usd"], json!(2.0));
    assert!(value["providers"].is_array() && value["models"].is_array() && value["days"].is_array());
}
