//! The studio as an MCP server: Streamable HTTP, stateless JSON-RPC at `/mcp`.
//!
//! Every tool is a route of the studio's own API, called inside the process
//! through the same router the page talks to, so an agent does exactly what
//! the page does through the same code. A file an agent names by its path is
//! sent to the route as the multipart upload the page would send.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde_json::{json, Value};
use tower::ServiceExt;

mod assistant;
mod atomic;
mod guides;
mod speech_tools;
mod studio_tools;
mod window;

pub(crate) use assistant::{agent_llm, ask_agent, follow_agent_steps};
pub(crate) use window::{carry, carry_job, current_actor, save_with_revision, tell_windows, track, PROJECT_CHANGED, REV_HEADER};
pub use window::{window_events, window_focus, window_result};

/// The studio's API router, set once the service has built it.
static API: OnceLock<Router> = OnceLock::new();

/// Takes the API router before the Origin/Host guard is layered: the guard
/// wraps this router together with `/mcp` and `/mcp/status` from outside, so
/// the MCP endpoints are guarded and the tools' own calls do not go through it.
/// The tools' requests still carry `Host: 127.0.0.1`, so they pass the guard
/// should it be put inside this router.
pub fn install(api: Router) {
    let _ = API.set(api);
}

/// The studio's name, as clients show it.
const STUDIO: &str = "Speech Studio";
/// The name clients register the server under.
const SERVER_NAME: &str = "speech-studio";
/// The skill an agent reads, served as a resource and a prompt.
pub(crate) const SKILL: &str = include_str!("../../../docs/mcp-skill.md");
const SKILL_URI: &str = "studio://skill";
const LANGUAGES_URI: &str = "studio://languages";
/// Answers longer than this are cut: a transcript of hundreds of lines is more
/// than an agent reads in one call.
const LIMIT: usize = 60_000;

/// What a tool sends to its route.
enum Payload {
    None,
    Json(Value),
    /// Multipart form fields and files, as the page uploads them.
    Form { fields: Vec<(String, String)>, files: Vec<(String, PathBuf, String)>,
    },
    /// A command for the studio's window: what is on screen and its controls. Answered by
    /// the page itself.
    Window { command: &'static str, args: Value, seconds: u64,
    },
    /// A stream the window follows (server-sent events), read for `seconds` or until it ends.
    Stream { body: Option<Value>, seconds: u64 },
}

struct Call {
    method: Method,
    path: String,
    payload: Payload,
}

struct Tool {
    name: &'static str,
    description: &'static str,
    schema: fn() -> Value,
    call: fn(&Value) -> Result<Call, String>,
}

fn get(path: String) -> Result<Call, String> {
    Ok(Call { method: Method::GET, path, payload: Payload::None,
    })
}

fn post(path: String, body: Value) -> Result<Call, String> {
    Ok(Call { method: Method::POST, path, payload: Payload::Json(body),
    })
}

fn send(method: Method, path: String, body: Value) -> Result<Call, String> {
    Ok(Call { method, path, payload: Payload::Json(body),
    })
}

fn composite(kind: &'static str) -> Result<Call, String> {
    Ok(Call { method: Method::GET, path: format!("composite:{kind}"), payload: Payload::None,
    })
}

/// A one-call tool run from elsewhere in the studio (the OpenAI-compatible routes), waited for until `until`.
pub(crate) async fn one_call(name: &str, args: &Value, until: std::time::Instant) -> Result<Value, String> {
    atomic::run_until(name, args, until).await
}

/// A GET inside the process, as JSON; a route that refuses says why.
async fn fetch(path: &str) -> Result<Value, String> {
    let (status, text) = call_route(Call { method: Method::GET, path: path.into(), payload: Payload::None,
    }).await?;
    if !status.is_success() {
        return Err(format!("GET {path}: {status} {text}"));
    }
    serde_json::from_str(&text).map_err(|_| {
        format!("GET {path} did not answer JSON: {}", text.chars().take(200).collect::<String>())
    })
}

/// The app's version: tauri.conf.json is its single source, as for the window.
fn app_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let conf: Value = serde_json::from_str(include_str!("../../../desktop/src-tauri/tauri.conf.json")).expect("tauri.conf.json is JSON");
        conf["version"].as_str().expect("tauri.conf.json names the app's version").to_string()
    })
}

// ---------------------------------------------------------------- jobs

/// A job that ended, one way or the other; any other status is still at work.
const FINISHED: &[&str] = &["done", "error", "failed", "cancelled", "abandoned", "interrupted",
];

fn finished(job: &Value) -> bool {
    job["status"].as_str().is_some_and(|status| FINISHED.contains(&status))
}

/// The rows of GET /jobs: its list, bare or under "jobs".
fn job_rows(listed: &Value) -> Result<Vec<Value>, String> {
    listed
        .as_array()
        .or_else(|| listed.get("jobs").and_then(Value::as_array))
        .cloned()
        .ok_or_else(|| {
            format!("GET /jobs answered no list of jobs: {}", listed.to_string().chars().take(200).collect::<String>())
        })
}

/// A job as an agent follows it: what it is, where it got, what it made. A
/// result that is a whole project is left to project_get.
fn compact_job(job: &Value) -> Value {
    let result = &job["result"];
    let result = if result.get("segments").is_some() {
        json!("the project, updated: project_get reads it")
    } else {
        result.clone()
    };
    let mut row = json!({
        "id": job.get("id").or_else(|| job.get("job_id")).cloned().unwrap_or(Value::Null),
        "kind": job["kind"], "pid": job["pid"], "status": job["status"],
        "stage": job["stage"], "msg": job["msg"], "pct": job["pct"],
    });
    if !job["position"].is_null() {
        row["position"] = job["position"].clone();
    }
    if !result.is_null() {
        row["result"] = result;
    }
    if !job["error"].is_null() {
        row["error"] = job["error"].clone();
    }
    row
}

/// Where a running job is, in one line.
fn describe_job(job: &Value) -> String {
    let text = |key: &str| job[key].as_str().map(str::to_string).unwrap_or_default();
    let pct = job["pct"].as_f64().map(|pct| format!(" {pct:.0}%")).unwrap_or_default();
    format!("{} {} of project {} ({}{pct})", text("status"), job["kind"].as_str().unwrap_or("job"), text("pid"), text("stage")).replace(" ()", "")
}

/// The required models still missing and whether the set is ready.
fn compact_setup(setup: &Value) -> Value {
    let missing: Vec<Value> = setup["components"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|component| component["installed"] == false && component["requirement"] == "required")
        .map(|component| json!({ "id": component["id"], "name": component["name"], "size": component["size"] }))
        .collect();
    json!({ "ready": setup["ready"], "driver_ok": setup["driverOk"], "download_pending_bytes": setup["downloadPending"], "missing_required": missing })
}

/// Every component in one line: what it is, whether it is there, what it takes.
fn compact_models(setup: &Value) -> Value {
    let components: Vec<Value> = setup["components"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|component| {
            json!({
                "id": component["id"], "name": component["name"], "purpose": component["purpose"], "requirement": component["requirement"],
                "installed": component["installed"], "size": component["size"], "on_disk": component["bytesOnDisk"], "vram": component["vram"],
            })
        })
        .collect();
    json!({ "ready": setup["ready"], "driver_ok": setup["driverOk"], "download_pending_bytes": setup["downloadPending"], "components": components })
}

// ---------------------------------------------------------------- projects

/// The OpenRouter key never leaves the studio, and a proxy's password is hidden.
fn redact(value: Value) -> Value {
    match value {
        Value::Object(fields) => {
            let mut clean = serde_json::Map::new();
            for (key, value) in fields {
                match key.as_str() {
                    "or_key" => {
                        let set = value.as_str().is_some_and(|key| !key.trim().is_empty());
                        clean.insert("or_key_set".into(), set.into());
                    }
                    "proxy_url" => {
                        let shown = value.as_str().map(|url| Value::String(crate::models::split_proxy_password(url).0)).unwrap_or(value);
                        clean.insert(key, shown);
                    }
                    _ => {
                        clean.insert(key, redact(value));
                    }
                }
            }
            Value::Object(clean)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(redact).collect()),
        other => other,
    }
}

/// What an agent is answered: the parts it acts on, unless it asked for
/// every field with response_format detailed.
fn shape(name: &str, args: &Value, value: Value) -> Value {
    let detailed = args.get("response_format").and_then(Value::as_str) == Some("detailed");
    match name {
        "url_probe" => compact_probe(value),
        _ if detailed => value,
        "jobs_list" => match job_rows(&value) {
            Ok(rows) => {
                let mut listed = json!({ "jobs": rows.iter().map(compact_job).collect::<Vec<_>>() });
                if !value["project_job"].is_null() {
                    listed["project_job"] = value["project_job"].clone();
                }
                listed
            }
            Err(problem) => json!({ "error": problem }),
        },
        "job_get" => compact_job(&value),
        "models_status" => compact_models(&value),
        "settings_get" => json!({
            "settings": value["selection"],
            "choices": {
                "asr_engine": value["asr_engines"], "whisper_model": value["whisper_models"], "whisper_compute": value["whisper_computes"],
                "llama_ubatch": value["llama_ubatches"], "higgs_ref_secs": value["higgs_ref_secs_opts"],
                "local_backend": ["auto", "gpu", "cpu"], "sep_backend": ["auto", "gpu", "cpu"], "diar_backend": ["auto", "gpu", "cpu"], "asr_backend": ["auto", "gpu", "cpu"],
            },
        }),
        "projects_list" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or_default().to_lowercase();
            let bound = |name: &str, end: bool| match args.get(name).and_then(Value::as_str) {
                Some(text) => moment(text, end).map(Some),
                None => Ok(None),
            };
            let (since, until) = match (bound("since", false), bound("until", true)) {
                (Ok(since), Ok(until)) => (since, until),
                (Err(problem), _) | (_, Err(problem)) => return json!({ "error": problem }),
            };
            let projects = value["projects"].as_array().cloned().unwrap_or_default().into_iter().filter(|project| {
                let edited = project["mtime"].as_i64().unwrap_or_default();
                (query.is_empty() || project["name"].as_str().unwrap_or_default().to_lowercase().contains(&query))
                    && since.is_none_or(|since| edited >= since)
                    && until.is_none_or(|until| edited <= until)
            });
            Value::Array(projects.collect())
        }
        _ => value,
    }
}

/// A probe as an agent chooses from it, whatever response_format says: the preview picture's base64
/// is the window's, the automatic subtitles are only their languages, and no track names its file
/// formats, which the download picks itself.
fn compact_probe(mut value: Value) -> Value {
    if let Value::Object(fields) = &mut value {
        fields.remove("thumbnail_data");
        fields.remove("thumbnail_error");
        if let Some(Value::Array(tracks)) = fields.get_mut("subtitles") {
            for track in tracks.iter_mut() {
                *track = json!({ "lang": track["lang"], "name": track["name"] });
            }
        }
        if let Some(Value::Array(tracks)) = fields.get_mut("auto_subtitles") {
            for track in tracks.iter_mut() {
                *track = track["lang"].take();
            }
        }
    }
    value
}

// ---------------------------------------------------------------- waiting

async fn status_summary() -> Value {
    let (jobs, setup, by_link) = tokio::join!(fetch("/jobs"), fetch("/setup/status"), fetch("/url/fetches"));
    let mut summary = json!({});
    match jobs.and_then(|jobs| job_rows(&jobs)) {
        Ok(rows) => {
            let (done, working): (Vec<Value>, Vec<Value>) = rows.into_iter().partition(finished);
            summary["jobs"] = Value::Array(working.iter().map(compact_job).collect());
            summary["finished_jobs"] = Value::Array(done.iter().take(5).map(compact_job).collect());
        }
        Err(problem) => summary["jobs_error"] = problem.into(),
    }
    match setup {
        Ok(setup) => {
            if let Some(download) = download_row(&setup) {
                if let Some(jobs) = summary["jobs"].as_array_mut() {
                    jobs.push(download);
                }
            }
            summary["models"] = compact_setup(&setup);
        }
        Err(problem) => summary["models_error"] = problem.into(),
    }
    match by_link {
        Ok(listed) => {
            let rows = listed["fetches"].as_array().into_iter().flatten().map(fetch_row);
            let (done, working): (Vec<Value>, Vec<Value>) = rows.partition(finished);
            if let Some(jobs) = summary["jobs"].as_array_mut() {
                jobs.extend(working);
            }
            if let Some(ended) = summary["finished_jobs"].as_array_mut() {
                ended.extend(done.into_iter().take(3));
            }
        }
        Err(problem) => summary["url_downloads_error"] = problem.into(),
    }
    summary
}

