//! The connected agent as the window's assistant (VideoSOS `mcp.rs`): when the person chooses «Agent (MCP)» as the
//! assistant, a message of the Assistant panel waits here as a request; the agent takes it with
//! `assistant_requests_wait`, does what it asks with its tools (the panel shows each call) and answers with
//! `assistant_request_answer`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use super::{composite, object, text, Tool};

/// How long the window waits for the agent's answer.
const AGENT_ANSWER: Duration = Duration::from_secs(900);
/// How long the studio's model work waits for the agent: under the 600 s its chat client waits.
const AGENT_AS_MODEL: Duration = Duration::from_secs(570);

struct AgentQuestion {
    question: Value,
    answer: tokio::sync::oneshot::Sender<String>,
}

static QUESTIONS: Mutex<Vec<AgentQuestion>> = Mutex::new(Vec::new());
static QUESTION_ID: AtomicU64 = AtomicU64::new(1);
static CHAT_STEPS: Mutex<Vec<(u64, tokio::sync::mpsc::Sender<Value>)>> = Mutex::new(Vec::new());
static STEP_ID: AtomicU64 = AtomicU64::new(1);

pub(super) fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "assistant_requests_wait",
            description: "When the person has made you Speech Studio's assistant (Settings, Agent: Assistant 'Agent (MCP)') or its language model (Settings, Models: LLM provider 'Agent (MCP)'), what is asked of you waits here: what they write in the Assistant panel, and the studio's own model work - a podcast script, the characters of a book, punctuation, enrichment - as kind 'studio_model'. Returns the waiting requests - id, kind, the instructions, the request, the conversation so far for the panel, and answer_schema, the JSON schema the studio's model work must match - as soon as there is one, or after seconds (30 by default, at most 55). Do what a panel request asks with the tools (the panel shows each call you make) and answer it in words; answer a studio_model request yourself, as the studio's model would, by its instructions: with the JSON object its answer_schema describes, or plain text without one. Answer with assistant_request_answer; keep calling while the person works.",
            schema: || object(json!({ "seconds": { "type": "integer", "minimum": 1, "maximum": 55 } }), &[]),
            call: |_| composite("questions"),
        },
        Tool {
            name: "assistant_request_answer",
            description: "Answer a request from assistant_requests_wait: in words for the Assistant panel, which shows it to the person; for a studio_model request the JSON object its answer_schema describes (or plain text without one), and the studio goes on with it as with its own model's answer.",
            schema: || object(json!({ "request_id": { "type": "string" }, "answer": { "anyOf": [{ "type": "object" }, { "type": "string" }] } }), &["request_id", "answer"]),
            call: |_| composite("answer"),
        },
    ]
}

/// The step id of an agent's tool call shown in the Assistant panel; the assistant's own tools are not steps.
pub(super) fn step_of(name: &str) -> Option<String> {
    (!name.starts_with("assistant_") && !CHAT_STEPS.lock().unwrap_or_else(|p| p.into_inner()).is_empty()).then(|| format!("agent-{}", STEP_ID.fetch_add(1, Ordering::Relaxed)))
}

/// Shows an agent's step in every Assistant panel waiting for it.
pub(super) fn chat_step(event: Value) {
    for (_, steps) in CHAT_STEPS.lock().unwrap_or_else(|p| p.into_inner()).iter() {
        if let Err(error) = steps.try_send(event.clone()) {
            tracing::warn!("an agent's step did not reach the assistant panel: {error}");
        }
    }
}

/// Shows the agent's tool calls in the Assistant panel while `steps` is open; they stop showing when the guard drops.
pub(crate) fn follow_agent_steps(steps: tokio::sync::mpsc::Sender<Value>) -> impl Drop {
    struct Following(u64);
    impl Drop for Following {
        fn drop(&mut self) {
            CHAT_STEPS.lock().unwrap_or_else(|p| p.into_inner()).retain(|(id, _)| *id != self.0);
        }
    }
    let id = STEP_ID.fetch_add(1, Ordering::Relaxed);
    CHAT_STEPS.lock().unwrap_or_else(|p| p.into_inner()).push((id, steps));
    Following(id)
}

pub(super) fn open_questions() -> Vec<Value> {
    let mut waiting = QUESTIONS.lock().unwrap_or_else(|p| p.into_inner());
    waiting.retain(|question| !question.answer.is_closed());
    waiting.iter().map(|question| question.question.clone()).collect()
}

pub(crate) async fn ask_agent(question: Value) -> Result<String, String> {
    ask_agent_within(question, AGENT_ANSWER).await
}

async fn ask_agent_within(question: Value, wait: Duration) -> Result<String, String> {
    if !super::agent_present() {
        return Err(t!("assistant-no-agent"));
    }
    let id = format!("q{}", QUESTION_ID.fetch_add(1, Ordering::Relaxed));
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut question = question;
    question["id"] = json!(id);
    QUESTIONS.lock().unwrap_or_else(|p| p.into_inner()).push(AgentQuestion { question, answer: sender });
    match tokio::time::timeout(wait, receiver).await {
        Ok(Ok(answer)) => Ok(answer),
        _ => {
            QUESTIONS.lock().unwrap_or_else(|p| p.into_inner()).retain(|question| question.question["id"] != id.as_str());
            Err(t!("assistant-no-answer", minutes = (wait.as_secs() / 60) as i64))
        }
    }
}

