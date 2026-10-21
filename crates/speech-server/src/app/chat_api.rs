//! HTTP/SSE adapters for the Assistant panel.

use super::chat::{agent_conversation, assistant, conversation, ChatBody, Paths};
use crate::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::convert::Infallible;
use tokio::sync::mpsc;

const PREFERRED: &[&str] = &[
    "anthropic/claude-haiku-5.5",
    "google/gemini-3.8-flash",
    "anthropic/claude-sonnet-5.5",
];

/// POST /chat: a message of the Assistant panel, answered as a stream of events.
pub async fn chat(State(st): State<AppState>, Json(body): Json<ChatBody>) -> Response {
    if body.messages.iter().all(|m| m.content.trim().is_empty()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "chat_empty", "detail": t!("chat-empty") })),
        )
            .into_response();
    }
    let (events, received) = mpsc::channel::<Value>(256);
    if assistant(&st.models_root) == "agent" {
        tokio::spawn(agent_conversation(body, events));
    } else {
        let Some(key) = crate::engines::models::openrouter_key() else {
            return (
                StatusCode::PRECONDITION_FAILED,
                Json(json!({ "error": "no_key", "detail": t!("cloud-no-key", provider = "OpenRouter") })),
            )
                .into_response();
        };
        if body.model.trim().is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "no_model", "detail": t!("chat-no-model") })),
            )
                .into_response();
        }
        tokio::spawn(conversation(
            Paths {
                repo_root: st.repo_root.clone(),
                models_root: st.models_root.clone(),
            },
            body,
            key,
            events,
        ));
    }
    event_stream(received)
}

fn event_stream(received: mpsc::Receiver<Value>) -> Response {
    let stream = tokio_stream::wrappers::ReceiverStream::new(received)
        .map(|event| Ok::<Event, Infallible>(Event::default().data(event.to_string())));
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

/// GET /chat/models: the OpenRouter models that can work the tools, the one to start with, and who answers.
pub async fn models(State(st): State<AppState>) -> Response {
    let models_root = st.models_root.clone();
    let listed = tokio::task::spawn_blocking(move || crate::integrations::openrouter::catalog(&models_root)).await;
    let cached = match listed {
        Ok(Ok(cached)) => cached,
        Ok(Err(e)) => return (StatusCode::BAD_GATEWAY, Json(json!({ "error": "catalog", "detail": e }))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let usable: Vec<_> = cached
        .models
        .iter()
        .filter(|m| {
            m.output_modalities.iter().any(|o| o == "text")
                && m.supported_parameters.iter().any(|p| p == "tools")
                && !m.id.ends_with(":batch")
        })
        .collect();
    let chosen = crate::engines::models::load_selection(&st.models_root)
        .get("assistant_model")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|id| usable.iter().any(|m| &m.id == id));
    let default = chosen
        .or_else(|| {
            PREFERRED
                .iter()
                .find(|id| usable.iter().any(|m| m.id == **id))
                .map(|id| id.to_string())
        })
        .or_else(|| usable.first().map(|m| m.id.clone()));
    let list: Vec<Value> = usable
        .iter()
        .map(|m| json!({ "id": m.id, "name": m.name, "pricing": m.pricing }))
        .collect();
    Json(json!({ "models": list, "default": default, "assistant": assistant(&st.models_root), "has_key": crate::engines::models::openrouter_key().is_some() })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::post, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn empty_chat_keeps_its_http_error_contract() {
        let root = tempfile::tempdir().unwrap();
        let app = Router::new().route("/chat", post(chat)).with_state(AppState::new(root.path()));
        let request = Request::builder()
            .method("POST")
            .uri("/chat")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"messages":[{"role":"user","content":"  "}]}"#))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap()).unwrap();
        assert_eq!(body["error"], "chat_empty");
        assert!(!body["detail"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn panel_events_keep_the_sse_data_envelope() {
        let events = [json!({"type":"text","text":"Привет 🌍"}), json!({"type":"done","cost":0.25})];
        let (send, received) = mpsc::channel(2);
        for event in &events {
            send.send(event.clone()).await.unwrap();
        }
        drop(send);
        let response = event_stream(received);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let body = std::str::from_utf8(&bytes).unwrap();
        let decoded: Vec<Value> = body
            .split("\n\n")
            .filter(|event| !event.is_empty())
            .map(|event| serde_json::from_str(event.strip_prefix("data: ").unwrap()).unwrap())
            .collect();
        assert_eq!(decoded, events);
    }
}
