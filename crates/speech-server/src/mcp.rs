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

mod atomic;
mod speech_tools;
mod window;

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
const SKILL: &str = include_str!("../../../docs/mcp-skill.md");
const SKILL_URI: &str = "studio://skill";
const LANGUAGES_URI: &str = "studio://languages";
const EDITS_URI: &str = "studio://patch-ops";
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

/// A line as an agent reads it: without its word timings.
fn compact_segment(segment: &Value) -> Value {
    let mut row = json!({
        "id": segment["id"], "start": segment["start"], "end": segment["end"], "speaker": segment["speaker"],
        "src_text": segment["src_text"], "tgt_text": segment["tgt_text"], "dirty": segment["dirty"],
    });
    for flag in ["hidden", "keep_original"] {
        if segment[flag] == true {
            row[flag] = true.into();
        }
    }
    for said in ["tts_skip", "tts_text"] {
        if segment[said].is_string() {
            row[said] = segment[said].clone();
        }
    }
    if !segment["voice"].is_null() {
        row["voice"] = segment["voice"].clone();
    }
    for computed in ["fit", "takes", "shortened"] {
        if !segment[computed].is_null() {
            row[computed] = segment[computed].clone();
        }
    }
    row
}

/// Titles and blur boxes with the idx their tools take.
fn indexed(items: &Value) -> Value {
    Value::Array(
        items
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(idx, item)| {
                let mut item = item.clone();
                item["idx"] = idx.into();
                item
            })
            .collect(),
    )
}

fn count_dirty(project: &Value) -> usize {
    project["segments"].as_array().into_iter().flatten().filter(|segment| segment["dirty"] == true).count()
}

/// A project without what only the studio reads (word timings, the vision
/// context, stage checksums), its lines narrowed by time or id when asked.
fn compact_project(project: &Value, args: &Value) -> Value {
    let from = args.get("from").and_then(Value::as_f64);
    let to = args.get("to").and_then(Value::as_f64);
    let ids: Vec<&str> = args.get("ids").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let lines: Vec<Value> = project["segments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|segment| {
            let start = segment["start"].as_f64().unwrap_or_default();
            let end = segment["end"].as_f64().unwrap_or_default();
            from.is_none_or(|from| end >= from) && to.is_none_or(|to| start <= to) && (ids.is_empty() || ids.contains(&segment["id"].as_str().unwrap_or_default()))
        })
        .map(compact_segment)
        .collect();
    let captions = &project["captions"];
    json!({
        "meta": { "video": project["meta"]["video"], "duration": project["meta"]["duration"], "width": project["meta"]["width"], "height": project["meta"]["height"], "fps": project["meta"]["fps"] },
        "mode": project["mode"], "tgt_lang": project["tgt_lang"], "subs": project["subs"], "audio": project["audio"],
        "blur_on": project["render"]["blur"], "casting_enabled": project["casting_enabled"],
        "captions": {
            "sub_style": captions["sub_style"], "sub_y": captions["sub_y"], "preset": captions["preset"], "overrides": captions["overrides"],
            "titles": indexed(&captions["titles"]), "blur_boxes": indexed(&captions["blur_boxes"]),
        },
        "segments_total": project["segments"].as_array().map_or(0, Vec::len),
        "dirty": count_dirty(project),
        "segments": lines,
    })
}

