# Where the code comes from

Speech Studio reuses the code of timoncool's other programs. What is shared as a library is a git
dependency pinned to a revision; what is not a library is a copy, listed here with the commit it was
taken at, so a fix made there can be carried over here and the other way round.

## Git dependencies

| Crate | Repository | Revision |
|---|---|---|
| `dub-core`, `dub-asr` (feature `cuda`), `dub-llm`, `dub-translate`, `dub-captions`, `dub-sep`, `dub-faces`, `audiocpp`, `drama` | `timoncool/dub-studio` | `a21ae6cefe1ce86b39e1b3524686b4082c36732c` |
| `vs-providers` | `timoncool/videosos` | `29b47ce7f0dfcf7601415a099687556e79fe9b43` |
| `studio-hub-client` | `timoncool/studio-hub` | `e44abf030d5a5acc48ef693a395a04109db8fa16` |
| `audio-post` (FLAC by libFLAC, MP3 by LAME, resampling) | `timoncool/YuE2-Studio` | `ce206f1ac5089daecead8d5cfccc60f533db78b3` |

## Copies from Dub Studio

Taken from `timoncool/dub-studio` at `11f909a8e798d56f5f43e0b7ae959d77a05a1866`.

| Here | There | How it differs |
|---|---|---|
| `crates/speech-server/src/` (`engines/bench.rs`, `integrations/cloud_voices.rs` + `integrations/voice_db.json`, `platform/credentials.rs`, `platform/dll_imports.rs`, `setup/downloads.rs`, `app/guard.rs`, `engines/hw.rs`, `i18n`, `jobs/store.rs`, `jobs/mod.rs`, `jobs/limiter.rs`, `integrations/llm_provider.rs`, `app/localize.rs`, `audio/media.rs`, `engines/models.rs`, `integrations/openrouter.rs`, `engines/presets.rs`, `platform/process_group.rs`, `voices/record.rs`, `app/secrets_api.rs`, `app/service.rs`, `app/spa.rs`, `app/studio_settings.rs`, `text/subimport.rs`, `text/tts.rs`, `audio/trim.rs`, `integrations/url_import.rs`, `audio/wavio.rs`, `integrations/ytdlp.rs`) | `crates/dub-server/src/` | names, environment variables (`SPEECH_STUDIO_*`) and port 8797; a link makes a Listening project |
| `crates/speech-server/src/lib.rs` | `crates/dub-server/src/lib.rs` | the router of the skeleton: no dubbing routes |
| `crates/speech-server/src/setup/mod.rs` | `crates/dub-server/src/setup.rs` | no casting and OCR components; WeSpeaker is a component of its own; Gemma and llama.cpp are optional |
| `crates/speech-server/src/app/endpoints.rs`, `crates/speech-server/src/integrations/google_tts.rs` | same files | only what the skeleton calls; the rest returns with its stage |
| `crates/speech-server/src/app/hub.rs` | `crates/dub-server/src/hub.rs` | app id `speech`, `track` for the router |
| `crates/speech-server/src/mcp/mod.rs`, `mcp/window.rs` | `crates/dub-server/src/mcp.rs`, `mcp/window.rs` | Speech and Listening tools; `editor_open`, `editor_state`, `editor_select` instead of Dub's editor |
| `crates/speech-server/locales/` | `crates/dub-server/locales/` | the component texts of voicing and recognition |
| `desktop/` | `desktop/` | identifier `com.speechstudio.desktop`, its own updater key, empty installer hooks |
| `frontend/src/components/` (`AgentPanel`, `ConfirmDialog`, `DownloadProgress`, `GoogleTts`, `GpuNotice`, `HubBars`, `HubTelemetryPreview`, `LlmProviders`, `ModelsFolder`, `OpenRouterKey`, `OpenRouterModelSelect`, `ProxySection`, `RemoveComponent`, `ResourceMonitor`, `ServerOffline`, `WhatsNew`, `YtDlpTool`, `settings/*`) | `frontend/src/components/` | `LlmProviders` keeps one stage, the script assistant |
| `frontend/src/components/setup/` (`FirstRun`, `ModelsSection`, `PresetsSection`), `components/shell/HelpModal`, the status bar of `components/shell/TopBar` | `frontend/src/App.tsx` | moved into files of their own |
| `frontend/src/lib/` | `frontend/src/lib/` | `useChanged` moved into `mcpBridge.ts` |
| `scripts/` | `scripts/` | release scripts still to be fitted (stage 7) |
| `.github/ISSUE_TEMPLATE/`, `issueReportUrl` in `frontend/src/lib/links.ts` | same | Speech Studio's name, links and examples |

## Copies from the music studios

| Here | There (`timoncool/YuE2-Studio` at `ce206f1ac5089daecead8d5cfccc60f533db78b3`) | How it differs |
|---|---|---|
| `crates/speech-server/src/audio/tagging.rs` | `crates/music-server/src/tagging.rs` | without the genre and tempo read from a song's caption |
| FLAC and MP3 export in `speech/jobs.rs` (`audio_post::encode`, then `tagging::write_tags`) | the song store in `crates/music-server/src/lib.rs` | mono speech goes in as both channels; MP3 at 192 kbps, 44.1 kHz for ACX, else 48 kHz |

## Patterns from VideoSOS

| Here | There (`timoncool/videosos` at `29b47ce7f0dfcf7601415a099687556e79fe9b43`) |
|---|---|
| the mode switch of `frontend/src/components/shell/TopBar.tsx` | `frontend/src/features/editor/components/workspace-switcher.tsx` |
| `crates/speech-server/src/app/journal.rs` (a JSONL file instead of SQLite) | `crates/vs-server/src/journal.rs` |
| `crates/speech-server/src/integrations/chat.rs`, `chat_tools` / `chat_call` / `run_tool` of `mcp/mod.rs` | `crates/vs-server/src/chat.rs`, `mcp.rs` |
| `crates/speech-server/src/mcp/assistant.rs` (`assistant_requests_wait`, `assistant_request_answer`) | `crates/vs-server/src/mcp.rs` |
| `frontend/src/lib/chat.ts`, `frontend/src/components/AssistantPanel.tsx` | `frontend/src/features/studio/services/chat.ts`, `components/chat-panel.tsx` |
