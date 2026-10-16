//! Speech said aloud as it is written: an agent sends its text piece by piece while it writes it, each finished phrase
//! becomes a block of the session's project and is voiced at once (`aloud`), the window playing the sound as it comes.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::speech_api::refuse;
use crate::speech_project as sp;
use crate::AppState;

/// A phrase longer than this is said up to its last comma rather than waiting for its full stop.
const LONG_PHRASE: usize = 120;

struct Session {
    buf: String,
    direction: String,
}

static SESSIONS: LazyLock<Mutex<HashMap<String, Session>>> = LazyLock::new(Default::default);

/// What the studio said aloud lately and when: a microphone hearing it back is the studio's own voice, not a person.
static SAID: LazyLock<Mutex<Vec<(std::time::Instant, String)>>> = LazyLock::new(Default::default);

/// How long a phrase said aloud may still come back through the microphone.
const ECHO_SECS: u64 = 30;

/// Whether `heard` is the studio's own recent speech coming back: most of its words are in what it said lately.
pub fn is_echo(heard: &str) -> bool {
    let words = crate::qc::normalize(heard);
    if words.is_empty() {
        return false;
    }
    let mut said = SAID.lock().unwrap_or_else(|p| p.into_inner());
    said.retain(|(at, _)| at.elapsed().as_secs() < ECHO_SECS);
    let spoken: std::collections::HashSet<String> = said.iter().flat_map(|(_, t)| crate::qc::normalize(t)).collect();
    let known = words.iter().filter(|w| spoken.contains(*w)).count();
    known * 3 >= words.len() * 2
}

/// A person started talking: every session stops speaking, as people stop when interrupted. Answers whether anything
/// was being said.
pub fn interrupt(st: &AppState) -> bool {
    let open: Vec<String> = sessions().keys().cloned().collect();
    let speaking = !open.is_empty() || !SAID.lock().unwrap_or_else(|p| p.into_inner()).is_empty();
    for sid in open.iter() {
        crate::speech_jobs::unqueue(sid, None);
        crate::live::silence(sid);
    }
    let jobs = st.jobs.clone();
    tokio::spawn(async move {
        for sid in open {
            if let Some((job, crate::jobs::JobKind::Synth)) = jobs.active_for(&sid).await {
                let _ = jobs.cancel(&job).await;
            }
        }
    });
    speaking
}

fn sessions() -> std::sync::MutexGuard<'static, HashMap<String, Session>> {
    SESSIONS.lock().unwrap_or_else(|p| p.into_inner())
}

/// The phrases of `buf` that are finished, taken out of it: up to a full stop, a question or an exclamation mark, an
/// ellipsis or a line break followed by more, a long phrase up to its last comma, and with `end` whatever is left.
pub fn cut(buf: &mut String, end: bool) -> Vec<String> {
    let mut out = Vec::new();
    loop {
        let chars: Vec<(usize, char)> = buf.char_indices().collect();
        let mut at = None;
        for (i, &(pos, c)) in chars.iter().enumerate() {
            let next = chars.get(i + 1).map(|&(_, n)| n);
            let stop = matches!(c, '。' | '！' | '？') || (matches!(c, '.' | '!' | '?' | '…') && next.is_some_and(|n| n.is_whitespace() || matches!(n, '"' | '»' | ')' | '”')));
            if c == '\n' || stop {
                let close = chars[i + 1..].iter().take_while(|&&(_, n)| matches!(n, '"' | '»' | ')' | '”')).count();
                at = Some(chars.get(i + 1 + close).map_or(buf.len(), |&(p, _)| p).max(pos + c.len_utf8()));
                break;
            }
        }
        if at.is_none() && buf.chars().count() > LONG_PHRASE {
            at = buf.rfind([',', ';', ':', '，', '；']).map(|p| p + buf[p..].chars().next().map_or(1, char::len_utf8));
        }
        let Some(at) = at else { break };
        let phrase = buf[..at].trim().to_string();
        buf.replace_range(..at, "");
        if !phrase.is_empty() {
            out.push(phrase);
        }
    }
    if end && !buf.trim().is_empty() {
        out.push(buf.trim().to_string());
        buf.clear();
    }
    out
}

