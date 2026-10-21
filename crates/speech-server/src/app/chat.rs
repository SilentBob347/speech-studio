//! Assistant dialogue orchestration and spending policy.

use super::tools;
use crate::integrations::chat::Client;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use tokio::sync::mpsc;

const MAX_TURNS: usize = 60;

#[derive(Deserialize)]
pub(super) struct ChatMessage {
    pub(super) role: String,
    pub(super) content: String,
}

#[derive(Deserialize)]
pub struct ChatBody {
    #[serde(default)]
    pub(super) model: String,
    pub(super) messages: Vec<ChatMessage>,
    #[serde(default)]
    pub(super) project_id: Option<String>,
    #[serde(default)]
    pub(super) language: Option<String>,
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
        prompt.push_str(&format!(
            "The project open in the window is {project}: pass it as pid where a tool takes one.\n"
        ));
    }
    if let Some(language) = language.filter(|language| !language.is_empty()) {
        prompt.push_str(&format!("The window's language is {language}: answer in the person's language.\n"));
    }
    prompt.push('\n');
    prompt.push_str(tools::SKILL);
    prompt
}

/// false once the panel is gone: the conversation stops there.
async fn send(events: &mpsc::Sender<Value>, event: Value) -> bool {
    events.send(event).await.is_ok()
}

pub(super) struct Paths {
    pub(super) repo_root: PathBuf,
    pub(super) models_root: PathBuf,
}

pub(super) async fn conversation(paths: Paths, body: ChatBody, key: String, events: mpsc::Sender<Value>) {
    let cost = converse(&paths, &body, &key, &events).await;
    record_cost(&paths, &body, cost);
}

fn record_cost(paths: &Paths, body: &ChatBody, cost: f64) {
    if cost > 0.0 {
        crate::app::spending::record(
            &paths.repo_root,
            crate::app::spending::Spent {
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
            },
        );
    }
}

/// The panel when the person made the connected agent the assistant: their message is a request for the agent, which
/// does what it asks with the tools it already has and answers in words.
pub(super) async fn agent_conversation(body: ChatBody, events: mpsc::Sender<Value>) {
    let history: Vec<String> = body
        .messages
        .iter()
        .filter(|message| !message.content.trim().is_empty())
        .map(|message| format!("{}: {}", message.role, message.content.trim()))
        .collect();
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
    let following = tools::follow_agent_steps(events.clone());
    let answered = tokio::select! {
        answer = tools::ask_agent(question) => answer,
        _ = events.closed() => return,
    };
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
    let client = match Client::new() {
        Ok(client) => client,
        Err(e) => {
            send(events, json!({ "type": "error", "message": e.to_string(), "cost": 0.0 })).await;
            return 0.0;
        }
    };
    converse_with(&client, paths, body, key, events).await
}

