//! The Assistant panel (VideoSOS `chat.rs`): an assistant on the person's OpenRouter key that works Speech Studio
//! with the very tools an agent connected over MCP has, so its changes go into the journal, its paid runs count
//! against the agents' daily limit and the window captions what it does. The server runs the model's loop (a turn, its
//! tool calls, their answers, the next turn) and streams the panel what happens: the text as it comes, each tool call
//! and how it ended, and what the conversation cost. With «Agent (MCP)» as the assistant, the person's message goes to
//! the connected agent instead and nothing is paid.

use std::convert::Infallible;
use std::path::PathBuf;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio::sync::mpsc;

use crate::AppState;

const API: &str = "https://openrouter.ai/api/v1/chat/completions";
/// Turns one message may take before the assistant has to stop and say where it got to.
const MAX_TURNS: usize = 60;
/// Models offered first, best first; the person picks any other that works tools.
const PREFERRED: &[&str] = &["anthropic/claude-haiku-5.5", "google/gemini-3.8-flash", "anthropic/claude-sonnet-5.5"];

#[derive(Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
pub struct ChatBody {
    #[serde(default)]
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

/// Who answers the Assistant panel: `openrouter` (a model on the person's key) or `agent` (the connected MCP agent).
pub fn assistant(models_root: &std::path::Path) -> String {
    match crate::models::load_selection(models_root).get("assistant").and_then(Value::as_str) {
        Some("agent") => "agent".into(),
        _ => "openrouter".into(),
    }
}

fn system_prompt(project: Option<&str>, language: Option<&str>) -> String {
    let mut prompt = String::from(
        "You are the assistant inside Speech Studio, a desktop studio that voices text (Speech: projects of blocks, \
         voices, engines Higgs, Drama 3 and cloud ones) and recognises speech and checks a voicing (Listening). You work \
         the program with its tools while the person watches the window: look before you change (project_get, \
         ui_read_page), say what you are about to do when it costs money, and answer briefly. Cloud engines are paid by \
         the person's keys: ask before voicing with one.\n\n",
    );
    if let Some(project) = project.filter(|project| !project.is_empty()) {
        prompt.push_str(&format!("The project open in the window is {project}: pass it as pid where a tool takes one.\n"));
    }
    if let Some(language) = language.filter(|language| !language.is_empty()) {
        prompt.push_str(&format!("The window's language is {language}: answer in the person's language.\n"));
    }
    prompt.push('\n');
    prompt.push_str(crate::mcp::SKILL);
    prompt
}

/// false once the panel is gone: the conversation stops there.
async fn send(events: &mpsc::Sender<Value>, event: Value) -> bool {
    events.send(event).await.is_ok()
}

/// One streamed turn of the model: its text, as it comes, and the tool calls it asks for.
struct Turn {
    text: String,
    calls: Vec<(String, String, String)>,
    cost: f64,
}

async fn turn(http: &reqwest::Client, key: &str, model: &str, messages: &[Value], events: &mpsc::Sender<Value>) -> Result<Turn, String> {
    // the guide and the tools are the same every step: cached, they cost a tenth from the second step on (Claude);
    // models that cache on their own ignore it
    let body = json!({ "model": model, "messages": messages, "tools": crate::mcp::chat_tools(), "stream": true, "usage": { "include": true }, "cache_control": { "type": "ephemeral" } });
    let response = http.post(API).bearer_auth(key).header("X-Title", "Speech Studio").json(&body).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let said = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string));
        return Err(said.unwrap_or_else(|| format!("{status}: {text}")));
    }
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut turn = Turn { text: String::new(), calls: Vec::new(), cost: 0.0 };
    // a tool call comes in pieces by its index: its id and name first, then its arguments bit by bit
    let mut partial: Vec<(String, String, String)> = Vec::new();
    while let Some(chunk) = stream.next().await {
        buffer.push_str(&String::from_utf8_lossy(&chunk.map_err(|e| e.to_string())?));
        while let Some(end) = buffer.find('\n') {
            let line = buffer[..end].trim().to_string();
            buffer.drain(..=end);
            let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue };
            if data == "[DONE]" {
                continue;
            }
            let event = match serde_json::from_str::<Value>(data) {
                Ok(event) => event,
                Err(e) => {
                    tracing::warn!("OpenRouter sent a stream line that is not JSON ({e}): {data}");
                    continue;
                }
            };
            if let Some(error) = event.get("error") {
                return Err(error.get("message").and_then(Value::as_str).unwrap_or("the model failed").to_string());
            }
            if let Some(cost) = event.pointer("/usage/cost").and_then(Value::as_f64) {
                turn.cost += cost;
            }
            let Some(delta) = event.pointer("/choices/0/delta") else { continue };
            if let Some(text) = delta.get("content").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                turn.text.push_str(text);
                if !send(events, json!({ "type": "text", "text": text })).await {
                    return Err("the panel left".into());
                }
            }
            for call in delta.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                while partial.len() <= index {
                    partial.push((String::new(), String::new(), String::new()));
                }
                let slot = &mut partial[index];
                if let Some(id) = call.get("id").and_then(Value::as_str) {
                    slot.0.push_str(id);
                }
                if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                    slot.1.push_str(name);
                }
                if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str) {
                    slot.2.push_str(arguments);
                }
            }
        }
    }
    turn.calls = partial.into_iter().filter(|(_, name, _)| !name.is_empty()).collect();
    Ok(turn)
}