/// A project's words as a transcript: each line's id, time, speaker and text - the recognised
/// original (text src, the default) or the translation (tgt) - narrowed by from and to seconds,
/// as JSON lines or, with format text, one "[0:14.2 SPK 1] words" line each.
fn transcript(project: &Value, args: &Value) -> Value {
    let from = args.get("from").and_then(Value::as_f64);
    let to = args.get("to").and_then(Value::as_f64);
    let translation = args.get("text").and_then(Value::as_str) == Some("tgt");
    let clock = |seconds: f64| {
        let tenths = (seconds.max(0.0) * 10.0).round() as u64;
        format!("{}:{:02}.{}", tenths / 600, tenths % 600 / 10, tenths % 10)
    };
    // the translation as the subtitles burn it: a line's own subtitle text over its translation,
    // and no line that keeps the original speech
    let own: std::collections::HashMap<&str, &str> = project["captions"]["overrides"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| Some((o["seg_id"].as_str()?, o["text"].as_str()?)))
        .collect();
    let lines: Vec<(&Value, &str)> = project["segments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|segment| {
            segment["hidden"] != true && !(translation && segment["keep_original"] == true)
        })
        .map(|segment| {
            let text = match translation {
                true => segment["id"].as_str().and_then(|id| own.get(id).copied()).unwrap_or_else(|| segment["tgt_text"].as_str().unwrap_or_default()),
                false => segment["src_text"].as_str().unwrap_or_default(),
            };
            (segment, text.trim())
        })
        .filter(|(_, text)| !text.is_empty())
        .filter(|(segment, _)| {
            let start = segment["start"].as_f64().unwrap_or_default();
            let end = segment["end"].as_f64().unwrap_or_default();
            from.is_none_or(|from| end >= from) && to.is_none_or(|to| start <= to)
        })
        .collect();
    let speakers: std::collections::BTreeSet<&str> = lines.iter().filter_map(|(segment, _)| segment["speaker"].as_str()).collect();
    if args.get("format").and_then(Value::as_str) == Some("text") {
        let text: Vec<String> = lines
            .iter()
            .map(|(segment, text)| {
                format!("[{} SPK {}] {text}", clock(segment["start"].as_f64().unwrap_or_default()), segment["speaker"].as_str().unwrap_or("-"))
            })
            .collect();
        return json!({ "text": text.join("\n"), "lines": lines.len(), "speakers": speakers });
    }
    let rows: Vec<Value> = lines.iter().map(|(segment, text)| json!({ "id": segment["id"], "start": segment["start"], "end": segment["end"], "speaker": segment["speaker"], "text": text })).collect();
    json!({ "language": if translation { project["tgt_lang"].as_str().unwrap_or_default() } else { "original" }, "speakers": speakers, "lines": rows })
}

/// What an edit changed: the project's state in brief and the part the edit
/// touched, instead of the whole project every edit answers with.
fn compact_change(name: &str, args: &Value, project: &Value) -> Value {
    // an edit made by its op answers as the op's own tool does
    let name = match name {
        "project_patch" => args.get("op").and_then(Value::as_str).and_then(tool_of_op).unwrap_or(name),
        _ => name,
    };
    let mut summary = json!({
        "saved": true, "mode": project["mode"], "tgt_lang": project["tgt_lang"],
        "segments": project["segments"].as_array().map_or(0, Vec::len), "dirty": count_dirty(project),
    });
    let mut named: Vec<String> = args.get("ids").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
    if let Some(id) = args.get("id").and_then(Value::as_str) {
        named.push(id.to_string());
    }
    let segments = project["segments"].as_array().cloned().unwrap_or_default();
    if name == "segments_reorder" {
        named.clear();
    }
    if name == "segment_split" {
        let after = args.get("id").and_then(Value::as_str).and_then(|id| segments.iter().position(|segment| segment["id"] == id)).and_then(|at| segments.get(at + 1));
        named.extend(after.and_then(|segment| segment["id"].as_str()).map(str::to_string),
        );
    }
    let mut changed: Vec<Value> = segments.iter().filter(|segment| named.iter().any(|id| segment["id"] == id.as_str())).map(compact_segment).collect();
    if name == "segment_add" && args.get("id").is_none() {
        let start = args.get("start").and_then(Value::as_f64).unwrap_or_default().max(0.0);
        changed.extend(segments.iter().filter(|segment| {
                    segment["start"].as_f64().is_some_and(|at| (at - start).abs() < 1e-6) && segment["src_text"] == ""
                }).map(compact_segment),
        );
    }
    if !changed.is_empty() {
        summary["changed"] = Value::Array(changed);
    }
    let captions = &project["captions"];
    if name.starts_with("title") {
        summary["titles"] = indexed(&captions["titles"]);
    }
    if name.starts_with("blur") {
        summary["blur_boxes"] = indexed(&captions["blur_boxes"]);
        summary["blur_on"] = project["render"]["blur"].clone();
    }
    if name.starts_with("caption") {
        summary["sub_style"] = captions["sub_style"].clone();
        summary["preset"] = captions["preset"].clone();
        if let Some(line) = args.get("seg_id").and_then(Value::as_str) {
            summary["override"] = captions["overrides"].as_array().into_iter().flatten().find(|entry| entry["seg_id"] == line).cloned().unwrap_or(Value::Null);
        }
    }
    if name.starts_with("subtitles_") || matches!(name, "project_mode_set" | "audio_output_set" | "translation_target_set") {
        summary["subs"] = project["subs"].clone();
        summary["sub_y"] = captions["sub_y"].clone();
    }
    if matches!(name, "voice_set" | "gain_set" | "loudness_set" | "voiceover_gain_set" | "original_track_set" | "rewrite_set" | "translation_style_set" | "translation_target_set" | "project_mode_set" | "audio_output_set") {
        summary["audio"] = project["audio"].clone();
    }
    summary
}