/// A download by link runs beside the job queue like the models download: it is shown and waited for as work of
/// kind download, and its result names the project it made.
fn fetch_row(fetch: &Value) -> Value {
    let status = match fetch["status"].as_str() {
        Some("downloading") => "running",
        Some("completed") => "done",
        Some("failed") => "error",
        Some(other) => other,
        None => "unknown",
    };
    let pct = match (fetch["downloaded"].as_f64(), fetch["total"].as_f64()) {
        (Some(done), Some(total)) if total > 0.0 => {
            Value::from((done / total * 100.0).min(100.0).round())
        }
        _ => Value::Null,
    };
    let mut row = json!({ "id": fetch["id"], "kind": "download", "pid": fetch["pid"], "status": status, "stage": fetch["phase"], "msg": fetch["title"], "pct": pct, "url": fetch["url"] });
    if fetch["status"] == "completed" {
        row["result"] = json!({ "project_id": fetch["pid"], "subs_imported": fetch["subsImported"] });
    }
    if !fetch["warning"].is_null() {
        row["warning"] = json!({ "code": fetch["warning"], "detail": fetch["warningDetail"] });
    }
    if !fetch["errorCode"].is_null() {
        row["error"] = json!({ "code": fetch["errorCode"], "detail": fetch["error"], "hint": fetch["hint"] });
    }
    row
}

/// The models download runs beside the job queue: while it goes it is shown and waited for as work of
/// kind download.
fn download_row(setup: &Value) -> Option<Value> {
    let active = setup.get("active")?;
    if active["status"] != "downloading" {
        return None;
    }
    let (done, total) = (active["downloaded"].as_f64().unwrap_or(0.0), active["total"].as_f64().unwrap_or(0.0),
    );
    Some(json!({
        "id": active["id"], "kind": "download", "status": "running", "stage": active["phase"],
        "pct": if total > 0.0 { Value::from((done / total * 100.0).round()) } else { Value::Null },
        "components": active["ids"], "waiting_seconds": active["waitingS"],
    }))
}

/// The kinds of work a status summary still has running or waiting.
fn busy(summary: &Value) -> Result<Vec<String>, String> {
    if let Some(problem) = summary.get("jobs_error").and_then(Value::as_str) {
        return Err(format!("The studio's jobs cannot be read: {problem}"));
    }
    Ok(summary["jobs"].as_array().into_iter().flatten().map(|job| job["kind"].as_str().unwrap_or("unknown").to_string()).collect())
}

/// The id of a download by link starts so; a job id is hex and never does.
const DOWNLOAD_BY_LINK: &str = "url";

/// How long one wait holds a call: clients give up on a tool call after about
/// a minute, so a wait answers before that and the agent calls it again.
const WAIT_DEFAULT: u64 = 30;
const WAIT_LONGEST: u64 = 55;

/// Waits for a job or for one kind of work or everything, a slice at a time.
async fn wait_for(args: &Value) -> Result<Value, String> {
    let seconds = args.get("seconds").and_then(Value::as_u64).unwrap_or(WAIT_DEFAULT).clamp(2, WAIT_LONGEST);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    let job = args.get("job_id").and_then(Value::as_str).map(str::to_string);
    let until = args.get("until").and_then(Value::as_str).unwrap_or("idle").to_string();
    loop {
        let (done, now) = match &job {
            Some(job) if job.starts_with(DOWNLOAD_BY_LINK) => {
                let state = fetch(&format!("/url/fetches/{}", segment(job)))
                    .await
                    .map_err(|why| format!("No download by link {job} ({why}): its id is fetch.id of project_create_from_url; url_fetches_list shows them."))?;
                let row = fetch_row(&state);
                (finished(&row), row)
            }
            Some(job) => {
                let state = fetch(&format!("/jobs/{}", segment(job)))
                    .await
                    .map_err(|why| format!("No job {job} ({why}): job_id is what project_synth, project_export, project_cast_characters, podcast_write, listen_transcribe, listen_cut, voice_transcribe, voice_clean, voices_download_pack or a one-call tool answering done false returned, or the fetch.id of a download by link (a models download is waited for with until download). Wait for other work with until."))?;
                if state.get("status").and_then(Value::as_str).is_none() {
                    return Err(format!("The job {job} has no status: {state}"));
                }
                (finished(&state), compact_job(&state))
            }
            None => {
                let summary = status_summary().await;
                let working = busy(&summary)?;
                let done = if until == "idle" { working.is_empty() } else { !working.contains(&until) };
                (done, summary)
            }
        };
        if done {
            return Ok(json!({ "done": true, "state": now }));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(json!({ "done": false, "note": "still running; call studio_wait again to keep waiting", "state": now }),
            );
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Tools whose route waits in the graphics card's queue: while a job runs they
/// would wait behind it for minutes, longer than a client holds a call.
const QUEUED_BEHIND_JOBS: &[&str] = &["project_frame"];

async fn graphics_card_free() -> Result<(), String> {
    let jobs = fetch("/jobs").await.map_err(|why| format!("The studio's jobs cannot be read, so a frame could wait behind a render for minutes: {why}"))?;
    let working: Vec<String> = job_rows(&jobs)?.iter().filter(|job| !finished(job)).map(describe_job).collect();
    if working.is_empty() {
        return Ok(());
    }
    Err(format!("The graphics card is busy: {}. A frame waits behind that work; call studio_wait until idle, then project_frame again.", working.join("; ")))
}

// ---------------------------------------------------------------- annotations

/// MCP tool annotations, from what each tool does.
fn annotations(name: &str) -> Value {
    const READS: &[&str] = &["_get", "_status", "_list", "_catalog", "_files", "_capabilities", "_system", "_frame", "_waveform", "_avatar", "_models", "_voices",
    ];
    // a verb that changes something outweighs a noun that reads
    const CHANGES: &[&str] = &[
        "import", "delete", "create", "update", "cancel", "select", "download", "apply", "add", "set", "assign", "analyze", "render", "remix", "retranslate",
        "align", "patch", "put", "rename", "save", "hide", "keep", "reorder", "regen", "enable", "resume", "export", "open", "reveal",
        "shorten", "pin", "refresh",
    ];
    // reads whose names the rules above miss
    const READ_NAMES: &[&str] = &[
        "studio_wait", "proxy_test", "openrouter_verify", "ui_screenshot", "ui_read_page", "ui_console", "url_probe", "url_probe_cookies_text", "studio_health",
        "engine_tags", "voice_sample", "voice_design_sample", "dictionary_try_file", "project_dictionary_counts", "project_unknown_words", "cloud_model_form",
        "cloud_estimate", "cloud_keys", "cloud_spend", "openai_api_log", "openai_api_models", "listen_audio", "project_queue", "history_list",
        "speech_live_watch", "job_events", "book_preview", "book_chapter", "project_take_file", "project_take_peaks", "project_export_file", "agent_status", "windows_watch",
        "player_state",
    ];
    // writes over what was stored, so the earlier content is gone: a client asks first
    const OVERWRITES: &[&str] = &[
        "openrouter_set_key", "voice_transcript_set", "cloud_set_key", "openai_api_key_create", "openai_api_log_clear", "journal_clear", "project_queue_clear",
        "project_queue_remove", "window_command_drop",
    ];
    let changes = CHANGES.iter().any(|verb| name.split('_').any(|word| word == *verb));
    let read_only = READ_NAMES.contains(&name) || !changes && READS.iter().any(|part| name.ends_with(part) || name.contains(&format!("{part}_")));
    let destructive = name.ends_with("_delete") || name.ends_with("_cancel") || name.contains("_cancel_") || name.ends_with("_delete_key") || OVERWRITES.contains(&name);
    // what reaches the internet: OpenRouter, Hugging Face, the sites of links and every download
    let open_world = name.starts_with("google_")
        && matches!(name, "google_set_key" | "google_models")
        || name.starts_with("openrouter_") && !matches!(name, "openrouter_status" | "openrouter_delete_key")
        || matches!(
            name,
            "models_download" | "voice_download" | "voices_download_pack" | "voices_catalog" | "proxy_test" | "url_probe" | "url_probe_cookies_text" | "project_create_from_url" | "url_fetch_resume" | "url_tool_update"
                | "cloud_catalog_refresh" | "cloud_model_form" | "cloud_set_key" | "assistant_chat" | "assistant_models"
        );
    let title = name.replace('_', " ");
    // a one-call tool on a file finds its project again and answers from the finished work
    let idempotent = read_only || name.ends_with("_set") || name.contains("_select") || name.ends_with("_file");
    json!({ "title": title, "readOnlyHint": read_only, "destructiveHint": destructive, "idempotentHint": idempotent, "openWorldHint": open_world })
}

/// What a tool that changes something did, for the journal: the verb and the kind of thing (the window words them).
const JOURNAL: &[(&str, &str, &str)] = &[
    ("project_create", "create", "project"),
    ("project_update", "update", "project"),
    ("project_delete", "delete", "project"),
    ("project_synth", "synth", "project"),
    ("project_export", "export", "project"),
    ("project_cast_characters", "cast", "project"),
    ("podcast_write", "write", "podcast"),
    ("speak_text", "synth", "file"),
    ("transcribe_file", "transcribe", "file"),
    ("verify_speech", "verify", "file"),
    ("separate_file", "separate", "file"),
    ("listen_create_from_file", "create", "listening"),
    ("listen_update", "update", "listening"),
    ("listen_transcribe", "transcribe", "listening"),
    ("listen_cut", "cut", "listening"),
    ("listen_export", "export", "listening"),
    ("listen_check", "verify", "listening"),
    ("listen_to_speech", "create", "project"),
    ("dictionary_set", "update", "dictionary"),
    ("dictionary_import", "import", "dictionary"),
    ("voice_download", "download", "voice"),
    ("voices_download_pack", "download", "voices"),
    ("voice_record_stop", "record", "voice"),
    ("voice_rename", "rename", "voice"),
    ("voice_delete", "delete", "voice"),
    ("voice_card_set", "update", "voice"),
    ("voice_transcript_set", "update", "voice"),
    ("voice_transcribe", "transcribe", "voice"),
    ("voice_clean", "clean", "voice"),
    ("voice_denoise", "clean", "voice"),
    ("voice_design", "create", "voice"),
    ("voice_design_keep", "create", "voice"),
    ("voice_restore", "restore", "voice"),
    ("voice_ref_apply", "update", "voice"),
    ("listen_speaker_remember", "update", "listening"),
    ("speaker_profile_delete", "delete", "listening"),
    ("voice_import", "import", "voice"),
    ("voice_photo_set", "update", "voice"),
    ("voices_export", "export", "voices"),
    ("voices_import_zip", "import", "voices"),
    ("models_download", "download", "model"),
    ("models_cancel_download", "cancel", "download"),
    ("models_remove", "delete", "model"),
    ("models_import", "import", "model"),
    ("models_select", "select", "model"),
    ("settings_set", "update", "setting"),
    ("engine_preset_apply", "apply", "preset"),
    ("proxy_settings_set", "update", "proxy"),
    ("google_set_key", "set", "key"),
    ("openrouter_set_key", "set", "key"),
    ("local_server_key_set", "set", "key"),
    ("google_delete_key", "delete", "key"),
    ("openrouter_delete_key", "delete", "key"),
    ("local_server_key_delete", "delete", "key"),
    ("openrouter_catalog_refresh", "refresh", "catalog"),
    ("url_fetch_cancel", "cancel", "download"),
    ("url_fetch_resume", "resume", "download"),
    ("url_fetch_delete", "delete", "download"),
    ("url_tool_update", "update", "tool"),
    ("job_cancel", "cancel", "job"),
    ("project_create_from_url", "create", "listening"),
    ("project_create_from_subtitles", "create", "project"),
    ("project_create_from_book", "create", "project"),
    ("project_import_package", "import", "project"),
    ("project_package", "export", "project"),
    ("project_export_save", "export", "file"),
    ("project_exports_to_folder", "export", "file"),
    ("project_export_save_as", "export", "file"),
    ("project_queue_clear", "cancel", "job"),
    ("project_queue_remove", "cancel", "job"),
    ("dictionary_try", "synth", "file"),
    ("listen_upload", "create", "listening"),
    ("listen_record_stop", "record", "listening"),
    ("cloud_set_key", "set", "key"),
    ("cloud_delete_key", "delete", "key"),
    ("cloud_catalog_refresh", "refresh", "catalog"),
    ("openai_api_key_create", "set", "key"),
    ("openai_api_key_delete", "delete", "key"),
    ("studio_language_set", "update", "setting"),
    ("models_import_pick", "import", "model"),
    ("diagnostics_report", "create", "file"),
];

/// The name a journal line gives what a tool changed: a name the agent passed, else the project's name, else the id it
/// named. Taken before the call, while a project about to be deleted still has its name.
fn journal_target(args: &Value) -> String {
    const NAMED: &[&str] = &["name", "new_name", "title", "voice", "model"];
    const IDS: &[&str] = &["id", "key", "provider", "path", "pid", "project_id", "job_id", "fetch_id"];
    let text = |key: &&str| args.get(*key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let project = || ["pid", "project_id"].iter().find_map(text).and_then(|pid| crate::journal::project_name(&pid));
    NAMED.iter().find_map(text).or_else(project).or_else(|| IDS.iter().find_map(text)).unwrap_or_default().chars().take(80).collect()
}

/// Journals a tool's change once it succeeded; work an agent still waits for (`done: false`) is journaled when it is
/// asked for again and answers whole, and a call that found its finished work (`reused`) changed nothing.
fn journal_call(name: &str, target: &str, answer: Option<&Value>) {
    let Some((_, verb, kind)) = JOURNAL.iter().find(|(tool, _, _)| *tool == name) else { return };
    if answer.is_some_and(|a| a.get("done") == Some(&Value::Bool(false)) || a.get("reused") == Some(&Value::Bool(true))) {
        return;
    }
    crate::journal::agent_change(verb, kind, target);
}

// ---------------------------------------------------------------- origin and agent

struct Agent {
    /// When an agent last called the server, and what it called.
    last_call: Mutex<Option<(std::time::Instant, String)>>,
    calls: AtomicU64,
}

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| Agent { last_call: Mutex::new(None), calls: AtomicU64::new(0),
    })
}

/// An agent that called within this long still counts as connected.
const AGENT_PRESENT: Duration = Duration::from_secs(600);

fn seen(what: &str) {
    *agent().last_call.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((std::time::Instant::now(), what.to_string()));
    agent().calls.fetch_add(1, Ordering::Relaxed);
}

fn agent_present() -> bool {
    agent().last_call.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().is_some_and(|(at, _)| at.elapsed() < AGENT_PRESENT)
}

/// Whether an agent is connected, for the settings page.
pub async fn status() -> Json<Value> {
    let last = agent().last_call.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    Json(json!({
        "agent_connected": agent_present(),
        "agent_last_call": last.as_ref().map(|(_, what)| what.clone()),
        "agent_seconds_ago": last.as_ref().map(|(at, _)| at.elapsed().as_secs()),
        "agent_calls": agent().calls.load(Ordering::Relaxed),
        "window_open": window::windows_open() > 0,
    }))
}

// ---------------------------------------------------------------- arguments

/// A required text argument, or a message saying which is missing.
fn text(args: &Value, name: &str) -> Result<String, String> {
    args.get(name).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).ok_or_else(|| format!("'{name}' is required"))
}

/// A path segment, escaped.
pub(crate) fn segment(value: &str) -> String {
    value.bytes().map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) { (byte as char).to_string() } else { format!("%{byte:02X}") }
        }).collect()
}

