//! OpenAI-compatible routes for other programs on this computer: `POST /v1/audio/speech`, `POST
//! /v1/audio/transcriptions`, `GET /v1/models`. `voice` is a voice name of the library, `model` an engine alias,
//! `instructions` the block direction; errors come in OpenAI's shape. The work is the agent's one-call tools, waited for
//! whole. With `SPEECH_STUDIO_API_TOKEN` set, a request needs `Authorization: Bearer <token>`.

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Multipart, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::AppState;

/// How long a request waits for its audio or transcript.
const LONGEST: Duration = Duration::from_secs(30 * 60);

/// Engine aliases: OpenAI's names map to the local engines.
const SPEECH_MODELS: &[(&str, &str)] = &[("tts-1", "higgs"), ("tts-1-hd", "higgs"), ("gpt-4o-mini-tts", "higgs"), ("higgs", "higgs"), ("drama3", "drama3")];
const TRANSCRIBE_MODELS: &[&str] = &["whisper-1", "gpt-4o-transcribe", "gpt-4o-mini-transcribe", "parakeet"];

fn error(status: StatusCode, kind: &str, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": { "message": message.into(), "type": kind, "param": Value::Null, "code": Value::Null } }))).into_response()
}

fn authorised(headers: &HeaderMap) -> Result<(), Response> {
    let Ok(token) = std::env::var("SPEECH_STUDIO_API_TOKEN") else { return Ok(()) };
    if token.is_empty() {
        return Ok(());
    }
    let given = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or_default();
    if given == token {
        Ok(())
    } else {
        Err(error(StatusCode::UNAUTHORIZED, "invalid_request_error", "Incorrect API key provided."))
    }
}

/// GET /v1/models
pub async fn models(headers: HeaderMap) -> Response {
    if let Err(r) = authorised(&headers) {
        return r;
    }
    let ids = SPEECH_MODELS.iter().map(|(id, _)| *id).chain(TRANSCRIBE_MODELS.iter().copied());
    Json(json!({ "object": "list", "data": ids.map(|id| json!({ "id": id, "object": "model", "created": 0, "owned_by": "speech-studio" })).collect::<Vec<_>>() })).into_response()
}

/// POST /v1/audio/speech {model, input, voice, response_format?, speed?, instructions?}
pub async fn speech(headers: HeaderMap, Json(body): Json<Value>) -> Response {
    if let Err(r) = authorised(&headers) {
        return r;
    }
    let input = body.get("input").and_then(Value::as_str).unwrap_or_default().trim().to_string();
    if input.is_empty() {
        return error(StatusCode::BAD_REQUEST, "invalid_request_error", "'input' is required.");
    }
    let model = body.get("model").and_then(Value::as_str).unwrap_or("tts-1");
    let Some((_, engine)) = SPEECH_MODELS.iter().find(|(id, _)| *id == model) else {
        return error(StatusCode::BAD_REQUEST, "invalid_request_error", format!("The model '{model}' does not exist: GET /v1/models lists them."));
    };
    let asked = body.get("response_format").and_then(Value::as_str).unwrap_or("mp3");
    let (format, mime) = match asked {
        "mp3" => ("mp3", "audio/mpeg"),
        "wav" => ("wav", "audio/wav"),
        "flac" => ("flac", "audio/flac"),
        "aac" => ("m4a", "audio/aac"),
        "pcm" => ("wav", "audio/pcm"),
        other => return error(StatusCode::BAD_REQUEST, "invalid_request_error", format!("response_format '{other}' is not supported here: mp3, wav, flac, aac or pcm.")),
    };
    let speed = body.get("speed").and_then(Value::as_f64).unwrap_or(1.0);
    if !(0.25..=4.0).contains(&speed) {
        return error(StatusCode::BAD_REQUEST, "invalid_request_error", "speed is between 0.25 and 4.0.");
    }
    let text = if (speed - 1.0).abs() > 1e-3 { format!("[rate:{speed:.2}] {input}") } else { input };
    let mut args = json!({ "text": text, "engine": engine, "format": format });
    if let Some(voice) = body.get("voice").and_then(Value::as_str).filter(|v| !v.is_empty()) {
        args["voice"] = json!(voice);
    }
    if let Some(instructions) = body.get("instructions").and_then(Value::as_str).filter(|v| !v.trim().is_empty()) {
        args["direction"] = json!(instructions.trim());
    }
    let done = match crate::mcp::one_call("speak_text", &args, Instant::now() + LONGEST).await {
        Ok(v) if v["done"] == true => v,
        Ok(_) => return error(StatusCode::GATEWAY_TIMEOUT, "server_error", "The speech took longer than the request may wait."),
        Err(e) => return error(StatusCode::BAD_REQUEST, "invalid_request_error", e),
    };
    let Some(path) = done["file"].as_str() else { return error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "the studio named no file") };
    let bytes = match tokio::fs::read(path).await {
        Ok(b) => b,
        Err(e) => return error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", format!("{path}: {e}")),
    };
    let bytes = if asked == "pcm" { pcm_of_wav(&bytes) } else { bytes };
    ([(header::CONTENT_TYPE, mime)], Body::from(bytes)).into_response()
}