/// Whether an answer is a whole project.
fn is_project(value: &Value) -> bool {
    value.get("segments").is_some_and(Value::is_array) && value.get("captions").is_some()
}

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
        "project_get" => compact_project(&value, args),
        "project_transcript" => transcript(&value, args),
        _ if is_project(&value) => compact_change(name, args, &value),
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
                (query.is_empty() || project["video"].as_str().unwrap_or_default().to_lowercase().contains(&query))
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
    const READ_NAMES: &[&str] = &["studio_wait", "proxy_test", "openrouter_verify", "ui_screenshot", "ui_read_page", "ui_console", "url_probe"];
    // writes over what was stored, so the earlier content is gone: a client asks first
    const OVERWRITES: &[&str] = &["openrouter_set_key", "voice_transcript_set"];
    let changes = CHANGES.iter().any(|verb| name.split('_').any(|word| word == *verb));
    let read_only = READ_NAMES.contains(&name) || !changes && READS.iter().any(|part| name.ends_with(part) || name.contains(&format!("{part}_")));
    let destructive = name.ends_with("_delete") || name.ends_with("_cancel") || name.contains("_cancel_") || name.ends_with("_delete_key") || OVERWRITES.contains(&name);
    // what reaches the internet: OpenRouter, Hugging Face, the sites of links and every download
    let open_world = name.starts_with("google_")
        && matches!(name, "google_set_key" | "google_models")
        || name.starts_with("openrouter_") && !matches!(name, "openrouter_status" | "openrouter_delete_key")
        || matches!(
            name,
            "models_download" | "voice_download" | "voices_download_pack" | "voices_catalog" | "proxy_test" | "url_probe" | "project_create_from_url" | "url_fetch_resume" | "url_tool_update"
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
    ("voice_restore", "restore", "voice"),
    ("voice_ref_apply", "update", "voice"),
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

/// An edit of a project: PATCH /projects/{pid} with the op and its fields.
fn edit(args: &Value, op: &str) -> Result<Call, String> {
    let path = project_path(args, "")?;
    let mut body = body_without(args, &["pid", "response_format"]);
    body["op"] = op.into();
    send(Method::PATCH, path, body)
}

/// The detailed answer an edit can give instead of its summary.
fn detail() -> Value {
    json!({ "type": "string", "enum": ["concise", "detailed"], "description": "detailed: the whole project instead of what changed" })
}

/// Records of a glossary, as glossary_get answers them.
fn glossary_entries() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "term": { "type": "string" }, "translation": { "type": "string" }, "keep": { "type": "boolean" },
                "pronunciation": { "type": "string" }, "asr_fix": { "type": "array", "items": { "type": "string" } },
                "note": { "type": "string" }, "source": { "type": "string", "enum": ["manual", "auto", "series"] }, "lang": { "type": "string" },
            },
            "required": ["term"],
        },
    })
}

fn ids(what: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "description": what })
}