/// The arguments without the ones that went into the path.
fn body_without(args: &Value, taken: &[&str]) -> Value {
    let mut body = args.as_object().cloned().unwrap_or_default();
    for name in taken {
        body.remove(*name);
    }
    Value::Object(body)
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

fn id_only(name: &str, what: &str) -> Value {
    object(json!({ name: { "type": "string", "description": what } }), &[name],
    )
}

fn nothing() -> Value {
    json!({ "type": "object", "additionalProperties": false })
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "video".into())
}

/// The project a tool acts on.
fn pid() -> Value {
    json!({ "type": "string", "description": "project id (projects_list, project_create)" })
}

fn project_only() -> Value {
    object(json!({ "pid": pid() }), &["pid"])
}

/// The route of a project, and what follows it.
fn project_path(args: &Value, rest: &str) -> Result<String, String> {
    Ok(format!("/projects/{}{rest}", segment(&text(args, "pid")?)))
}

/// A query string of the arguments given, in this order.
fn query(pairs: &[(&str, Option<String>)]) -> String {
    let given: Vec<String> = pairs.iter().filter_map(|(name, value)| {
            value.as_ref().map(|value| format!("{name}={}", segment(value)))
        }).collect();
    if given.is_empty() { String::new() } else { format!("?{}", given.join("&")) }
}