/// POST /speech/say {voice?, engine?, lang?, direction?, name?} — a session said aloud as its text comes; refused when no
/// window listens, there being nothing to play it on.
pub async fn start(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    if !crate::live::listening() {
        return refuse(StatusCode::CONFLICT, "no_window", t!("speak-aloud-no-window"));
    }
    let field = |n: &str| body.get(n).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let engine = field("engine").unwrap_or_else(|| crate::qwen_tts::ENGINE.to_string());
    let Some(manifest) = crate::engines::get(&engine) else {
        return refuse(StatusCode::BAD_REQUEST, "bad_engine", t!("speech-engine-unknown", engine = engine));
    };
    let name = field("name").unwrap_or_else(|| t!("say-session-name", time = chrono::Local::now().format("%H:%M").to_string()));
    let mut project = sp::from_text(&name, "", &t!("speech-narrator"));
    project.chapters[0].blocks.clear();
    project.settings.voice = field("voice");
    project.settings.lang = field("lang").unwrap_or_else(|| "auto".into());
    project.settings.params.engine_defaults(&manifest);
    project.settings.engine = engine;
    project.settings.qc.asr_check = false;
    project.settings.qc.heuristics = vec!["defect".into()];
    let pid = sp::new_id();
    if let Err(e) = sp::save(&st.workspace, &pid, &project) {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, "save_failed", e);
    }
    sessions().insert(pid.clone(), Session { buf: String::new(), direction: field("direction").unwrap_or_default() });
    let (paths, engine, voice, lang) = (crate::speech_api::paths(&st), project.settings.engine.clone(), project.settings.voice.clone(), project.settings.lang.clone());
    tokio::task::spawn_blocking(move || {
        if let Err(e) = crate::speech_jobs::warm(&paths, &engine, voice.as_deref(), &lang) {
            tracing::warn!("say: the engine was not warmed before the first phrase: {e}");
        }
    });
    Json(json!({ "session": pid, "engine": project.settings.engine, "voice": project.settings.voice })).into_response()
}

/// POST /speech/say/{session} {text, end?} — more of the text; each finished phrase is voiced at once, `end` says the
/// rest and closes the session.
pub async fn text(State(st): State<AppState>, AxPath(sid): AxPath<String>, Json(body): Json<Value>) -> Response {
    let piece = body.get("text").and_then(Value::as_str).unwrap_or_default();
    let end = body.get("end").and_then(Value::as_bool).unwrap_or(false);
    let (phrases, direction) = {
        let mut all = sessions();
        let Some(session) = all.get_mut(&sid) else {
            return refuse(StatusCode::NOT_FOUND, "no_session", t!("say-no-session", session = sid));
        };
        session.buf.push_str(piece);
        let phrases = cut(&mut session.buf, end);
        let direction = session.direction.clone();
        if end {
            all.remove(&sid);
        }
        (phrases, direction)
    };
    if phrases.is_empty() {
        return Json(json!({ "voicing": 0 })).into_response();
    }
    let mut project = match sp::load(&st.workspace, &sid) {
        Ok(p) => p,
        Err(e) => return refuse(StatusCode::NOT_FOUND, "no_project", e),
    };
    SAID.lock().unwrap_or_else(|p| p.into_inner()).extend(phrases.iter().map(|p| (std::time::Instant::now(), p.clone())));
    let speaker = t!("speech-narrator");
    let ids: Vec<String> = phrases
        .iter()
        .map(|phrase| {
            let id = sp::new_id();
            project.chapters[0].blocks.push(sp::Block { id: id.clone(), speaker: speaker.clone(), text: phrase.clone(), direction: direction.clone(), ..Default::default() });
            id
        })
        .collect();
    if let Err(e) = sp::save(&st.workspace, &sid, &project) {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, "save_failed", e);
    }
    let started = crate::speech_api::synth(State(st), AxPath(sid), Json(json!({ "ids": ids, "aloud": true }))).await;
    if !started.status().is_success() {
        return started;
    }
    Json(json!({ "voicing": ids.len(), "blocks": ids })).into_response()
}

/// DELETE /speech/say/{session} — stop now: what is queued is dropped, the phrase being voiced is cancelled, the window
/// stops its sound.
pub async fn stop(State(st): State<AppState>, AxPath(sid): AxPath<String>) -> Response {
    sessions().remove(&sid);
    crate::speech_jobs::unqueue(&sid, None);
    if let Some((job, crate::jobs::JobKind::Synth)) = st.jobs.active_for(&sid).await {
        let _ = st.jobs.cancel(&job).await;
    }
    crate::live::silence(&sid);
    Json(json!({ "stopped": sid })).into_response()
}

/// GET /speech/say/{session} — what was said and how soon each phrase was heard, what waits, whether it is open.
pub async fn state(State(st): State<AppState>, AxPath(sid): AxPath<String>) -> Response {
    let project = match sp::load(&st.workspace, &sid) {
        Ok(p) => p,
        Err(e) => return refuse(StatusCode::NOT_FOUND, "no_project", e),
    };
    let pending = sessions().get(&sid).map(|s| s.buf.clone());
    let phrases: Vec<Value> = project
        .blocks()
        .map(|b| {
            let take = b.takes.iter().find(|t| Some(t.n) == b.active);
            json!({ "text": b.text, "state": if b.error.is_some() { "failed" } else if take.is_some() { "said" } else { "waiting" }, "first_ms": take.and_then(|t| t.first_ms), "seconds": take.map(|t| t.duration) })
        })
        .collect();
    Json(json!({ "session": sid, "open": pending.is_some(), "waiting_text": pending.unwrap_or_default(), "phrases": phrases })).into_response()
}