/// The studio's own model work (a podcast script, the characters of a book, punctuation, enrichment) when the agent
/// is the LLM provider: an OpenAI chat completion from the studio's client becomes a request to the agent with the
/// instructions, the request and the answer schema the studio's model would get, and its answer comes back as the
/// model's. The studio's client gives up after 600 s, so the agent is waited for a little less.
pub(crate) async fn agent_llm(axum::Json(body): axum::Json<Value>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let text_of = |m: &Value| match &m["content"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    };
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    let instructions = messages.iter().filter(|m| m["role"] == "system").map(text_of).collect::<Vec<_>>().join("\n\n");
    let request = messages.iter().filter(|m| m["role"] != "system").map(text_of).collect::<Vec<_>>().join("\n\n");
    let schema = body.pointer("/response_format/json_schema/schema").cloned();
    let question = json!({ "kind": "studio_model", "instructions": instructions, "request": request, "answer_schema": schema });
    let fail = |code: axum::http::StatusCode, message: String| (code, axum::Json(json!({ "error": { "message": message } }))).into_response();
    if request.is_empty() {
        return fail(axum::http::StatusCode::BAD_REQUEST, "messages carry no request".into());
    }
    if !super::agent_present() {
        return fail(axum::http::StatusCode::CONFLICT, t!("assistant-no-agent"));
    }
    match ask_agent_within(question, AGENT_AS_MODEL).await {
        Ok(answer) => axum::Json(json!({ "choices": [{ "message": { "role": "assistant", "content": answer }, "finish_reason": "stop" }] })).into_response(),
        Err(e) => fail(axum::http::StatusCode::GATEWAY_TIMEOUT, e),
    }
}

pub(super) async fn wait_for_questions(args: &Value) -> Value {
    let seconds = args.get("seconds").and_then(Value::as_u64).unwrap_or(30).clamp(1, 55);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        let waiting = open_questions();
        if !waiting.is_empty() {
            return json!({ "requests": waiting });
        }
        if tokio::time::Instant::now() >= deadline {
            return json!({ "requests": [], "note": "no request yet; call assistant_requests_wait again to keep listening" });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub(super) fn answer_question(args: &Value) -> Result<Value, String> {
    let id = text(args, "request_id")?;
    let answer = match args.get("answer") {
        Some(Value::String(text)) => text.clone(),
        Some(value) if !value.is_null() => value.to_string(),
        _ => return Err("'answer' is required".into()),
    };
    let found = {
        let mut waiting = QUESTIONS.lock().unwrap_or_else(|p| p.into_inner());
        waiting.iter().position(|question| question.question["id"] == id.as_str()).map(|at| waiting.remove(at))
    };
    let Some(question) = found else {
        return Err(format!("No request {id} is waiting; assistant_requests_wait lists the open ones."));
    };
    let model_work = question.question["kind"] == "studio_model";
    question.answer.send(answer).map_err(|_| format!("Request {id} was given up by the studio before the answer came."))?;
    let note = if model_work { "The studio goes on with it as with its own model's answer." } else { "The Assistant panel shows it." };
    Ok(json!({ "answered": id, "note": note }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_answer_reaches_the_question_it_names() {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        QUESTIONS.lock().unwrap().push(AgentQuestion { question: json!({ "id": "q-test", "request": "hi" }), answer: sender });
        assert!(open_questions().iter().any(|q| q["id"] == "q-test"));
        assert!(answer_question(&json!({ "request_id": "q-nobody", "answer": "x" })).is_err());
        answer_question(&json!({ "request_id": "q-test", "answer": "привет" })).unwrap();
        assert_eq!(receiver.await.unwrap(), "привет");
        assert!(!open_questions().iter().any(|q| q["id"] == "q-test"));
    }

    #[tokio::test]
    async fn the_studios_model_work_reaches_the_agent_with_its_schema_and_comes_back_as_the_models_answer() {
        super::super::seen("test");
        let schema = json!({ "type": "object", "properties": { "lines": { "type": "array" } } });
        let body = json!({ "model": "agent", "messages": [{ "role": "system", "content": "Write a podcast." }, { "role": "user", "content": [{ "type": "text", "text": "About tea." }] }], "response_format": { "type": "json_schema", "json_schema": { "name": "answer", "schema": schema } } });
        let call = tokio::spawn(agent_llm(axum::Json(body)));
        let question = loop {
            if let Some(q) = open_questions().into_iter().find(|q| q["kind"] == "studio_model" && q["request"] == "About tea.") {
                break q;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert_eq!(question["instructions"], "Write a podcast.");
        assert_eq!(question["answer_schema"], schema);
        answer_question(&json!({ "request_id": question["id"], "answer": { "lines": [] } })).unwrap();
        let response = call.await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let answer: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(answer["choices"][0]["message"]["content"], r#"{"lines":[]}"#);
        assert_eq!(answer["choices"][0]["finish_reason"], "stop");
    }
}