/// An argument as a query value: text as it is, a number, or a switch as 1 or 0.
fn given(args: &Value, name: &str) -> Option<String> {
    match args.get(name)? {
        Value::String(text) => Some(text.clone()),
        Value::Bool(on) => Some((if *on { "1" } else { "0" }).into()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn ids(what: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "description": what })
}

/// The languages a project is dubbed from and into.
fn languages() -> Value {
    Value::Array(dub_translate::WHISPER_LANGS.iter().map(|(code, name)| json!({ "code": code, "name": name })).collect(),
    )
}

/// A resource by its uri: its type and its text.
fn resource(uri: &str) -> Option<(&'static str, String)> {
    match uri {
        SKILL_URI => Some(("text/markdown", SKILL.to_string())),
        LANGUAGES_URI => Some(("application/json", serde_json::to_string_pretty(&languages()).unwrap_or_default(),
        )),
        _ => uri.strip_prefix(guides::GUIDE_URI).and_then(guides::guide).map(|text| ("text/plain", text)),
    }
}

// ---------------------------------------------------------------- tools

fn tools() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        let mut all = vec![
            // ---------------------------------------------------------------- the studio
            Tool {
                name: "studio_status",
                description: "What the studio is doing now, in one short summary: the jobs running or waiting (voicing, export, recognition, cut by text, the director model, voice cleaning, downloads) with their project, stage and percent, the last finished ones with their result or error, and whether the required models are there. Call it first, and use studio_wait to wait.",
                schema: nothing,
                call: |_| composite("status"),
            },
            Tool {
                name: "studio_wait",
                description: "Wait for work to finish instead of polling: a job (job_id, as project_synth, project_export, project_cast_characters, project_enrich, podcast_write, listen_transcribe, listen_cut, voice_transcribe, voice_clean, voices_download_pack or a one-call tool answering done false returned it, or the fetch.id of a download by link), or until one kind of work is over - synth, export, analyze (recognition), remix (cut by text), write (podcast script), cast (characters), enrich (the director's tags), voice_transcript, voice_clean, separate, download, voices_pack - or everything (until: idle, the default). Returns when it is done or after seconds (30 by default, at most 55, under the minute clients allow a call) with how far it got; call it again to keep waiting.",
                schema: || object(json!({ "job_id": { "type": "string" }, "until": { "type": "string", "enum": ["idle", "synth", "export", "analyze", "remix", "write", "cast", "enrich", "voice_transcript", "voice_clean", "voice_design", "separate", "download", "voices_pack"] }, "seconds": { "type": "integer" } }), &[]),
                call: |_| composite("wait"),
            },
            Tool {
                name: "studio_system",
                description: "The graphics card, its used and free video memory, load, temperature and power, and the computer's and the studio's RAM: check them before a long job.",
                schema: nothing,
                call: |_| get("/hw/snapshot".into()),
            },
            Tool {
                name: "studio_capabilities",
                description: "What the studio can do here: the languages it dubs, the voice modes, whether ffmpeg is found, the speech recognisers with their models and precisions, the memory limits to choose from, and the current settings (the OpenRouter key only as or_key_set).",
                schema: nothing,
                call: |_| get("/engine/capabilities".into()),
            },
            // ---------------------------------------------------------------- jobs
            Tool {
                name: "jobs_list",
                description: "The studio's jobs, running, waiting and lately finished (preview frames are not jobs here): id, kind, project, status, stage, message, percent, place in the queue, and the result or the error. pid narrows them to one project and adds its last job as stored with it (project_job), an interrupted one included: project_resume continues it.",
                schema: || object(json!({ "pid": pid() }), &[]),
                call: |args| get(format!("/jobs{}", query(&[("pid", given(args, "pid"))]))),
            },
            Tool {
                name: "job_get",
                description: "One job: its status (queued, running, done, error, cancelled), kind, project, stage, message, percent, and its result (the output's path, a new project_id) or its error.",
                schema: || id_only("job_id", "job id a tool returned"),
                call: |args| get(format!("/jobs/{}", segment(&text(args, "job_id")?))),
            },
            Tool {
                name: "job_cancel",
                description: "Cancel a job: a waiting one never starts, a running one stops at its next step and ends as cancelled. project_resume starts it again later.",
                schema: || id_only("job_id", "job id a tool returned"),
                call: |args| post(format!("/jobs/{}/cancel", segment(&text(args, "job_id")?)), json!({})),
            },
            // ---------------------------------------------------------------- models and settings
            Tool {
                name: "models_status",
                description: "Every model and engine the studio uses: whether it is installed, its size, what is on disk, the video memory it takes, whether it is required, and whether the required set is ready (ready, driver_ok). Download what is missing with models_download.",
                schema: nothing,
                call: |_| get("/setup/status".into()),
            },
            Tool {
                name: "models_download",
                description: "Download components by id (from models_status): models, their quantisations, engines and runtimes. It runs in the background beside the jobs, not as a job: studio_wait until download waits for it, models_status shows it. Resumes what is partly there; every file is checked against its pinned SHA-256.",
                schema: || object(json!({ "ids": ids("component ids from models_status") }), &["ids"]),
                call: |args| post("/setup/download".into(), json!({ "ids": args.get("ids").cloned().unwrap_or_default() })),
            },
            Tool {
                name: "models_cancel_download",
                description: "Pause the running models download: what came is kept, and models_download with the same ids continues from there.",
                schema: nothing,
                call: |_| post("/setup/cancel".into(), json!({})),
            },
            Tool {
                name: "models_remove",
                description: "Delete downloaded components by id (from models_status) with what is partly downloaded, to free disk space; a stage whose chosen variant is removed falls back to an installed one. Answers what was removed, the bytes freed, the errors and the models' status.",
                schema: || object(json!({ "ids": ids("component ids from models_status") }), &["ids"]),
                call: |args| post("/setup/remove".into(), json!({ "ids": args.get("ids").cloned().unwrap_or_default() })),
            },
            Tool {
                name: "models_folder_open",
                description: "Open the models folder in Explorer, for the user (to put model files there by hand or see what takes the disk); answers its path.",
                schema: nothing,
                call: |_| post("/setup/open-models".into(), json!({})),
            },
            Tool {
                name: "models_import",
                description: "Take model files already on this computer instead of downloading them: every component found in the folder is copied into the studio's models; id limits it to one component. Answers what was imported and the models' status.",
                schema: || object(json!({ "path": { "type": "string", "description": "folder with the model files" }, "id": { "type": "string", "description": "one component id from models_status" } }), &["path"]),
                call: |args| {
                    let mut body = json!({ "path": text(args, "path")? });
                    if let Some(id) = args.get("id").and_then(Value::as_str) {
                        body["id"] = id.into();
                    }
                    post("/setup/import".into(), body)
                },
            },
            Tool {
                name: "models_select",
                description: "Use a downloaded variant for its stage (id from models_status): a quantisation of Higgs (higgs, higgs-q6_k, higgs-q4_k_m), of Gemma (gemma, gemma-q5_0, gemma-q6_k, gemma-q8_0) or of the separator (roformer, roformer-q5, roformer-q4), or a speech recogniser (parakeet, parakeet-ultra-int8, whisper-tiny ... whisper-large-v3-turbo). Applies from the next job, no restart.",
                schema: || id_only("id", "component id from models_status"),
                call: |args| post("/engine/select".into(), json!({ "id": text(args, "id")? })),
            },
            Tool {
                name: "settings_get",
                description: "The studio's settings and the values each takes: the model variant of each stage (tts, mt, sep, asr, asr_engine, whisper_model, whisper_compute, whisper_device), where each local stage runs (local_backend, sep_backend, diar_backend, asr_backend: auto, gpu, cpu), the voicing switches (qc_asr, qc_duration, multitake, breath_on, speech_rate_on, emo_ref_on, duck_on, auto_shorten: rewrite the translation of lines that do not fit their slot and voice them again during a render, lead_into_silence: a line that does not fit starts up to a second earlier, into the silence before it, instead of being sped up; off by default), the memory limits (llama_ubatch, higgs_ref_secs), who translates and who reads frames (llm_provider, vision_provider: local = the studio's Gemma, server = a local OpenAI-compatible server at srv_url with the models srv_llm and srv_vision, openrouter = the cloud with or_llm and or_vision), the other cloud stages through OpenRouter (or_tts_on, or_tts_model, or_tts_voice, or_tts_autocast, or_asr_on, or_asr, or_concurrency), whether the proxy is on (proxy_on; proxy_settings_set sets its address) and the stage benchmark (bench). The OpenRouter key is never shown: or_key_set says whether there is one.",
                schema: nothing,
                call: |_| get("/engine/capabilities".into()),
            },
            Tool {
                name: "studio_paths",
                description: "Where the studio keeps its data on this computer: data_dir, projects_dir (a folder per project), models_dir.",
                schema: nothing,
                call: |_| get("/app/paths".into()),
            },
            Tool {
                name: "settings_set",
                description: "Change one setting to a text value. TTS: tts_provider=local|openrouter|google, google_tts_model=<id without models/>, google_tts_mode=standard|batch|auto (auto: direct up to 10 phrases, batch from 11). Voice/autocast use or_tts_voice/or_tts_autocast, parallel requests use or_concurrency (1..16). Applies from the next job. Secrets: openrouter_set_key, google_set_key; proxy: proxy_settings_set.",
                schema: || object(json!({ "key": { "type": "string" }, "value": { "type": "string" } }), &["key", "value"]),
                call: |args| {
                    let key = text(args, "key")?;
                    if key == "google_key" {
                        return Err("The Google key is set with google_set_key.".into());
                    }
                    if key == "or_key" {
                        return Err("The OpenRouter key is set with openrouter_set_key.".into());
                    }
                    if key == "proxy_url" {
                        return Err("The proxy's address is set with proxy_settings_set.".into());
                    }
                    let value = match args.get("value") {
                        Some(Value::String(value)) => value.clone(),
                        Some(Value::Bool(on)) => (if *on { "1" } else { "0" }).to_string(),
                        Some(Value::Number(number)) => number.to_string(),
                        _ => return Err("'value' is required".into()),
                    };
                    post("/engine/select".into(), json!({ "key": key, "value": value }))
                },
            },
            Tool {
                name: "engine_presets_get",
                description: "The hardware presets (by graphics card, a weak card with the cloud, the cloud alone), this computer's card, video memory and RAM, and the preset recommended for it.",
                schema: nothing,
                call: |_| get("/engine/presets".into()),
            },
            Tool {
                name: "engine_preset_apply",
                description: "Apply a hardware preset (id from engine_presets_get): it sets the quantisations and where each stage runs, and the cloud stages for the cloud presets. Answers the settings it changed.",
                schema: || id_only("id", "preset id from engine_presets_get"),
                call: |args| post("/engine/preset".into(), json!({ "id": text(args, "id")? })),
            },
            Tool {
                name: "proxy_test",
                description: "Check, before saving, whether Hugging Face (model downloads) and OpenRouter (the cloud stages) are reachable: mode custom (default) through the proxy at url - any seller's notation, host:port:user:password included, kind giving the scheme of an address without one - mode system through the Windows proxy, mode off directly. password goes with a url that names the user without one; the stored password is used only for the stored address.",
                schema: || {
                    object(
                        json!({
                            "mode": { "type": "string", "enum": ["custom", "system", "off"] },
                            "kind": { "type": "string", "enum": ["http", "https", "socks5", "socks4"] },
                            "url": { "type": "string" },
                            "password": { "type": "string" },
                        }),
                        &[],
                    )
                },
                call: |args| {
                    let mut body = json!({ "url": args.get("url").and_then(Value::as_str).unwrap_or_default() });
                    for field in ["mode", "kind", "password"] {
                        if let Some(value) = args.get(field).and_then(Value::as_str) {
                            body[field] = value.into();
                        }
                    }
                    post("/engine/proxy/test".into(), body)
                },
            },
            Tool {
                name: "proxy_settings_get",
                description: "The proxy all the studio's traffic goes through (model downloads, OpenRouter): mode (system - as in Windows, custom - its own address, off - direct), kind (the scheme an address without one gets), its address without the password, whether a password is stored, and problem - why a stored address of its own cannot be read.",
                schema: nothing,
                call: |_| get("/engine/proxy/settings".into()),
            },
            Tool {
                name: "proxy_settings_set",
                description: "Set the proxy: mode system (as in Windows), custom (the address in url) or off (direct); kind (http, https, socks5, socks4) for an address written without a scheme; url in any seller's notation (user@host:port, host:port:user:password; a password written into it is stored apart and never shown; empty removes the address); password to change only the password (null removes it). Fields left out stay; mode custom needs an address. proxy_test checks first.",
                schema: || {
                    object(
                        json!({
                            "mode": { "type": "string", "enum": ["system", "custom", "off"] },
                            "kind": { "type": "string", "enum": ["http", "https", "socks5", "socks4"] },
                            "url": { "type": "string" },
                            "password": { "type": ["string", "null"] },
                        }),
                        &[],
                    )
                },
                call: |args| send(Method::PUT, "/engine/proxy/settings".into(), body_without(args, &[])),
            },
            // ---------------------------------------------------------------- OpenRouter
            Tool {
                name: "google_status",
                description: "Whether a direct Google Gemini API key is configured and its source. Never returns the key.",
                schema: nothing,
                call: |_| get("/engine/google/settings".into()),
            },
            Tool {
                name: "google_set_key",
                description: "Verify a Gemini API key against Google's model catalog and store it separately from settings. No paid generation is made.",
                schema: || id_only("key", "Google Gemini API key"),
                call: |args| send(Method::PUT,"/engine/google/settings".into(),json!({"api_key":text(args,"key")?})),
            },
            Tool {
                name: "google_delete_key",
                description: "Remove the stored Google Gemini API key. An environment key cannot be removed by the app.",
                schema: nothing,
                call: |_| send(Method::DELETE,"/engine/google/settings".into(),json!({})),
            },
            Tool {
                name: "google_models",
                description: "List TTS models available through the direct Google API, including supported generation methods. Needs the Google key.",
                schema: nothing,
                call: |_| get("/engine/google/models".into()),
            },
            Tool {
                name: "openrouter_status",
                description: "Whether an OpenRouter key is set for the cloud stages and where it comes from (the studio's store or an environment variable). The key itself is never shown.",
                schema: nothing,
                call: |_| get("/engine/openrouter/settings".into()),
            },
            Tool {
                name: "openrouter_set_key",
                description: "Set the OpenRouter key the cloud stages use: OpenRouter checks it first and it is stored only if it works. It is never shown back.",
                schema: || id_only("key", "the OpenRouter API key, sk-or-..."),
                call: |args| send(Method::PUT, "/engine/openrouter/settings".into(), json!({ "api_key": text(args, "key")? })),
            },
            Tool {
                name: "openrouter_delete_key",
                description: "Remove the stored OpenRouter key; the cloud stages stop until a key is set again.",
                schema: nothing,
                call: |_| send(Method::DELETE, "/engine/openrouter/settings".into(), json!({})),
            },
            Tool {
                name: "openrouter_verify",
                description: "Check an OpenRouter key without storing it: whether it works, its label, limit and usage.",
                schema: || id_only("key", "the OpenRouter API key, sk-or-..."),
                call: |args| post("/engine/openrouter/verify".into(), json!({ "key": text(args, "key")? })),
            },
            Tool {
                name: "openrouter_models",
                description: "The OpenRouter models for a cloud stage: llm (translation), vision (reading frames), tts (voices), asr (speech recognition). Needs the key.",
                schema: || object(json!({ "kind": { "type": "string", "enum": ["llm", "vision", "tts", "asr"] } }), &["kind"]),
                call: |args| get(format!("/engine/openrouter/models{}", query(&[("kind", Some(text(args, "kind")?))]))),
            },
            Tool {
                name: "openrouter_catalog",
                description: "The state of the studio's OpenRouter model catalog (kept on disk): when it was fetched and how many models each cloud stage can choose from (llm, vision, tts, asr); openrouter_models lists them with prices and context.",
                schema: nothing,
                call: |_| get("/engine/openrouter/catalog".into()),
            },
            Tool {
                name: "openrouter_catalog_refresh",
                description: "Fetch the OpenRouter model catalog again now instead of the copy on disk.",
                schema: nothing,
                call: |_| post("/engine/openrouter/catalog/refresh".into(), json!({})),
            },
            Tool {
                name: "local_server_models",
                description: "The models of the local OpenAI-compatible server (Ollama, LM Studio, vLLM, llama-server) for translation and vision: url is its address (default: the one in settings); a stored key goes only to the address it was saved for.",
                schema: || object(json!({ "url": { "type": "string" } }), &[]),
                call: |args| get(format!("/engine/server/models{}", query(&[("url", given(args, "url"))]))),
            },
            Tool {
                name: "local_server_key_status",
                description: "Whether an API key is stored for the local server's address (url, default: the one in settings); the key itself is never shown.",
                schema: || object(json!({ "url": { "type": "string" } }), &[]),
                call: |args| get(format!("/engine/server/key{}", query(&[("url", given(args, "url"))]))),
            },
            Tool {
                name: "local_server_key_set",
                description: "Store an API key for the local server (vLLM and LM Studio can ask for one); it is sent only to that address (url, default: the one in settings).",
                schema: || object(json!({ "api_key": { "type": "string" }, "url": { "type": "string" } }), &["api_key"]),
                call: |args| {
                    let mut body = json!({ "api_key": text(args, "api_key")? });
                    if let Some(url) = given(args, "url") {
                        body["url"] = url.into();
                    }
                    send(Method::PUT, "/engine/server/key".into(), body)
                },
            },
            Tool {
                name: "local_server_key_delete",
                description: "Remove the API key stored for the local server's address (url, default: the one in settings).",
                schema: || object(json!({ "url": { "type": "string" } }), &[]),
                call: |args| send(Method::DELETE, format!("/engine/server/key{}", query(&[("url", given(args, "url"))])), json!({})),
            },
            Tool {
                name: "openrouter_voices",
                description: "The voices of an OpenRouter speech model with their gender and age, and whether it speaks Russian: for or_tts_voice and the characters' voices.",
                schema: || id_only("model", "OpenRouter TTS model id"),
                call: |args| get(format!("/engine/openrouter/voices{}", query(&[("model", Some(text(args, "model")?))]))),
            },
            // ---------------------------------------------------------------- projects
            // ---------------------------------------------------------------- videos by link
            Tool {
                name: "url_probe",
                description: "Look at a video link before downloading it (YouTube and the other sites yt-dlp knows): title, length, uploader, the highest height, the qualities worth offering, the site's subtitles made by people (subtitles, each with lang and name) apart from the languages of the automatic ones (auto_subtitles), the expected size and the preview's link (thumbnail). cookies is the path of a cookies.txt for videos that need a signed-in browser (age, members, bot checks). A refusal names its code and what to do (hint). Needs the component ytdlp (models_download).",
                schema: || object(json!({ "url": { "type": "string", "description": "link of one video" }, "cookies": { "type": "string", "description": "path of a Netscape cookies.txt" } }), &["url"]),
                call: |args| get(format!("/url/probe{}", query(&[("url", Some(text(args, "url")?)), ("cookies", given(args, "cookies"))]))),
            },
            Tool {
                name: "url_fetches_list",
                description: "The downloads by link, newest first: id, link, quality, status (downloading, completed, failed, cancelled, interrupted), phase, bytes, speed, the project it made (pid), a warning about the subtitles, and the error with its code and hint.",
                schema: nothing,
                call: |_| get("/url/fetches".into()),
            },
            Tool {
                name: "url_fetch_get",
                description: "One download by link, as url_fetches_list shows it.",
                schema: || id_only("id", "download id from project_create_from_url or url_fetches_list"),
                call: |args| get(format!("/url/fetches/{}", segment(&text(args, "id")?))),
            },
            Tool {
                name: "url_fetch_cancel",
                description: "Stop a download by link: yt-dlp and what it started are stopped and the part downloaded is deleted.",
                schema: || id_only("id", "download id"),
                call: |args| post(format!("/url/fetches/{}/cancel", segment(&text(args, "id")?)), json!({})),
            },
            Tool {
                name: "url_fetch_resume",
                description: "Continue an interrupted or failed download by link from where it stopped, with the same settings (after fixing what its hint said: a proxy, cookies, an update of yt-dlp).",
                schema: || id_only("id", "download id"),
                call: |args| post(format!("/url/fetches/{}/resume", segment(&text(args, "id")?)), json!({})),
            },
            Tool {
                name: "url_fetch_delete",
                description: "Remove a finished, failed or interrupted download from the list, with what it downloaded partly; the project it made stays.",
                schema: || id_only("id", "download id"),
                call: |args| send(Method::DELETE, format!("/url/fetches/{}", segment(&text(args, "id")?)), json!({})),
            },
            Tool {
                name: "url_tool_status",
                description: "The yt-dlp that downloads by link: whether the component is there, the version in use, the pinned one, the latest release and when it was checked, whether an update runs, and why the last one failed.",
                schema: nothing,
                call: |_| get("/url/tool".into()),
            },
            Tool {
                name: "url_tool_update",
                description: "Check for a newer yt-dlp now (it is checked once a day anyway) and install it beside the pinned one: it is used only after its SHA-256 matches its release and it runs; otherwise the one in use stays. Runs in the background: url_tool_status shows when it is over.",
                schema: nothing,
                call: |_| post("/url/tool/update".into(), json!({})),
            },
            // ---------------------------------------------------------------- the lines
            // ---------------------------------------------------------------- what the project makes
            // ---------------------------------------------------------------- subtitles, titles, blur
            // ---------------------------------------------------------------- casting
            // ---------------------------------------------------------------- glossary
            // ---------------------------------------------------------------- voices
            Tool {
                name: "voices_list",
                description: "The voices of the studio's library: each with its card (gender, age, lang, tags, source, consent, favorite, kind, cleaned, transcript_auto), the transcript of its reference and whether it has a photo and a kept original. broken lists a voice whose card file is damaged, with the reason. brief true answers only {\"voices\":[{\"name\",\"card\":{\"kind\",\"lang\",\"gender\"}}],\"broken\"}: the transcripts make the full answer large.",
                schema: || object(json!({ "brief": { "type": "boolean" } }), &[]),
                call: |args| get(if args.get("brief").and_then(Value::as_bool).unwrap_or(false) { "/voices?brief=true" } else { "/voices" }.into()),
            },
            Tool {
                name: "voices_catalog",
                description: "The extra voices that can be downloaded into the library, with their gender.",
                schema: nothing,
                call: |_| get("/voices/catalog".into()),
            },
            Tool {
                name: "voice_download",
                description: "Download one voice of voices_catalog into the library.",
                schema: || id_only("name", "voice name from voices_catalog"),
                call: |args| post("/voices/get".into(), json!({ "name": text(args, "name")? })),
            },
            Tool {
                name: "voices_download_pack",
                description: "Download the voice pack into the library. A job: studio_wait with its job_id.",
                schema: nothing,
                call: |_| post("/voices/download-pack".into(), json!({})),
            },
            Tool {
                name: "voice_record_devices",
                description: "The microphones the studio can record a voice from.",
                schema: nothing,
                call: |_| get("/record/devices".into()),
            },
            Tool {
                name: "voice_record_start",
                description: "Start recording a new library voice from the user's microphone: name of the voice, device from voice_record_devices (omitted: the default microphone). Ask the user to read 10-20 seconds of clear speech, watch voice_record_level, then voice_record_stop keeps it. ok false names why it could not start.",
                schema: || object(json!({ "name": { "type": "string" }, "device": { "type": "string" } }), &["name"]),
                call: |args| {
                    let mut body = json!({ "name": text(args, "name")? });
                    if let Some(device) = args.get("device").and_then(Value::as_str) {
                        body["device"] = json!(device);
                    }
                    post("/record/start".into(), body)
                },
            },
            Tool {
                name: "voice_record_level",
                description: "How loud the microphone hears the user now, 0 to 1, while voice_record_start records.",
                schema: nothing,
                call: |_| get("/record/level".into()),
            },
            Tool {
                name: "voice_record_stop",
                description: "Stop the recording and keep it as a voice of the user's own: answers its name, the job that cleans it (clean; by default as the setting clean new voices, on unless turned off) and recognises its reference text (transcribe, default true) - follow it with studio_wait - and the library.",
                schema: || object(json!({ "clean": { "type": "boolean" }, "transcribe": { "type": "boolean" } }), &[]),
                call: |args| {
                    let mut body = json!({ "transcribe": args.get("transcribe").and_then(Value::as_bool).unwrap_or(true) });
                    if let Some(clean) = args.get("clean").and_then(Value::as_bool) {
                        body["clean"] = json!(clean);
                    }
                    post("/record/stop".into(), body)
                },
            },
            Tool {
                name: "voice_rename",
                description: "Rename a voice of the library.",
                schema: || object(json!({ "from": { "type": "string" }, "to": { "type": "string" } }), &["from", "to"]),
                call: |args| post("/voices/rename".into(), json!({ "from": text(args, "from")?, "to": text(args, "to")? })),
            },
            Tool {
                name: "voice_delete",
                description: "Delete a voice from the library.",
                schema: || id_only("name", "voice name from voices_list"),
                call: |args| post("/voices/delete".into(), json!({ "name": text(args, "name")? })),
            },
            Tool {
                name: "voice_card_set",
                description: "Change a voice's card: gender (female, male or empty), age (child, teen, adult, senior or empty), lang (a code such as ru), tags (the whole list), source (where the voice comes from), consent (own, permitted, public or empty), favorite, kind (clone, preset, design), variant_of with variant (this voice is a variant of another voice, the same character angry or whispering: the other voice's name and the variant's name; empty variant_of makes it a voice of its own), license (an SPDX id such as CC-BY-4.0, or the terms in words; it travels with the voice in voices_export), rotate (a reference longer than the engines take gives each new take its next good piece instead of always the best one). Only the fields given change; an unknown field or value is refused.",
                schema: || object(json!({ "name": { "type": "string" }, "card": { "type": "object" } }), &["name", "card"]),
                call: |args| post("/voices/card".into(), json!({ "name": text(args, "name")?, "card": args.get("card").cloned().unwrap_or(Value::Null) })),
            },
            Tool {
                name: "voice_transcript_set",
                description: "Write the text spoken in a voice's reference by hand (the engine clones better with the right text); an empty text removes it.",
                schema: || object(json!({ "name": { "type": "string" }, "text": { "type": "string" } }), &["name", "text"]),
                call: |args| post("/voices/transcript".into(), json!({ "name": text(args, "name")?, "text": args.get("text").and_then(Value::as_str).unwrap_or("") })),
            },
            Tool {
                name: "voice_transcribe",
                description: "Recognise the text spoken in a voice's reference with the recognition engine chosen in the settings: a job, follow it with studio_wait.",
                schema: || id_only("name", "voice name from voices_list"),
                call: |args| post("/voices/transcribe".into(), json!({ "name": text(args, "name")? })),
            },
            Tool {
                name: "voice_clean",
                description: "Clean a voice of music and noise (Mel-Band Roformer) before cloning: a job, follow it with studio_wait. The original is kept and voice_restore brings it back. Refused with clean_missing when the separation engine is not downloaded (models_download roformer and bsroformer-engine).",
                schema: || id_only("name", "voice name from voices_list"),
                call: |args| post("/voices/clean".into(), json!({ "name": text(args, "name")? })),
            },
            Tool {
                name: "voice_denoise",
                description: "Remove noise from a voice's reference (DPDFNet) before cloning: a job, follow it with studio_wait. db limits how far the noise is pushed down (18 keeps speech natural; 0 or none removes as much as it can). The original is kept and voice_restore brings it back.",
                schema: || object(json!({ "name": { "type": "string", "description": "voice name from voices_list" }, "db": { "type": "number", "minimum": 0 } }), &["name"]),
                call: |args| post("/voices/denoise".into(), json!({ "name": text(args, "name")?, "db": args.get("db").cloned().unwrap_or(Value::Null) })),
            },
            Tool {
                name: "voice_design",
                description: "Make a voice from a description (Qwen3-TTS VoiceDesign): gender, age, timbre and manner in a sentence or two, best in English or Chinese, and a sample text in the language the voice will speak. A job, follow it with studio_wait: its result has the batch id and candidates (k, seconds); each sounds at /voices/design/sample?id=&k=. Keep one with voice_design_keep. Downloads the model (about 1.2 GB) the first time.",
                schema: || object(json!({ "description": { "type": "string" }, "sample": { "type": "string" }, "lang": { "type": "string" }, "count": { "type": "integer", "minimum": 1, "maximum": 6 } }), &["description", "sample"]),
                call: |args| post("/voices/design".into(), args.clone()),
            },
            Tool {
                name: "voice_design_keep",
                description: "Keep a candidate of voice_design as a library voice: the sample text becomes its reference transcript and the description its source.",
                schema: || object(json!({ "id": { "type": "string" }, "k": { "type": "integer" }, "name": { "type": "string" }, "description": { "type": "string" }, "sample": { "type": "string" }, "lang": { "type": "string" } }), &["id", "k", "name", "description", "sample"]),
                call: |args| post("/voices/design/keep".into(), args.clone()),
            },
            Tool {
                name: "voice_restore",
                description: "Bring back a voice's sound as it was before voice_clean or voice_denoise.",
                schema: || id_only("name", "voice name from voices_list"),
                call: |args| post("/voices/restore".into(), json!({ "name": text(args, "name")? })),
            },
            Tool {
                name: "voice_ref_candidates",
                description: "The best pieces of a voice's reference for cloning, best first: windows cut at pauses between the recognised words, no longer than the engine's reference limit, each with its duration, longest pause inside, signal-to-noise (snr_db), clipping share and issues (long_pause, noisy, clipped, short). Needs the reference text recognised first (voice_transcribe).",
                schema: || id_only("name", "voice name from voices_list"),
                call: |args| post("/voices/ref-candidates".into(), json!({ "name": text(args, "name")? })),
            },
            Tool {
                name: "voice_ref_apply",
                description: "Keep only the piece start..end (seconds) of a voice's reference: the sound is cut and its text and word timings follow; the original is kept and voice_restore brings it back.",
                schema: || object(json!({ "name": { "type": "string" }, "start": { "type": "number" }, "end": { "type": "number" } }), &["name", "start", "end"]),
                call: |args| post("/voices/ref-apply".into(), json!({ "name": text(args, "name")?, "start": args.get("start").and_then(Value::as_f64), "end": args.get("end").and_then(Value::as_f64) })),
            },
            Tool {
                name: "voice_import",
                description: "Make voices of sound files on this computer (WAV, MP3, FLAC, M4A, OGG), each brought to the reference loudness. name names a single file's voice (a taken name gets a number). clean (by default the setting clean new voices, on unless turned off) and transcribe (default true) start a job on every new voice; follow them with studio_wait.",
                schema: || object(json!({ "paths": { "type": "array", "items": { "type": "string" } }, "name": { "type": "string" }, "clean": { "type": "boolean" }, "transcribe": { "type": "boolean" } }), &["paths"]),
                call: |args| {
                    let paths = args.get("paths").and_then(Value::as_array).ok_or("paths must be a list of files")?;
                    let mut files = Vec::new();
                    for p in paths {
                        let path = PathBuf::from(p.as_str().ok_or("every path must be a string")?);
                        if !path.is_file() {
                            return Err(format!("{} is not a file on this computer.", path.display()));
                        }
                        let name = file_name(&path);
                        files.push(("file".to_string(), path, name));
                    }
                    let flag = |key: &str, default: bool| if args.get(key).and_then(Value::as_bool).unwrap_or(default) { "1" } else { "0" }.to_string();
                    let mut fields = vec![("transcribe".to_string(), flag("transcribe", true))];
                    if args.get("clean").and_then(Value::as_bool).is_some() {
                        fields.push(("clean".to_string(), flag("clean", false)));
                    }
                    if let Some(n) = args.get("name").and_then(Value::as_str).filter(|n| !n.trim().is_empty()) {
                        fields.push(("name".to_string(), n.trim().to_string()));
                    }
                    Ok(Call { method: Method::POST, path: "/voices/import".into(), payload: Payload::Form { fields, files } })
                },
            },
            Tool {
                name: "voice_photo_set",
                description: "Give a voice a photo from a JPG, PNG or WebP file on this computer.",
                schema: || object(json!({ "name": { "type": "string" }, "path": { "type": "string" } }), &["name", "path"]),
                call: |args| {
                    let path = PathBuf::from(text(args, "path")?);
                    if !path.is_file() {
                        return Err(format!("{} is not a file on this computer.", path.display()));
                    }
                    let file = file_name(&path);
                    Ok(Call { method: Method::POST, path: "/voices/photo".into(), payload: Payload::Form { fields: vec![("name".to_string(), text(args, "name")?)], files: vec![("file".to_string(), path, file)] } })
                },
            },
            Tool {
                name: "voices_export",
                description: "Save chosen voices with everything of theirs (sound, reference text, card, photo) as a ZIP file at path.",
                schema: || object(json!({ "names": { "type": "array", "items": { "type": "string" } }, "path": { "type": "string", "description": "the .zip file to write" } }), &["names", "path"]),
                call: |args| post("/voices/export".into(), json!({ "names": args.get("names").cloned().unwrap_or(Value::Null), "path": text(args, "path")? })),
            },
            Tool {
                name: "voices_import_zip",
                description: "Take voices from a ZIP file made by voices_export: a voice already in the library (the same sound) is recognised and skipped, a different one under a taken name gets a number.",
                schema: || id_only("path", "the .zip file"),
                call: |args| {
                    let path = PathBuf::from(text(args, "path")?);
                    if !path.is_file() {
                        return Err(format!("{} is not a file on this computer.", path.display()));
                    }
                    let file = file_name(&path);
                    Ok(Call { method: Method::POST, path: "/voices/import-zip".into(), payload: Payload::Form { fields: Vec::new(), files: vec![("file".to_string(), path, file)] } })
                },
            },
        ];
        all.extend(speech_tools::tools());
        all.extend(atomic::tools());
        all.extend(window::tools());
        all.extend(studio_tools::tools());
        all.extend(guides::tools());
        all.extend(assistant::tools());
        all
    })
}