struct Paths {
    repo_root: PathBuf,
    models_root: PathBuf,
}

async fn conversation(paths: Paths, body: ChatBody, key: String, events: mpsc::Sender<Value>) {
    let cost = converse(&paths, &body, &key, &events).await;
    // the conversation is paid like a cloud run: it goes into the spending and the agents' daily limit
    if cost > 0.0 {
        crate::cloud::record(&paths.repo_root, crate::cloud::Spent {
            at: chrono::Utc::now().timestamp_millis(),
            day: chrono::Local::now().format("%Y-%m-%d").to_string(),
            project: body.project_id.clone().unwrap_or_default(),
            provider: "openrouter".into(),
            model: body.model.clone(),
            task: "chat".into(),
            estimate: None,
            cost: Some(cost),
            cost_source: "provider".into(),
            origin: "agent".into(),
        });
    }
}

/// The panel when the person made the connected agent the assistant: their message is a request for the agent, which
/// does what it asks with the tools it already has and answers in words.
async fn agent_conversation(body: ChatBody, events: mpsc::Sender<Value>) {
    let history: Vec<String> = body.messages.iter().filter(|message| !message.content.trim().is_empty()).map(|message| format!("{}: {}", message.role, message.content.trim())).collect();
    let asked = history.last().cloned().unwrap_or_default();
    let question = json!({
        "target": "chat",
        "instructions": format!(
            "The person writes to you in Speech Studio's Assistant panel{}. Do what they ask with the Speech Studio tools you have (the panel shows each call you make until you answer), then answer them in words, briefly, in the language with the code \"{}\". The conversation so far is in context.",
            body.project_id.as_deref().map(|id| format!(" with the project {id} open")).unwrap_or_default(),
            body.language.as_deref().unwrap_or("en"),
        ),
        "request": asked,
        "context": history,
        "project_id": body.project_id,
    });
    let following = crate::mcp::follow_agent_steps(events.clone());
    let answered = crate::mcp::ask_agent(question).await;
    drop(following);
    match answered {
        Ok(answer) => {
            send(&events, json!({ "type": "text", "text": answer })).await;
            send(&events, json!({ "type": "done", "cost": 0.0 })).await;
        }
        Err(error) => {
            send(&events, json!({ "type": "error", "message": error, "cost": 0.0 })).await;
        }
    }
}

/// Runs the conversation to its end and answers with what it cost.
async fn converse(paths: &Paths, body: &ChatBody, key: &str, events: &mpsc::Sender<Value>) -> f64 {
    let http = match dub_llm::net::async_builder().timeout(std::time::Duration::from_secs(600)).build() {
        Ok(http) => http,
        Err(e) => {
            send(events, json!({ "type": "error", "message": e.to_string(), "cost": 0.0 })).await;
            return 0.0;
        }
    };
    let mut messages = vec![json!({ "role": "system", "content": system_prompt(body.project_id.as_deref(), body.language.as_deref()) })];
    for message in &body.messages {
        if matches!(message.role.as_str(), "user" | "assistant") && !message.content.trim().is_empty() {
            messages.push(json!({ "role": message.role, "content": message.content }));
        }
    }
    let mut cost = 0.0;
    for _ in 0..MAX_TURNS {
        if let Err(limit) = crate::cloud::check_limit_as("agent", &paths.repo_root, &paths.models_root, Some(cost)) {
            send(events, json!({ "type": "error", "message": limit, "cost": cost })).await;
            return cost;
        }
        let turn = match turn(&http, key, &body.model, &messages, events).await {
            Ok(turn) => turn,
            Err(error) => {
                send(events, json!({ "type": "error", "message": error, "cost": cost })).await;
                return cost;
            }
        };
        cost += turn.cost;
        if turn.calls.is_empty() {
            send(events, json!({ "type": "done", "cost": cost })).await;
            return cost;
        }
        if !send(events, json!({ "type": "cost", "cost": cost })).await {
            return cost;
        }
        let calls: Vec<Value> = turn.calls.iter().map(|(id, name, arguments)| json!({ "id": id, "type": "function", "function": { "name": name, "arguments": arguments } })).collect();
        messages.push(json!({ "role": "assistant", "content": turn.text, "tool_calls": calls }));
        let mut pictures = Vec::new();
        for (id, name, arguments) in &turn.calls {
            let parsed: Result<Value, String> = if arguments.trim().is_empty() {
                Ok(Value::Object(Map::new()))
            } else {
                serde_json::from_str(arguments).map_err(|error| format!("the arguments of {name} are not JSON ({error}): call it again with a JSON object"))
            };
            let args = parsed.clone().unwrap_or(Value::Null);
            if !send(events, json!({ "type": "tool", "id": id, "name": name, "args": args })).await {
                return cost;
            }
            let answer = match parsed {
                Ok(args) => crate::mcp::chat_call(name, &args).await,
                Err(problem) => Err(problem),
            };
            let (ok, text) = match &answer {
                Ok(answer) => (true, answer.text.clone()),
                Err(problem) => (false, problem.clone()),
            };
            if let Ok(crate::mcp::ChatAnswer { image: Some(image), .. }) = &answer {
                pictures.push(image.clone());
            }
            send(events, json!({ "type": "tool_done", "id": id, "ok": ok, "summary": text.chars().take(240).collect::<String>() })).await;
            messages.push(json!({ "role": "tool", "tool_call_id": id, "content": if ok { text } else { format!("Error: {text}") } }));
        }
        // a tool's picture (the window's screenshot) reaches the model as a picture of the next message
        if !pictures.is_empty() {
            let mut content = vec![json!({ "type": "text", "text": "The pictures the tools above answered with." })];
            content.extend(pictures.into_iter().map(|url| json!({ "type": "image_url", "image_url": { "url": url } })));
            messages.push(json!({ "role": "user", "content": content }));
        }
    }
    send(events, json!({ "type": "error", "message": t!("chat-too-many-turns", turns = MAX_TURNS as i64), "cost": cost })).await;
    cost
}

