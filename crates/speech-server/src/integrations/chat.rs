//! OpenRouter chat transport. Dialogue and tool policy are supplied by the application.

use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::future::Future;

const API: &str = "https://openrouter.ai/api/v1/chat/completions";
const MAX_TOOL_CALLS: u64 = 128;
const MAX_EVENT_BYTES: usize = 8 * 1024 * 1024;

pub(crate) struct Client {
    http: reqwest::Client,
    endpoint: String,
}

#[derive(Debug, Default)]
pub(crate) struct Turn {
    pub text: String,
    pub calls: Vec<(String, String, String)>,
    pub cost: f64,
}

#[derive(Debug)]
pub(crate) struct TurnError {
    pub message: String,
    // Provider usage can precede a failed stream or a closed panel.
    pub cost: f64,
}

impl Client {
    pub(crate) fn new() -> Result<Self, String> {
        let http = dub_llm::net::async_builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            http,
            endpoint: API.into(),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(endpoint: String) -> Self {
        Self {
            http: reqwest::Client::builder().no_proxy().build().unwrap(),
            endpoint,
        }
    }

    pub(crate) async fn turn<F, Fut>(
        &self,
        key: &str,
        model: &str,
        messages: &[Value],
        tools: &[Value],
        on_text: F,
    ) -> Result<Turn, TurnError>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<(), String>>,
    {
        let body = json!({ "model": model, "messages": messages, "tools": tools, "stream": true,
            "usage": { "include": true }, "cache_control": { "type": "ephemeral" } });
        let response = self
            .http
            .post(&self.endpoint)
            .bearer_auth(key)
            .header("X-Title", "Speech Studio")
            .json(&body)
            .send()
            .await
            .map_err(|error| TurnError {
                message: error.to_string(),
                cost: 0.0,
            })?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.map_err(|error| TurnError {
                message: format!("{status}: failed to read the model's error: {error}"),
                cost: 0.0,
            })?;
            let said = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| value.pointer("/error/message").and_then(Value::as_str).map(str::to_string));
            return Err(TurnError {
                message: said.unwrap_or_else(|| format!("{status}: {text}")),
                cost: 0.0,
            });
        }
        read_turn(response.bytes_stream(), on_text).await
    }
}

#[derive(Default)]
struct Decoder {
    turn: Turn,
    partial: BTreeMap<u64, (String, String, String)>,
    data: String,
    done: bool,
    finished: bool,
}

impl Decoder {
    fn line(&mut self, line: &[u8]) -> Result<Option<String>, String> {
        let line = std::str::from_utf8(line).map_err(|error| format!("OpenRouter sent invalid UTF-8: {error}"))?;
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            return self.event();
        }
        if let Some(data) = line.strip_prefix("data:") {
            if !self.data.is_empty() {
                self.data.push('\n');
            }
            self.data.push_str(data.strip_prefix(' ').unwrap_or(data));
            if self.data.len() > MAX_EVENT_BYTES {
                return Err("OpenRouter stream event is too large".into());
            }
        }
        Ok(None)
    }

    fn event(&mut self) -> Result<Option<String>, String> {
        let data = std::mem::take(&mut self.data);
        if self.done || data.is_empty() {
            return Ok(None);
        }
        if data.trim() == "[DONE]" {
            self.done = true;
            return Ok(None);
        }
        let event: Value = serde_json::from_str(&data).map_err(|error| format!("OpenRouter sent invalid stream JSON: {error}"))?;
        if let Some(cost) = event.pointer("/usage/cost").and_then(Value::as_f64) {
            self.turn.cost += cost;
        }
        if let Some(error) = event.get("error") {
            return Err(error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("the model failed")
                .to_string());
        }
        if event.pointer("/choices/0/finish_reason").is_some_and(|reason| !reason.is_null()) {
            self.finished = true;
        }
        let Some(delta) = event.pointer("/choices/0/delta") else {
            return Ok(None);
        };
        for call in delta.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
            let index = call.get("index").and_then(Value::as_u64).unwrap_or(0);
            if index >= MAX_TOOL_CALLS {
                return Err(format!("OpenRouter tool-call index {index} exceeds the limit"));
            }
            let slot = self.partial.entry(index).or_default();
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
        let text = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_string);
        if let Some(text) = &text {
            self.turn.text.push_str(text);
        }
        Ok(text)
    }

    fn finish(mut self) -> Result<Turn, TurnError> {
        if !self.done && !self.finished {
            return Err(TurnError {
                message: "OpenRouter stream ended before the model finished".into(),
                cost: self.turn.cost,
            });
        }
        if self.partial.values().any(|(id, name, _)| id.is_empty() || name.is_empty()) {
            return Err(TurnError {
                message: "OpenRouter returned an incomplete tool call".into(),
                cost: self.turn.cost,
            });
        }
        self.turn.calls = self.partial.into_values().collect();
        Ok(self.turn)
    }
}