fn idxs(what: &str) -> Value {
    json!({ "type": "array", "items": { "type": "integer" }, "description": what })
}

/// Each edit of a project and the tool that makes it; the resource
/// studio://patch-ops lists them with their fields.
const PATCH_OPS: &[(&str, &str)] = &[
    ("segment", "segment_update"),
    ("add_segment", "segment_add"),
    ("del_segments", "segments_delete"),
    ("hide_segments", "segments_hide"),
    ("keep_segments", "segments_keep_original"),
    ("reorder_segments", "segments_reorder"),
    ("split_segment", "segment_split"),
    ("merge_segments", "segments_merge"),
    ("regen", "segment_regen"),
    ("regen_all", "segments_regen_all"),
    ("take_select", "take_select"),
    ("take_pin", "take_pin"),
    ("mode", "project_mode_set"),
    ("dub", "audio_output_set"),
    ("subs_content", "subtitles_content_set"),
    ("subs_burn", "subtitles_burn_set"),
    ("subpos", "subtitles_position_set"),
    ("translate", "translation_target_set"),
    ("translate_style", "translation_style_set"),
    ("rewrite", "rewrite_set"),
    ("recast", "voice_set"),
    ("gain", "gain_set"),
    ("loudness", "loudness_set"),
    ("voiceover_gain", "voiceover_gain_set"),
    ("keep_original", "original_track_set"),
    ("caption", "caption_style_set"),
    ("preset", "caption_preset_set"),
    ("title_add", "title_add"),
    ("title", "title_update"),
    ("del_titles", "titles_delete"),
    ("blur_add", "blur_add"),
    ("blur", "blur_update"),
    ("del_blurs", "blurs_delete"),
    ("blur_enable", "blur_enable"),
];

/// Ops for one thing that a tool of many already makes, and the op it makes:
/// sub_blur sets the same switch as blur_enable.
const PATCH_ALIASES: &[(&str, &str)] = &[
    ("del_segment", "del_segments"),
    ("hide_segment", "hide_segments"),
    ("keep_segment", "keep_segments"),
    ("title_del", "del_titles"),
    ("blur_del", "del_blurs"),
    ("sub_blur", "blur_enable"),
];

/// The tool that makes an op, or makes what an alias does.
fn tool_of_op(op: &str) -> Option<&'static str> {
    let op = PATCH_ALIASES.iter().find(|(alias, _)| *alias == op).map_or(op, |(_, same)| *same);
    PATCH_OPS.iter().find(|(known, _)| *known == op).map(|(_, tool)| *tool)
}

fn every_op() -> Vec<&'static str> {
    PATCH_OPS.iter().map(|(op, _)| *op).chain(PATCH_ALIASES.iter().map(|(op, _)| *op)).collect()
}