static LISTENING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// POST /speech/listen {lang?, device?} — the microphone heard live for a conversation: each phrase the person ends with
/// a pause is recognised (`next`), and their speaking over the studio stops its speech.
pub async fn listen_start(State(st): State<AppState>, body: Option<Json<Value>>) -> Response {
    let body = body.map(|Json(v)| v).unwrap_or(Value::Null);
    let lang = body.get("lang").and_then(Value::as_str).unwrap_or("auto");
    let started = crate::listen_api::record_start(State(st.clone()), Some(Json(json!({ "live": true, "lang": lang, "device": body.get("device") })))).await;
    if !started.status().is_success() {
        return started;
    }
    LISTENING.store(true, std::sync::atomic::Ordering::Relaxed);
    tokio::spawn(async move {
        let mut heard = String::new();
        while LISTENING.load(std::sync::atomic::Ordering::Relaxed) {
            let state = crate::listen_live::state();
            let partial = state["partial"]["text"].as_str().unwrap_or_default().to_string();
            if !partial.is_empty() && partial != heard && !is_echo(&partial) {
                interrupt(&st);
                heard = partial;
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
    });
    started
}

/// GET /speech/listen/next?after=N&seconds=S — the person's phrases after the first N, waiting up to S seconds (at most 55)
/// for one; the studio's own voice heard back is left out.
pub async fn listen_next(axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>) -> Json<Value> {
    let after: usize = q.get("after").and_then(|v| v.parse().ok()).unwrap_or(0);
    let seconds: u64 = q.get("seconds").and_then(|v| v.parse().ok()).unwrap_or(30).min(55);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    loop {
        let state = crate::listen_live::state();
        let all = state["lines"].as_array().cloned().unwrap_or_default();
        let fresh: Vec<Value> = all
            .iter()
            .enumerate()
            .skip(after)
            .filter(|(_, l)| !is_echo(l["text"].as_str().unwrap_or_default()))
            .map(|(n, l)| json!({ "n": n + 1, "text": l["text"], "start": l["start"], "end": l["end"] }))
            .collect();
        let running = state["running"].as_bool().unwrap_or(false);
        if !fresh.is_empty() || !running || std::time::Instant::now() >= until {
            return Json(json!({ "lines": fresh, "heard": all.len(), "partial": state["partial"], "listening": running, "error": state["error"] }));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// DELETE /speech/listen — the microphone off; the conversation's recording and what was heard stay a Listening project.
pub async fn listen_stop(State(st): State<AppState>) -> Response {
    LISTENING.store(false, std::sync::atomic::Ordering::Relaxed);
    crate::listen_api::record_stop(State(st)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_phrases_are_cut_and_the_rest_waits() {
        let mut buf = "Привет! Это первая фраза. А это ещё не".to_string();
        assert_eq!(cut(&mut buf, false), ["Привет!", "Это первая фраза."]);
        assert_eq!(buf, " А это ещё не");
        buf.push_str(" кончилась");
        assert!(cut(&mut buf, false).is_empty());
        assert_eq!(cut(&mut buf, true), ["А это ещё не кончилась"]);
        assert!(buf.is_empty());
    }

    #[test]
    fn a_number_with_a_dot_and_a_long_phrase_without_a_stop() {
        let mut buf = "Встреча в 10.30 у входа. Дальше".to_string();
        assert_eq!(cut(&mut buf, false), ["Встреча в 10.30 у входа."]);
        let mut long = format!("{}, и ещё немного", "слово ".repeat(25));
        let said = cut(&mut long, false);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].ends_with(','));
        assert_eq!(long.trim(), "и ещё немного");
    }

    #[test]
    fn the_studio_hearing_its_own_phrase_is_echo_and_a_person_is_not() {
        SAID.lock().unwrap().push((std::time::Instant::now(), "Старинный замок построили в тысяча восемьсот восемьдесят седьмом году.".into()));
        assert!(is_echo("замок построили в тысяча восемьсот"));
        assert!(!is_echo("подожди, а кто его построил"));
        assert!(!is_echo(""));
    }

    #[test]
    fn chinese_phrases_end_at_their_own_marks() {
        let mut buf = "你好。今天天气很好！".to_string();
        assert_eq!(cut(&mut buf, false), ["你好。", "今天天气很好！"]);
    }

    #[test]
    fn a_quote_closing_the_phrase_stays_with_it() {
        let mut buf = "Он сказал: «Иду!» Потом ушёл".to_string();
        assert_eq!(cut(&mut buf, false), ["Он сказал: «Иду!»"]);
    }
}
