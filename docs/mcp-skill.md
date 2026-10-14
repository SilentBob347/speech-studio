---
name: speech-studio
description: Drive Speech Studio on this computer through its MCP server - check the graphics card, download and choose the speech models (Higgs Audio, Parakeet, Whisper, diarization, voice separation), manage the voice library and record a voice from the microphone, keep the OpenRouter and Google keys and the proxy, fetch audio and video by a link, and work the studio's window while the user watches. Use whenever the user asks for anything the studio does.
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

Speech (voicing text) and Listening (recognising speech and checking a voicing) arrive in the next stages; their tools
will be listed here when they exist.
