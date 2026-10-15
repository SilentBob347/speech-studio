//! The music studios of this computer — YuE2 Studio, MiniMax Music3 Studio, ACE-Step Studio — making the music of a
//! book or play through their own MCP, as VideoSOS uses them (`vs-server/src/studios.rs`): the form is the input
//! schema of their `song_create`, the song is made and kept in the studio's library, and its audio comes here as a copy.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Map, Value};
use vs_providers::schema_form::{fields_of_object, Skip};

pub struct Studio {
    pub id: &'static str,
    pub name: &'static str,
    port: u16,
    /// The studio's page, where it is downloaded (as VideoSOS's family list links them).
    page: &'static str,
}

/// The ports each studio's desktop shell serves on (their `SERVER_PORT` / `studio.json`), as VideoSOS knows them.
pub const STUDIOS: &[Studio] = &[
    Studio { id: "yue2-studio", name: "YuE2 Studio", port: 8791, page: "https://timoncool.github.io/YuE2-Studio/" },
    Studio { id: "minimax-music3-studio", name: "MiniMax Music3 Studio", port: 8765, page: "https://timoncool.github.io/MiniMax-Music3-Studio/" },
    Studio { id: "ace-step-studio", name: "ACE-Step Studio", port: 8792, page: "https://timoncool.github.io/ACE-Step-Studio/" },
];

const PROTOCOL: &str = "2026-07-28";
/// The order of a song form's fields, as VideoSOS shows them; the studio's own keys that are not a person's choice stay out.
const ORDER: &[&str] = &["style", "caption", "prompt", "tags", "lyrics", "title", "duration_seconds", "duration", "seed"];
const SKIP: &[&str] = &["playlist_id", "client_ref", "cover_of", "semantic_tokens"];

pub fn studio(id: &str) -> Option<&'static Studio> {
    STUDIOS.iter().find(|s| s.id == id)
}

fn client(timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder().timeout(timeout).no_proxy().build().map_err(|e| e.to_string())
}

/// One JSON-RPC request to a studio's MCP, as the studios' stateless revision asks it.
fn call(client: &reqwest::blocking::Client, studio: &Studio, method: &str, name: Option<&str>, params: Value) -> Result<Value, String> {
    let mut request = client
        .post(format!("http://127.0.0.1:{}/mcp", studio.port))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", PROTOCOL)
        .header("Mcp-Method", method);
    if let Some(name) = name {
        request = request.header("Mcp-Name", name);
    }
    let text = request.json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })).send().and_then(|r| r.text()).map_err(|e| format!("{}: {e}", studio.name))?;
    let body = text.lines().find_map(|line| line.strip_prefix("data:")).map(str::trim).unwrap_or(text.trim());
    let answer: Value = serde_json::from_str(body).map_err(|e| format!("{}: {e}", studio.name))?;
    if let Some(error) = answer.get("error") {
        return Err(format!("{}: {}", studio.name, error["message"].as_str().unwrap_or("error")));
    }
    Ok(answer["result"].clone())
}

/// A studio tool's answer: its structured content, or the text when it gave none.
fn tool(client: &reqwest::blocking::Client, studio: &Studio, name: &str, arguments: Value) -> Result<Value, String> {
    let result = call(client, studio, "tools/call", Some(name), json!({ "name": name, "arguments": arguments }))?;
    let text = result["content"][0]["text"].as_str().unwrap_or_default().to_string();
    if result["isError"].as_bool() == Some(true) {
        return Err(format!("{}: {text}", studio.name));
    }
    Ok(match result.get("structuredContent") {
        Some(structured) if !structured.is_null() => structured.clone(),
        _ => serde_json::from_str(&text).unwrap_or(Value::String(text)),
    })
}

/// Every music studio, whether it answers now and, when it does, the form of its `song_create`; the studios are asked
/// at once, as a closed port takes a while to refuse.
pub fn list() -> Result<Vec<Value>, String> {
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(3)).connect_timeout(Duration::from_millis(800)).no_proxy().build().map_err(|e| e.to_string())?;
    let ask = |studio: &Studio| {
        let schema = call(&client, studio, "tools/list", None, json!({})).ok().and_then(|tools| {
            tools["tools"].as_array().and_then(|tools| tools.iter().find(|t| t["name"] == "song_create")).map(|t| t["inputSchema"].clone())
        });
        match schema {
            Some(schema) => {
                let order: Vec<String> = ORDER.iter().map(|k| k.to_string()).collect();
                let fields = fields_of_object(&schema, &schema, "", &Skip { keys: SKIP }, false, Some(&order));
                json!({ "id": studio.id, "name": studio.name, "page": studio.page, "running": true, "fields": fields })
            }
            None => json!({ "id": studio.id, "name": studio.name, "page": studio.page, "running": false, "fields": [] }),
        }
    };
    std::thread::scope(|scope| {
        let asked: Vec<_> = STUDIOS.iter().map(|studio| scope.spawn(|| ask(studio))).collect();
        asked.into_iter().zip(STUDIOS).map(|(h, studio)| h.join().map_err(|_| format!("{}: asking it broke down", studio.name))).collect()
    })
}