// ---------------------------------------------------------------- calling a route

/// The router the tools call.
fn studio_api() -> Result<Router, String> {
    API.get().cloned().ok_or_else(|| "the studio is still starting".to_string())
}

/// A route's answer as it came.
struct Reply {
    status: StatusCode,
    mime: String,
    bytes: axum::body::Bytes,
}

/// Builds the route's request and calls it inside the process. A page instead of an answer means the route is not
/// there.
async fn respond(call: Call) -> Result<Response, String> {
    let api = studio_api()?;
    let asked = format!("{} {}", call.method, call.path);
    let mut builder = Request::builder().method(call.method).uri(&call.path).header(header::HOST, "127.0.0.1").header(window::AGENT_HEADER, "1");
    // the OpenAI-compatible routes ask other programs for the key; a tool is the studio itself
    if call.path.starts_with("/v1/") {
        if let Some((token, _)) = crate::credentials::api_token() {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
    }
    let request = match call.payload {
        Payload::None | Payload::Stream { body: None, .. } => builder.body(Body::empty()),
        Payload::Window { command, .. } => return Err(format!("{command} is a command of the studio's window, not a route")),
        Payload::Json(body) | Payload::Stream { body: Some(body), .. } => builder.header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())),
        Payload::Form { fields, files } => {
            let boundary = format!("studio-mcp-{}", uuid::Uuid::new_v4().simple());
            let mut parts: Vec<Part> = Vec::new();
            for (name, value) in fields {
                parts.push(Part::Text(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")));
            }
            for (name, path, file) in files {
                if let Err(error) = tokio::fs::metadata(&path).await {
                    return Err(format!("read {}: {error}", path.display()));
                }
                parts.push(Part::Text(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n", file.replace('"', "'"))));
                parts.push(Part::File(path));
                parts.push(Part::Text("\r\n".into()));
            }
            parts.push(Part::Text(format!("--{boundary}--\r\n")));
            builder.header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}")).body(Body::from_stream(streamed(parts)))
        }
    }
    .map_err(|error| error.to_string())?;
    let response = api.oneshot(request).await.map_err(|error| error.to_string())?;
    if mime_of(&response).starts_with("text/html") {
        return Err(format!("The studio has no route {asked}: its page answered instead."));
    }
    Ok(response)
}

fn mime_of(response: &Response) -> String {
    response.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default().to_string()
}

/// A route's whole answer.
async fn reply_of(response: Response) -> Result<Reply, String> {
    let status = response.status();
    let mime = mime_of(&response);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024).await.map_err(|error| error.to_string())?;
    Ok(Reply { status, mime, bytes })
}

/// Calls a route inside the process and returns what it answered.
async fn call_route_raw(call: Call) -> Result<Reply, String> {
    reply_of(respond(call).await?).await
}

/// A route's answer as text, cut to a size an agent reads.
fn reply_text(reply: &Reply) -> String {
    let mut text = match serde_json::from_slice::<Value>(&reply.bytes) {
        Ok(value) => serde_json::to_string(&value).unwrap_or_default(),
        Err(_) => String::from_utf8_lossy(&reply.bytes).into_owned(),
    };
    if text.trim().is_empty() {
        text = if reply.status.is_success() { "Done.".into() } else { reply.status.to_string() };
    }
    text
}

/// Calls a route inside the process and returns what it answered.
async fn call_route(call: Call) -> Result<(StatusCode, String), String> {
    let reply = call_route_raw(call).await?;
    Ok((reply.status, reply_text(&reply)))
}

/// A piece of a multipart body: text, or a file read as it is sent.
enum Part {
    Text(String),
    File(PathBuf),
}

/// The parts as a stream, a file a megabyte at a time: a long video is sent
/// without ever being held in memory whole.
fn streamed(parts: Vec<Part>,
) -> impl futures_util::Stream<Item = std::io::Result<axum::body::Bytes>> {
    futures_util::stream::unfold((parts.into_iter(), None::<tokio::fs::File>), |(mut parts, mut open)| async move {
        loop {
            if let Some(file) = open.as_mut() {
                let mut chunk = vec![0u8; 1 << 20];
                match tokio::io::AsyncReadExt::read(file, &mut chunk).await {
                    Ok(0) => open = None,
                    Ok(read) => {
                        chunk.truncate(read);
                        return Some((Ok(chunk.into()), (parts, open)));
                    }
                    Err(error) => return Some((Err(error), (parts, None))),
                }
                continue;
            }
            match parts.next()? {
                Part::Text(text) => return Some((Ok(text.into()), (parts, None))),
                Part::File(path) => match tokio::fs::File::open(&path).await {
                    Ok(file) => open = Some(file),
                    Err(error) => return Some((Err(error), (parts, None))),
                },
            }
        }
    },
    )
}

/// Cuts an answer to what an agent reads in one go, and says how to get the rest.
fn cut(mut text: String) -> String {
    if text.len() > LIMIT {
        let mut end = LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n... (cut: ask for a part, e.g. project_get with from and to seconds or ids)",
        );
    }
    text
}

