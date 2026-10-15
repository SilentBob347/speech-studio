---
name: speech-studio
description: Drive Speech Studio on this computer through its MCP server - voice texts, scripts, podcasts and books (Higgs Audio, Drama 3, Supertonic, Qwen3-TTS, OpenRouter, Gemini, fal, Runware) with stress marks, normalisation and a pronunciation dictionary, recognise recordings with speakers and word times (Parakeet, Whisper, OpenRouter), edit them by text, check voicing against a text, manage the voice library and models, and work the studio's window while the user watches. Use whenever the user asks for anything the studio does.
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
- **Voices**: `voices_list` (`brief` for names, kind, language and gender only), `voices_catalog` (the cloud voice
  library), `voice_download`, `voices_download_pack` (the starter pack), `voice_import`, `voices_import_zip`,
  `voices_export`, `voice_rename`, `voice_delete`, `voice_card_set`, `voice_photo_set`, `voice_transcribe`,
  `voice_transcript_set`; a voice from the user's microphone: `voice_record_devices`, `voice_record_start`,
  `voice_record_level`, `voice_record_stop`. Cleaning a reference: `voice_clean` (voice from music, Roformer),
  `voice_denoise` (noise, DPDFNet, `db` limits how far), `voice_restore`; its best piece: `voice_ref_candidates`,
  `voice_ref_apply`. A voice from a description: `voice_design` (description best in English or Chinese, a sample text,
  1-6 candidates; a job) then `voice_design_keep` (one candidate into the library).
- **By link**: `url_probe` (title, length, qualities, the site's subtitles), `url_fetches_list`, `url_fetch_get`,
  `url_fetch_cancel`, `url_fetch_resume`, `url_fetch_delete`, `url_tool_status`, `url_tool_update`.
- **The window**: `ui_screenshot`, `ui_read_page`, `ui_click`, `ui_type`, `ui_select`, `ui_press_key`, `ui_scroll`,
  `ui_navigate`, `ui_open_settings`, `ui_notify`, `ui_console` work in the studio's window in front of the user; a key or
  password field reads only as filled or empty. `editor_open` opens a project in the window, `editor_state` says what is
  open (its blocks or lines, what is selected and playing), `editor_select` shows the user a block or a line.
- **The journal**: every change you make (create, update, delete, voice, export...) is written into the window's
  activity log with the name of what it changed, and stays there after a restart.
- **Being the assistant**: when the user chose «Agent (MCP)» as the assistant (Settings, Agent), what they write in the
  Assistant panel waits for you: `assistant_requests_wait`, do what the request asks with the tools (the panel shows each
  call), then `assistant_request_answer` with your answer in words, in the language the request names.


## Speech: voicing

- **One call**: `speak_text` (text, voice, engine, lang, format, direction) gives an audio file; the same arguments
  again answer with the same file at once. Longer work answers `done: false` with its job: `studio_wait`, then call
  again.
- **Projects**: `projects_list`, `project_create` (paragraphs become blocks; "Anna: ..." lines become a dialogue),
  `project_get`, `project_update` (send the whole project back with your edits; changed blocks go stale),
  `project_synth` (new and stale blocks, or `ids`, `force`, `from`), `project_export_check` (what keeps the export back:
  missing, stale, failed, a take file changed after voicing, a take longer than its cue), `project_export` (one file
  with subtitles, M4B with chapters, `part_minutes` for long books, or `per_block`; `allow_duration` lets only the
  too-long takes through), `project_exports`, `project_timeline` (blocks on the export's time scale with overlaps; a
  block's `cut_previous` cuts the reply it interrupts, a previous block's `pause_ms` of 0 joins them), `project_delete`.
  A block's `pause_ms` is the pause after it; a negative one makes the next block interrupt.
- **Engines**: `engines_list` says what each engine understands (languages, sampling, tags, cloning, `needs_voice`) and
  why one is not available here (Drama 3 needs a 6 GB card, and 32 GB of RAM when the card is under 24 GB or the studio setting `drama_vram_gb` is below the whole model). Local engines: `higgs`, `drama3`, `supertonic` (its own ten
  voices), `qwen3-tts` (nine named voices, cloning, the block's direction as its instruction) and `qwen3-tts-ru` (Russian
  stress marks, a library voice of at most 10 s). A speaker's engine (`cast_engines`) and a block's (`engine`) override
  the project's; cloud engines are set for the whole project only. Cloud engines cost money: `openrouter`, `google` (Gemini; the project's
  `cloud_model` and `cloud_voice`, Batch mode from the studio setting `google_tts_mode` voices the whole job as one
  half-price batch that can take hours), `fal`, `runware`. Ask the user before a paid run.
- **Text**: `text_prepare` shows how a text will be read (dictionary, Russian normalisation, stress marks, the engine's
  text), `text_stress` gives stress marks and ё with unsure homographs, `dictionary_get` / `dictionary_set` /
  `dictionary_import` (Balabolka .dic, .rex, .bxd).
- **Director model**: `podcast_write` (topic and speakers with their manner -> a new project),
  `project_cast_characters` (one chapter split into the narrator and characters word for word).

## Listening: recognition

- **One call**: `transcribe_file` (path, format text/srt/vtt/ass/json, lang, speakers), `verify_speech` (path and the
  text it should say -> word error rate and each missed, extra or replaced word), `separate_file` (voice and music apart).
- **Recognition settings** (in `listen_update`): `prompt` (a hint Whisper takes, with the corrections' spellings),
  `denoise` and `denoise_db` (DPDFNet before recognition), `vad` (Silero VAD cuts a long recording for Parakeet at
  pauses: `threshold`, `hysteresis`, `min_speech_ms`, `min_silence_ms`, `pad_ms`).
- **Projects**: `listen_create_from_file`, `listen_get`, `listen_update` (rename speakers, edit or hide lines, mark
  words `cut`, recognition settings, corrections), `listen_transcribe`, `listen_cut` (a new recording without the
  marked words), `listen_export`, `listen_check`, `listen_align` (the reference text laid over the recording, each word
  at the time it was heard; put the lines back with `listen_update`), `listen_to_speech` (the transcript as a Speech project),
  `listen_voices` (each speaker's best clean speech into the voice library).

## OpenAI-compatible API

Programs that speak OpenAI's audio API can use the studio directly: `POST http://127.0.0.1:8797/v1/audio/speech`
(`model` tts-1 / higgs / drama3, `voice` a voice name of the library, `instructions`, `speed`, `response_format` mp3,
wav, flac, aac, pcm), `POST /v1/audio/transcriptions` (multipart `file`, `model` whisper-1, `language`,
`response_format` json, text, srt, vtt, verbose_json) and `GET /v1/models`. With a key (`SPEECH_STUDIO_API_TOKEN`, or
one the user made in Settings, API) send `Authorization: Bearer <key>`. Settings, API also shows examples and the latest
requests with their answers (`ui_open_settings` with section api).