/// A song made in `studio` with the form's `values`: the audio file of each song it made, with the song's id and title,
/// as the studio's library keeps them. The job is cancelled in the studio when this one is.
pub fn make_song(studio: &Studio, values: &Map<String, Value>, progress: &dyn Fn(String)) -> Result<Vec<(PathBuf, String, String)>, String> {
    let client = client(Duration::from_secs(60))?;
    let arguments: Map<String, Value> = values.iter().filter(|(_, v)| !v.is_null() && v.as_str() != Some("")).map(|(k, v)| (k.clone(), v.clone())).collect();
    let job = tool(&client, studio, "song_create", Value::Object(arguments))?;
    let job_id = job["id"].as_str().or_else(|| job["job_id"].as_str()).ok_or_else(|| t!("music-no-job", studio = studio.name))?.to_string();
    let done = loop {
        std::thread::sleep(Duration::from_secs(2));
        if crate::jobs::cancelled() {
            let _ = tool(&client, studio, "song_job_cancel", json!({ "job_id": job_id }));
            return Err(crate::jobs::CANCELLED.to_string());
        }
        let job = tool(&client, studio, "song_job_get", json!({ "job_id": job_id, "response_format": "detailed" }))?;
        match job["status"].as_str() {
            Some("completed") => break job,
            Some("failed") | Some("cancelled") => return Err(format!("{}: {}", studio.name, job["message"].as_str().unwrap_or_default())),
            _ => progress([job["phase"].as_str(), job["message"].as_str()].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(": ")),
        }
    };
    let songs: Vec<&Value> = done["songs"].as_array().into_iter().flatten().chain(done.get("song")).collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for song in songs {
        let Some(id) = song["id"].as_str().filter(|id| seen.insert(id.to_string())) else { continue };
        let files = tool(&client, studio, "library_song_files", json!({ "song_id": id }))?;
        let audio = files["audio"].as_str().map(PathBuf::from).ok_or_else(|| t!("music-no-song", studio = studio.name))?;
        let title = song["song"]["title"].as_str().or_else(|| song["title"].as_str()).filter(|t| !t.trim().is_empty()).unwrap_or(studio.name).to_string();
        out.push((audio, id.to_string(), title));
    }
    if out.is_empty() {
        return Err(t!("music-no-song", studio = studio.name));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dub_llm::test_http::{serve, Reply};

    fn answer(content: Value) -> Reply {
        Reply::json(200, &json!({ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": content.to_string() }], "structuredContent": content } }).to_string())
    }

    #[test]
    fn a_song_is_asked_followed_and_its_audio_taken_from_the_studios_library() {
        let server = serve(vec![
            answer(json!({ "id": "job-1", "status": "queued" })),
            answer(json!({ "id": "job-1", "status": "running", "phase": "lm", "message": "codes" })),
            answer(json!({ "id": "job-1", "status": "completed", "songs": [{ "id": "song-9", "song": { "title": "Rain" } }] })),
            answer(json!({ "audio": "C:/songs/rain.flac" })),
        ]);
        let port: u16 = server.base().rsplit(':').next().unwrap().parse().unwrap();
        let studio = Studio { id: "yue2-studio", name: "YuE2 Studio", port, page: "" };
        let mut values = Map::new();
        values.insert("style".into(), json!("calm piano"));
        values.insert("lyrics".into(), json!(""));
        let said = std::sync::Mutex::new(Vec::new());
        let songs = make_song(&studio, &values, &|s| said.lock().unwrap().push(s)).unwrap();
        assert_eq!(songs, vec![(PathBuf::from("C:/songs/rain.flac"), "song-9".to_string(), "Rain".to_string())]);
        assert_eq!(said.lock().unwrap().as_slice(), ["lm: codes"]);
        let create = server.request(0);
        assert!(create.contains("\"name\":\"song_create\"") && create.contains("calm piano") && !create.contains("lyrics"), "{create}");
        assert!(server.request(3).contains("\"song_id\":\"song-9\""));
    }
}
