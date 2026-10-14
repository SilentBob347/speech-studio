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
            description: "When the person has made you Speech Studio's assistant (Settings, Agent: Assistant 'Agent (MCP)'), what they write in the Assistant panel waits here. Returns the waiting requests - id, the instructions, the request and the conversation so far - as soon as there is one, or after seconds (30 by default, at most 55). Do what a request asks with the tools (the panel shows each call you make), then answer it in words with assistant_request_answer; keep calling while the person works.",
            schema: || object(json!({ "seconds": { "type": "integer", "minimum": 1, "maximum": 55 } }), &[]),
            call: |_| composite("questions"),
        },
        Tool {
            name: "assistant_request_answer",
            description: "Answer a request from assistant_requests_wait in words; the Assistant panel shows the answer to the person.",
            schema: || object(json!({ "request_id": { "type": "string" }, "answer": { "type": "string" } }), &["request_id", "answer"]),
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

/// Asks the connected agent what the person wrote to the assistant; answers with the agent's answer.
pub(crate) async fn ask_agent(question: Value) -> Result<String, String> {
    if !super::agent_present() {
        return Err(t!("assistant-no-agent"));
    }
    let id = format!("q{}", QUESTION_ID.fetch_add(1, Ordering::Relaxed));
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut question = question;
    question["id"] = json!(id);
    QUESTIONS.lock().unwrap_or_else(|p| p.into_inner()).push(AgentQuestion { question, answer: sender });
    match tokio::time::timeout(AGENT_ANSWER, receiver).await {
        Ok(Ok(answer)) => Ok(answer),
        _ => {
            QUESTIONS.lock().unwrap_or_else(|p| p.into_inner()).retain(|question| question.question["id"] != id.as_str());
            Err(t!("assistant-no-answer", minutes = (AGENT_ANSWER.as_secs() / 60) as i64))
        }
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
    question.answer.send(answer).map_err(|_| format!("Request {id} was given up by the window before the answer came."))?;
    Ok(json!({ "answered": id, "note": "The Assistant panel shows it." }))
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
}
