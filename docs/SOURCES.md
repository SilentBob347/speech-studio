# Where the code comes from

Speech Studio reuses the code of timoncool's other programs. What is shared as a library is a git
dependency pinned to a revision; what is not a library is a copy, listed here with the commit it was
taken at, so a fix made there can be carried over here and the other way round.

## Git dependencies

| Crate | Repository | Revision |
|---|---|---|
| `dub-core`, `dub-asr` (feature `cuda`), `dub-llm`, `dub-translate`, `dub-captions`, `dub-sep`, `audiocpp` | `timoncool/dub-studio` | `11f909a8e798d56f5f43e0b7ae959d77a05a1866` |
| `vs-providers` | `timoncool/videosos` | `106272591abd7d29193145b82b5528108f3db1a8` |
| `studio-hub-client` | `timoncool/studio-hub` | `e44abf030d5a5acc48ef693a395a04109db8fa16` |

## Copies from Dub Studio

Taken from `timoncool/dub-studio` at `11f909a8e798d56f5f43e0b7ae959d77a05a1866`.

| Here | There | How it differs |
|---|---|---|
| `crates/speech-server/src/` (`bench`, `cloud_voices` + `voice_db.json`, `credentials`, `dll_imports`, `downloads`, `guard`, `hw`, `i18n`, `job_store`, `jobs`, `limiter`, `llm_provider`, `localize`, `media`, `models`, `openrouter`, `presets`, `process_group`, `record`, `secrets_api`, `service`, `spa`, `studio_settings`, `subimport`, `tts_text`, `tts_trim`, `url_import`, `wavio`, `ytdlp`) | `crates/dub-server/src/` | names, environment variables (`SPEECH_STUDIO_*`) and port 8797; a link makes a Listening project |
| `crates/speech-server/src/lib.rs` | `crates/dub-server/src/lib.rs` | the router of the skeleton: no dubbing routes |
| `crates/speech-server/src/setup.rs` | `crates/dub-server/src/setup.rs` | no casting and OCR components; WeSpeaker is a component of its own; Gemma and llama.cpp are optional |
| `crates/speech-server/src/endpoints.rs`, `google_tts.rs` | same files | only what the skeleton calls; the rest returns with its stage |
| `crates/speech-server/src/hub.rs` | `crates/dub-server/src/hub.rs` | app id `speech`, `track` for the router |
| `crates/speech-server/src/mcp.rs`, `mcp/window.rs` | `crates/dub-server/src/mcp.rs`, `mcp/window.rs` | the tools of the skeleton; no `editor_*`, no atomic project tools |
| `crates/speech-server/locales/` | `crates/dub-server/locales/` | the component texts of voicing and recognition |
| `desktop/` | `desktop/` | identifier `com.speechstudio.desktop`, its own updater key, empty installer hooks |
| `frontend/src/components/` (`AgentPanel`, `ConfirmDialog`, `DownloadProgress`, `GoogleTts`, `GpuNotice`, `HubBars`, `HubTelemetryPreview`, `LlmProviders`, `ModelsFolder`, `OpenRouterKey`, `OpenRouterModelSelect`, `ProxySection`, `RemoveComponent`, `ResourceMonitor`, `ServerOffline`, `WhatsNew`, `YtDlpTool`, `settings/*`) | `frontend/src/components/` | `LlmProviders` keeps one stage, the script assistant |
| `frontend/src/components/setup/` (`FirstRun`, `ModelsSection`, `PresetsSection`), `components/shell/HelpModal`, the status bar of `components/shell/TopBar` | `frontend/src/App.tsx` | moved into files of their own |
| `frontend/src/lib/` | `frontend/src/lib/` | `useChanged` moved into `mcpBridge.ts` |
| `scripts/` | `scripts/` | release scripts still to be fitted (stage 7) |

## Patterns from VideoSOS

| Here | There (`timoncool/videosos` at `106272591abd7d29193145b82b5528108f3db1a8`) |
|---|---|
| the mode switch of `frontend/src/components/shell/TopBar.tsx` | `frontend/src/features/editor/components/workspace-switcher.tsx` |
