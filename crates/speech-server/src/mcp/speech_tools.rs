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
            description: "A new Speech project from a text: paragraphs become blocks; lines like \"Anna: ...\" or \"[Anna] ...\" become a dialogue, each speaker with a voice of its own. split lines makes each line a block. voice is a voice name of voices_list (none: the engine's base voice); engine an id of engines_list (default higgs; drama3 needs a 6 GB card and, under 24 GB, 32 GB of RAM); lang a code or auto.",
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
            name: "project_enrich",
            description: "The director model suggests, never changing a word: with mode tags (the default) expression tags for blocks by their meaning, only tags the block's engine reads; with mode punctuation the punctuation for reading aloud (commas, dashes and full stops where speech pauses, a run-on sentence cut in two), tags kept. A job, follow it with studio_wait. Its result lists each changed block as id, before and after, and how many answers were turned down for changed words; nothing is applied - put the texts you accept with project_update. Without ids, every block.",
            schema: || object(json!({ "pid": pid(), "ids": { "type": "array", "items": { "type": "string" } }, "mode": { "type": "string", "enum": ["tags", "punctuation"] } }), &["pid"]),
            call: |args| post(project_path(args, "/enrich")?, body_without(args, &["pid"])),
        },
        Tool {
            name: "project_export",
            description: "Build the project's audio into one file (format wav, flac, mp3, ogg, m4a or m4b with chapters) with subtitles beside it (srt, vtt, ass and lrc) and optionally its sample_rate, bits and kbps, or with per_block one file per block: a job, follow it with studio_wait; its result names the files in the project's exports folder, among them player: an HTML page that plays the file beside it with the text, lighting the line and the word being read (a click on a line plays from it), and recipe: a JSON of how it was made (each block's take, engine, voice, seed and sampling, and the project's settings). It refuses while project_export_check lists problems; allow_duration lets only takes that do not fit their cue through.",
            schema: || object(json!({ "pid": pid(), "format": { "type": "string", "enum": ["wav", "flac", "mp3", "ogg", "m4a", "m4b"] }, "per_block": { "type": "boolean" }, "chapter": { "type": "string" }, "part_minutes": { "type": "integer", "minimum": 1 }, "per_chapter": { "type": "boolean", "description": "a file per chapter, levelled as one book so the chapters keep their levels against each other" }, "stems": { "type": "boolean", "description": "also a ZIP (result stems) with a FLAC track per speaker, as long as the file, only that speaker's lines in their places with the file's gain, so the tracks add up to the file - for mixing" }, "allow_duration": { "type": "boolean" }, "sample_rate": { "type": "integer", "enum": [16000, 22050, 24000, 32000, 44100, 48000], "description": "Hz; mp3 takes 32000, 44100 or 48000" }, "bits": { "type": "integer", "enum": [16, 24, 32], "description": "wav 16, 24 or 32 (float); flac 16 or 24" }, "kbps": { "type": "integer", "minimum": 64, "maximum": 320, "description": "mp3, ogg, m4a and m4b" } }), &["pid", "format"]),
            call: |args| post(project_path(args, "/export")?, body_without(args, &["pid"])),
        },
        Tool {
            name: "project_export_check",
            description: "What keeps a project from being exported: each problem names its block and state (new, stale, failed, missing — the take file is gone, modified — the take file changed after it was voiced, duration — the take does not fit its cue even sped up). Empty means it can be exported.",
            schema: project_only,
            call: |args| get(project_path(args, "/export-check")?),
        },
        Tool {
            name: "project_timeline",
            description: "The project on the export's time scale: each block that goes into the export with its speaker, start and end in seconds (by its take, or estimated from the text while it is not voiced) and the overlap with the previous block when a negative pause after that block makes this one interrupt it. A block's cut_previous true cuts the interrupted reply off; a pause after of 0 on the previous block joins them back to back.",
            schema: project_only,
            call: |args| get(project_path(args, "/timeline")?),
        },
        Tool {
            name: "project_exports",
            description: "The files built for a project (audio, subtitles, chapters, transcripts) with their size, and the folder they are in.",
            schema: project_only,
            call: |args| get(project_path(args, "/exports")?),
        },
        Tool {
            name: "project_exports_all",
            description: "Every audio file built for any Speech project, newest first: its project (pid and name), file, size, time and the recipe beside it (how it was made), if any. project_export_file, project_export_save_as and project_export_reveal take the pid and file.",
            schema: super::nothing,
            call: |_| get("/renders".into()),
        },
        Tool {
            name: "project_auto_cast",
            description: "Pick library voices for the characters that have no voice in the cast: never a voice of the other gender, then the nearest age and the project's language, a voice to one character only, the project's own voices left out. A character's gender and age come from the director's casting (two passes: first the characters with gender and age, then their lines) or from settings you put into the project's characters (project_update: characters {name: {gender: female|male, age: child|teen|adult|senior}}). Answers picks (speaker, voice or null when none fits); nothing changes until you put the voices into cast with project_update.",
            schema: project_only,
            call: |args| get(project_path(args, "/auto-cast")?),
        },
        Tool {
            name: "project_lint",
            description: "Check a project before voicing it: block by block, what would go wrong (level error: an empty block, an engine unavailable or without the project's language, no voice where the engine needs one, a voice missing or without the reference text Drama 3 needs, a broken dictionary rule) or not be read as written (level warn: markup that would not work - a pause or tempo tag that does not parse or a tempo out of range, ** or [[ ]] left open, a span tag without its pair, an engine tag not <|category:value|> - tags or stress marks the engine does not read, a direction it ignores, what normalisation left unread).",
            schema: project_only,
            call: |args| get(project_path(args, "/lint")?),
        },
        Tool {
            name: "project_cast_characters",
            description: "The director model splits one chapter of a Speech project into the narrator and the characters, word for word, and replaces the chapter's blocks with their lines: a job, follow it with studio_wait. Two passes: first the characters who speak, each under one name with gender and age (kept in the project's characters), then every line goes to one of them. Characters keep the names met before in the project; project_auto_cast then picks their voices.",
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
            description: "The voicing engines with what each understands: languages, sampling parameters with their range and default, seed modes, expression tags (`tags` by category in `tag_format`; `sentence_tags` go at the start of a sentence, `glued_tags` are followed with no space by the sound's onomatopoeia, a `scope` category wraps a span as <tag>...</tag>), whether the block's direction reaches it (`direction`, with the engine's own voices only when `direction_presets_only`), voice cloning, its own voices with a description, and why one is not available here (Drama 3 needs a 6 GB card and, under 24 GB, 32 GB of RAM).",
            schema: super::nothing,
            call: |_| get("/engines".into()),
        },
        // ---------------------------------------------------------------- text preparation
        Tool {
            name: "text_prepare",
            description: "How a text will be read: the pronunciation dictionary, Russian normalisation (numbers, dates, time, money, phones, e-mail and sites, codes, Roman numerals in words with their cases) and stress marks with ё, each step traced, and the text the engine gets. With a project's pid its dictionary, engine and settings apply (normalize, normalize_off, auto_stress; a block's id adds its voice); lang overrides the project's language.",
            schema: || object(json!({ "text": { "type": "string" }, "pid": pid(), "block": { "type": "string" }, "lang": { "type": "string" } }), &["text"]),
            call: |args| post("/text/prepare".into(), args.clone()),
        },
        Tool {
            name: "text_stress",
            description: "Stress marks and ё for a Russian text, with the homographs the model is unsure of and their variants.",
            schema: || object(json!({ "text": { "type": "string" }, "lang": { "type": "string" } }), &["text"]),
            call: |args| post("/text/stress".into(), args.clone()),
        },
        Tool {
            name: "text_direct_speech",
            description: "A text's direct speech apart from the author's words, as books print it: a paragraph opening with a dash (`— Привет, — сказал он. — Как дела?`) or a quotation after a colon, at the paragraph's start or before the author's words after a dash. Answers the parts in order (speech true for a line) and the speaker name lines get; put them into a project as blocks with project_update. A book takes it at creation with dialogue true (project_create_from_book).",
            schema: || object(json!({ "text": { "type": "string" } }), &["text"]),
            call: |args| post("/text/direct-speech".into(), args.clone()),
        },
        Tool {
            name: "dictionary_get",
            description: "The global pronunciation dictionary: entries (word, regex or phonemes) with what to find, what to say and where they apply.",
            schema: super::nothing,
            call: |_| get("/dictionary".into()),
        },
        Tool {
            name: "dictionary_set",
            description: "Save the global pronunciation dictionary whole (entries as dictionary_get gives them; each finds `find` and says `say`, kind word, regex or phonemes, with whole_word, case_sensitive, lang, voice, engine, enabled and review: approved when checked by ear, fix, or empty). A project's own entries are part of the project (project_update).",
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
            name: "listen_align",
            description: "Lay a known text over a recognised recording (forced alignment): the answer is the recording's lines with the reference text, each word at the time it was heard, a line per sentence with the speaker heard there. Nothing is saved: put the lines into the recording with listen_update to keep them.",
            schema: || object(json!({ "pid": listen_pid(), "reference": { "type": "string" } }), &["pid", "reference"]),
            call: |args| post(listen_path(args, "/aligned")?, body_without(args, &["pid"])),
        },
        Tool {
            name: "listen_to_speech",
            description: "A Speech project of the transcript: the lines as blocks with the same speakers on the recording's times; speakers with a voice of that name in the library get it.",
            schema: listen_only,
            call: |args| post(listen_path(args, "/to-speech")?, json!({})),
        },
        Tool {
            name: "listen_speakers_match",
            description: "Who each speaker of a transcribed recording is like, by the voice print (WeSpeaker): the most alike named speaker (a profile kept with listen_speaker_remember) or library voice, its score and whether it is the same person by the studio's threshold. Rename the speaker with listen_update when it is.",
            schema: || object(json!({ "pid": listen_pid() }), &["pid"]),
            call: |args| post(listen_path(args, "/speakers/match")?, json!({})),
        },
        Tool {
            name: "listen_speaker_remember",
            description: "Keep a recording's speaker voice print under the name the speaker has there (speaker: its key), so later recordings recognise the person; a name kept before learns from this recording too.",
            schema: || object(json!({ "pid": listen_pid(), "speaker": { "type": "string" } }), &["pid", "speaker"]),
            call: |args| post(listen_path(args, "/speakers/remember")?, body_without(args, &["pid"])),
        },
        Tool {
            name: "speaker_profiles_list",
            description: "The named speakers the studio recognises by voice, with how many recordings each was learnt from.",
            schema: super::nothing,
            call: |_| get("/speaker-profiles".into()),
        },
        Tool {
            name: "speaker_profile_delete",
            description: "Forget a named speaker's voice print.",
            schema: || object(json!({ "name": { "type": "string" } }), &["name"]),
            call: |args| send(Method::DELETE, format!("/speaker-profiles/{}", segment(&text(args, "name")?)), Value::Null),
        },
        Tool {
            name: "listen_voices",
            description: "Voices of the recording's speakers (or of one speaker key): the best clean piece of each speaker's speech goes into the voice library with its text and word times.",
            schema: || object(json!({ "pid": listen_pid(), "speaker": { "type": "string" } }), &["pid"]),
            call: |args| post(listen_path(args, "/voices")?, body_without(args, &["pid"])),
        },
    ]
}
