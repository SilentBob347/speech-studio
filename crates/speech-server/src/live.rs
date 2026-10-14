//! A block's audio while it is being voiced: the pieces the engine streams go to `GET /speech/live` (SSE).

use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use serde_json::{json, Value};
use tokio::sync::broadcast;

fn channel() -> &'static broadcast::Sender<Value> {
    static TX: OnceLock<broadcast::Sender<Value>> = OnceLock::new();
    TX.get_or_init(|| broadcast::channel(1024).0)
}

/// One block being voiced: its pieces in order, a restart when a defective take is said again, its end.
pub struct Phrase {
    project: String,
    block: String,
    started: Instant,
    first_ms: Mutex<Option<u64>>,
    ended: AtomicBool,
}

impl Drop for Phrase {
    fn drop(&mut self) {
        if !self.ended.load(Ordering::Relaxed) {
            self.end(false);
        }
    }
}

impl Phrase {
    pub fn begin(project: &str, block: &str) -> Self {
        let phrase = Self {
            project: project.into(),
            block: block.into(),
            started: Instant::now(),
            first_ms: Mutex::new(None),
            ended: AtomicBool::new(false),
        };
        phrase.send("start", json!({}));
        phrase
    }

    fn send(&self, kind: &str, extra: Value) {
        let tx = channel();
        if tx.receiver_count() == 0 {
            return;
        }
        let mut ev = json!({ "kind": kind, "project": self.project, "block": self.block });
        if let (Some(ev), Value::Object(extra)) = (ev.as_object_mut(), extra) {
            ev.extend(extra);
        }
        let _ = tx.send(ev);
    }

    /// Mono samples at `sr`, as the engine decoded them.
    pub fn chunk(&self, samples: &[f32], sr: u32) {
        if samples.is_empty() {
            return;
        }
        if let Ok(mut first) = self.first_ms.lock() {
            first.get_or_insert_with(|| self.started.elapsed().as_millis() as u64);
        }
        if channel().receiver_count() == 0 {
            return;
        }
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.send("chunk", json!({ "sr": sr, "data": base64::engine::general_purpose::STANDARD.encode(bytes) }));
    }

    /// Silence the block's text asks for between its pieces.
    pub fn pause(&self, ms: u32, sr: u32) {
        if channel().receiver_count() > 0 && ms > 0 {
            self.chunk(&vec![0.0; (sr as u64 * ms as u64 / 1000) as usize], sr);
        }
    }

    /// The take was defective and is said again: what was played of it is dropped.
    pub fn restart(&self) {
        self.send("restart", json!({}));
    }

    /// Milliseconds from the start of the block to its first sound.
    pub fn first_ms(&self) -> Option<u64> {
        self.first_ms.lock().ok().and_then(|f| *f)
    }

    pub fn end(&self, ok: bool) {
        self.ended.store(true, Ordering::Relaxed);
        self.send("end", json!({ "ok": ok, "first_ms": self.first_ms() }));
    }
}

/// GET /speech/live — every block voiced from now on, while it is voiced.
pub async fn events() -> Response {
    let mut rx = channel().subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(ev) => yield Ok::<_, Infallible>(Event::default().data(ev.to_string())),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    yield Ok(Event::default().data(json!({ "kind": "lagged", "missed": n }).to_string()));
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_listener_gets_the_pieces_in_order_and_the_first_sound() {
        let mut rx = channel().subscribe();
        let phrase = Phrase::begin("p1", "b1");
        assert_eq!(phrase.first_ms(), None);
        phrase.chunk(&[0.5, -0.5], 24_000);
        phrase.restart();
        phrase.end(true);
        let kinds: Vec<String> = (0..4).map(|_| rx.try_recv().unwrap()["kind"].as_str().unwrap().to_string()).collect();
        assert_eq!(kinds, ["start", "chunk", "restart", "end"]);
        assert!(phrase.first_ms().is_some());
        let mut again = channel().subscribe();
        phrase.chunk(&[0.25], 24_000);
        let ev = again.try_recv().unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(ev["data"].as_str().unwrap()).unwrap();
        assert_eq!(f32::from_le_bytes(bytes[..4].try_into().unwrap()), 0.25);
        assert_eq!(ev["sr"], 24_000);
    }
}
