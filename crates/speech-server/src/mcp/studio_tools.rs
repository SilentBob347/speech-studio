//! The rest of the studio as tools: the service, the voices' sound, the cloud models of fal and Runware, the
//! OpenAI-compatible API, recording and the queue, books and subtitles, the export files, the Assistant, the
//! diagnostics, the journal, the streams the window follows and the agent connection itself.

use std::path::PathBuf;

use axum::http::Method;
use serde_json::{json, Value};

use super::{body_without, composite, file_name, get, given, id_only, nothing, object, pid, post, project_only, project_path, query, segment, send, text, Call, Payload, Tool};

/// A file on this computer an agent names, checked before it is sent.
fn local_file(args: &Value, name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(text(args, name)?);
    if !path.is_file() {
        return Err(format!("{} is not a file on this computer.", path.display()));
    }
    Ok(path)
}

/// A file sent to a route as the page uploads it, with the text fields given.
fn upload(route: &str, path: PathBuf, field: &str, fields: Vec<(String, String)>) -> Result<Call, String> {
    let name = file_name(&path);
    Ok(Call { method: Method::POST, path: route.into(), payload: Payload::Form { fields, files: vec![(field.to_string(), path, name)] } })
}

/// A stream the window follows, read for `seconds` (at most 55) or until it ends.
fn stream(method: Method, path: String, body: Option<Value>, args: &Value) -> Result<Call, String> {
    let seconds = args.get("seconds").and_then(Value::as_u64).unwrap_or(10).clamp(1, 55);
    Ok(Call { method, path, payload: Payload::Stream { body, seconds } })
}

fn seconds() -> Value {
    json!({ "type": "integer", "minimum": 1, "maximum": 55, "description": "how long to listen, 10 s by default" })
}

