//! A block's audio while it is being voiced: the pieces the engine streams go to `GET /speech/live` (SSE).

use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// The projects whose voicing the window plays aloud wherever it is, as it comes (an agent asked to say something).
fn aloud_set() -> &'static Mutex<std::collections::HashSet<String>> {
    static SET: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

/// Plays `project`'s voicing aloud until the guard is dropped; refused when no window listens, there being nothing
/// to play it on.
pub fn aloud(project: &str) -> Result<Aloud, String> {
    if channel().receiver_count() == 0 {
        return Err(t!("speak-aloud-no-window"));
    }
    aloud_set().lock().unwrap_or_else(|p| p.into_inner()).insert(project.to_string());
    Ok(Aloud(project.to_string()))
}

pub struct Aloud(String);

impl Drop for Aloud {
    fn drop(&mut self) {
        aloud_set().lock().unwrap_or_else(|p| p.into_inner()).remove(&self.0);
    }
}

/// Whether a window listens to the voicing, and so can play it.
pub fn listening() -> bool {
    channel().receiver_count() > 0
}

/// The window stops playing `project` now and drops what it queued.
pub fn silence(project: &str) {
    let _ = channel().send(json!({ "kind": "stop", "project": project, "aloud": true }));
}

/// The windows that play what is said aloud, by connection, in the order they opened: only the first plays it, so a
/// second window does not sound the same phrase again a moment later.
fn players() -> &'static Mutex<std::collections::BTreeSet<u64>> {
    static SET: OnceLock<Mutex<std::collections::BTreeSet<u64>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

/// A window's connection while it plays aloud; leaving the set when the connection closes.
struct Player(u64);

impl Player {
    fn join() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        players().lock().unwrap_or_else(|p| p.into_inner()).insert(id);
        Player(id)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        players().lock().unwrap_or_else(|p| p.into_inner()).remove(&self.0);
    }
}

/// The window that plays what is said aloud now.
fn player() -> Option<u64> {
    players().lock().unwrap_or_else(|p| p.into_inner()).first().copied()
}

fn is_aloud(project: &str) -> bool {
    aloud_set().lock().unwrap_or_else(|p| p.into_inner()).contains(project)
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
        let aloud = is_aloud(&self.project);
        let mut ev = json!({ "kind": kind, "project": self.project, "block": self.block, "aloud": aloud });
        if aloud {
            ev["player"] = json!(player());
        }
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
        self.play(samples, sr);
    }

    fn play(&self, samples: &[f32], sr: u32) {
        if channel().receiver_count() == 0 {
            return;
        }
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.send("chunk", json!({ "sr": sr, "data": base64::engine::general_purpose::STANDARD.encode(bytes) }));
    }

    /// Silence the block's text asks for between its pieces; it is not the block's first sound.
    pub fn pause(&self, ms: u32, sr: u32) {
        if ms > 0 {
            self.play(&vec![0.0; (sr as u64 * ms as u64 / 1000) as usize], sr);
        }
    }

    /// The take was defective and is said again: what was played of it is dropped.
    pub fn restart(&self) {
        self.send("restart", json!({}));
    }

    /// Whether the phrase is heard aloud as it comes (an agent speaking), not only in a project's player.
    pub fn is_aloud(&self) -> bool {
        is_aloud(&self.project)
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

/// GET /speech/live[?aloud=1] — every block voiced from now on, while it is voiced; a window that plays what is said
/// aloud asks with `aloud=1`, learns its number from the first event and plays an aloud phrase only when it is that
/// phrase's `player`.
pub async fn events(axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>) -> Response {
    let mut rx = channel().subscribe();
    let player = (q.get("aloud").map(String::as_str) == Some("1")).then(Player::join);
    let stream = async_stream::stream! {
        let player = player;
        if let Some(p) = &player {
            yield Ok::<_, Infallible>(Event::default().data(json!({ "kind": "hello", "id": p.0 }).to_string()));
        }
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

    #[test]
    fn the_first_open_window_plays_what_is_said_aloud() {
        let first = Player::join();
        let second = Player::join();
        assert_eq!(player(), Some(first.0));
        drop(first);
        assert_eq!(player(), Some(second.0), "the next window plays once the first closes");
    }

    #[tokio::test]
    async fn a_listener_gets_the_pieces_in_order_and_the_first_sound() {
        let mut rx = channel().subscribe();
        let phrase = Phrase::begin("p1", "b1");
        phrase.pause(10, 24_000);
        assert_eq!(phrase.first_ms(), None, "a pause is no first sound");
        phrase.chunk(&[0.5, -0.5], 24_000);
        phrase.restart();
        phrase.end(true);
        let kinds: Vec<String> = (0..5).map(|_| rx.try_recv().unwrap()["kind"].as_str().unwrap().to_string()).collect();
        assert_eq!(kinds, ["start", "chunk", "chunk", "restart", "end"]);
        assert!(phrase.first_ms().is_some());
        let mut again = channel().subscribe();
        phrase.chunk(&[0.25], 24_000);
        let ev = again.try_recv().unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(ev["data"].as_str().unwrap()).unwrap();
        assert_eq!(f32::from_le_bytes(bytes[..4].try_into().unwrap()), 0.25);
        assert_eq!(ev["sr"], 24_000);
    }
}