/// Every edit of a project: its op, its tool and the tool's fields.
fn edits() -> Value {
    let mut ops: Vec<Value> = PATCH_OPS
        .iter()
        .map(|(op, tool)| {
            let mut fields = tools().iter().find(|entry| entry.name == *tool).map(|entry| (entry.schema)()["properties"].clone()).unwrap_or(Value::Null);
            if let Some(fields) = fields.as_object_mut() {
                fields.remove("pid");
                fields.remove("response_format");
            }
            json!({ "op": op, "tool": tool, "fields": fields })
        })
        .collect();
    ops.extend(PATCH_ALIASES.iter().map(|(op, same)| json!({ "op": op, "same_as": same })),
    );
    json!({ "route": "PATCH /projects/{pid}", "body": "{ op, ...fields }", "answer": "the whole project, saved", "ops": ops })
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
        EDITS_URI => Some(("application/json", serde_json::to_string_pretty(&edits()).unwrap_or_default(),
        )),
        _ => None,
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
                description: "Wait for work to finish instead of polling: a job (job_id, as project_synth, project_export, project_cast_characters, podcast_write, listen_transcribe, listen_cut, voice_transcribe, voice_clean, voices_download_pack or a one-call tool answering done false returned it, or the fetch.id of a download by link), or until one kind of work is over - synth, export, analyze (recognition), remix (cut by text), write (podcast script), cast (characters), voice_transcript, voice_clean, separate, download, voices_pack - or everything (until: idle, the default). Returns when it is done or after seconds (30 by default, at most 55, under the minute clients allow a call) with how far it got; call it again to keep waiting.",
                schema: || object(json!({ "job_id": { "type": "string" }, "until": { "type": "string", "enum": ["idle", "synth", "export", "analyze", "remix", "write", "cast", "voice_transcript", "voice_clean", "separate", "download", "voices_pack"] }, "seconds": { "type": "integer" } }), &[]),
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
                description: "Change one setting to a text value. TTS: tts_provider=local|openrouter|google, google_tts_model=<id without models/>, google_tts_mode=standard|batch. Voice/autocast use or_tts_voice/or_tts_autocast, parallel requests use or_concurrency (1..16). Applies from the next job. Secrets: openrouter_set_key, google_set_key; proxy: proxy_settings_set.",
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
                description: "The voices of the studio's library: each with its card (gender, age, lang, tags, source, consent, favorite, kind, cleaned, transcript_auto), the transcript of its reference and whether it has a photo and a kept original. broken lists a voice whose card file is damaged, with the reason.",
                schema: nothing,
                call: |_| get("/voices".into()),
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
                description: "Change a voice's card: gender (female, male or empty), age (child, teen, adult, senior or empty), lang (a code such as ru), tags (the whole list), source (where the voice comes from), consent (own, permitted, public or empty), favorite, kind (clone, preset, design). Only the fields given change; an unknown field or value is refused.",
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
                name: "voice_restore",
                description: "Bring back a voice's sound as it was before voice_clean.",
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

/// Builds the route's request, calls it inside the process and returns what it
/// answered. A page instead of an answer means the route is not there.
async fn call_route_raw(call: Call) -> Result<Reply, String> {
    let api = studio_api()?;
    let asked = format!("{} {}", call.method, call.path);
    let builder = Request::builder().method(call.method).uri(&call.path).header(header::HOST, "127.0.0.1").header(window::AGENT_HEADER, "1");
    let request = match call.payload {
        Payload::None => builder.body(Body::empty()),
        Payload::Window { command, .. } => return Err(format!("{command} is a command of the studio's window, not a route")),
        Payload::Json(body) => builder.header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())),
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
    let status = response.status();
    let mime = response.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024).await.map_err(|error| error.to_string())?;
    if mime.starts_with("text/html") {
        return Err(format!("The studio has no route {asked}: its page answered instead."));
    }
    Ok(Reply { status, mime, bytes,
    })
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