/// Raw 16-bit samples of a WAV file (the `data` chunk).
fn pcm_of_wav(wav: &[u8]) -> Vec<u8> {
    let mut at = 12;
    while at + 8 <= wav.len() {
        let size = u32::from_le_bytes([wav[at + 4], wav[at + 5], wav[at + 6], wav[at + 7]]) as usize;
        if &wav[at..at + 4] == b"data" {
            return wav[at + 8..(at + 8 + size).min(wav.len())].to_vec();
        }
        at += 8 + size + (size & 1);
    }
    Vec::new()
}

/// POST /v1/audio/transcriptions (multipart: file, model, language?, response_format?)
pub async fn transcriptions(State(st): State<AppState>, headers: HeaderMap, mut multipart: Multipart) -> Response {
    if let Err(r) = authorised(&headers) {
        return r;
    }
    let (mut bytes, mut name, mut model, mut language, mut format) = (Vec::new(), String::from("audio.wav"), String::from("whisper-1"), String::from("auto"), String::from("json"));
    while let Ok(Some(field)) = multipart.next_field().await {
        let key = field.name().unwrap_or_default().to_string();
        if key == "file" {
            name = field.file_name().unwrap_or("audio.wav").to_string();
            match field.bytes().await {
                Ok(b) => bytes = b.to_vec(),
                Err(e) => return error(StatusCode::BAD_REQUEST, "invalid_request_error", e.to_string()),
            }
        } else {
            let value = field.text().await.unwrap_or_default();
            match key.as_str() {
                "model" => model = value,
                "language" if !value.trim().is_empty() => language = value.trim().to_lowercase(),
                "response_format" => format = value,
                _ => {}
            }
        }
    }
    if bytes.is_empty() {
        return error(StatusCode::BAD_REQUEST, "invalid_request_error", "'file' is required.");
    }
    if !TRANSCRIBE_MODELS.contains(&model.as_str()) {
        return error(StatusCode::BAD_REQUEST, "invalid_request_error", format!("The model '{model}' does not exist: GET /v1/models lists them."));
    }
    let file_format = match format.as_str() {
        "json" | "text" | "verbose_json" => "json",
        "srt" => "srt",
        "vtt" => "vtt",
        other => return error(StatusCode::BAD_REQUEST, "invalid_request_error", format!("response_format '{other}' is not supported: json, text, srt, verbose_json or vtt.")),
    };
    let ext = std::path::Path::new(&name).extension().and_then(|e| e.to_str()).unwrap_or("wav").to_ascii_lowercase();
    let dir = st.workspace.join(".uploads");
    let path = dir.join(format!("{}.{ext}", blake3::hash(&bytes).to_hex()));
    if !path.is_file() {
        if let Err(e) = tokio::fs::create_dir_all(&dir).await.and(tokio::fs::write(&path, &bytes).await) {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", format!("{}: {e}", path.display()));
        }
    }
    let args = json!({ "path": path, "format": file_format, "lang": language });
    let done = match crate::mcp::one_call("transcribe_file", &args, Instant::now() + LONGEST).await {
        Ok(v) if v["done"] == true => v,
        Ok(_) => return error(StatusCode::GATEWAY_TIMEOUT, "server_error", "The recognition took longer than the request may wait."),
        Err(e) => return error(StatusCode::BAD_REQUEST, "invalid_request_error", e),
    };
    let body = done["transcript"].as_str().unwrap_or_default().to_string();
    match format.as_str() {
        "srt" | "vtt" => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response(),
        _ => {
            let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let lines = parsed["lines"].as_array().cloned().unwrap_or_default();
            let text = lines.iter().filter_map(|l| l["text"].as_str()).collect::<Vec<_>>().join(" ");
            match format.as_str() {
                "text" => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response(),
                "verbose_json" => Json(json!({
                    "task": "transcribe", "language": language, "duration": parsed["duration"], "text": text,
                    "segments": lines.iter().enumerate().map(|(i, l)| json!({ "id": i, "start": l["start"], "end": l["end"], "text": l["text"], "speaker": l["speaker"] })).collect::<Vec<_>>(),
                    "words": lines.iter().flat_map(|l| l["words"].as_array().cloned().unwrap_or_default()).map(|w| json!({ "word": w["text"], "start": w["start"], "end": w["end"] })).collect::<Vec<_>>(),
                }))
                .into_response(),
                _ => Json(json!({ "text": text })).into_response(),
            }
        }
    }
}
