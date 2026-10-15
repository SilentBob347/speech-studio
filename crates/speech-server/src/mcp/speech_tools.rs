//! Tools of Speech and Listening: each is a route of the studio's API, as the window calls it.

use std::path::PathBuf;

use axum::http::Method;
use serde_json::{json, Value};

use super::{body_without, get, object, pid, post, project_only, project_path, segment, send, text, Call, Payload, Tool};

fn listen_pid() -> Value {
    json!({ "type": "string", "description": "recording id (listen_create_from_file, projects_list with kind listening)" })
}

fn listen_only() -> Value {
    object(json!({ "pid": listen_pid() }), &["pid"])
}

fn listen_path(args: &Value, rest: &str) -> Result<String, String> {
    Ok(format!("/listen/{}{rest}", segment(&text(args, "pid")?)))
}

fn local_file(args: &Value, name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(text(args, name)?);
    if !path.is_file() {
        return Err(format!("{} is not a file on this computer.", path.display()));
    }
    Ok(path)
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        // ---------------------------------------------------------------- Speech projects
        Tool {
            name: "projects_list",
            description: "The projects: Speech scripts (kind speech: name, blocks, ready) and Listening recordings (kind listening: name, duration, lines, speakers, transcribed), the newest first.",
            schema: super::nothing,
            call: |_| get("/projects".into()),
        },
        Tool {
            name: "project_create",
            description: "A new Speech project from a text: paragraphs become blocks; lines like \"Anna: ...\" or \"[Anna] ...\" become a dialogue, each speaker with a voice of its own. split lines makes each line a block. voice is a voice name of voices_list (none: the engine's base voice); engine an id of engines_list (default higgs; drama3 needs a 24 GB card); lang a code or auto.",
            schema: || object(json!({ "name": { "type": "string" }, "text": { "type": "string" }, "voice": { "type": "string" }, "lang": { "type": "string" }, "engine": { "type": "string", "enum": crate::engines::all().iter().map(|e| e.id).collect::<Vec<_>>() }, "split": { "type": "string", "enum": ["paragraphs", "lines"] } }), &["text"]),
            call: |args| post("/projects".into(), args.clone()),
        },
        Tool {
            name: "project_get",
            description: "A Speech project whole: settings, cast (speaker -> voice), chapters with blocks, each block with its text, speaker, voice, direction, takes (with their check: WER, similarity, issues) and status (new, stale, ready, error).",
            schema: project_only,
            call: |args| get(project_path(args, "")?),
        },
        Tool {
            name: "project_update",
            description: "Save a Speech project as edited: send the whole project as project_get gave it with your changes (texts, speakers, cast, settings, blocks added or removed). Changed blocks go stale by themselves. Refused with project_changed when the window saved it meanwhile: read it again.",
            schema: || object(json!({ "pid": pid(), "project": { "type": "object" } }), &["pid", "project"]),
            call: |args| send(Method::PUT, project_path(args, "")?, args.get("project").cloned().ok_or("'project' is required")?),
        },
        Tool {
            name: "project_delete",
            description: "Move a project (Speech or Listening) to the recycle bin with its takes and exports.",
            schema: project_only,
            call: |args| send(Method::DELETE, project_path(args, "")?, json!({})),
        },
        Tool {
            name: "project_synth",
            description: "Voice blocks of a Speech project: a job, follow it with studio_wait. Without ids, the blocks that are new or stale; ids voices those blocks; force voices them again even when ready (a new take each); from voices from that block to the end. While a voicing runs, a new call is queued after it.",
            schema: || object(json!({ "pid": pid(), "ids": { "type": "array", "items": { "type": "string" } }, "from": { "type": "string" }, "force": { "type": "boolean" } }), &["pid"]),
            call: |args| post(project_path(args, "/synth")?, body_without(args, &["pid"])),
        },
        Tool {
            name: "project_export",
            description: "Build the project's audio into one file (format wav, flac, mp3, ogg, m4a or m4b with chapters) with subtitles, or with per_block one file per block: a job, follow it with studio_wait; its result names the files in the project's exports folder. It refuses while project_export_check lists problems; allow_duration lets only takes that do not fit their cue through.",
            schema: || object(json!({ "pid": pid(), "format": { "type": "string", "enum": ["wav", "flac", "mp3", "ogg", "m4a", "m4b"] }, "per_block": { "type": "boolean" }, "chapter": { "type": "string" }, "part_minutes": { "type": "integer", "minimum": 1 }, "allow_duration": { "type": "boolean" } }), &["pid", "format"]),
            call: |args| post(project_path(args, "/export")?, body_without(args, &["pid"])),
        },
        Tool {
            name: "project_export_check",
            description: "What keeps a project from being exported: each problem names its block and state (new, stale, failed, missing — the take file is gone, modified — the take file changed after it was voiced, duration — the take does not fit its cue even sped up). Empty means it can be exported.",
            schema: project_only,
            call: |args| get(project_path(args, "/export-check")?),
        },
        Tool {
            name: "project_exports",
            description: "The files built for a project (audio, subtitles, chapters, transcripts) with their size, and the folder they are in.",
            schema: project_only,
            call: |args| get(project_path(args, "/exports")?),
        },
        Tool {
            name: "project_cast_characters",
            description: "The director model splits one chapter of a Speech project into the narrator and the characters, word for word, and replaces the chapter's blocks with their lines: a job, follow it with studio_wait. Characters keep the names met before in the project.",
            schema: || object(json!({ "pid": pid(), "chapter": { "type": "string", "description": "chapter id from project_get" } }), &["pid", "chapter"]),
            call: |args| post(project_path(args, "/cast")?, json!({ "chapter": text(args, "chapter")? })),
        },
        Tool {
            name: "podcast_write",
            description: "The director model writes a podcast dialogue on a topic for 2-6 named speakers, each with a manner of speech, and opens it as a new Speech project: a job, follow it with studio_wait; its result has project_id.",
            schema: || object(json!({ "topic": { "type": "string" }, "speakers": { "type": "array", "items": { "type": "object", "properties": { "name": { "type": "string" }, "manner": { "type": "string" } }, "required": ["name"] } }, "minutes": { "type": "integer" }, "lang": { "type": "string" }, "name": { "type": "string" } }), &["topic", "speakers"]),
            call: |args| post("/projects/podcast".into(), args.clone()),
        },
        Tool {
            name: "engines_list",
            description: "The voicing engines with what each understands: languages, sampling parameters with their range and default, seed modes, expression tags, voice cloning, and why one is not available here (Drama 3 needs a 24 GB card).",
            schema: super::nothing,
            call: |_| get("/engines".into()),
        },
        // ---------------------------------------------------------------- text preparation
        Tool {
            name: "text_prepare",
            description: "How a text will be read: the pronunciation dictionary, Russian normalisation (numbers, dates, units in words with their cases) and stress marks with ё, each step traced, and the text the engine gets.",
            schema: || object(json!({ "text": { "type": "string" }, "lang": { "type": "string" }, "voice": { "type": "string" }, "engine": { "type": "string" }, "normalize": { "type": "boolean" }, "stress": { "type": "boolean" } }), &["text"]),
            call: |args| post("/text/prepare".into(), args.clone()),
        },
        Tool {
            name: "text_stress",
            description: "Stress marks and ё for a Russian text, with the homographs the model is unsure of and their variants.",
            schema: || object(json!({ "text": { "type": "string" }, "lang": { "type": "string" } }), &["text"]),
            call: |args| post("/text/stress".into(), args.clone()),
        },
        Tool {
            name: "dictionary_get",
            description: "The global pronunciation dictionary: entries (word, regex or phonemes) with what to find, what to say and where they apply.",
            schema: super::nothing,
            call: |_| get("/dictionary".into()),
        },
        Tool {
            name: "dictionary_set",
            description: "Save the global pronunciation dictionary whole (entries as dictionary_get gives them). A project's own entries are part of the project (project_update).",
            schema: || object(json!({ "entries": { "type": "array", "items": { "type": "object" } } }), &["entries"]),
            call: |args| send(Method::PUT, "/dictionary".into(), json!({ "entries": args.get("entries").cloned().unwrap_or(Value::Array(Vec::new())) })),
        },
        Tool {
            name: "dictionary_import",
            description: "Import a Balabolka dictionary file of this computer (.dic, .rex or .bxd) into the global dictionary.",
            schema: || object(json!({ "path": { "type": "string" } }), &["path"]),
            call: |args| {
                let path = local_file(args, "path")?;
                let name = super::file_name(&path);
                Ok(Call { method: Method::POST, path: "/dictionary/import".into(), payload: Payload::Form { fields: Vec::new(), files: vec![("file".to_string(), path, name)] } })
            },
        },
        // ---------------------------------------------------------------- Listening
        Tool {
            name: "listen_create_from_file",
            description: "A Listening project of an audio or video file on this computer (the file is copied in); the same file again finds the same project. Recognise it with listen_transcribe.",
            schema: || object(json!({ "path": { "type": "string" } }), &["path"]),
            call: |args| post("/listen/from-path".into(), json!({ "path": local_file(args, "path")? })),
        },
        Tool {
            name: "listen_get",
            description: "A Listening project: the recording, recognition settings, speakers (key -> name), lines with speaker, times, text, words with times and the cut marks, hidden hallucinations with their reason.",
            schema: listen_only,
            call: |args| get(listen_path(args, "")?),
        },
        Tool {
            name: "listen_update",
            description: "Save a Listening project as edited: send it whole as listen_get gave it, with renamed speakers, edited lines, hidden or returned lines, words marked cut, recognition settings (engine, lang, speakers 0 auto or 1-8, separate, filter) or corrections.",
            schema: || object(json!({ "pid": listen_pid(), "project": { "type": "object" } }), &["pid", "project"]),
            call: |args| send(Method::PUT, listen_path(args, "")?, args.get("project").cloned().ok_or("'project' is required")?),
        },
        Tool {
            name: "listen_transcribe",
            description: "Recognise the recording with the project's settings: a job, follow it with studio_wait. The previous transcript is replaced; speaker names stay by key.",
            schema: listen_only,
            call: |args| post(listen_path(args, "/transcribe")?, json!({})),
        },
        Tool {
            name: "listen_cut",
            description: "Delete the words marked cut: a job that makes a new Listening project with the recording without them and its lines on shifted times (the original stays); the result has its pid.",
            schema: listen_only,
            call: |args| post(listen_path(args, "/cut")?, json!({})),
        },
        Tool {
            name: "listen_export",
            description: "The transcript as a file in the project's exports and as text: srt, vtt (speakers as voice tags), ass, txt (speakers and times optional) or json with word timings.",
            schema: || object(json!({ "pid": listen_pid(), "format": { "type": "string", "enum": ["srt", "vtt", "ass", "txt", "json"] }, "speakers": { "type": "boolean" }, "times": { "type": "boolean" } }), &["pid", "format"]),
            call: |args| post(listen_path(args, "/export")?, body_without(args, &["pid"])),
        },
        Tool {
            name: "listen_check",
            description: "The recognised text against the text the recording should say: word error rate, and each word same, missed, extra or replaced.",
            schema: || object(json!({ "pid": listen_pid(), "reference": { "type": "string" } }), &["pid", "reference"]),
            call: |args| post(listen_path(args, "/check")?, json!({ "reference": text(args, "reference")? })),
        },
        Tool {
            name: "listen_to_speech",
            description: "A Speech project of the transcript: the lines as blocks with the same speakers on the recording's times; speakers with a voice of that name in the library get it.",
            schema: listen_only,
            call: |args| post(listen_path(args, "/to-speech")?, json!({})),
        },
        Tool {
            name: "listen_voices",
            description: "Voices of the recording's speakers (or of one speaker key): the best clean piece of each speaker's speech goes into the voice library with its text and word times.",
            schema: || object(json!({ "pid": listen_pid(), "speaker": { "type": "string" } }), &["pid"]),
            call: |args| post(listen_path(args, "/voices")?, body_without(args, &["pid"])),
        },
    ]
}