fn provider() -> Value {
    json!({ "type": "string", "enum": ["fal", "runware"] })
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        // ---------------------------------------------------------------- the service
        Tool {
            name: "studio_health",
            description: "The service's identity: status, app id, version, the executable that runs, its folder (repo_root) and port - what the desktop shell checks before starting a second copy.",
            schema: nothing,
            call: |_| get("/health".into()),
        },
        Tool {
            name: "folder_pick",
            description: "Ask the user to choose a folder in a dialog on their screen; answers its path (dir), null when they cancelled. Use it when the user, not you, should decide where something goes.",
            schema: nothing,
            call: |_| post("/pick-folder".into(), json!({})),
        },
        Tool {
            name: "models_import_pick",
            description: "Ask the user to choose a folder with model files in a dialog on their screen, then take every component found there as models_import does (id limits it to one component). Answers what was imported, or nothing when they cancelled.",
            schema: || object(json!({ "id": { "type": "string", "description": "one component id from models_status" } }), &[]),
            call: |args| post("/setup/browse".into(), match given(args, "id") { Some(id) => json!({ "id": id }), None => json!({}) }),
        },
        Tool {
            name: "studio_language_set",
            description: "The language the studio speaks in its messages and errors (the window sets it to the one it is shown in): en, ru, es, fr, pt or zh.",
            schema: || object(json!({ "lang": { "type": "string", "enum": ["en", "ru", "es", "fr", "pt", "zh"] } }), &["lang"]),
            call: |args| send(Method::PUT, "/settings/ui-lang".into(), json!({ "lang": text(args, "lang")? })),
        },
        Tool {
            name: "engine_tags",
            description: "The expression tags an engine reads (engine id from engines_list; higgs by default): their categories with each tag, the format a tag is written in, and whether the engine takes a block's direction. Tags differ per engine: write only the ones listed here.",
            schema: || object(json!({ "engine": { "type": "string" } }), &[]),
            call: |args| get(format!("/engine/tags{}", query(&[("engine", given(args, "engine"))]))),
        },
        // ---------------------------------------------------------------- the voices' sound
        Tool {
            name: "voice_sample",
            description: "A voice's reference sound as a file on this computer (path, type, size): listen to it or send it on. original true gives it as it was before voice_clean or voice_denoise.",
            schema: || object(json!({ "name": { "type": "string", "description": "voice name from voices_list" }, "original": { "type": "boolean" } }), &["name"]),
            call: |args| {
                let original = args.get("original").and_then(Value::as_bool).unwrap_or(false).then(|| "1".to_string());
                get(format!("/voices/sample{}", query(&[("name", Some(text(args, "name")?)), ("original", original)])))
            },
        },
        Tool {
            name: "voice_photo_get",
            description: "A voice's photo, as a picture you see.",
            schema: || id_only("name", "voice name from voices_list"),
            call: |args| get(format!("/voices/photo{}", query(&[("name", Some(text(args, "name")?))]))),
        },
        Tool {
            name: "voice_design_sample",
            description: "One candidate of voice_design as a sound file on this computer: id is the batch id its job answered, k the candidate's number.",
            schema: || object(json!({ "id": { "type": "string" }, "k": { "type": "integer" } }), &["id", "k"]),
            call: |args| get(format!("/voices/design/sample{}", query(&[("id", Some(text(args, "id")?)), ("k", given(args, "k"))]))),
        },
        // ---------------------------------------------------------------- videos and sound by link
        Tool {
            name: "url_probe_cookies_text",
            description: "url_probe with the cookies.txt content given as text (what the window sends when the user drops the file) instead of its path.",
            schema: || object(json!({ "url": { "type": "string" }, "cookies_text": { "type": "string", "description": "the content of a Netscape cookies.txt" } }), &["url", "cookies_text"]),
            call: |args| post("/url/probe".into(), json!({ "url": text(args, "url")?, "cookies_text": text(args, "cookies_text")? })),
        },
        Tool {
            name: "project_create_from_url",
            description: "Download a video or sound by link (YouTube and the other sites yt-dlp knows) into a new Listening recording: quality from url_probe (best by default), subs_lang to take the site's subtitles in that language, cookies the path of a cookies.txt for videos that need a signed-in browser. It runs in the background beside the jobs: its fetch.id goes to studio_wait (or url_fetch_get), and the finished download names the recording (pid).",
            schema: || object(json!({ "url": { "type": "string" }, "quality": { "type": "string" }, "subs_lang": { "type": "string" }, "cookies": { "type": "string", "description": "path of a Netscape cookies.txt" } }), &["url"]),
            call: |args| {
                let mut body = json!({ "url": text(args, "url")? });
                for field in ["quality", "subs_lang", "cookies"] {
                    if let Some(value) = given(args, field) {
                        body[field] = value.into();
                    }
                }
                post("/projects/from_url".into(), body)
            },
        },
        // ---------------------------------------------------------------- the pronunciation dictionary of a project
        Tool {
            name: "project_dictionary_counts",
            description: "How many times each pronunciation rule (of the studio's dictionary and the project's) fires in a project's text: a rule that never fires is misspelt or not needed.",
            schema: project_only,
            call: |args| get(project_path(args, "/dictionary-counts")?),
        },
        Tool {
            name: "project_unknown_words",
            description: "The words of a project's text that no dictionary knows (where the stress or the reading may come out wrong), with how often each occurs: the words to check and give a rule with dictionary_set.",
            schema: project_only,
            call: |args| get(project_path(args, "/unknown-words")?),
        },
        Tool {
            name: "dictionary_try",
            description: "Hear a pronunciation rule before saving it: the phrase is voiced without and with the rule (an entry as dictionary_set takes it: find, say, kind word, regex or phonemes, whole_word, case_sensitive) in the project's engine and voice (or voice). A job: follow it with studio_wait; its result names the file, which dictionary_try_file gives.",
            schema: || object(json!({ "pid": pid(), "phrase": { "type": "string" }, "rule": { "type": "object", "properties": { "find": { "type": "string" }, "say": { "type": "string" }, "kind": { "type": "string", "enum": ["word", "regex", "phonemes"] }, "whole_word": { "type": "boolean" }, "case_sensitive": { "type": "boolean" } }, "required": ["find", "say"], "description": "one dictionary entry" }, "voice": { "type": "string" } }), &["pid", "phrase", "rule"]),
            call: |args| post(project_path(args, "/dictionary/try")?, body_without(args, &["pid"])),
        },
        Tool {
            name: "dictionary_try_file",
            description: "The sound of a dictionary_try as a file on this computer: file is the name its job answered.",
            schema: || object(json!({ "pid": pid(), "file": { "type": "string", "description": "file name from the dictionary_try job" } }), &["pid", "file"]),
            call: |args| get(project_path(args, &format!("/tries/{}", segment(&text(args, "file")?)))?),
        },
        // ---------------------------------------------------------------- cloud models: fal and Runware
        Tool {
            name: "cloud_catalog",
            description: "The cloud speech models of fal and Runware (models) and the state of each provider's catalog and key (providers); task narrows the models to speech or transcribe.",
            schema: || object(json!({ "task": { "type": "string", "enum": ["speech", "transcribe"] } }), &[]),
            call: |args| get(format!("/cloud/catalog{}", query(&[("task", given(args, "task"))]))),
        },
        Tool {
            name: "cloud_catalog_refresh",
            description: "Read the catalogs of fal and Runware again now, in the background; cloud_catalog shows them when they arrive.",
            schema: nothing,
            call: |_| post("/cloud/catalog/refresh".into(), json!({})),
        },
        Tool {
            name: "cloud_model_form",
            description: "The settings a cloud model takes, read from its own schema: each field with its type, choices, default and limits - the values cloud_estimate and a block's cloud settings use.",
            schema: || object(json!({ "provider": provider(), "id": { "type": "string", "description": "model id from cloud_catalog" } }), &["provider", "id"]),
            call: |args| get(format!("/cloud/form{}", query(&[("provider", Some(text(args, "provider")?)), ("id", Some(text(args, "id")?))]))),
        },
        Tool {
            name: "cloud_estimate",
            description: "What a cloud model will cost before running it: runs times for chars characters of text with the settings in values. Nothing is paid.",
            schema: || object(json!({ "provider": provider(), "id": { "type": "string" }, "values": { "type": "object" }, "runs": { "type": "integer", "minimum": 1 }, "chars": { "type": "integer", "minimum": 0 } }), &["provider", "id"]),
            call: |args| post("/cloud/estimate".into(), json!({
                "provider": text(args, "provider")?,
                "id": text(args, "id")?,
                "values": args.get("values").cloned().unwrap_or_else(|| json!({})),
                "runs": args.get("runs").and_then(Value::as_u64).unwrap_or(1),
                "chars": args.get("chars").and_then(Value::as_u64).unwrap_or(0),
            })),
        },
        Tool {
            name: "cloud_keys",
            description: "Whether a key of fal and of Runware is stored and where it comes from (the studio's store or an environment variable). The keys themselves are never shown.",
            schema: nothing,
            call: |_| get("/cloud/keys".into()),
        },
        Tool {
            name: "cloud_set_key",
            description: "Store the key of fal or Runware: the provider checks it first and it is kept only if it works. It is never shown back.",
            schema: || object(json!({ "provider": provider(), "api_key": { "type": "string" } }), &["provider", "api_key"]),
            call: |args| send(Method::PUT, format!("/cloud/keys/{}", segment(&text(args, "provider")?)), json!({ "api_key": text(args, "api_key")? })),
        },
        Tool {
            name: "cloud_delete_key",
            description: "Remove the stored key of fal or Runware; its models stop until a key is set again.",
            schema: || object(json!({ "provider": provider() }), &["provider"]),
            call: |args| send(Method::DELETE, format!("/cloud/keys/{}", segment(&text(args, "provider")?)), json!({})),
        },
        Tool {
            name: "cloud_spend",
            description: "What the cloud models of fal and Runware cost so far (the price after each run and the estimate before it), with the daily limit for agents and programs (daily_limit_usd); pid narrows it to one project.",
            schema: || object(json!({ "pid": pid() }), &[]),
            call: |args| get(format!("/cloud/spend{}", query(&[("project", given(args, "pid"))]))),
        },
        // ---------------------------------------------------------------- the OpenAI-compatible API for other programs
        Tool {
            name: "openai_api_status",
            description: "The studio's OpenAI-compatible API for other programs on this computer (/v1/audio/speech, /v1/audio/transcriptions, /v1/models): whether a key guards it and where it comes from, and the model names it takes for speech (each with its engine) and for transcription. The key itself is never shown.",
            schema: nothing,
            call: |_| get("/openai/settings".into()),
        },
        Tool {
            name: "openai_api_key_create",
            description: "Make a new key for the OpenAI-compatible API, replacing the old one: from now on a program needs Authorization: Bearer <key>. The key is answered this once, for the user to give their program.",
            schema: nothing,
            call: |_| post("/openai/key".into(), json!({})),
        },
        Tool {
            name: "openai_api_key_delete",
            description: "Remove the key of the OpenAI-compatible API: programs on this computer reach it without one again.",
            schema: nothing,
            call: |_| send(Method::DELETE, "/openai/key".into(), json!({})),
        },
        Tool {
            name: "openai_api_log",
            description: "The latest requests other programs made to the OpenAI-compatible API, newest first: when, which endpoint, model, voice, the text's size, the status, how long it took and the error.",
            schema: nothing,
            call: |_| get("/openai/log".into()),
        },
        Tool {
            name: "openai_api_log_clear",
            description: "Empty the log of the OpenAI-compatible API's requests.",
            schema: nothing,
            call: |_| send(Method::DELETE, "/openai/log".into(), json!({})),
        },
        Tool {
            name: "openai_api_models",
            description: "GET /v1/models of the OpenAI-compatible API, as a program sees it: the model names it takes.",
            schema: nothing,
            call: |_| get("/v1/models".into()),
        },
        Tool {
            name: "openai_api_speech",
            description: "Call /v1/audio/speech as a program would (to check what it gets): model (an engine alias from openai_api_status), input text, voice (a library voice name), response_format (mp3 by default, wav, flac, aac, pcm), speed (0.25 to 4), instructions (the direction). Answers the sound as a file on this computer.",
            schema: || object(json!({ "model": { "type": "string" }, "input": { "type": "string" }, "voice": { "type": "string" }, "response_format": { "type": "string", "enum": ["mp3", "wav", "flac", "aac", "pcm"] }, "speed": { "type": "number" }, "instructions": { "type": "string" } }), &["model", "input", "voice"]),
            call: |args| post("/v1/audio/speech".into(), args.clone()),
        },
        Tool {
            name: "openai_api_transcribe",
            description: "Call /v1/audio/transcriptions as a program would: a sound or video file on this computer (path), model (from openai_api_status), response_format (json, text, verbose_json, srt, vtt).",
            schema: || object(json!({ "path": { "type": "string" }, "model": { "type": "string" }, "response_format": { "type": "string", "enum": ["json", "text", "verbose_json", "srt", "vtt"] }, "language": { "type": "string" } }), &["path"]),
            call: |args| {
                let mut fields = Vec::new();
                for field in ["model", "response_format", "language"] {
                    if let Some(value) = given(args, field) {
                        fields.push((field.to_string(), value));
                    }
                }
                upload("/v1/audio/transcriptions", local_file(args, "path")?, "file", fields)
            },
        },
        // ---------------------------------------------------------------- Listening: sound in
        Tool {
            name: "listen_upload",
            description: "Make a Listening recording of a sound or video file by uploading a copy of it, as the window does with a dropped file (listen_create_from_file reads it where it lies). Answers the recording's id.",
            schema: || id_only("path", "a sound or video file on this computer"),
            call: |args| upload("/listen/upload", local_file(args, "path")?, "file", Vec::new()),
        },
        Tool {
            name: "listen_record_start",
            description: "Start recording a new Listening recording from the user's microphone (device from voice_record_devices; the default one when left out). listen_record_stop ends it and makes the recording.",
            schema: || object(json!({ "device": { "type": "string" } }), &[]),
            call: |args| post("/listen/record/start".into(), match given(args, "device") { Some(device) => json!({ "device": device }), None => json!({}) }),
        },
        Tool {
            name: "listen_record_stop",
            description: "Stop recording from the microphone and keep it as a Listening recording: answers its id; listen_transcribe recognises it.",
            schema: nothing,
            call: |_| post("/listen/record/stop".into(), json!({})),
        },
        Tool {
            name: "listen_audio",
            description: "A Listening recording's sound as a file on this computer.",
            schema: || id_only("pid", "recording id"),
            call: |args| get(format!("/listen/{}/audio", segment(&text(args, "pid")?))),
        },
        // ---------------------------------------------------------------- Speech: the queue, history, the live sound
        Tool {
            name: "project_queue",
            description: "The requests of a project waiting behind its running voicing, in order, each with its number (n) and when it was added.",
            schema: project_only,
            call: |args| get(project_path(args, "/queue")?),
        },
        Tool {
            name: "project_google_report",
            description: "Google Gemini voicing of a project: the batch waiting at Google now (its state and how many phrases are ready), the last job's result (mode standard or batch, phrases asked, ready, failed with Google's reason, cost estimate, seconds) and every finished or expired batch. The mode is the setting google_tts_mode: standard (direct requests, or_concurrency at a time), batch (half the tariff, Google takes up to a day) or auto (direct up to 10 phrases, batch from 11).",
            schema: project_only,
            call: |args| get(project_path(args, "/google")?),
        },
        Tool {
            name: "project_queue_clear",
            description: "Take every waiting request of a project off its queue; the voicing that runs goes on.",
            schema: project_only,
            call: |args| send(Method::DELETE, project_path(args, "/queue")?, json!({})),
        },
        Tool {
            name: "project_queue_remove",
            description: "Take one waiting request off a project's queue by its number (n) in project_queue.",
            schema: || object(json!({ "pid": pid(), "n": { "type": "integer", "minimum": 0 } }), &["pid", "n"]),
            call: |args| send(Method::DELETE, project_path(args, &format!("/queue/{}", args.get("n").and_then(Value::as_u64).ok_or("'n' is required")?))?, json!({})),
        },
        Tool {
            name: "history_list",
            description: "The takes voiced, across projects or in one (pid): the project, the block, its speaker and text, the take's number and file, when it was made, its length, how long it took (rtf), the first sound (first_ms), the seed and sampling, the voice and engine, its recognition error (wer) and whether it is the block's active take.",
            schema: || object(json!({ "pid": pid() }), &[]),
            call: |args| get(format!("/history{}", query(&[("pid", given(args, "pid"))]))),
        },
        Tool {
            name: "speech_live_watch",
            description: "Listen to what is being voiced right now, for seconds: each block that starts, restarts (a defective take said again) and ends, with how soon its first sound came (first_ms); the sound itself is kept as a WAV file per block on this computer (files).",
            schema: || object(json!({ "seconds": seconds() }), &[]),
            call: |args| stream(Method::GET, "/speech/live".into(), None, args),
        },
        Tool {
            name: "job_events",
            description: "A job's progress as the window follows it, for seconds or until it ends: every stage, message and percent in order (studio_wait answers only where it got).",
            schema: || object(json!({ "job_id": { "type": "string" }, "seconds": seconds() }), &["job_id"]),
            call: |args| stream(Method::GET, format!("/jobs/{}/events", segment(&text(args, "job_id")?)), None, args),
        },
        // ---------------------------------------------------------------- Speech: projects from files, packages
        Tool {
            name: "project_package",
            description: "Pack a project into one .speechproj file in its export folder (the project and the voices it uses; with its voiced sound unless audio is false) to move it to another computer. Answers the file's name and size; project_export_file or project_export_save takes it from there.",
            schema: || object(json!({ "pid": pid(), "audio": { "type": "boolean" } }), &["pid"]),
            call: |args| post(project_path(args, "/package")?, json!({ "audio": args.get("audio").and_then(Value::as_bool).unwrap_or(true) })),
        },
        Tool {
            name: "project_import_package",
            description: "Make a project of a .speechproj file (project_package): answers its id and the voices it added to the library.",
            schema: || id_only("path", "the .speechproj file"),
            call: |args| upload("/speechproj/import", local_file(args, "path")?, "file", Vec::new()),
        },
        Tool {
            name: "project_create_from_subtitles",
            description: "Make a Speech project of a subtitles file (SRT, VTT, ASS, SSA): its lines become blocks. name names it (the file's name otherwise).",
            schema: || object(json!({ "path": { "type": "string" }, "name": { "type": "string" } }), &["path"]),
            call: |args| upload("/projects/from-subtitles", local_file(args, "path")?, "file", given(args, "name").map(|name| vec![("name".to_string(), name)]).unwrap_or_default()),
        },
        Tool {
            name: "book_preview",
            description: "Read a book (EPUB, FB2, PDF, DOCX, HTML, Markdown, TXT) to choose its chapters: answers its id, title, author, language, whether it has a cover, and the chapters (index, title, characters, estimated seconds of speech, its opening, skip naming one to leave out: empty, toc, front or cover). book_chapter gives a whole one; project_create_from_book makes the audiobook project.",
            schema: || id_only("path", "the book file"),
            call: |args| upload("/books/preview", local_file(args, "path")?, "file", Vec::new()),
        },
        Tool {
            name: "book_chapter",
            description: "One chapter of a book read by book_preview: its title and text.",
            schema: || object(json!({ "id": { "type": "string", "description": "book id from book_preview" }, "index": { "type": "integer", "minimum": 0 } }), &["id", "index"]),
            call: |args| get(format!("/books/{}/chapters/{}", segment(&text(args, "id")?), args.get("index").and_then(Value::as_u64).ok_or("'index' is required")?)),
        },
        Tool {
            name: "project_create_from_book",
            description: "Make an audiobook project of a book read by book_preview: chapters lists the ones to take (index, and a title or text of your own to replace theirs), voice the narrator, name the project's name.",
            schema: || object(json!({ "id": { "type": "string" }, "name": { "type": "string" }, "voice": { "type": "string" }, "chapters": { "type": "array", "items": { "type": "object", "properties": { "index": { "type": "integer" }, "title": { "type": "string" }, "text": { "type": "string" } }, "required": ["index"] } } }), &["id", "chapters"]),
            call: |args| post("/projects/from-book".into(), args.clone()),
        },
        // ---------------------------------------------------------------- Speech: takes and export files
        Tool {
            name: "project_take_file",
            description: "A block's take as a sound file on this computer: file is the take's name from project_get.",
            schema: || object(json!({ "pid": pid(), "file": { "type": "string" } }), &["pid", "file"]),
            call: |args| get(project_path(args, &format!("/takes/{}", segment(&text(args, "file")?)))?),
        },
        Tool {
            name: "project_export_file",
            description: "An export of a project (file from project_exports) as a file on this computer.",
            schema: || object(json!({ "pid": pid(), "file": { "type": "string" } }), &["pid", "file"]),
            call: |args| get(project_path(args, &format!("/exports/{}", segment(&text(args, "file")?)))?),
        },
        Tool {
            name: "project_export_save",
            description: "Copy an export of a project (file from project_exports) to path: a file, or a folder to put it in under its name.",
            schema: || object(json!({ "pid": pid(), "file": { "type": "string" }, "path": { "type": "string" } }), &["pid", "file", "path"]),
            call: |args| post(project_path(args, "/save-export")?, json!({ "file": text(args, "file")?, "path": text(args, "path")? })),
        },
        Tool {
            name: "project_exports_to_folder",
            description: "Copy exports of a project (files from project_exports) into folder; without folder the user chooses it in a dialog on their screen.",
            schema: || object(json!({ "pid": pid(), "files": { "type": "array", "items": { "type": "string" } }, "folder": { "type": "string" } }), &["pid", "files"]),
            call: |args| post(project_path(args, "/exports-to-folder")?, body_without(args, &["pid"])),
        },
        Tool {
            name: "project_export_reveal",
            description: "Show an export of a project in Explorer, selected, for the user.",
            schema: || object(json!({ "pid": pid(), "file": { "type": "string" } }), &["pid", "file"]),
            call: |args| post(project_path(args, &format!("/exports/{}/reveal", segment(&text(args, "file")?)))?, json!({})),
        },
        Tool {
            name: "project_export_save_as",
            description: "Ask the user where to save an export of a project, in a save dialog on their screen, and copy it there: answers the path, null when they cancelled.",
            schema: || object(json!({ "pid": pid(), "file": { "type": "string" } }), &["pid", "file"]),
            call: |args| post(project_path(args, &format!("/exports/{}/save-as", segment(&text(args, "file")?)))?, json!({})),
        },
        // ---------------------------------------------------------------- the Assistant panel
        Tool {
            name: "assistant_models",
            description: "The OpenRouter models the studio's Assistant panel can work with (those that call tools), the one chosen, and who answers there: a model, or the connected agent.",
            schema: nothing,
            call: |_| get("/chat/models".into()),
        },
        Tool {
            name: "assistant_chat",
            description: "Talk to the studio's Assistant panel as the user does: messages (role user or assistant, content) go to model (assistant_models) with project_id and language for context; answers what it said, the tools it ran and what it cost, read for seconds or until it finished. It pays from the user's OpenRouter key.",
            schema: || object(json!({ "model": { "type": "string" }, "messages": { "type": "array", "items": { "type": "object", "properties": { "role": { "type": "string" }, "content": { "type": "string" } }, "required": ["role", "content"] } }, "project_id": { "type": "string" }, "language": { "type": "string" }, "seconds": seconds() }), &["model", "messages"]),
            call: |args| stream(Method::POST, "/chat".into(), Some(body_without(args, &["seconds"])), args),
        },
        // ---------------------------------------------------------------- diagnostics and the journal
        Tool {
            name: "diagnostics_get",
            description: "The studio's diagnostics: version, system, graphics card against CUDA, ffmpeg, every component and whether it is on disk, what the engines' DLLs miss, and the latest log lines - as text to read and as data.",
            schema: nothing,
            call: |_| get("/diagnostics".into()),
        },
        Tool {
            name: "diagnostics_report",
            description: "Pack a bug report (the diagnostics, the activity journal and the log, keys masked) into a ZIP file: answers its path.",
            schema: nothing,
            call: |_| post("/diagnostics/report".into(), json!({})),
        },
        Tool {
            name: "diagnostics_report_reveal",
            description: "Show a bug report (path from diagnostics_report) in Explorer, selected, for the user.",
            schema: || id_only("path", "the report's path"),
            call: |args| post("/diagnostics/report/reveal".into(), json!({ "path": text(args, "path")? })),
        },
        Tool {
            name: "diagnostics_logs_reveal",
            description: "Open the folder of the studio's logs in Explorer, for the user.",
            schema: nothing,
            call: |_| post("/diagnostics/logs/reveal".into(), json!({})),
        },
        Tool {
            name: "journal_get",
            description: "The studio's activity journal, oldest first: what agents and the Assistant changed (verb, kind, target) and what the window told the user, each with its id and time.",
            schema: nothing,
            call: |_| get("/journal".into()),
        },
        Tool {
            name: "journal_note",
            description: "Write a line into the activity journal the user reads: text, and tone agent (default), work, done or error.",
            schema: || object(json!({ "text": { "type": "string" }, "tone": { "type": "string", "enum": ["agent", "work", "done", "error"] } }), &["text"]),
            call: |args| post("/journal".into(), json!({ "source": "studio", "tone": given(args, "tone").unwrap_or_else(|| "agent".into()), "text": text(args, "text")? })),
        },
        Tool {
            name: "journal_clear",
            description: "Empty the activity journal.",
            schema: nothing,
            call: |_| send(Method::DELETE, "/journal".into(), json!({})),
        },
        Tool {
            name: "journal_delete",
            description: "Remove one line of the activity journal by its id (journal_get).",
            schema: || object(json!({ "id": { "type": "integer", "minimum": 0 } }), &["id"]),
            call: |args| send(Method::DELETE, format!("/journal/{}", args.get("id").and_then(Value::as_u64).ok_or("'id' is required")?), json!({})),
        },
        // ---------------------------------------------------------------- this connection and the windows
        Tool {
            name: "agent_status",
            description: "This connection as the studio's settings show it: whether an agent is connected, its last call and how long ago, how many calls it made, and whether a window of the studio is open.",
            schema: nothing,
            call: |_| composite("agent"),
        },
        Tool {
            name: "windows_watch",
            description: "The studio's open windows (the last is the one the ui_* tools work in) and, for seconds, what the studio tells them: what changed behind them (a project, the journal, the voices) and the commands the ui_* tools send.",
            schema: || object(json!({ "seconds": seconds() }), &[]),
            call: |_| composite("windows"),
        },
        Tool {
            name: "window_focus",
            description: "Make an open window (its number from windows_watch) the one the ui_* tools work in, as when the user turns to it.",
            schema: || object(json!({ "window": { "type": "integer", "minimum": 0 } }), &["window"]),
            call: |_| composite("window_focus"),
        },
        Tool {
            name: "window_command_drop",
            description: "Give up a ui_* command a window has not answered (its id from windows_watch): the call waiting for it ends now with reason as its error.",
            schema: || object(json!({ "id": { "type": "string" }, "reason": { "type": "string" } }), &["id"]),
            call: |_| composite("window_drop"),
        },
        Tool {
            name: "studio_batch",
            description: "Run several tools in one call, in order, through this MCP server: calls is a list of {name, arguments}. It stops at the first that fails unless keep_going is true; answers each one's result or error. One call's time limit still applies, so put long work in jobs and studio_wait for them.",
            schema: || object(json!({ "calls": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "arguments": { "type": "object" } }, "required": ["name"] } }, "keep_going": { "type": "boolean" } }), &["calls"]),
            call: |_| composite("batch"),
        },
    ]
}