// ---------------------------------------------------------------- the protocol

/// The protocol revisions the studio speaks: the stateless one, where every
/// request carries its version, and the handshake ones older clients open
/// with `initialize`.
const MODERN: &[&str] = &["2026-07-28"];
const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
/// How long a client may keep the tool list, the prompts and the resources:
/// they change only with the studio, but an update should show within minutes.
const LIST_TTL_MS: u64 = 300_000;
const HEADER_MISMATCH: i64 = -32020;
const UNSUPPORTED_VERSION: i64 = -32022;

fn server_info() -> Value {
    json!({ "name": SERVER_NAME, "title": STUDIO, "version": app_version() })
}

fn supported_versions() -> Vec<&'static str> {
    MODERN.iter().chain(LEGACY).copied().collect()
}

/// A complete result, signed with the server's identity.
fn rpc(id: Value, mut result: Value) -> Response {
    if let Some(fields) = result.as_object_mut() {
        fields.entry("resultType").or_insert_with(|| "complete".into());
        let meta = fields.entry("_meta").or_insert_with(|| json!({}));
        meta["io.modelcontextprotocol/serverInfo"] = server_info();
    }
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

fn rpc_error(id: Value, code: i64, message: String) -> Response {
    rpc_failure(StatusCode::OK, id, code, message, None)
}

fn rpc_failure(status: StatusCode, id: Value, code: i64, message: String, data: Option<Value>,
) -> Response {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    (status, Json(json!({ "jsonrpc": "2.0", "id": id, "error": error })),
    ).into_response()
}

/// A list or a read a client may cache: the same for everyone, fresh for five minutes.
fn cacheable(mut result: Value) -> Value {
    result["ttlMs"] = LIST_TTL_MS.into();
    result["cacheScope"] = "public".into();
    result
}

/// A header value, with the Base64 sentinel form decoded.
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    use base64::Engine;
    let raw = headers.get(name)?.to_str().ok()?;
    match raw.strip_prefix("=?base64?").and_then(|inner| inner.strip_suffix("?=")) {
        Some(encoded) => base64::engine::general_purpose::STANDARD.decode(encoded).ok().and_then(|bytes| String::from_utf8(bytes).ok()),
        None => Some(raw.to_string()),
    }
}

/// Why a stateless request is refused, if it is: a version the studio does
/// not speak, or headers that do not say what its body says.
fn refused(headers: &HeaderMap, method: &str, params: &Value, version: &str,
) -> Option<(i64, String, Option<Value>)> {
    if !MODERN.contains(&version) {
        return Some((UNSUPPORTED_VERSION, "Unsupported protocol version".into(), Some(json!({ "supported": supported_versions(), "requested": version })),
        ));
    }
    let mismatch = |what: String| Some((HEADER_MISMATCH, format!("Header mismatch: {what}"), None));
    match header_value(headers, "mcp-protocol-version") {
        Some(value) if value == version => {}
        Some(value) => {
            return mismatch(format!("MCP-Protocol-Version header value '{value}' does not match body value '{version}'"))
        }
        None => return mismatch("the MCP-Protocol-Version header is missing".into()),
    }
    match header_value(headers, "mcp-method") {
        Some(value) if value == method => {}
        Some(value) => {
            return mismatch(format!("Mcp-Method header value '{value}' does not match body value '{method}'"))
        }
        None => return mismatch("the Mcp-Method header is missing".into()),
    }
    let named = match method {
        "tools/call" | "prompts/get" => params.get("name"),
        "resources/read" => params.get("uri"),
        _ => return None,
    }
    .and_then(Value::as_str)
    .unwrap_or_default();
    match header_value(headers, "mcp-name") {
        Some(value) if value == named => None,
        Some(value) => mismatch(format!("Mcp-Name header value '{value}' does not match body value '{named}'")),
        None => mismatch("the Mcp-Name header is missing".into()),
    }
}

/// A tool's answer: the text an agent reads, and the same data structured
/// when it is JSON and whole.
fn tool_result(id: Value, text: String, structured: Option<Value>, error: bool) -> Response {
    let mut result = json!({ "content": [{ "type": "text", "text": text }], "isError": error });
    if let Some(structured) = structured.filter(|value| value.is_object() || value.is_array()) {
        result["structuredContent"] = structured;
    }
    rpc(id, result)
}

fn tool_json(id: Value, value: Value) -> Response {
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
    if text.len() > LIMIT {
        tool_result(id, cut(text), None, false)
    } else {
        tool_result(id, text, Some(value), false)
    }
}

/// What a tool answered: JSON, text, a picture, or the window's own answer.
pub(crate) enum Answer {
    Json(Value),
    Text(String),
    Image { data: String, mime: String, text: String },
    Window(Value),
}

/// A route answered with a file, not with data: audio, video, an archive.
fn is_file_answer(mime: &str) -> bool {
    let mime = mime.split(';').next().unwrap_or_default().trim();
    mime.starts_with("audio/") || mime.starts_with("video/") || matches!(mime, "application/octet-stream" | "application/zip")
}

/// The studio's `agent-files` folder, where the files tools answer with are kept.
pub(super) async fn agent_files() -> Result<PathBuf, String> {
    let paths = fetch("/app/paths").await?;
    let data = paths["data_dir"].as_str().ok_or("the studio named no data folder")?;
    let folder = Path::new(data).join("agent-files");
    tokio::fs::create_dir_all(&folder).await.map_err(|e| format!("{}: {e}", folder.display()))?;
    Ok(folder)
}

/// The file a route answered with, written as it comes into the `agent-files` folder: the agent gets its path, size
/// and type. A type that names no format keeps the extension of the file the route was asked for.
async fn keep_file(name: &str, asked: &str, response: Response) -> Result<Value, String> {
    let mime = mime_of(&response).split(';').next().unwrap_or_default().trim().to_string();
    let ext = match mime.as_str() {
        "audio/wav" | "audio/x-wav" | "audio/wave" | "audio/pcm" => "wav",
        "audio/flac" => "flac",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "audio/mp4" | "audio/aac" | "audio/x-m4a" => "m4a",
        "video/mp4" => "mp4",
        "application/zip" => "zip",
        "application/octet-stream" => Path::new(asked.split('?').next().unwrap_or_default()).extension().and_then(|ext| ext.to_str()).unwrap_or("bin"),
        other => other.rsplit('/').next().unwrap_or("bin"),
    };
    let file = agent_files().await?.join(format!("{name}-{}.{ext}", chrono_stamp()));
    let written = async {
        let mut out = tokio::fs::File::create(&file).await.map_err(|e| e.to_string())?;
        let mut body = response.into_body().into_data_stream();
        let mut bytes = 0u64;
        while let Some(chunk) = futures_util::StreamExt::next(&mut body).await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            bytes += chunk.len() as u64;
            tokio::io::AsyncWriteExt::write_all(&mut out, &chunk).await.map_err(|e| e.to_string())?;
        }
        tokio::io::AsyncWriteExt::flush(&mut out).await.map_err(|e| e.to_string())?;
        Ok::<u64, String>(bytes)
    }
    .await;
    match written {
        Ok(bytes) => Ok(json!({ "file": file.to_string_lossy(), "mime": mime, "bytes": bytes })),
        Err(error) => {
            let _ = tokio::fs::remove_file(&file).await;
            Err(format!("{}: {error}", file.display()))
        }
    }
}