/// POST /chat: a message of the Assistant panel, answered as a stream of events.
pub async fn chat(State(st): State<AppState>, Json(body): Json<ChatBody>) -> Response {
    if body.messages.iter().all(|m| m.content.trim().is_empty()) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "chat_empty", "detail": t!("chat-empty") }))).into_response();
    }
    let (events, received) = mpsc::channel::<Value>(256);
    if assistant(&st.models_root) == "agent" {
        tokio::spawn(agent_conversation(body, events));
    } else {
        let Some(key) = crate::models::openrouter_key() else {
            return (StatusCode::PRECONDITION_FAILED, Json(json!({ "error": "no_key", "detail": t!("cloud-no-key", provider = "OpenRouter") }))).into_response();
        };
        if body.model.trim().is_empty() {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "no_model", "detail": t!("chat-no-model") }))).into_response();
        }
        tokio::spawn(conversation(Paths { repo_root: st.repo_root.clone(), models_root: st.models_root.clone() }, body, key, events));
    }
    let stream = tokio_stream::wrappers::ReceiverStream::new(received).map(|event| Ok::<Event, Infallible>(Event::default().data(event.to_string())));
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

/// GET /chat/models: the OpenRouter models that can work the tools, the one to start with, and who answers.
pub async fn models(State(st): State<AppState>) -> Response {
    let models_root = st.models_root.clone();
    let listed = tokio::task::spawn_blocking(move || crate::openrouter::catalog(&models_root)).await;
    let cached = match listed {
        Ok(Ok(cached)) => cached,
        Ok(Err(e)) => return (StatusCode::BAD_GATEWAY, Json(json!({ "error": "catalog", "detail": e }))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let usable: Vec<_> = cached
        .models
        .iter()
        .filter(|m| m.output_modalities.iter().any(|o| o == "text") && m.supported_parameters.iter().any(|p| p == "tools") && !m.id.ends_with(":batch"))
        .collect();
    let chosen = crate::models::load_selection(&st.models_root).get("assistant_model").and_then(Value::as_str).map(str::to_string).filter(|id| usable.iter().any(|m| &m.id == id));
    let default = chosen.or_else(|| PREFERRED.iter().find(|id| usable.iter().any(|m| m.id == **id)).map(|id| id.to_string())).or_else(|| usable.first().map(|m| m.id.clone()));
    let list: Vec<Value> = usable.iter().map(|m| json!({ "id": m.id, "name": m.name, "pricing": m.pricing })).collect();
    Json(json!({ "models": list, "default": default, "assistant": assistant(&st.models_root), "has_key": crate::models::openrouter_key().is_some() })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_names_the_project_and_the_language_and_carries_the_skill() {
        let prompt = system_prompt(Some("abc123"), Some("ru"));
        assert!(prompt.contains("abc123") && prompt.contains("ru"));
        assert!(prompt.contains(crate::mcp::SKILL));
    }

    #[test]
    fn every_tool_but_the_assistant_requests_is_offered_to_the_chat() {
        let tools = crate::mcp::chat_tools();
        assert!(tools.iter().all(|tool| tool["type"] == "function" && tool["function"]["parameters"]["type"] == "object"));
        assert!(tools.iter().any(|tool| tool["function"]["name"] == "project_update"));
        assert!(!tools.iter().any(|tool| tool["function"]["name"].as_str().is_some_and(|n| n.starts_with("assistant_"))));
    }
}
