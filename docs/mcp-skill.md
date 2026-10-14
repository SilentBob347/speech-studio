---
name: speech-studio
description: Drive Speech Studio on this computer through its MCP server - voice texts, scripts, podcasts and books (Higgs Audio, Drama 3, OpenRouter) with stress marks, normalisation and a pronunciation dictionary, recognise recordings with speakers and word times (Parakeet, Whisper, OpenRouter), edit them by text, check voicing against a text, manage the voice library and models, and work the studio's window while the user watches. Use whenever the user asks for anything the studio does.
---

# Speech Studio through MCP

Speech Studio serves MCP at `http://127.0.0.1:8797/mcp` while it is open (Streamable HTTP, stateless JSON-RPC). Every
tool runs the same code as a control of the studio, through the routes its window calls.

## Connect

```bash
claude mcp add --transport http speech-studio http://127.0.0.1:8797/mcp
```

Other clients: `{ "mcpServers": { "speech-studio": { "type": "streamable-http", "url": "http://127.0.0.1:8797/mcp" } } }`.
Only agents on this computer and the studio's own window may connect. Settings, **Agent (MCP)** shows whether an agent is
connected and the address to paste.

## Ground rules

- **Start with `studio_status`**: what runs now, what finished last, whether the required models are there.
- **Long work is a job**: `studio_wait` with its `job_id` (or `until: download | idle`) instead of polling; it returns
  within a minute with how far the work got - call it again.
- **Models**: `models_status` lists every component with its size and state, `models_download` fetches by id in the
  background (resumable, every file checked against its pinned SHA-256), `models_cancel_download` pauses,
  `models_import` takes files already on disk, `models_select` picks a quantisation, `models_remove` frees disk space,
  `engine_presets_get` / `engine_preset_apply` set everything for this computer's card or for the cloud at once.
- **Keys never come back**: `openrouter_status`, `google_status` and `local_server_key_status` say only whether a key is
  set and where it comes from.
- **Voices**: `voices_list`, `voices_catalog` (the cloud voice library), `voice_download`, `voices_download_pack` (the
  starter pack), `voice_rename`, `voice_delete`; a voice from the user's microphone: `voice_record_devices`,
  `voice_record_start`, `voice_record_level`, `voice_record_stop`.
- **By link**: `url_probe` (title, length, qualities, the site's subtitles), `url_fetches_list`, `url_fetch_get`,
  `url_fetch_cancel`, `url_fetch_resume`, `url_fetch_delete`, `url_tool_status`, `url_tool_update`.
- **The window**: `ui_screenshot`, `ui_read_page`, `ui_click`, `ui_type`, `ui_select`, `ui_press_key`, `ui_scroll`,
  `ui_navigate`, `ui_open_settings`, `ui_notify`, `ui_console` work in the studio's window in front of the user; a key or
  password field reads only as filled or empty.


## Speech: voicing

- **One call**: `speak_text` (text, voice, engine, lang, format, direction) gives an audio file; the same arguments
  again answer with the same file at once. Longer work answers `done: false` with its job: `studio_wait`, then call
  again.
- **Projects**: `projects_list`, `project_create` (paragraphs become blocks; "Anna: ..." lines become a dialogue),
  `project_get`, `project_update` (send the whole project back with your edits; changed blocks go stale),
  `project_synth` (new and stale blocks, or `ids`, `force`, `from`), `project_export` (one file with subtitles, M4B with
  chapters, or `per_block`), `project_exports`, `project_delete`.
- **Engines**: `engines_list` says what each engine understands (languages, sampling, tags, cloning) and why one is not
  available here (Drama 3 needs a 24 GB card).
- **Text**: `text_prepare` shows how a text will be read (dictionary, Russian normalisation, stress marks, the engine's
  text), `text_stress` gives stress marks and ё with unsure homographs, `dictionary_get` / `dictionary_set` /
  `dictionary_import` (Balabolka .dic, .rex, .bxd).
- **Director model**: `podcast_write` (topic and speakers with their manner -> a new project),
  `project_cast_characters` (one chapter split into the narrator and characters word for word).

## Listening: recognition

- **One call**: `transcribe_file` (path, format text/srt/vtt/ass/json, lang, speakers), `verify_speech` (path and the
  text it should say -> word error rate and each missed, extra or replaced word), `separate_file` (voice and music apart).
- **Projects**: `listen_create_from_file`, `listen_get`, `listen_update` (rename speakers, edit or hide lines, mark
  words `cut`, recognition settings, corrections), `listen_transcribe`, `listen_cut` (a new recording without the
  marked words), `listen_export`, `listen_check`, `listen_to_speech` (the transcript as a Speech project),
  `listen_voices` (each speaker's best clean speech into the voice library).

## OpenAI-compatible API

Programs that speak OpenAI's audio API can use the studio directly: `POST http://127.0.0.1:8797/v1/audio/speech`
(`model` tts-1 / higgs / drama3, `voice` a voice name of the library, `instructions`, `speed`, `response_format` mp3,
wav, flac, aac, pcm), `POST /v1/audio/transcriptions` (multipart `file`, `model` whisper-1, `language`,
`response_format` json, text, srt, vtt, verbose_json) and `GET /v1/models`. With `SPEECH_STUDIO_API_TOKEN` set, send
`Authorization: Bearer <token>`.