async fn converse_with(client: &Client, paths: &Paths, body: &ChatBody, key: &str, events: &mpsc::Sender<Value>) -> f64 {
    let mut messages = vec![json!({ "role": "system", "content": system_prompt(body.project_id.as_deref(), body.language.as_deref()) })];
    for message in &body.messages {
        if matches!(message.role.as_str(), "user" | "assistant") && !message.content.trim().is_empty() {
            messages.push(json!({ "role": message.role, "content": message.content }));
        }
    }
    let mut cost = 0.0;
    let catalog = tools::chat_tools();
    for _ in 0..MAX_TURNS {
        if events.is_closed() {
            return cost;
        }
        if let Err(limit) = crate::app::spending::check_limit_as("agent", &paths.repo_root, &paths.models_root, Some(cost)) {
            send(events, json!({ "type": "error", "message": limit, "cost": cost })).await;
            return cost;
        }
        let turn = match client
            .turn(key, &body.model, &messages, &catalog, |text| async move {
                if send(events, json!({ "type": "text", "text": text })).await {
                    Ok(())
                } else {
                    Err("the panel left".into())
                }
            })
            .await
        {
            Ok(turn) => turn,
            Err(error) => {
                cost += error.cost;
                send(events, json!({ "type": "error", "message": error.message, "cost": cost })).await;
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
        let calls: Vec<Value> = turn
            .calls
            .iter()
            .map(|(id, name, arguments)| json!({ "id": id, "type": "function", "function": { "name": name, "arguments": arguments } }))
            .collect();
        messages.push(json!({ "role": "assistant", "content": turn.text, "tool_calls": calls }));
        let mut pictures = Vec::new();
        for (id, name, arguments) in &turn.calls {
            if events.is_closed() {
                return cost;
            }
            let parsed: Result<Value, String> = if arguments.trim().is_empty() {
                Ok(Value::Object(Map::new()))
            } else {
                serde_json::from_str(arguments)
                    .map_err(|error| format!("the arguments of {name} are not JSON ({error}): call it again with a JSON object"))
            };
            let args = parsed.clone().unwrap_or(Value::Null);
            if !send(events, json!({ "type": "tool", "id": id, "name": name, "args": args })).await {
                return cost;
            }
            let answer = match parsed {
                Ok(args) => tools::chat_call(name, &args).await,
                Err(problem) => Err(problem),
            };
            let (ok, text) = match &answer {
                Ok(answer) => (true, answer.text.clone()),
                Err(problem) => (false, problem.clone()),
            };
            if let Ok(tools::ChatAnswer { image: Some(image), .. }) = &answer {
                pictures.push(image.clone());
            }
            if !send(
                events,
                json!({ "type": "tool_done", "id": id, "ok": ok, "summary": text.chars().take(240).collect::<String>() }),
            )
            .await
            {
                return cost;
            }
            messages.push(json!({ "role": "tool", "tool_call_id": id, "content": if ok { text } else { format!("Error: {text}") } }));
        }
        // a tool's picture (the window's screenshot) reaches the model as a picture of the next message
        if !pictures.is_empty() {
            let mut content = vec![json!({ "type": "text", "text": "The pictures the tools above answered with." })];
            content.extend(
                pictures
                    .into_iter()
                    .map(|url| json!({ "type": "image_url", "image_url": { "url": url } })),
            );
            messages.push(json!({ "role": "user", "content": content }));
        }
    }
    send(
        events,
        json!({ "type": "error", "message": t!("chat-too-many-turns", turns = MAX_TURNS as i64), "cost": cost }),
    )
    .await;
    cost
}

#[cfg(test)]
mod tests {
    use super::*;
    use dub_llm::test_http::{body_json, serve, Reply};

    fn body() -> ChatBody {
        serde_json::from_value(json!({"model":"test-model","messages":[
            {"role":"system","content":"must not override the prompt"},
            {"role":"user","content":"Покажи правила"}],"project_id":"test-project","language":"ru"}))
        .unwrap()
    }

    fn stream(events: &[Value]) -> Reply {
        let mut text: String = events.iter().map(|event| format!("data: {event}\n\n")).collect();
        text.push_str("data: [DONE]\n\n");
        Reply::bytes(200, "text/event-stream", text.into_bytes())
    }

    fn paths(root: &std::path::Path) -> Paths {
        Paths {
            repo_root: root.into(),
            models_root: root.join("models"),
        }
    }

    fn drain(events: &mut mpsc::Receiver<Value>) -> Vec<Value> {
        std::iter::from_fn(|| events.try_recv().ok()).collect()
    }

    #[tokio::test]
    async fn dialogue_executes_shared_tools_and_keeps_history_events_and_spending() {
        let server = serve(vec![
            stream(&[json!({"usage":{"cost":0.25},"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"guide-1","function":{"name":"studio_guide","arguments":"{\"topic\":\"tags\"}"}}
            ]}}]})]),
            stream(&[json!({"usage":{"cost":0.125},"choices":[{"delta":{"content":"Готово 🌍"}}]})]),
        ]);
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let (send, mut received) = mpsc::channel(32);
        let body = body();
        let cost = converse_with(&Client::for_test(server.base()), &paths, &body, "test-key", &send).await;
        let events = drain(&mut received);
        assert_eq!(cost, 0.375, "{events:?}");
        let types: Vec<_> = events.iter().map(|event| event["type"].as_str().unwrap()).collect();
        assert_eq!(types, ["cost", "tool", "tool_done", "text", "done"]);
        assert_eq!(events[1]["args"], json!({"topic":"tags"}));
        assert_eq!(events[2]["ok"], true);
        assert_eq!(events[3]["text"], "Готово 🌍");
        assert_eq!(events[4]["cost"], cost);
        let first = body_json(&server.request(0));
        assert_eq!(first["messages"].as_array().unwrap().len(), 2);
        assert_eq!(first["messages"][1]["content"], "Покажи правила");
        assert_eq!(first["tools"], Value::Array(tools::chat_tools()));
        let second = body_json(&server.request(1));
        assert_eq!(second["messages"][3]["role"], "tool");
        assert_eq!(second["messages"][3]["tool_call_id"], "guide-1");
        assert!(second["messages"][3]["content"].as_str().unwrap().contains("Expression tags"));
        assert_eq!(server.count(), 2);
        record_cost(&paths, &body, cost);
        let spent = crate::app::spending::spent(root.path());
        assert_eq!(spent.len(), 1);
        assert_eq!(spent[0].cost, Some(cost));
        assert_eq!(spent[0].project, "test-project");
        assert_eq!(spent[0].model, "test-model");
        assert_eq!(spent[0].origin, "agent");
        assert_eq!(spent[0].task, "chat");
    }

    #[tokio::test]
    async fn malformed_tool_arguments_are_reported_to_the_model_without_executing() {
        let server = serve(vec![
            stream(&[json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"bad-1","function":{"name":"studio_guide","arguments":"broken"}}
            ]}}]})]),
            stream(&[json!({"choices":[{"delta":{"content":"Please retry"}}]})]),
        ]);
        let root = tempfile::tempdir().unwrap();
        let (send, mut received) = mpsc::channel(32);
        converse_with(&Client::for_test(server.base()), &paths(root.path()), &body(), "test-key", &send).await;
        let events = drain(&mut received);
        assert!(events.len() >= 3, "{events:?}");
        assert_eq!(events[1]["args"], Value::Null);
        assert_eq!(events[2]["ok"], false);
        let second = body_json(&server.request(1));
        assert!(second["messages"][3]["content"]
            .as_str()
            .unwrap()
            .starts_with("Error: the arguments"));
    }

    #[tokio::test]
    async fn provider_failures_keep_usage_and_closed_panels_start_no_requests() {
        let server = serve(vec![stream(&[json!({"usage":{"cost":0.5},"error":{"message":"provider failed"}})])]);
        let root = tempfile::tempdir().unwrap();
        let (send, mut received) = mpsc::channel(32);
        let client = Client::for_test(server.base());
        let cost = converse_with(&client, &paths(root.path()), &body(), "test-key", &send).await;
        assert_eq!(cost, 0.5, "{:?}", drain(&mut received));
        assert_eq!(
            received.recv().await.unwrap(),
            json!({"type":"error","message":"provider failed","cost":0.5})
        );
        drop(received);
        assert_eq!(converse_with(&client, &paths(root.path()), &body(), "test-key", &send).await, 0.0);
        assert_eq!(server.count(), 1);
    }

    #[tokio::test]
    async fn the_daily_limit_prevents_another_paid_turn() {
        let server = serve(vec![stream(&[json!({"usage":{"cost":0.25},"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"guide-1","function":{"name":"studio_guide","arguments":"{\"topic\":\"tags\"}"}}
        ]}}]})])]);
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        std::fs::create_dir_all(&paths.models_root).unwrap();
        std::fs::write(paths.models_root.join("active.json"), r#"{"cloud_daily_limit_usd":0.1}"#).unwrap();
        let (send, mut received) = mpsc::channel(32);
        let cost = converse_with(&Client::for_test(server.base()), &paths, &body(), "test-key", &send).await;
        assert_eq!(cost, 0.25);
        let events = drain(&mut received);
        let error = events.last().unwrap();
        assert_eq!(error["type"], "error");
        assert_eq!(error["cost"], cost);
        assert!(error["message"].as_str().unwrap().contains("0.10"), "{error}");
        assert_eq!(server.count(), 1);
    }

    #[test]
    fn the_prompt_names_the_project_and_the_language_and_carries_the_skill() {
        let prompt = system_prompt(Some("abc123"), Some("ru"));
        assert!(prompt.contains("abc123") && prompt.contains("ru"));
        assert!(prompt.contains(tools::SKILL));
    }

    #[test]
    fn every_tool_but_the_assistant_requests_is_offered_to_the_chat() {
        let tools = tools::chat_tools();
        assert!(tools
            .iter()
            .all(|tool| tool["type"] == "function" && tool["function"]["parameters"]["type"] == "object"));
        assert!(tools.iter().any(|tool| tool["function"]["name"] == "project_update"));
        assert!(!tools
            .iter()
            .any(|tool| tool["function"]["name"].as_str().is_some_and(|n| n.starts_with("assistant_"))));
    }
}