/// A picture a route answered with, as an image the agent sees.
fn tool_image(id: Value, reply: &Reply, name: &str, args: &Value) -> Response {
    use base64::Engine;
    let mime = reply.mime.split(';').next().unwrap_or_default().trim().to_string();
    let what = match name {
        "project_frame" => format!(
            "The {} frame of project {} at {} s.",
            if args.get("source").and_then(Value::as_str) == Some("original") { "original" } else { "dubbed" },
            args.get("pid").and_then(Value::as_str).unwrap_or_default(),
            args.get("t").and_then(Value::as_f64).unwrap_or_default()
        ),
        _ => format!("The avatar of character {}.", args.get("character_id").and_then(Value::as_str).unwrap_or_default()),
    };
    let image = base64::engine::general_purpose::STANDARD.encode(&reply.bytes);
    rpc(id, json!({ "content": [{ "type": "image", "data": image, "mimeType": mime }, { "type": "text", "text": what }], "isError": false }),
    )
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
        "resources/list" => rpc(id, cacheable(json!({ "resources": [
            { "uri": SKILL_URI, "name": "studio skill", "title": "How to drive the studio", "description": "How to drive Speech Studio: every tool by area, the ground rules, step-by-step recipes.", "mimeType": "text/markdown" },
            { "uri": LANGUAGES_URI, "name": "languages", "title": "Languages", "description": "The language codes a video is dubbed from and into: tgt_lang, src_lang, lang.", "mimeType": "application/json" },
            { "uri": EDITS_URI, "name": "project edits", "title": "Project edits", "description": "Every edit of a project (PATCH op), the tool that makes it and its fields.", "mimeType": "application/json" },
        ] })),
        ),
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
        "prompts/list" => rpc(id, cacheable(json!({ "prompts": [
            { "name": "studio", "title": "Studio skill", "description": "Load the studio's skill: the tools, the ground rules and the recipes." },
        ] })),
        ),
        "prompts/get" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let text = match name {
                "studio" => Some(SKILL.to_string()),
                _ => None,
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
            let answer = |text: String, error: bool| tool_result(id.clone(), text, None, error);
            let Some(tool) = tools().iter().find(|tool| tool.name == name) else {
                return rpc_error(id, -32602, format!("Unknown tool: {name}; tools/list names them all."),
                );
            };
            let target = if JOURNAL.iter().any(|(tool, _, _)| *tool == name) { journal_target(&args) } else { String::new() };
            // a tool may look at a file on disk before its call: off the runtime
            let prepare = tool.call;
            let prepared = {
                let args = args.clone();
                tokio::task::spawn_blocking(move || prepare(&args)).await.unwrap_or_else(|error| Err(format!("{name} failed: {error}")))
            };
            match prepared {
                Err(problem) => answer(problem, true),
                Ok(Call { payload: Payload::Window { command, args, seconds,
                        }, .. }) => match window::ask_window(command, args, seconds).await {
                    Ok(result) => window::window_reply(id, result),
                    Err(problem) => answer(problem, true),
                },
                Ok(call) if call.path == "composite:status" => {
                    tool_json(id, status_summary().await)
                }
                Ok(call) if call.path == "composite:wait" => match wait_for(&args).await {
                    Ok(state) => tool_json(id, state),
                    Err(problem) => answer(problem, true),
                },
                Ok(call) if call.path.starts_with(atomic::PREFIX) => match atomic::run(&call.path[atomic::PREFIX.len()..], &args).await {
                    Ok(state) => {
                        journal_call(name, &target, Some(&state));
                        tool_json(id, state)
                    }
                    Err(problem) => answer(problem, true),
                },
                Ok(call) => {
                    if QUEUED_BEHIND_JOBS.contains(&name) {
                        if let Err(problem) = graphics_card_free().await {
                            return answer(problem, true);
                        }
                    }
                    match call_route_raw(call).await {
                        Ok(reply) if reply.status.is_success() && reply.mime.starts_with("image/") =>
                        {
                            tool_image(id, &reply, name, &args)
                        }
                        Ok(reply) if reply.status.is_success() => {
                            let text = reply_text(&reply);
                            match serde_json::from_str::<Value>(&text) {
                                Ok(value) => {
                                    journal_call(name, &target, Some(&value));
                                    tool_json(id, shape(name, &args, redact(value)))
                                }
                                Err(_) => {
                                    journal_call(name, &target, None);
                                    answer(text, false)
                                }
                            }
                        }
                        Ok(reply) => answer(reply_text(&reply), true),
                        Err(problem) => answer(problem, true),
                    }
                }
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
const INSTRUCTIONS: &str = "You drive Speech Studio on this computer: it voices text (Speech) and recognises speech and checks a voicing (Listening). Every tool runs the same code as a control of the studio, through the routes its window calls. Start with studio_status. Long work - downloads and link fetches - is a job: start it, then studio_wait with its job_id instead of polling. Look ids up instead of guessing them: voices_list, models_status, url_fetches_list. Files on this computer are passed by path. To work in front of the user, use the ui_* tools (ui_navigate opens speech or listening); they need the window open, the rest works without it. The whole guide is the resource studio://skill (prompt 'studio').";

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_journal_line_names_what_the_agent_named() {
        assert_eq!(journal_target(&json!({ "pid": "abc", "name": " Глава 1 " })), "Глава 1");
        assert_eq!(journal_target(&json!({ "pid": "abc" })), "abc");
        assert_eq!(journal_target(&json!({})), "");
    }
}