/// A stream the window follows, read for `seconds` or until it ends: its events in order.
async fn read_stream(name: &str, call: Call, seconds: u64) -> Result<Value, String> {
    let response = respond(call).await?;
    if !response.status().is_success() {
        return Err(reply_text(&reply_of(response).await?));
    }
    let until = tokio::time::Instant::now() + Duration::from_secs(seconds);
    let mut body = response.into_body().into_data_stream();
    let mut pending: Vec<u8> = Vec::new();
    let mut events = Vec::new();
    let ended = loop {
        match tokio::time::timeout_at(until, futures_util::StreamExt::next(&mut body)).await {
            Err(_) => break false,
            Ok(None) => break true,
            Ok(Some(Err(error))) => return Err(format!("{name}: {error}")),
            Ok(Some(Ok(chunk))) => {
                pending.extend(chunk.iter().filter(|byte| **byte != b'\r'));
                while let Some(at) = pending.windows(2).position(|pair| pair == b"\n\n") {
                    let event: Vec<u8> = pending.drain(..at + 2).collect();
                    let event = String::from_utf8_lossy(&event);
                    let data: Vec<&str> = event.lines().filter_map(|line| line.strip_prefix("data:")).map(|data| data.strip_prefix(' ').unwrap_or(data)).collect();
                    if !data.is_empty() {
                        let data = data.join("\n");
                        events.push(serde_json::from_str(&data).unwrap_or(Value::String(data)));
                    }
                }
            }
        }
    };
    Ok(json!({ "events": events, "ended": ended }))
}

/// Several tools in one call, run as `tools/call` runs each. Its future is boxed: it runs tools, and a tool runs it.
fn batch(args: &Value) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send + '_>> {
    Box::pin(async move {
    let calls = args.get("calls").and_then(Value::as_array).ok_or("'calls' must be a list of {name, arguments}")?;
    let keep_going = args.get("keep_going").and_then(Value::as_bool).unwrap_or(false);
    let mut done = Vec::new();
    for call in calls {
        let name = call.get("name").and_then(Value::as_str).unwrap_or_default();
        let arguments = call.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let answer = match tools().iter().find(|tool| tool.name == name) {
            None => Err(format!("Unknown tool: {name}; tools/list names them all.")),
            Some(tool) if tool.name == "studio_batch" => Err("studio_batch does not run inside itself.".into()),
            Some(tool) => run_tool(tool, &arguments).await,
        };
        let failed = answer.is_err();
        done.push(match answer {
            Ok(Answer::Json(value)) | Ok(Answer::Window(value)) => json!({ "name": name, "result": value }),
            Ok(Answer::Text(text)) => json!({ "name": name, "result": text }),
            Ok(Answer::Image { mime, .. }) => json!({ "name": name, "result": format!("a picture ({mime}): call {name} alone to see it") }),
            Err(error) => json!({ "name": name, "error": error }),
        });
        if failed && !keep_going {
            break;
        }
    }
    Ok(json!({ "calls": done }))
    })
}

/// A name part that keeps files of one tool apart: milliseconds since 1970.
fn chrono_stamp() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

/// Runs a tool as `tools/call` runs one, for an agent and for the Assistant panel alike, and journals its change.
async fn run_tool(tool: &'static Tool, args: &Value) -> Result<Answer, String> {
    let name = tool.name;
    let target = if JOURNAL.iter().any(|(tool, _, _)| *tool == name) { journal_target(args) } else { String::new() };
    // a tool may look at a file on disk before its call: off the runtime
    let prepare = tool.call;
    let call = {
        let args = args.clone();
        tokio::task::spawn_blocking(move || prepare(&args)).await.unwrap_or_else(|error| Err(format!("{name} failed: {error}")))?
    };
    Ok(match call {
        Call { payload: Payload::Window { command, args, seconds }, .. } => Answer::Window(window::ask_window(command, args, seconds).await?),
        call if call.path == "composite:status" => Answer::Json(status_summary().await),
        call if call.path == "composite:wait" => Answer::Json(wait_for(args).await?),
        call if call.path == "composite:questions" => Answer::Json(assistant::wait_for_questions(args).await),
        call if call.path == "composite:answer" => Answer::Json(assistant::answer_question(args)?),
        call if call.path == "composite:agent" => Answer::Json(status().await.0),
        call if call.path == "composite:windows" => Answer::Json(window::watch(args).await),
        call if call.path == "composite:window_focus" => Answer::Json(window::focus(args)?),
        call if call.path == "composite:window_drop" => Answer::Json(window::drop_command(args)?),
        call if call.path == "composite:batch" => Answer::Json(batch(args).await?),
        call if call.path == "composite:guide" => {
            let topic = text(args, "topic")?;
            Answer::Text(guides::guide(&topic).ok_or_else(|| format!("No guide {topic}: {}.", guides::TOPICS.iter().map(|(topic, _)| *topic).collect::<Vec<_>>().join(", ")))?)
        }
        call @ Call { payload: Payload::Stream { .. }, .. } => {
            let seconds = if let Payload::Stream { seconds, .. } = &call.payload { *seconds } else { WAIT_DEFAULT };
            let read = read_stream(name, call, seconds).await?;
            Answer::Json(if name == "speech_live_watch" { studio_tools::live_files(read).await? } else { read })
        }
        call if call.path.starts_with(atomic::PREFIX) => {
            let state = atomic::run(&call.path[atomic::PREFIX.len()..], args).await?;
            journal_call(name, &target, Some(&state));
            Answer::Json(state)
        }
        call => {
            if QUEUED_BEHIND_JOBS.contains(&name) {
                graphics_card_free().await?;
            }
            let asked = call.path.clone();
            let response = respond(call).await?;
            if response.status().is_success() && is_file_answer(&mime_of(&response)) {
                let file = keep_file(name, &asked, response).await?;
                journal_call(name, &target, None);
                return Ok(Answer::Json(file));
            }
            let reply = reply_of(response).await?;
            if !reply.status.is_success() {
                return Err(reply_text(&reply));
            }
            if reply.mime.starts_with("image/") {
                use base64::Engine;
                let mime = reply.mime.split(';').next().unwrap_or_default().trim().to_string();
                Answer::Image { data: base64::engine::general_purpose::STANDARD.encode(&reply.bytes), mime, text: format!("The picture {name} answered with.") }
            } else {
                let text = reply_text(&reply);
                match serde_json::from_str::<Value>(&text) {
                    Ok(value) => {
                        journal_call(name, &target, Some(&value));
                        Answer::Json(shape(name, args, redact(value)))
                    }
                    Err(_) => {
                        journal_call(name, &target, None);
                        Answer::Text(text)
                    }
                }
            }
        }
    })
}

/// The tools for the Assistant panel, as OpenAI-style functions: the same ones an agent over MCP has, but the
/// assistant's own request tools.
pub(crate) fn chat_tools() -> Vec<Value> {
    tools().iter().filter(|tool| !tool.name.starts_with("assistant_")).map(|tool| json!({ "type": "function", "function": { "name": tool.name, "description": tool.description, "parameters": (tool.schema)() } })).collect()
}

/// A tool's answer for the Assistant panel's model: its text, and a picture it answered with as a data URL.
pub(crate) struct ChatAnswer {
    pub text: String,
    pub image: Option<String>,
}

/// Runs a tool for the Assistant panel as `tools/call` runs one for an agent, journal line included.
pub(crate) async fn chat_call(name: &str, args: &Value) -> Result<ChatAnswer, String> {
    let tool = tools().iter().find(|tool| tool.name == name && !tool.name.starts_with("assistant_")).ok_or_else(|| format!("Unknown tool: {name}"))?;
    Ok(match run_tool(tool, args).await? {
        Answer::Json(value) => ChatAnswer { text: cut(serde_json::to_string(&value).unwrap_or_default()), image: None },
        Answer::Text(text) => ChatAnswer { text: cut(text), image: None },
        Answer::Image { data, mime, text } => ChatAnswer { text, image: Some(format!("data:{mime};base64,{data}")) },
        Answer::Window(result) => {
            let image = result.get("image").and_then(Value::as_str).map(|data| format!("data:{};base64,{data}", result.get("mime").and_then(Value::as_str).unwrap_or("image/png")));
            let text = match result.get("text").and_then(Value::as_str) {
                Some(text) => text.to_string(),
                None if image.is_some() || result.is_null() => "Done.".into(),
                None => serde_json::to_string(&result).unwrap_or_default(),
            };
            ChatAnswer { text: cut(text), image }
        }
    })
}

/// The name a tool shows the user: its words, the first capitalised.
fn tool_title(name: &str) -> String {
    let words = name.replace('_', " ");
    let mut letters = words.chars();
    letters.next().map(|first| first.to_uppercase().chain(letters).collect()).unwrap_or_default()
}

pub async fn handle(headers: HeaderMap, body: axum::body::Bytes) -> Response {
    if !crate::guard::local_origin(&headers) {
        return crate::guard::foreign_origin();
    }
    let Ok(message) = serde_json::from_slice::<Value>(&body) else {
        return rpc_failure(StatusCode::BAD_REQUEST, Value::Null, -32700, "Parse error".into(), None,
        );
    };
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return rpc_failure(StatusCode::BAD_REQUEST, Value::Null, -32600, "Invalid request: one JSON-RPC request or notification per POST".into(), None,
        );
    };
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    // a notification is only acknowledged
    if message.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    // A request of the stateless revision says its version in _meta and is
    // checked against its headers; one without it is of the handshake era.
    if let Some(version) = params.get("_meta").and_then(|meta| meta.get(META_VERSION)).and_then(Value::as_str) {
        if let Some((code, text, data)) = refused(&headers, method, &params, version) {
            return rpc_failure(StatusCode::BAD_REQUEST, id, code, text, data);
        }
    }
    if method != "tools/call" {
        seen(method);
    }
    match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            let protocol = LEGACY.iter().find(|version| **version == asked).copied().unwrap_or(LEGACY[0]);
            rpc(id, json!({
                "protocolVersion": protocol,
                "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
                "serverInfo": server_info(),
                "instructions": INSTRUCTIONS,
            }),
            )
        }
        "server/discover" => rpc(id, cacheable(json!({
            "supportedVersions": supported_versions(),
            "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
            "instructions": INSTRUCTIONS,
        })),
        ),
        "ping" => rpc(id, json!({})),
        "resources/list" => {
            let mut list = vec![
                json!({ "uri": SKILL_URI, "name": "studio skill", "title": "How to drive the studio", "description": "How to drive Speech Studio: every tool by area, the ground rules, step-by-step recipes.", "mimeType": "text/markdown" }),
                json!({ "uri": LANGUAGES_URI, "name": "languages", "title": "Languages", "description": "The language codes a project is voiced or recognised in: lang.", "mimeType": "application/json" }),
            ];
            list.extend(guides::resources());
            rpc(id, cacheable(json!({ "resources": list })))
        }
        "resources/templates/list" => rpc(id, cacheable(json!({ "resourceTemplates": [] }))),
        "resources/read" => {
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or_default();
            match resource(uri) {
                Some((mime, text)) => rpc(id, cacheable(json!({ "contents": [{ "uri": uri, "mimeType": mime, "text": text }] }),
                    ),
                ),
                None => rpc_error(id, -32002, format!("Resource not found: {uri}")),
            }
        }
        "prompts/list" => {
            let mut list = vec![json!({ "name": "studio", "title": "Studio skill", "description": "Load the studio's skill: the tools, the ground rules and the recipes." })];
            list.extend(guides::prompts());
            rpc(id, cacheable(json!({ "prompts": list })))
        }
        "prompts/get" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            let text = match name {
                "studio" => Some(SKILL.to_string()),
                other => guides::prompt(other, &arguments),
            };
            match text {
                Some(text) => rpc(id, json!({ "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] }),
                ),
                None => rpc_error(id, -32602, format!("Unknown prompt: {name}")),
            }
        }
        "tools/list" => {
            let list: Vec<Value> = tools().iter().map(|tool| json!({ "name": tool.name, "title": tool_title(tool.name), "description": tool.description, "inputSchema": (tool.schema)(), "annotations": annotations(tool.name) })).collect();
            rpc(id, cacheable(json!({ "tools": list })))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            seen(name);
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let Some(tool) = tools().iter().find(|tool| tool.name == name) else {
                return rpc_error(id, -32602, format!("Unknown tool: {name}; tools/list names them all."),
                );
            };
            let step = assistant::step_of(name);
            if let Some(step) = &step {
                assistant::chat_step(json!({ "type": "tool", "id": step, "name": name, "args": args }));
            }
            let answer = run_tool(tool, &args).await;
            if let Some(step) = step {
                let (ok, summary) = match &answer {
                    Ok(Answer::Json(value)) | Ok(Answer::Window(value)) => (true, serde_json::to_string(value).unwrap_or_default()),
                    Ok(Answer::Text(text)) | Ok(Answer::Image { text, .. }) => (true, text.clone()),
                    Err(problem) => (false, problem.clone()),
                };
                assistant::chat_step(json!({ "type": "tool_done", "id": step, "ok": ok, "summary": summary.chars().take(240).collect::<String>() }));
            }
            match answer {
                Ok(Answer::Json(value)) => tool_json(id, value),
                Ok(Answer::Text(text)) => tool_result(id, text, None, false),
                Ok(Answer::Image { data, mime, text }) => rpc(id, json!({ "content": [{ "type": "image", "data": data, "mimeType": mime }, { "type": "text", "text": text }], "isError": false })),
                Ok(Answer::Window(result)) => window::window_reply(id, result),
                Err(problem) => tool_result(id, problem, None, true),
            }
        }
        _ => rpc_failure(StatusCode::NOT_FOUND, id, -32601, format!("Method not found: {method}"), None,
        ),
    }
}