async fn read_turn<S, E, F, Fut>(stream: S, mut on_text: F) -> Result<Turn, TurnError>
where
    S: Stream<Item = Result<axum::body::Bytes, E>>,
    E: std::fmt::Display,
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let mut decoder = Decoder::default();
    let result: Result<(), String> = async {
        futures_util::pin_mut!(stream);
        let mut buffer = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| error.to_string())?;
            // Decode only complete lines: a network chunk can split a UTF-8 character.
            for piece in chunk.split_inclusive(|byte| *byte == b'\n') {
                buffer.extend_from_slice(piece);
                if buffer.len() > MAX_EVENT_BYTES {
                    return Err("OpenRouter stream line is too large".into());
                }
                if buffer.last() == Some(&b'\n') {
                    if let Some(text) = decoder.line(&buffer[..buffer.len() - 1])? {
                        on_text(text).await?;
                    }
                    buffer.clear();
                }
                if decoder.done {
                    return Ok(());
                }
            }
        }
        if !buffer.is_empty() {
            if let Some(text) = decoder.line(&buffer)? {
                on_text(text).await?;
            }
        }
        if let Some(text) = decoder.event()? {
            on_text(text).await?;
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => decoder.finish(),
        Err(message) => Err(TurnError {
            message,
            cost: decoder.turn.cost,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn decode(chunks: Vec<Result<axum::body::Bytes, &'static str>>) -> Result<Turn, TurnError> {
        read_turn(futures_util::stream::iter(chunks), |_| async { Ok(()) }).await
    }

    #[tokio::test]
    async fn split_unicode_tools_and_unterminated_last_event_survive_chunk_boundaries() {
        let events = [
            json!({"choices":[{"delta":{"content":"Привет 🌍"}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"studio_guide","arguments":"{\"topic\":"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"tags\"}"}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
        ];
        let mut data = String::from(": keepalive\r\n\r\n");
        for event in events {
            data.push_str(&format!("data: {event}\r\n\r\n"));
        }
        data.push_str("data: {\"usage\":{\"cost\":0.125}}");
        for size in [1, 2, 7, data.len()] {
            let chunks = data.as_bytes().chunks(size).map(|chunk| Ok(chunk.to_vec().into())).collect();
            let turn = decode(chunks).await.unwrap();
            assert_eq!(turn.text, "Привет 🌍");
            assert_eq!(turn.calls, [("a".into(), "studio_guide".into(), "{\"topic\":\"tags\"}".into())]);
            assert_eq!(turn.cost, 0.125);
        }
    }

    #[tokio::test]
    async fn malformed_events_and_transport_errors_keep_reported_cost() {
        let usage = "data: {\"usage\":{\"cost\":0.25}}\n\n";
        let failures: Vec<Vec<u8>> = vec![
            b"data: broken\n\n".to_vec(),
            b"data: {\"error\":{\"message\":\"quota exceeded\"}}\n\n".to_vec(),
            b"data: \xff\n\n".to_vec(),
        ];
        for failure in failures {
            let err = decode(vec![Ok(usage.into()), Ok(failure.into())]).await.unwrap_err();
            assert_eq!(err.cost, 0.25);
            assert!(!err.message.is_empty());
        }
        let err = decode(vec![Ok(usage.into()), Err("connection lost")]).await.unwrap_err();
        assert_eq!(err.cost, 0.25);
        assert_eq!(err.message, "connection lost");
    }

    #[tokio::test]
    async fn invalid_tool_indices_and_incomplete_calls_are_errors() {
        for call in [json!({"index":u64::MAX}), json!({"index":0,"id":"a"})] {
            let data = format!("data: {}\n\n", json!({"choices":[{"delta":{"tool_calls":[call]}}]}));
            assert!(decode(vec![Ok(data.into())]).await.is_err());
        }
    }

    #[tokio::test]
    async fn multiline_events_work_and_unfinished_responses_are_errors() {
        let complete = "data: {\"choices\":[\ndata: {\"delta\":{\"content\":\"Hi\"},\"finish_reason\":\"stop\"}]}\n\n";
        assert_eq!(decode(vec![Ok(complete.into())]).await.unwrap().text, "Hi");
        let partial = "data: {\"usage\":{\"cost\":0.125},\"choices\":[{\"delta\":{\"content\":\"Partial\"}}]}\n\n";
        let error = decode(vec![Ok(partial.into())]).await.unwrap_err();
        assert_eq!(error.cost, 0.125);
        assert!(error.message.contains("before the model finished"));
    }

    #[tokio::test]
    async fn stopped_consumer_keeps_usage_and_done_ends_the_stream() {
        let data = "data: {\"usage\":{\"cost\":0.5},\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n";
        let err = read_turn(futures_util::stream::iter([Ok::<_, &str>(data.into())]), |_| async {
            Err("closed".into())
        })
        .await
        .unwrap_err();
        assert_eq!(err.cost, 0.5);
        let turn = decode(vec![Ok("data: [DONE]\n\n".into()), Err("must not be read")]).await.unwrap();
        assert!(turn.text.is_empty());
    }
}
