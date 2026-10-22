use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn assembled_router_keeps_guards_protocols_methods_and_body_limits() {
    const CHILD: &str = "SPEECH_TEST_ROUTER_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::router::tests::assembled_router_keeps_guards_protocols_methods_and_body_limits",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("SPEECH_STUDIO_API_TOKEN", "router-contract-token")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    // Router assembly installs process-wide tools and journal state; isolate it from other unit tests.
    let root = tempfile::tempdir().unwrap();
    let state = AppState::new(root.path());
    let api = build_router(state);
    let request = |method, path: &str, body: Body| {
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, "127.0.0.1:8797")
            .body(body)
            .unwrap()
    };

    for path in ["/health", "/setup/status", "/mcp/status"] {
        let response = api.clone().oneshot(request(Method::GET, path, Body::empty())).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
        if path == "/setup/status" {
            assert_eq!(value["ready"], false);
            assert!(value["components"].as_array().unwrap().len() > 20);
            assert!(value["active"].is_null());
        }
    }
    let mut foreign = request(Method::GET, "/v1/models", Body::empty());
    foreign.headers_mut().insert(header::HOST, "evil.example".parse().unwrap());
    assert_eq!(api.clone().oneshot(foreign).await.unwrap().status(), StatusCode::FORBIDDEN);
    let mut origin = request(Method::POST, "/setup/download", Body::from("{}"));
    origin.headers_mut().insert(header::ORIGIN, "https://evil.example".parse().unwrap());
    assert_eq!(api.clone().oneshot(origin).await.unwrap().status(), StatusCode::FORBIDDEN);
    let mut no_host = Request::builder().uri("/health").body(Body::empty()).unwrap();
    no_host.headers_mut().remove(header::HOST);
    assert_eq!(api.clone().oneshot(no_host).await.unwrap().status(), StatusCode::FORBIDDEN);
    assert_eq!(
        api.clone()
            .oneshot(request(Method::POST, "/health", Body::empty()))
            .await
            .unwrap()
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );

    let mut preflight = request(Method::OPTIONS, "/setup/download", Body::empty());
    preflight
        .headers_mut()
        .insert(header::ORIGIN, "http://localhost:5173".parse().unwrap());
    preflight
        .headers_mut()
        .insert(header::ACCESS_CONTROL_REQUEST_METHOD, "POST".parse().unwrap());
    let response = api.clone().oneshot(preflight).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "http://localhost:5173");
    let unauthorized = api
        .clone()
        .oneshot(request(Method::GET, "/v1/models", Body::empty()))
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let mut authorized = request(Method::GET, "/v1/models", Body::empty());
    authorized
        .headers_mut()
        .insert(header::AUTHORIZATION, "Bearer router-contract-token".parse().unwrap());
    let response = api.clone().oneshot(authorized).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(value["object"], "list");

    for (method, params) in [
        (
            "initialize",
            json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}),
        ),
        ("tools/list", json!({})),
    ] {
        let mut req = request(
            Method::POST,
            "/mcp",
            Body::from(json!({"jsonrpc":"2.0","id":7,"method":method,"params":params}).to_string()),
        );
        req.headers_mut().insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        let response = api.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(value["id"], 7);
        assert!(value["error"].is_null(), "{value}");
        if method == "tools/list" {
            assert!(value["result"]["tools"].as_array().unwrap().len() > 50);
        }
    }
    let big = format!("{{\"ids\":[],\"padding\":\"{}\"}}", "x".repeat(3 * 1024 * 1024));
    let mut req = request(Method::POST, "/setup/download", Body::from(big));
    req.headers_mut().insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    let response = api.clone().oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "the regular API keeps its disabled default body limit"
    );
    let value: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(value["code"], "no_ids");
    let mut req = request(Method::POST, "/mcp/window/result", Body::from(vec![b' '; 64 * 1024 * 1024 + 1]));
    req.headers_mut().insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    assert_eq!(api.oneshot(req).await.unwrap().status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(!root.path().join("models/active.json").exists());
}