/// A moment an agent names, as unix seconds in this computer's time zone:
/// today, yesterday, a date (its start, or its end for an upper bound) or a date-time.
fn moment(text: &str, end: bool) -> Result<i64, String> {
    use chrono::{Duration as Days, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
    let text = text.trim();
    let day = match text.to_lowercase().as_str() {
        "today" => Some(Local::now().date_naive()),
        "yesterday" => Some(Local::now().date_naive() - Days::days(1)),
        _ => NaiveDate::parse_from_str(text, "%Y-%m-%d").ok(),
    };
    let at = match day {
        Some(day) => day.and_time(if end { NaiveTime::from_hms_opt(23, 59, 59).unwrap_or_default() } else { NaiveTime::MIN }),
        None => NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S")
            .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M"))
            .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M"))
            .map_err(|_| {
                format!("'{text}' is not a moment: today, yesterday, 2026-09-26 or 2026-09-26T18:00.")
            })?,
    };
    Local.from_local_datetime(&at).earliest().map(|at| at.timestamp()).ok_or_else(|| format!("'{text}' does not exist in this time zone."))
}

/// What an agent is told when it connects.
const INSTRUCTIONS: &str = "You drive Speech Studio on this computer: it voices text (Speech) and recognises speech and checks a voicing (Listening). Every tool runs the same code as a control of the studio, through the routes its window calls, and every change to projects, recordings, voices, models, keys and settings is written into its activity journal. Start with studio_status. Long work - voicing, export, recognition, downloads and link fetches - is a job: start it, then studio_wait with its job_id instead of polling. Look ids up instead of guessing them: voices_list, models_status, projects_list, url_fetches_list. Files on this computer are passed by path, and a sound a tool answers comes back as a file path. Before writing text for an engine read studio_guide: tags (each engine reads its own), podcast, audiobook. To work in front of the user, use the ui_* tools (ui_navigate opens speech or listening), editor_* for the open project and player_* to play it to the user; they need the window open, the rest works without it. studio_batch runs several tools in one call. The whole guide is the resource studio://skill (prompt 'studio'); prompts write_podcast and make_audiobook start those tasks.";

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------- every route is a tool (as in Dub Studio)

    /// The routes of build_router: (METHOD, path pattern).
    fn router_routes() -> Vec<(String, String)> {
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let body = source.split("pub fn build_router(").nth(1).expect("build_router").split("\n}\n").next().unwrap();
        let code: String = body.lines().map(|line| line.split("//").next().unwrap_or_default()).collect::<Vec<_>>().join("\n");
        let mut routes = Vec::new();
        for chunk in code.split(".route(").skip(1) {
            let path = chunk.trim_start().strip_prefix('"').and_then(|rest| rest.split('"').next()).expect("a route's path").to_string();
            let handlers = chunk.split(".fallback(").next().unwrap().split(".layer(").next().unwrap().split(".with_state(").next().unwrap();
            for method in ["get", "post", "patch", "put", "delete"] {
                let call = format!("{method}(");
                let called = handlers.match_indices(&call).any(|(at, _)| !handlers[..at].chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_'));
                if called {
                    routes.push((method.to_uppercase(), path.clone()));
                }
            }
        }
        routes
    }

    fn route_of(pattern: &str, path: &str) -> bool {
        let path = path.split('?').next().unwrap_or_default();
        let (wanted, given): (Vec<&str>, Vec<&str>) = (pattern.split('/').collect(), path.split('/').collect());
        wanted.len() == given.len() && wanted.iter().zip(&given).all(|(part, value)| part.starts_with('{') && !value.is_empty() || part == value)
    }

    /// Routes an agent is not given, and why.
    const NOT_TOOLS: &[(&str, &str, &str)] = &[];

    /// Arguments for every property of a tool, and one set more for each other choice of an enum.
    fn samples(tool: &Tool, file: &std::path::Path) -> Vec<Value> {
        let schema = (tool.schema)();
        let properties = schema["properties"].as_object().cloned().unwrap_or_default();
        let sample = |name: &str, property: &Value| -> Value {
            if let Some(first) = property["enum"].as_array().and_then(|choices| choices.first()) {
                return first.clone();
            }
            let kind = match &property["type"] {
                Value::Array(kinds) => kinds[0].as_str().unwrap_or("string").to_string(),
                other => other.as_str().unwrap_or("string").to_string(),
            };
            match (kind.as_str(), name) {
                ("integer", _) => json!(1),
                ("number", _) => json!(1.5),
                ("boolean", _) => json!(true),
                ("array", _) if property["items"]["type"] == "integer" => json!([1]),
                ("array", n) if n.ends_with("paths") => json!([file.to_string_lossy()]),
                ("array", _) => json!(["x1"]),
                ("object", _) => json!({}),
                (_, n) if n.ends_with("path") || n == "file" => json!(file.to_string_lossy()),
                _ => json!("x1"),
            }
        };
        let base: serde_json::Map<String, Value> = properties.iter().map(|(name, property)| (name.clone(), sample(name, property))).collect();
        let mut all = vec![Value::Object(base.clone())];
        for (name, property) in &properties {
            for choice in property["enum"].as_array().into_iter().flatten().skip(1) {
                let mut other = base.clone();
                other.insert(name.clone(), choice.clone());
                all.push(Value::Object(other));
            }
        }
        all
    }

    /// The routes a composite tool serves by running the studio's code itself.
    const COMPOSITE_ROUTES: &[(&str, &[(&str, &str)])] = &[
        ("composite:atomic:separate_file", &[("POST", "/separate"), ("GET", "/jobs/x1")]),
        ("composite:agent", &[("GET", "/mcp/status")]),
        ("composite:questions", &[("POST", "/mcp/agent-llm/v1/chat/completions")]),
        ("composite:windows", &[("GET", "/mcp/window")]),
        ("composite:window_focus", &[("POST", "/mcp/window/focus")]),
        ("composite:window_drop", &[("POST", "/mcp/window/result")]),
        ("composite:batch", &[("POST", "/mcp")]),
    ];

    /// The routes a tool reaches: (METHOD, path).
    fn reached_by(tool: &Tool, file: &std::path::Path) -> Vec<(String, String)> {
        let mut reached = Vec::new();
        for args in samples(tool, file) {
            let Ok(call) = (tool.call)(&args) else { continue };
            if matches!(call.payload, Payload::Window { .. }) {
                continue;
            }
            if call.path.starts_with("composite:") {
                let served = COMPOSITE_ROUTES.iter().find(|(kind, _)| *kind == call.path).map(|(_, routes)| *routes).unwrap_or_default();
                reached.extend(served.iter().map(|(method, path)| (method.to_string(), path.to_string())));
                continue;
            }
            reached.push((call.method.to_string(), call.path.clone()));
        }
        reached
    }

    #[test]
    fn every_route_is_a_tool() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("clip.wav");
        std::fs::write(&file, b"x").unwrap();
        let routes = router_routes();
        assert!(routes.len() > 100, "build_router read: {routes:?}");
        let mut covered = vec![false; routes.len()];
        for tool in tools() {
            for (method, path) in reached_by(tool, &file) {
                let found: Vec<usize> = routes.iter().enumerate().filter(|(_, (m, p))| *m == method && route_of(p, &path)).map(|(at, _)| at).collect();
                assert!(!found.is_empty(), "{} calls {method} {path}, a route the studio does not have", tool.name);
                for at in found {
                    covered[at] = true;
                }
            }
        }
        let by_tool = covered.clone();
        let mut stale = Vec::new();
        for (method, path) in NOT_TOOLS.iter().map(|(method, path, _)| (method, path)) {
            let at = routes.iter().position(|(m, p)| m == method && p == path).unwrap_or_else(|| panic!("{method} {path} of NOT_TOOLS is not a route of build_router"));
            if by_tool[at] {
                stale.push(format!("{method} {path}"));
            }
            covered[at] = true;
        }
        assert!(stale.is_empty(), "these routes of NOT_TOOLS are reached by a tool: drop them from NOT_TOOLS: {stale:?}");
        let missing: Vec<String> = routes.iter().zip(&covered).filter(|(_, done)| !**done).map(|((method, path), _)| format!("{method} {path}")).collect();
        assert!(missing.is_empty(), "these routes have no tool: give each one, or put it into NOT_TOOLS with why: {missing:#?}");
    }

    #[test]
    fn every_journaled_tool_exists_and_changes_something() {
        let names: Vec<&str> = tools().iter().map(|tool| tool.name).collect();
        for (tool, verb, kind) in JOURNAL {
            assert!(names.contains(tool), "{tool} is journaled but is not a tool");
            assert_eq!(annotations(tool)["readOnlyHint"], false, "{tool} is journaled but reads only");
            assert!(!verb.is_empty() && !kind.is_empty());
        }
    }

    #[test]
    fn the_window_has_words_for_every_journaled_change() {
        let en: Value = serde_json::from_str(include_str!("../../../frontend/src/locales/en.json")).unwrap();
        for (tool, verb, kind) in JOURNAL {
            assert!(en["journal"]["verb"][verb].is_string(), "{tool}: no words for the verb {verb}");
            assert!(en["journal"]["kind"][kind].is_string(), "{tool}: no words for the kind {kind}");
        }
    }

    #[test]
    fn every_tool_is_in_the_skill() {
        let missing: Vec<&str> = tools().iter().map(|tool| tool.name).filter(|name| !SKILL.contains(&format!("`{name}`"))).collect();
        assert!(missing.is_empty(), "docs/mcp-skill.md does not name {missing:?}");
    }

    #[test]
    fn every_guide_reads_and_every_prompt_takes_its_arguments() {
        for (topic, _) in guides::TOPICS {
            let (mime, text) = resource(&format!("{}{topic}", guides::GUIDE_URI)).unwrap_or_else(|| panic!("no guide {topic}"));
            assert_eq!(mime, "text/plain");
            assert!(text.len() > 300, "{topic}: {text}");
        }
        let tags = guides::guide("tags").unwrap();
        assert!(tags.starts_with("- **Expression tags differ per engine**"), "the skill's tags part leads the guide");
        for engine in crate::engines::all() {
            assert!(tags.contains(&format!("({})", engine.id)), "{} is missing from the tags guide", engine.id);
        }
        assert!(guides::guide("podcast").unwrap().contains("podcast scriptwriter"));
        assert!(guides::guide("audiobook").unwrap().contains("VERBATIM"));
        for prompt in guides::prompts() {
            let name = prompt["name"].as_str().unwrap();
            let text = guides::prompt(name, &json!({ "topic": "кофе по утрам", "path": "C:/books/war.epub" })).unwrap_or_else(|| panic!("{name}"));
            assert!(text.contains("кофе по утрам") || text.contains("C:/books/war.epub"), "{name} drops its argument");
        }
        assert!(resource("studio://guide/nothing").is_none());
    }

    #[test]
    fn a_journal_line_names_what_the_agent_named() {
        assert_eq!(journal_target(&json!({ "pid": "abc", "name": " Глава 1 " })), "Глава 1");
        assert_eq!(journal_target(&json!({ "pid": "abc" })), "abc");
        assert_eq!(journal_target(&json!({})), "");
    }
}