/// What the live stream said, the sound of each block written to a WAV file in place of its pieces.
pub(super) async fn live_files(read: Value) -> Result<Value, String> {
    use base64::Engine;
    let mut events = Vec::new();
    let mut sound: Vec<((String, String), u32, Vec<f32>)> = Vec::new();
    for event in read["events"].as_array().cloned().unwrap_or_default() {
        if event["kind"] != "chunk" {
            events.push(event);
            continue;
        }
        let key = (event["project"].as_str().unwrap_or_default().to_string(), event["block"].as_str().unwrap_or_default().to_string());
        let rate = event["sr"].as_u64().and_then(|rate| u32::try_from(rate).ok()).filter(|rate| *rate > 0).ok_or("a live piece came without its sample rate")?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(event["data"].as_str().unwrap_or_default()).map_err(|e| format!("a live piece: {e}"))?;
        let samples = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        match sound.iter_mut().find(|(known, known_rate, _)| *known == key && *known_rate == rate) {
            Some((_, _, all)) => all.extend(samples),
            None => sound.push((key, rate, samples.collect())),
        }
    }
    let folder = super::agent_files().await?;
    let mut files = Vec::new();
    for ((project, block), rate, samples) in sound {
        let file = folder.join(format!("speech_live_watch-{}-{}-{}.wav", segment(&project), segment(&block), super::chrono_stamp()));
        let spec = hound::WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut writer = hound::WavWriter::create(&file, spec).map_err(|e| format!("{}: {e}", file.display()))?;
        for sample in &samples {
            writer.write_sample(*sample).map_err(|e| format!("{}: {e}", file.display()))?;
        }
        writer.finalize().map_err(|e| format!("{}: {e}", file.display()))?;
        files.push(json!({ "project": project, "block": block, "file": file.to_string_lossy(), "seconds": samples.len() as f64 / f64::from(rate) }));
    }
    Ok(json!({ "events": events, "files": files, "ended": read["ended"] }))
}
