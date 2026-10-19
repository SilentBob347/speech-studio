//! speech-server — axum-реализация REST/SSE контракта Speech Studio. Цель: SPA (frontend/dist) работает
//! без правок против этого сервера так же, как против backend/app.py.
//!
//! Раунд 1 реализует: раздачу SPA с защитой от traversal, GET /engine/capabilities, POST /projects
//! (multipart-загрузка видео -> workspace/<pid>/), GET /projects/{id}, каркас очереди джоб + SSE
//! GET /jobs/{id}/events. GPU-эндпоинты (analyze/render/preview/patch и т.д.) — каркас на следующие
//! раунды; их карта в docs/PORT-CONTRACT.md.

#[macro_use]
pub mod i18n;
mod asr_util;
mod audio_flags;
mod bench;
mod credentials;
mod denoise;
mod direct_speech;
pub mod diagnostics;
#[cfg(windows)]
mod dll_imports;
mod cloud_voices;
mod downloads;
mod endpoints;
mod guard;
mod hub;
pub use hub::flush_on_exit as flush_hub_on_exit;
pub use models::apply_faces_backend;
mod hw;
mod job_store;
mod jobs;
mod limiter;
mod llm_provider;
mod localize;
mod loudness;
mod media;
mod chat;
mod google_speech;
mod journal;
mod google_tts;
mod subimport;
mod mcp;
mod models;
mod openrouter;
mod presets;
mod qc;
mod read_along;
pub mod process_group;
mod record;
mod ref_pick;
mod secrets_api;
pub mod service;
mod setup;
mod spa;
mod studio_settings;
mod live;
mod supertonic;
mod qwen_tts;
mod tagging;
mod tts_text;
mod tts_trim;
mod speaker_profiles;
mod ssml;
mod say_api;
mod speech_api;
mod speech_jobs;
mod speech_project;
mod speech_text;
mod stress;
mod normalize_ru;
mod pron_dict;
mod prep;
mod dict_api;
mod engines;
mod cloud_tts;
mod speechproj;
mod book_import;
mod book_rebuild;
mod acx;
mod auto_cast;
mod cloud;
mod openai_api;
mod director;
mod director_api;
mod listen_api;
mod listen_jobs;
mod listen_project;
mod transcript_out;
mod tts_host;
mod url_import;
mod vad;
mod voice_api;
mod voice_fx;
mod tracks;
mod listen_watch;
mod hard_corpus;
mod take_fix;
mod listen_live;
mod itn;
mod music_studios;
mod voice_jobs;
mod voice_library;
mod wavio;
mod ytdlp;

use axum::extract::{Path as AxPath, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use dub_core::{EngineOpts, Project};
use futures_util::stream::Stream;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Маршрут прокси приложения — для клиентов вне сервера (апдейтер десктопа).
pub use dub_llm::net;
pub use jobs::JobQueue;


#[derive(Clone)]
pub struct AppState {
    pub repo_root: PathBuf,
    pub workspace: PathBuf,
    pub web_root: Option<PathBuf>,
    pub opts: Arc<EngineOpts>,
    pub jobs: JobQueue,
    /// Каталог TDT-модели ASR (analyze). Env SPEECH_STUDIO_TDT, иначе <models_root>/tdt.
    pub tdt_dir: PathBuf,
    /// Путь к модели диаризации (Nemotron 3 Diarization .onnx). Env SPEECH_STUDIO_SORTFORMER, иначе
    /// <models_root>/nemotron-diar/nemotron3_diar_v3.onnx.
    pub sortformer_onnx: PathBuf,
    /// llama-server(.exe) — сайдкар перевода/vision. Env SPEECH_STUDIO_LLAMA_BIN, иначе <repo>/tools/llama/llama-server(.exe).
    pub llama_bin: PathBuf,
    /// BSRoformer CLI (сепарация). Env SPEECH_STUDIO_BSROFORMER_DIR/<cli>, иначе <repo>/tools/bsroformer/bs_roformer-cli.exe.
    pub bsroformer_cli: PathBuf,
    /// GGUF модель сепарации. Env SPEECH_STUDIO_BSROFORMER_MODEL, иначе <repo>/models/bsroformer/voc_fv6-Q8_0.gguf.
    pub bsroformer_model: PathBuf,
    /// Higgs audiocpp_engine.dll (TTS). Env SPEECH_STUDIO_HIGGS_DLL, иначе <models>/higgs-engine/audiocpp_engine.dll.
    pub higgs_dll: PathBuf,
    /// Каталог весов Higgs (q8_0). Env SPEECH_STUDIO_HIGGS_MODEL, иначе <models>/higgs-q8_0.
    pub higgs_model_root: PathBuf,
    /// Каталог bundled-шрифтов субтитров. Env SPEECH_STUDIO_FONTS_DIR, иначе <repo>/fonts.
    pub fonts_dir: PathBuf,
    /// Корень моделей (для OCR: <root>/ocr/…). Env SPEECH_STUDIO_MODELS_ROOT, иначе <repo>/models.
    pub models_root: PathBuf,
    /// Каталог голосов-паков + записей с микрофона. Env SPEECH_STUDIO_VOICES, иначе <repo>/voices.
    pub voices_dir: PathBuf,
    /// Фоновая закачка компонентов (мимо GPU-очереди джоб; состояние — в /setup/status).
    pub downloads: downloads::Downloads,
    /// Загрузки видео по ссылке (yt-dlp), тоже мимо GPU-очереди; состояние — в /url/fetches.
    pub fetches: url_import::Fetches,
}

/// Корень моделей: env SPEECH_STUDIO_MODELS_ROOT, иначе <repo_root>/models.
pub(crate) fn models_root(repo_root: &Path) -> PathBuf {
    std::env::var("SPEECH_STUDIO_MODELS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("models"))
}

/// Linux: CUDA-библиотеки «Первого запуска» (models/higgs-engine) нужны и самому процессу — CUDA-провайдер
/// onnxruntime связан с cuBLAS и cuRAND без RUNPATH, — а загрузчик читает LD_LIBRARY_PATH только при старте.
/// Если каталога в нём нет, процесс заменяет себя собой же с этим каталогом (exec). Вызывать первым делом при
/// старте, до того как что-то загружено.
pub fn ensure_library_path(repo_root: &Path) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;

        let dir = models_root(repo_root).join("higgs-engine");
        let current = std::env::var_os("LD_LIBRARY_PATH");
        if current.as_ref().is_some_and(|v| std::env::split_paths(v).any(|p| p == dir)) {
            return;
        }
        let mut paths = vec![dir];
        if let Some(v) = &current {
            paths.extend(std::env::split_paths(v));
        }
        let restarted: Result<(), String> = std::env::join_paths(paths).map_err(|e| e.to_string()).and_then(|joined| {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            Err(std::process::Command::new(exe).args(std::env::args_os().skip(1)).env("LD_LIBRARY_PATH", joined).exec().to_string())
        });
        if let Err(e) = restarted {
            eprintln!("[ERROR] the restart with LD_LIBRARY_PATH failed ({e}): onnxruntime will not find the CUDA libraries, recognition runs on the CPU");
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = repo_root;
}

/// Прописать в PATH процесса каталоги скачанных бинарей (ffmpeg, llama, движок Higgs с CUDA/VC-DLL),
/// чтобы `Command::new("ffmpeg")` и подобные находили их без перезапуска после автозакачки. Вызывается
/// один раз при старте сервера (main.rs / Tauri-shell). Идемпотентно: не дублирует уже присутствующие.
pub fn augment_path_for_tools(repo_root: &Path) {
    let dirs = [
        repo_root.join("tools").join("ffmpeg"),
        repo_root.join("tools").join("llama"),
        repo_root.join("models").join("higgs-engine"),
    ];
    let sep = if cfg!(windows) { ";" } else { ":" };
    let cur = std::env::var("PATH").unwrap_or_default();
    let mut prefix = String::new();
    for d in dirs {
        let s = d.to_string_lossy().to_string();
        if !s.is_empty() && !cur.split(sep).any(|p| p == s) && !prefix.split(sep).any(|p| p == s) {
            if !prefix.is_empty() {
                prefix.push_str(sep);
            }
            prefix.push_str(&s);
        }
    }
    if !prefix.is_empty() {
        std::env::set_var("PATH", format!("{prefix}{sep}{cur}"));
    }
}

/// Прописать ORT_DYLIB_PATH (onnxruntime 1.28) в окружение процесса до старта сервера: ort грузит dylib лениво,
/// а по голому имени Windows нашла бы раньше PATH onnxruntime.dll другой версии из System32 (Windows AI).
/// GPU-сборка (cuda13) приоритетнее — суперсет CPU+CUDA; `beside_exe` — копия рядом с exe (портатив).
pub fn set_ort_dylib_env(repo_root: &Path, beside_exe: &Path) {
    if std::env::var_os("ORT_DYLIB_PATH").is_some() {
        return;
    }
    let rt = repo_root.join("models").join("runtime");
    let found = dub_core::runtime::ort_candidates(&repo_root.join("models"))
        .into_iter()
        .chain([
            beside_exe.join(dub_core::runtime::ORT_LIBRARY),
            rt.join("onnxruntime-1.28.dll"),
            rt.join("onnxruntime.dll"),
        ])
    .find(|cand| cand.is_file());
    match found {
        Some(cand) => std::env::set_var("ORT_DYLIB_PATH", cand),
        None => eprintln!("[ERROR] onnxruntime 1.28 not found in {}: recognition and diarization will not start until the ONNX Runtime component is downloaded", rt.display()),
    }
}

/// ONNX Runtime, скачанный задачей, — CUDA-сборка: путь на неё, пока процесс ещё не загрузил библиотеку (ort
/// грузит её при первой сессии; загруженную до этого CPU-сборку сменит только перезапуск студии).
fn point_ort_at_gpu(repo_root: &Path) {
    if let Some(gpu) = dub_core::runtime::ort_candidates(&repo_root.join("models")).into_iter().find(|c| c.is_file()) {
        std::env::set_var("ORT_DYLIB_PATH", gpu);
    }
}

/// Маршрут прокси из models/active.json для всех HTTP-клиентов приложения (dub_llm::net): вызывать на старте.
/// Сохранение формы прокси перестраивает его сразу, без рестарта.
pub fn init_proxy_route(repo_root: &Path) {
    models::apply_proxy_route(&repo_root.join("models"));
}

/// Поднять axum-сервер БЛОКИРУЮЩЕ на собственном tokio-рантайме. Для встраивания в десктоп-оболочку ОДНИМ
/// процессом (вместо запуска speech-server.exe отдельным subprocess) — вызывать из фонового std::thread.
/// Слушатель уже занят вызывающим (service::claim_port); augment PATH под инструменты делается здесь же.
pub fn serve_blocking(repo_root: impl AsRef<Path>, listener: std::net::TcpListener,
) -> anyhow::Result<()> {
    let root = repo_root.as_ref().to_path_buf();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        augment_path_for_tools(&root);
        init_proxy_route(&root);
        serve(AppState::new(&root), listener).await
    })
}

/// Обслуживать API и SPA на занятом слушателе до остановки рантайма.
pub async fn serve(state: AppState, listener: std::net::TcpListener) -> anyhow::Result<()> {
    listener.set_nonblocking(true)?;
    service::record_port(listener.local_addr()?.port());
    let listener = tokio::net::TcpListener::from_std(listener)?;
    hub::start(&state.repo_root, &state.workspace);
    listen_watch::resume(&state);
    // models another program already has are linked in at start; only what is missing everywhere is downloaded
    let root = state.repo_root.clone();
    tokio::task::spawn_blocking(move || {
        for (id, app) in setup::import_from_neighbors(&root, &hub::neighbors(), false) {
            tracing::info!("model {id} taken from {app}, not downloaded");
        }
    });
    axum::serve(listener, build_router(state)).await?;
    Ok(())
}

impl AppState {
    pub fn new(repo_root: impl AsRef<Path>) -> Self {
        let repo_root = repo_root.as_ref().to_path_buf();
        let workspace = repo_root.join("workspace");
        let _ = std::fs::create_dir_all(&workspace);
        // the language the window last showed, so what the studio says before a window opens is in it too
        match std::fs::read_to_string(repo_root.join(UI_LANG_FILE)) {
            Ok(code) => {
                if let Err(e) = i18n::set_language(code.trim()) {
                    tracing::error!("[ERROR] {UI_LANG_FILE}: {e}");
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::error!("[ERROR] {UI_LANG_FILE}: {e}"),
        }
        let interrupted = job_store::recover(&workspace);
        if interrupted > 0 {
            eprintln!("[jobs] interrupted project jobs: {interrupted} (they can be resumed)");
        }
        let web_root = spa::find_web_root(&repo_root);
        let mroot = models_root(&repo_root);
        match credentials::migrate_legacy_selection(&mroot) {
            Ok(moved) if moved.openrouter_key || moved.proxy_password => {
                tracing::info!("secrets moved from active.json to the secret store: {moved:?}")
            }
            Ok(_) => {}
            Err(e) => tracing::error!("secrets from active.json were not moved to the store: {e:#}"),
        }
        let tdt_dir = std::env::var("SPEECH_STUDIO_TDT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| mroot.join("tdt"));
        let sortformer_onnx = std::env::var("SPEECH_STUDIO_SORTFORMER")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                mroot.join(dub_asr::DIAR_MODEL_DIR).join(dub_asr::DIAR_MODEL_FILE)
            });
        // llama-server: env-override, иначе <repo>/tools/llama/llama-server(.exe) (негитуемый каталог,
        // кладёт установщик/раунд 3). Существование проверяет сама стадия перевода (fail-safe).
        let llama_bin = dub_llm::resolve_llama_bin(&repo_root.join("tools").join("llama"));
        let bsroformer_cli = std::env::var("SPEECH_STUDIO_BSROFORMER_DIR")
            .map(|d| PathBuf::from(d).join(dub_sep::ENGINE_CLI_FILE))
            .unwrap_or_else(|_| dub_sep::engine_dir(&repo_root).join(dub_sep::ENGINE_CLI_FILE));
        let bsroformer_model = dub_sep::model_path(&repo_root);
        let higgs_dll = std::env::var("SPEECH_STUDIO_HIGGS_DLL")
            .map(PathBuf::from)
            .unwrap_or_else(|_| mroot.join("higgs-engine").join(if cfg!(windows) { "audiocpp_engine.dll" } else { "libaudiocpp_engine.so" }));
        let higgs_model_root = std::env::var("SPEECH_STUDIO_HIGGS_MODEL")
            .map(PathBuf::from)
            .unwrap_or_else(|_| mroot.join("higgs-q8_0"));
        let fonts_dir = std::env::var("SPEECH_STUDIO_FONTS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join("fonts"));
        let voices_dir = std::env::var("SPEECH_STUDIO_VOICES")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join("voices"));
        let downloads = downloads::Downloads::open(&repo_root);
        let fetches = url_import::Fetches::open(&repo_root, &workspace);
        AppState {
            repo_root,
            workspace,
            web_root,
            opts: Arc::new(EngineOpts::default()),
            jobs: JobQueue::new(),
            tdt_dir,
            sortformer_onnx,
            llama_bin,
            bsroformer_cli,
            bsroformer_model,
            higgs_dll,
            higgs_model_root,
            fonts_dir,
            models_root: mroot,
            voices_dir,
            downloads,
            fetches,
        }
    }

    fn proj_dir(&self, pid: &str) -> Result<PathBuf, Response> {
        // pid — hex uuid; отсекаем любые сепараторы/`..` до касания ФС (защита от traversal в pid).
        if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err((StatusCode::NOT_FOUND, "project not found").into_response());
        }
        let d = self.workspace.join(pid);
        if !d.exists() {
            return Err((StatusCode::NOT_FOUND, "project not found").into_response());
        }
        Ok(d)
    }

    fn load_project(&self, pid: &str) -> Result<Project, Response> {
        let d = self.proj_dir(pid)?;
        let f = d.join("project.json");
        if !f.is_file() {
            return Err((StatusCode::CONFLICT, "project not analyzed yet").into_response());
        }
        let text = std::fs::read_to_string(&f)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response())?;
        Project::from_json(&text)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response())
    }
}




/// Ответ на неудачную постановку джобы: 409 с id уже идущей джобы того же класса или 500.
pub(crate) fn enqueue_error(e: jobs::EnqueueError) -> Response {
    match e {
        jobs::EnqueueError::Conflict { job_id, kind } => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "job_conflict", "job_id": job_id, "kind": kind.as_str() })),
        )
            .into_response(),
        jobs::EnqueueError::Store(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("job.json: {e}")).into_response()
        }
    }
}

/// Начало тела джобы проекта: убрать недописанные временные файлы прошлого (упавшего) процесса.
fn clean_partials(dir: &Path) {
    let mut n = dub_core::atomic::cleanup_stale(dir) + dub_core::atomic::cleanup_stale(&dir.join("stems"));
    // Джобы идут по одной, поэтому недописанная сепарация на старте джобы — остаток оборванного прогона.
    let stems_part = dub_sep::part_dir(&dir.join("stems"));
    if stems_part.is_dir() {
        match std::fs::remove_dir_all(&stems_part) {
            Ok(()) => n += 1,
            Err(e) => eprintln!("[jobs] {}: {e}", stems_part.display()),
        }
    }
    if n > 0 {
        eprintln!("[jobs] {}: unfinished files removed: {n}", dir.display());
    }
}

pub fn build_router(state: AppState) -> Router {
    journal::open(&state.repo_root);
    tts_host::watch_idle(state.models_root.clone());
    let api = Router::new()
        .route("/health", get(service::health))
        .route("/engine/capabilities", get(capabilities))
        .route(
            "/engine/server/key",
            get(secrets_api::server_key_settings).put(secrets_api::update_server_key).delete(secrets_api::delete_server_key),
        )
        .route(
            "/engine/openrouter/settings",
            get(secrets_api::openrouter_settings)
                .put(secrets_api::update_openrouter_settings)
                .delete(secrets_api::delete_openrouter_settings),
        )
        .route(
            "/engine/google/settings",
            get(secrets_api::google_settings)
                .put(secrets_api::update_google_settings)
                .delete(secrets_api::delete_google_settings),
        )
        .route("/engine/google/models", get(secrets_api::google_models))
        .route("/engine/proxy/settings", get(secrets_api::proxy_settings).put(secrets_api::update_proxy_settings))
        .route("/engine/proxy/test", post(endpoints::proxy_test))
        .route("/engine/select", post(endpoints::select_model))
        .route("/engine/presets", get(endpoints::presets_list))
        .route("/engine/preset", post(endpoints::preset_apply))
        .route("/engine/openrouter/models", get(endpoints::openrouter_models))
        .route("/engine/openrouter/voices", get(endpoints::openrouter_voices))
        .route("/engine/openrouter/verify", post(endpoints::openrouter_verify))
        .route("/engine/openrouter/catalog", get(endpoints::openrouter_catalog))
        .route("/engine/openrouter/catalog/refresh", post(endpoints::openrouter_catalog_refresh))
        .route("/engine/server/models", get(endpoints::server_models))
        .route("/setup/status", get(setup_status))
        .route("/setup/download", post(setup_download))
        .route("/setup/cancel", post(setup_cancel))
        .route("/setup/remove", post(setup_remove))
        .route("/setup/open-models", post(setup_open_models))
        .route("/setup/browse", post(setup_browse))
        .route("/pick-folder", post(pick_folder))
        .route("/setup/import", post(setup_import))
        .route("/hw/snapshot", get(hw_snapshot))
        .route("/record/devices", get(record_devices))
        .route("/record/level", get(record_level))
        .route("/record/start", post(record_start))
        .route("/record/stop", post(record_stop))
        .route("/record/test", post(record_test))
        .route("/record/test/stop", post(record_test_stop))
        .route("/record/test/audio", get(record_test_audio))
        .route("/voices", get(voice_api::list))
        .route("/voices/download-pack", post(voices_download_pack))
        .route("/voices/catalog", get(voices_catalog))
        .route("/voices/sample", get(voice_api::sample))
        .route("/voices/photo", get(voice_api::photo).post(voice_api::set_photo))
        .route("/voices/card", post(voice_api::set_card))
        .route("/voices/transcript", post(voice_api::set_transcript))
        .route("/voices/transcribe", post(voice_api::transcribe))
        .route("/voices/clean", post(voice_api::clean))
        .route("/voices/denoise", post(voice_api::denoise))
        .route("/voices/design", post(voice_api::design))
        .route("/voices/design/sample", get(voice_api::design_sample))
        .route("/voices/design/keep", post(voice_api::design_keep))
        .route("/voices/restore", post(voice_api::restore))
        .route("/voices/import", post(voice_api::import))
        .route("/voices/import-zip", post(voice_api::import_zip_route))
        .route("/voices/export", post(voice_api::export))
        .route("/voices/ref-candidates", post(voice_api::ref_candidates))
        .route("/voices/ref-apply", post(voice_api::ref_apply))
        .route("/voices/get", post(voices_get))
        .route("/voices/rename", post(voice_api::rename))
        .route("/voices/delete", post(voice_api::delete))
        .route("/settings/ui-lang", axum::routing::put(set_ui_lang))
        .route("/app/paths", get(studio_settings::app_paths))
        .route("/url/probe", get(url_import::probe_get).post(url_import::probe_post))
        .route("/url/fetches", get(url_import::fetches_list))
        .route("/url/fetches/{id}", get(url_import::fetch_get).delete(url_import::fetch_forget))
        .route("/url/fetches/{id}/cancel", post(url_import::fetch_cancel))
        .route("/url/fetches/{id}/resume", post(url_import::fetch_resume))
        .route("/url/tool", get(url_import::tool_status))
        .route("/url/tool/update", post(url_import::tool_update))
        .route("/engine/tags", get(speech_api::tags))
        .route("/engines", get(speech_api::engines))
        .route("/text/stress", post(speech_api::stress))
        .route("/text/direct-speech", post(speech_api::direct_speech))
        .route("/text/prepare", post(dict_api::prepare))
        .route("/dictionary", get(dict_api::get_global).put(dict_api::put_global))
        .route("/dictionary/import", post(dict_api::import))
        .route("/projects/{pid}/dictionary-counts", get(dict_api::counts))
        .route("/projects/{pid}/unknown-words", get(dict_api::unknown_words))
        .route("/cloud/catalog", get(cloud::catalog))
        .route("/cloud/catalog/refresh", post(cloud::refresh))
        .route("/cloud/form", get(cloud::form))
        .route("/cloud/estimate", post(cloud::estimate))
        .route("/cloud/keys", get(cloud::keys))
        .route("/cloud/keys/{provider}", axum::routing::put(cloud::set_key).delete(cloud::delete_key))
        .route("/cloud/spend", get(cloud::spend))
        .route("/v1/models", get(openai_api::models))
        .route("/openai/settings", get(openai_api::settings))
        .route("/openai/key", post(openai_api::new_key).delete(openai_api::delete_key))
        .route("/openai/log", get(openai_api::requests).delete(openai_api::clear_requests))
        .route("/v1/audio/speech", post(openai_api::speech))
        .route("/v1/audio/transcriptions", post(openai_api::transcriptions))
        .route("/listen/upload", post(listen_api::upload))
        .route("/listen/from-path", post(listen_api::from_path))
        .route("/separate", post(listen_api::separate_file))
        .route("/listen/record/start", post(listen_api::record_start))
        .route("/listen/record/stop", post(listen_api::record_stop))
        .route("/listen/record/live", get(listen_api::record_live))
        .route("/listen/watch", get(listen_api::watch_status).post(listen_api::watch_start))
        .route("/listen/watch/stop", post(listen_api::watch_stop))
        .route("/listen/{pid}", get(listen_api::get).put(listen_api::put))
        .route("/listen/{pid}/audio", get(listen_api::audio))
        .route("/listen/{pid}/transcribe", post(listen_api::transcribe))
        .route("/listen/{pid}/cut", post(listen_api::cut))
        .route("/listen/{pid}/export", post(listen_api::export))
        .route("/listen/subtitle-rules", get(listen_api::subtitle_rules))
        .route("/listen/{pid}/stats", get(listen_api::stats))
        .route("/listen/{pid}/check", post(listen_api::check))
        .route("/listen/{pid}/aligned", post(listen_api::aligned))
        .route("/listen/{pid}/to-speech", post(listen_api::to_speech))
        .route("/listen/{pid}/voices", post(listen_api::voices))
        .route("/listen/{pid}/speakers/match", post(listen_api::speakers_match))
        .route("/listen/{pid}/speakers/remember", post(listen_api::speaker_remember))
        .route("/speaker-profiles", get(listen_api::speaker_profiles))
        .route("/speaker-profiles/{name}", axum::routing::delete(listen_api::speaker_forget))
        .route("/projects/from_url", post(url_import::create_from_url))
        .route("/projects/podcast", post(director_api::podcast))
        .route("/projects/{pid}/cast", post(director_api::cast))
        .route("/projects", get(speech_api::list).post(speech_api::create))
        .route("/projects/{pid}", get(speech_api::get).put(speech_api::put).delete(speech_api::delete))
        .route("/projects/{pid}/restore", post(speech_api::restore))
        .route("/projects/{pid}/synth", post(speech_api::synth))
        .route("/projects/{pid}/queue", get(speech_api::queue).delete(speech_api::queue_clear))
        .route("/projects/{pid}/google", get(speech_api::google_report))
        .route("/projects/{pid}/queue/{n}", axum::routing::delete(speech_api::queue_remove))
        .route("/history", get(speech_api::history))
        .route("/speech/live", get(live::events))
        .route("/speech/say", post(say_api::start).delete(say_api::stop_all))
        .route("/speech/say/{session}", get(say_api::state).post(say_api::text).delete(say_api::stop))
        .route("/speech/listen", post(say_api::listen_start).delete(say_api::listen_stop))
        .route("/speech/listen/next", get(say_api::listen_next))
        .route("/speech/listen/mute", post(say_api::listen_mute))
        .route("/speech/say/voice", get(say_api::voice_get).post(say_api::voice_set))
        .route("/speech/listen/text", post(say_api::listen_text))
        .route("/projects/{pid}/export", post(speech_api::export))
        .route("/projects/{pid}/blocks/{bid}/ref", post(speech_api::own_ref))
        .route("/projects/{pid}/blocks/{bid}/ab", post(speech_api::ab))
        .route("/projects/{pid}/blocks/{bid}/fix", post(speech_api::fix))
        .route("/projects/{pid}/enrich", post(director_api::enrich))
        .route("/projects/{pid}/package", post(speech_api::package))
        .route("/speechproj/import", post(speech_api::import_package))
        .route("/projects/from-subtitles", post(speech_api::from_subtitles))
        .route("/books/preview", post(speech_api::book_preview))
        .route("/books/{id}/chapters/{index}", get(speech_api::book_chapter))
        .route("/projects/from-book", post(speech_api::from_book))
        .route("/projects/{pid}/export-check", get(speech_api::export_check))
        .route("/projects/{pid}/quality", get(speech_api::quality))
        .route("/projects/{pid}/check", post(speech_api::check))
        .route("/projects/{pid}/lint", get(speech_api::lint))
        .route("/projects/{pid}/auto-cast", get(speech_api::auto_cast))
        .route("/projects/{pid}/timeline", get(speech_api::timeline))
        .route("/projects/{pid}/save-export", post(speech_api::save_export))
        .route("/projects/{pid}/takes/{file}", get(speech_api::take_file))
        .route("/projects/{pid}/takes/{file}/peaks", get(speech_api::take_peaks))
        .route("/projects/{pid}/media/{file}", get(speech_api::media_file))
        .route("/projects/{pid}/media/{file}/peaks", get(speech_api::media_peaks))
        .route("/projects/{pid}/tracks/import", post(speech_api::tracks_import))
        .route("/projects/{pid}/tracks/upload", post(speech_api::tracks_upload))
        .route("/projects/{pid}/tracks/music", post(speech_api::tracks_music))
        .route("/music/studios", get(speech_api::music_studios))
        .route("/projects/{pid}/dictionary/try", post(speech_api::try_rule))
        .route("/projects/{pid}/tries/{file}", get(speech_api::try_file))
        .route("/projects/{pid}/exports/{file}", get(speech_api::export_file))
        .route("/projects/{pid}/exports", get(speech_api::exports_list))
        .route("/renders", get(speech_api::renders))
        .route("/projects/{pid}/exports-to-folder", post(speech_api::exports_to_folder))
        .route("/projects/{pid}/exports/{file}/reveal", post(speech_api::export_reveal))
        .route("/projects/{pid}/exports/{file}/save-as", post(speech_api::export_save_as))
        .route("/chat", post(chat::chat))
        .route("/chat/models", get(chat::models))
        .route("/diagnostics", get(diagnostics::get))
        .route("/diagnostics/report", post(diagnostics::make_report))
        .route("/diagnostics/report/reveal", post(diagnostics::reveal_report))
        .route("/diagnostics/logs/reveal", post(diagnostics::reveal_logs))
        .route("/journal", get(journal::read).post(journal::write).delete(journal::clear))
        .route("/journal/{id}", axum::routing::delete(journal::remove))
        .route("/jobs", get(jobs_list))
        .route("/jobs/{job_id}", get(job_get))
        .route("/jobs/{job_id}/cancel", post(job_cancel))
        .route("/jobs/{job_id}/events", get(job_events))
        .fallback(spa_fallback)
        .layer(axum::extract::DefaultBodyLimit::disable())
        .layer(axum::middleware::from_fn(mcp::track))
        .merge(hub::router())
        .with_state(state);
    // counted before the MCP tools take the router, so an agent's calls count as the features they use
    let api = hub::track(api);
    mcp::install(api.clone());
    api.route("/mcp", post(mcp::handle))
        .route("/mcp/status", get(mcp::status))
        .route("/mcp/window", get(mcp::window_events))
        .route("/mcp/window/result", post(mcp::window_result).layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024)))
        .route("/mcp/window/focus", post(mcp::window_focus))
        .route("/mcp/agent-llm/v1/chat/completions", post(mcp::agent_llm))
        .layer(guard::cors())
        .layer(axum::middleware::from_fn(guard::origin_guard))
}

// ─── /engine/capabilities ───────────────────────────────────────────────────

async fn capabilities(State(st): State<AppState>) -> Json<Value> {
    let o = &st.opts;
    let ffmpeg = which_ffmpeg();
    // Текущий выбор вариантов (active.json) — фронт рисует по нему селекторы движка/модели/кванта ASR.
    // Без секретов: вместо ключа OpenRouter и пароля прокси — флаги or_key_set / proxy_password_set.
    let sel = models::public_selection(&st.models_root);
    // Языки контента = полный набор Whisper (99). Единый источник — dub_translate::WHISPER_LANGS.
    let langs: Vec<&str> = dub_translate::WHISPER_LANGS.iter().map(|(c, _)| *c).collect();
    Json(json!({
        "device": o.device,
        "tts_quant": o.tts_quant,
        "asr_model": o.asr_model,
        "models": {
            "asr": o.asr_model,
            "llm": o.mt_model_path.to_string_lossy(),
            "vision": o.mmproj_path.to_string_lossy(),
            "tts": o.tts_model,
        },
        "ffmpeg": ffmpeg,
        "languages": langs,
        "modes": ["speech","listening"],
        // Выбор ASR: движок (parakeet|nemotron|whisper), модель Whisper, квант Whisper (compute_type).
        "selection": sel,
        "asr_engines": ["parakeet","nemotron","whisper"],
        "whisper_models": ["tiny","base","small","medium","large-v3","large-v3-turbo"],
        // Кванты Whisper (compute_type): float16 / int8_float16 задействуют Tensor Cores на CUDA GPU.
        "whisper_computes": ["int8","int8_float16","float16","int8_float32","float32"],
        // Видимые лимиты RAM (настройки, не авто-магия): против OOM на слабой памяти. "0" = авто (дефолт).
        "llama_ubatches": ["0","512","256","128"],      // prefill-батч Gemma (меньше = меньше RAM)
        "higgs_ref_secs_opts": ["12","8","6","4"],       // длина реф-клипа клона (сек; <12 спасает 32ГБ)
    }))
}

fn which_ffmpeg() -> bool {
    setup::ffmpeg_on_path()
}

// ─── /setup/status ; /setup/download ; /setup/cancel ; /setup/remove ; /setup/open-models ───────────

/// Ответ на отказ закачки/удаления: HTTP-статус по коду + {code, detail}.
fn dl_error_response(e: setup::DlError) -> Response {
    let status = match e.code {
        "busy" => StatusCode::CONFLICT,
        "disk_space" => StatusCode::INSUFFICIENT_STORAGE,
        "nothing_to_download" | "unknown_component" | "not_removable" | "no_ids" => {
            StatusCode::BAD_REQUEST
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({ "code": e.code, "detail": e.detail }))).into_response()
}

fn join_failure(e: tokio::task::JoinError) -> Response {
    internal_error(e.to_string())
}

fn body_ids(body: &Value) -> Vec<String> {
    body.get("ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()
        })
        .unwrap_or_default()
}

/// Полный статус «Первого запуска» с фоновой закачкой (ФС-обход и первая проба драйвера — в блокирующем пуле).
/// Ошибка — текст для ответа 500 (internal_error).
async fn full_setup_status(st: &AppState) -> Result<Value, String> {
    let root = st.repo_root.clone();
    let mut status = tokio::task::spawn_blocking(move || setup::setup_status(&root))
        .await
        .map_err(|e| e.to_string())?;
    status.active = st.downloads.active();
    serde_json::to_value(status).map_err(|e| e.to_string())
}

fn internal_error(detail: String) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "code": "internal", "detail": detail })),
    ).into_response()
}

/// GET /setup/status — статус каждого компонента (installed/missing/размер/место), папка моделей и свободное
/// место, видеокарта против CUDA 13 и фоновая закачка (active). Читает только диск — безопасно до закачки.
async fn setup_status(State(st): State<AppState>) -> Response {
    match full_setup_status(&st).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal_error(e),
    }
}

/// POST /setup/download {ids:[...]} — запустить фоновую закачку (мимо GPU-очереди): {download}. Идущая
/// закачка — 409 busy, нехватка места — 507 disk_space. Прогресс — в GET /setup/status (active).
async fn setup_download(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let ids = body_ids(&body);
    if ids.is_empty() {
        return dl_error_response(setup::DlError::new("no_ids", t!("setup-no-ids")));
    }
    let dl = st.downloads.clone();
    match tokio::task::spawn_blocking(move || dl.start(ids)).await {
        Ok(Ok(job)) => Json(json!({ "download": job })).into_response(),
        Ok(Err(e)) => dl_error_response(e),
        Err(e) => join_failure(e),
    }
}

/// POST /setup/cancel — пауза фоновой закачки: скачанное остаётся и докачивается следующим /setup/download.
async fn setup_cancel(State(st): State<AppState>) -> Json<Value> {
    Json(json!({ "paused": st.downloads.pause() }))
}

/// POST /setup/remove {ids:[...]} — удалить скачанные компоненты: {removed, freedBytes, errors, status}.
async fn setup_remove(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let ids = body_ids(&body);
    if ids.is_empty() {
        return dl_error_response(setup::DlError::new("no_ids", t!("setup-no-ids")));
    }
    let root = st.repo_root.clone();
    let dl = st.downloads.clone();
    let res = tokio::task::spawn_blocking(move || {
        let mut report = setup::remove_components(&root, &ids)?;
        dl.forget_covering(&ids);
        if ids.iter().any(|id| id == ytdlp::COMPONENT) {
            report.errors.extend(ytdlp::remove_updates(&root));
        }
        Ok::<_, setup::DlError>(report)
    })
    .await;
    let report = match res {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return dl_error_response(e),
        Err(e) => return join_failure(e),
    };
    let status = match full_setup_status(&st).await {
        Ok(v) => v,
        Err(e) => return internal_error(e),
    };
    Json(json!({ "removed": report.removed, "freedBytes": report.freed_bytes, "errors": report.errors, "status": status }))
        .into_response()
}

/// Открыть каталог в проводнике (без выделения файла).
fn open_folder(dir: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let cmd = "explorer.exe";
    #[cfg(not(windows))]
    let cmd = "xdg-open";
    std::process::Command::new(cmd).arg(dir).spawn().map(|_| ())
}

/// POST /setup/open-models — открыть папку моделей в проводнике: {path}.
async fn setup_open_models(State(st): State<AppState>) -> Response {
    let dir = st.repo_root.join("models");
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| open_folder(&dir)) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "code": "open_failed", "detail": format!("{}: {e}", dir.display()) })),
        )
            .into_response();
    }
    Json(json!({ "path": dir.to_string_lossy() })).into_response()
}

/// Перед задачей догружаются компоненты, нужные именно ей, если их нет на диске (прогресс — в тот же SSE):
/// проверка похожести голоса — WeSpeaker, GPU-стадия без CUDA-стека — cuFFT, onnxruntime-GPU, cuDNN.
/// Докачать в задаче недостающие компоненты из списка (отмена задачи прерывает закачку).
pub(crate) fn ensure_components(repo_root: &Path, ids: &[&str], progress: &setup::ProgressCb) -> Result<(), String> {
    let manifest = setup::manifest();
    let need: Vec<String> = ids
        .iter()
        .filter(|id| manifest.iter().find(|c| c.id == **id).is_some_and(|c| !setup::component_status(repo_root, c).installed))
        .map(|id| id.to_string())
        .collect();
    if need.is_empty() {
        return Ok(());
    }
    progress(json!({ "stage": "download", "msg": t!("setup-fetching-missing") }));
    let ctl = jobs::current();
    let cancel = move || ctl.as_ref().is_some_and(|c| c.is_cancelled());
    setup::download_components(repo_root, &need, &cancel, progress).map(|_| ()).map_err(|e| e.detail)
}

/// Для своей Gemma: llama-server и модель «Первого запуска», если их нет; сервер пользователя и OpenRouter ничего
/// не скачивают.
pub(crate) fn ensure_llm_components(repo_root: &Path, models_root: &Path, progress: &setup::ProgressCb) -> Result<(), String> {
    if models::llm_backend(models_root, "llm") != models::LlmBackend::Local {
        return Ok(());
    }
    let manifest = setup::manifest();
    let missing = |id: &str| manifest.iter().find(|c| c.id == id).is_some_and(|c| !setup::component_status(repo_root, c).installed);
    let (model, _) = models::resolve_mt(models_root, &models::load_selection(models_root));
    let mut need: Vec<String> = Vec::new();
    if missing("llama") {
        need.push("llama".to_string());
    }
    if !model.is_file() && missing("gemma") {
        need.push("gemma".to_string());
    }
    if need.is_empty() {
        return Ok(());
    }
    progress(json!({ "stage": "download", "msg": t!("setup-fetching-missing") }));
    let ctl = jobs::current();
    let cancel = move || ctl.as_ref().is_some_and(|c| c.is_cancelled());
    setup::download_components(repo_root, &need, &cancel, progress).map_err(|e| e.detail)?;
    augment_path_for_tools(repo_root);
    Ok(())
}

pub(crate) fn ensure_job_components(
    repo_root: &Path,
    models_root: &Path,
    similarity: bool,
    diarization: bool,
    progress: &setup::ProgressCb,
) -> Result<(), String> {
    let manifest = setup::manifest();
    let missing = |id: &str| -> bool {
        manifest
            .iter()
            .find(|c| c.id == id)
            .map(|c| !setup::component_status(repo_root, c).installed)
            .unwrap_or(false)
    };
    let mut need: Vec<String> = Vec::new();
    if similarity && missing("wespeaker") {
        need.push("wespeaker".to_string());
    }
    // Любая локальная стадия на GPU → полный CUDA-стек: диаризация/ASR грузят onnxruntime-GPU + cuDNN,
    // ggml-движки (сепарация/Higgs) — cuda-runtime (cudart/cublas/cuFFT). Без cuFFT CUDA-EP не поднимется.
    let gpu = ["sep_backend", "diar_backend", "asr_backend"]
        .iter()
        .any(|k| models::stage_backend(models_root, k) == "gpu");
    if gpu {
        for id in ["cuda-runtime", "onnxruntime-gpu", "cudnn", "bsroformer-engine",
        ] {
            if missing(id) {
                need.push(id.to_string());
            }
        }
    }
    // Whisper на GPU: CTranslate2 требует cuBLAS12 + cuDNN8 РЯДОМ с whisper-faster.exe (наш ONNX-стек
    // выше — CUDA 13, whisper'у не подходит). Без них whisper молча идёт на CPU. Тянем их только когда
    // движок ASR = Whisper И стадия ASR на GPU.
    let asr_whisper = models::load_selection(models_root)
        .get("asr_engine")
        .and_then(|v| v.as_str())
        == Some("whisper");
    if asr_whisper && models::stage_backend(models_root, "asr_backend") == "gpu" && missing("whisper-cuda") {
        need.push("whisper-cuda".to_string());
    }
    // Диаризация (дубляж/транскрипт) → модель Nemotron 3 Diarization. Её сбой закачки не валит джобу:
    // анализ штатно деградирует в single-speaker и пишет об этом в журнал (см. analyze::run).
    let need_diar = diarization && missing("sortformer");
    if need.is_empty() && !need_diar {
        return Ok(());
    }
    progress(json!({ "stage": "download", "msg": t!("setup-fetching-missing") }),
    );
    let ctl = jobs::current();
    let cancel = move || ctl.as_ref().is_some_and(|c| c.is_cancelled());
    if !need.is_empty() {
        setup::download_components(repo_root, &need, &cancel, progress).map_err(|e| e.detail)?;
        if need.iter().any(|id| id == "onnxruntime-gpu") {
            point_ort_at_gpu(repo_root);
        }
    }
    if need_diar {
        if let Err(e) = setup::download_components(repo_root, &["sortformer".to_string()], &cancel, progress) {
            progress(json!({
                "stage": "download",
                "msg": t!("setup-diarization-missing", error = e.detail.clone()),
            }));
        }
    }
    Ok(())
}

/// GET /hw/snapshot — снимок GPU/VRAM/темп/мощность + RAM для монитора ресурсов.
async fn hw_snapshot() -> Json<hw::HardwareSnapshot> {
    Json(tokio::task::spawn_blocking(hw::snapshot).await.unwrap_or_default(),
    )
}

/// Безопасное имя голоса: только буквы/цифры/пробел/дефис/подчёркивание (без разделителей путей).
/// GET /record/devices — список микрофонов.
async fn record_devices() -> Json<Value> {
    Json(json!({ "devices": tokio::task::spawn_blocking(record::input_devices).await.unwrap_or_default() }),
    )
}

/// GET /record/level — текущий пик 0..1 (метр уровня).
async fn record_level() -> Json<Value> {
    Json(json!({ "level": record::level() }))
}

/// Where a microphone test is recorded: one file, each test replaces the last.
fn mic_test_path(st: &AppState) -> PathBuf {
    st.workspace.join(".mic-test.wav")
}

/// POST /record/test {device?} — a microphone test, as call apps have one: records until /record/test/stop while
/// /record/level shows how loud it hears; the recording is then played back to the person.
async fn record_test(State(st): State<AppState>, body: Option<Json<Value>>) -> Response {
    let device = body.as_ref().and_then(|Json(v)| v.get("device")).and_then(Value::as_str).filter(|d| !d.is_empty()).map(str::to_string);
    let path = mic_test_path(&st);
    match tokio::task::spawn_blocking(move || record::start(&path, device)).await {
        Ok(Ok(())) => Json(json!({ "testing": true })).into_response(),
        Ok(Err(e)) => (StatusCode::CONFLICT, Json(json!({ "error": "record_busy", "detail": e }))).into_response(),
        Err(e) => join_failure(e),
    }
}

/// POST /record/test/stop — ends the microphone test: how long it heard and how loud at most; the file to play back.
async fn record_test_stop(State(st): State<AppState>) -> Response {
    let path = match tokio::task::spawn_blocking(record::stop).await {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "record_failed", "detail": e }))).into_response(),
        Err(e) => return join_failure(e),
    };
    if path != mic_test_path(&st) {
        return (StatusCode::CONFLICT, Json(json!({ "error": "not_a_test", "detail": t!("record-test-other") }))).into_response();
    }
    let (seconds, peak) = match hound::WavReader::open(&path) {
        Ok(mut r) => {
            let spec = r.spec();
            let samples: Vec<f32> = r.samples::<i16>().filter_map(Result::ok).map(|s| s as f32 / i16::MAX as f32).collect();
            let seconds = samples.len() as f64 / (spec.sample_rate as f64 * spec.channels as f64);
            (seconds, samples.iter().fold(0.0f32, |m, s| m.max(s.abs())))
        }
        Err(e) => return internal_error(format!("{}: {e}", path.display())),
    };
    Json(json!({ "path": path.display().to_string(), "seconds": (seconds * 100.0).round() / 100.0, "peak": (peak * 1000.0).round() / 1000.0 })).into_response()
}

/// GET /record/test/audio — the last microphone test's recording.
async fn record_test_audio(State(st): State<AppState>, req: axum::http::Request<axum::body::Body>) -> Response {
    let path = mic_test_path(&st);
    if !path.is_file() {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "no_file", "detail": t!("record-test-none") }))).into_response();
    }
    serve_file_range(&path, req, None).await
}

/// POST /record/start {name, device?} — начать запись голоса в новый файл voices/<имя>.wav (занятое имя получает номер).
async fn record_start(State(st): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let base = voice_library::sanitize(body.get("name").and_then(|v| v.as_str()).unwrap_or(""));
    let base = if base.is_empty() { t!("voice-default-name") } else { base };
    let device = body.get("device").and_then(|v| v.as_str()).map(|s| s.to_string());
    if let Err(e) = std::fs::create_dir_all(&st.voices_dir) {
        return Json(json!({ "ok": false, "error": format!("{}: {e}", st.voices_dir.display()) }));
    }
    let name = voice_library::free_name(&st.voices_dir, &base);
    let path = st.voices_dir.join(format!("{name}.wav"));
    match tokio::task::spawn_blocking(move || record::start(&path, device)).await {
        Ok(Ok(())) => Json(json!({ "ok": true, "name": name })),
        Ok(Err(e)) => Json(json!({ "ok": false, "error": e })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// POST /record/stop {clean?, transcribe?} — остановить запись: голос получает карточку (свой голос, с
/// микрофона) и задачу очистки и транскрипта рефа. Ответ — имя, задача и библиотека.
async fn record_stop(State(st): State<AppState>, body: Option<Json<Value>>) -> Response {
    let body = body.map(|Json(v)| v).unwrap_or(Value::Null);
    let path = match tokio::task::spawn_blocking(record::stop).await {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "record_failed", "detail": e }))).into_response(),
        Err(e) => return join_failure(e),
    };
    let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
        return internal_error(format!("{}: no name", path.display()));
    };
    let card = voice_library::VoiceCard { source: "mic".into(), consent: "own".into(), ..Default::default() };
    if let Err(e) = voice_library::write_card(&st.voices_dir, &name, &card) {
        return internal_error(e);
    }
    let clean = body.get("clean").and_then(Value::as_bool).unwrap_or_else(|| models::voice_clean_default(&st.models_root));
    let transcribe = body.get("transcribe").and_then(Value::as_bool).unwrap_or(true);
    let job = if clean {
        Some(voice_api::enqueue(&st, jobs::JobKind::VoiceClean, name.clone(), transcribe).await)
    } else if transcribe {
        Some(voice_api::enqueue(&st, jobs::JobKind::VoiceTranscript, name.clone(), false).await)
    } else {
        None
    };
    let job = match job {
        Some(r) if !r.status().is_success() => return r,
        Some(r) => {
            let bytes = axum::body::to_bytes(r.into_body(), 1 << 16).await.unwrap_or_default();
            serde_json::from_slice::<Value>(&bytes).ok().and_then(|v| v.get("job_id").cloned())
        }
        None => None,
    };
    mcp::tell_windows(json!({ "changed": "voices" }));
    let mut out = voice_api::listing(&st.voices_dir);
    out["name"] = json!(name);
    out["job_id"] = job.unwrap_or(Value::Null);
    Json(out).into_response()
}



// ─── Кастинг персонажей (#115) ──────────────────────────────────────────────






// ─── App-библиотека кастингов (#115): профили, переносимые между роликами ─────






/// GET /voices/catalog — каталог доп-голосов (HF датасет Slait/russia_voices): имя, пол, URL-превью. Hugging Face
/// не ответил — 502 {error, detail}.
async fn voices_catalog() -> Response {
    match tokio::task::spawn_blocking(record::catalog).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": "catalog_unavailable", "detail": e })),
        ).into_response(),
        Err(e) => join_failure(e),
    }
}

/// POST /voices/get {name} — скачать один голос (mp3+txt) из датасета в каталог голосов.
async fn voices_get(State(st): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let dir = st.voices_dir.clone();
    let fetch_name = name.clone();
    let ok = tokio::task::spawn_blocking(move || record::fetch_voice(&dir, &fetch_name)).await;
    match ok {
        Ok(Ok(())) => {
            let base = name.clone();
            let card = voice_library::VoiceCard { source: "russia_voices".into(), ..voice_library::guess_card(&base) };
            if let Err(e) = voice_library::write_card(&st.voices_dir, &base, &card) {
                return Json(json!({ "ok": false, "error": e }));
            }
            mcp::tell_windows(json!({ "changed": "voices" }));
            Json(json!({ "ok": true, "voices": voice_api::listing(&st.voices_dir)["voices"] }))
        }
        Ok(Err(e)) => Json(json!({ "ok": false, "error": e })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// POST /voices/download-pack — скачать пак голосов (VibeVoice) и распаковать в каталог голосов.
async fn voices_download_pack(State(st): State<AppState>) -> Response {
    let dir = st.voices_dir.clone();
    let job: jobs::JobFn = Box::new(move |progress: jobs::ProgressFn| {
        let cb = |ev: Value| progress(ev);
        record::download_pack(&dir, &cb)
    });
    match st.jobs.enqueue(jobs::JobMeta::new(jobs::JobKind::VoicesPack, None), job).await {
        Ok(job_id) => Json(json!({ "job_id": job_id })).into_response(),
        Err(e) => enqueue_error(e),
    }
}

/// POST /setup/browse — открыть нативный диалог выбора папки и импортировать оттуда готовые веса (без
/// докачки): {picked, imported, files, errors, status}.
async fn setup_browse(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let root = st.repo_root.clone();
    let only = body.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
    let res = tokio::task::spawn_blocking(move || {
        let title = if only.is_some() { t!("setup-pick-model-files") } else { t!("setup-pick-models-folder") };
        rfd::FileDialog::new()
            .set_title(title)
            .pick_folder()
            .map(|d| setup::import_from_dir(&root, &d, only.as_deref(), true, false))
    })
    .await;
    let report = match res {
        Ok(r) => r,
        Err(e) => return join_failure(e),
    };
    let status = match full_setup_status(&st).await {
        Ok(v) => v,
        Err(e) => return internal_error(e),
    };
    let picked = report.is_some();
    let report = report.unwrap_or_default();
    Json(json!({ "picked": picked, "imported": report.imported, "files": report.files, "errors": report.errors, "status": status }))
        .into_response()
}

/// POST /setup/import {path, id?} — импортировать готовые веса из заданной папки (без диалога):
/// {imported, files, errors, status}.
async fn setup_import(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let path = body.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if path.is_empty() || !Path::new(&path).is_dir() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "code": "bad_path", "detail": t!("setup-no-folder", path = path.clone()) })),
        ).into_response();
    }
    let only = body.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
    let root = st.repo_root.clone();
    let report = match tokio::task::spawn_blocking(move || {
        setup::import_from_dir(&root, Path::new(&path), only.as_deref(), true, false)
    }).await {
        Ok(r) => r,
        Err(e) => return join_failure(e),
    };
    let status = match full_setup_status(&st).await {
        Ok(v) => v,
        Err(e) => return internal_error(e),
    };
    Json(json!({ "imported": report.imported, "files": report.files, "errors": report.errors, "status": status })).into_response()
}

// ─── POST /projects (multipart video upload) ────────────────────────────────





// ─── GET /projects/{pid} ────────────────────────────────────────────────────





// ─── POST /projects/{pid}/analyze ───────────────────────────────────────────



// ─── PATCH /projects/{pid} ──────────────────────────────────────────────────


// ─── POST /projects/{pid}/render ────────────────────────────────────────────














// ─── GET /projects/{pid}/output ; /original ; /dub (Range-раздача файла) ─────




/// POST /pick-folder — нативный диалог выбора папки (rfd). Возвращает {dir} или {dir:null} при отмене.
/// Для batch-экспорта: юзер выбирает ОДНУ папку назначения, дальше save-output кладёт туда все файлы.
async fn pick_folder() -> Json<Value> {
    let dir = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new().set_title(t!("export-pick-folder")).pick_folder()
    })
    .await
    .ok()
    .flatten();
    Json(json!({ "dir": dir.map(|d| d.to_string_lossy().into_owned()) }))
}



/// Открыть проводник/файловый менеджер с ВЫДЕЛЕННЫМ файлом (не плеер). Windows: explorer /select.
pub(crate) fn reveal_in_explorer(path: String) {
    tokio::task::spawn_blocking(move || {
        #[cfg(windows)]
        {
            // explorer /select,"<path>" — выделяет файл в открытом каталоге. Один аргумент.
            let _ = process_group::detach_from_group(&mut std::process::Command::new("explorer"))
                .arg(format!("/select,{path}"))
                .spawn();
        }
        #[cfg(not(windows))]
        {
            // прочие ОС: открыть родительский каталог (выделение файла непортабельно).
            let parent = std::path::Path::new(&path).parent().map(|p| p.to_path_buf()).unwrap_or_else(|| std::path::PathBuf::from("."));
            let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
        }
    });
}




/// Отдать файл с поддержкой Range (для <video> seek). Используем tower-http ServeFile — он
/// корректно обрабатывает Range/If-Range/Content-Range. dl -> Content-Disposition attachment.
pub(crate) async fn serve_file_range(
    path: &Path,
    req: axum::http::Request<axum::body::Body>,
    download_name: Option<String>,
) -> Response {
    use tower::ServiceExt;
    use tower_http::services::ServeFile;
    let svc = ServeFile::new(path);
    match svc.oneshot(req).await {
        Ok(mut resp) => {
            if let Some(name) = download_name {
                // RFC 6266: filename — ASCII для старых клиентов, filename* — настоящее имя в UTF-8.
                let ascii: String = name.chars().map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' { c } else { '_' }).collect();
                let utf8 = percent_encoding::utf8_percent_encode(&name, percent_encoding::NON_ALPHANUMERIC);
                if let Ok(v) = axum::http::HeaderValue::from_str(&format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{utf8}")) {
                    resp.headers_mut().insert(axum::http::header::CONTENT_DISPOSITION, v);
                }
            }
            resp.map(axum::body::Body::new)
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// ─── GET /jobs/{job_id}/events (SSE) ────────────────────────────────────────

async fn job_events(
    State(st): State<AppState>,
    AxPath(job_id): AxPath<String>) -> Response {
    let Some(sub) = st.jobs.subscribe(&job_id).await else {
        return (StatusCode::NOT_FOUND, "job not found").into_response();
    };
    Sse::new(sse_stream(sub))
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

// ─── GET /jobs?pid= ; GET /jobs/{id}?wait= ; POST /jobs/{id}/cancel ─────────

/// Активные и недавние джобы (без кадр-джоб). С `pid` — только этого проекта плюс его job.json
/// (последняя джоба проекта, в т.ч. прерванная до рестарта: её можно продолжить).
async fn jobs_list(State(st): State<AppState>, Query(q): Query<HashMap<String, String>>,
) -> Response {
    let pid = q.get("pid").map(String::as_str).filter(|s| !s.is_empty());
    let project_job = match pid {
        Some(pid) => {
            let dir = match st.proj_dir(pid) {
                Ok(d) => d,
                Err(resp) => return resp,
            };
            match job_store::read(&dir) {
                Ok(rec) => serde_json::to_value(rec).unwrap_or(Value::Null),
                Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
            }
        }
        None => Value::Null,
    };
    Json(json!({ "jobs": st.jobs.list(pid).await, "project_job": project_job })).into_response()
}

/// Снапшот джобы {id, kind, pid, status, stage, msg, pct, position?, result|error}. `wait=N` (сек,
/// до jobs::WAIT_LONGEST_SECS) — long-poll: ответ приходит при завершении или по истечении N.
async fn job_get(
    State(st): State<AppState>,
    AxPath(job_id): AxPath<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let wait: u64 = match q.get("wait").map(|v| v.parse::<u64>()) {
        None => 0,
        Some(Ok(v)) => v,
        Some(Err(_)) => {
            return (StatusCode::BAD_REQUEST, t!("jobs-wait-not-number")).into_response()
        }
    };
    let snap = if wait > 0 { st.jobs.wait(&job_id, wait).await } else { st.jobs.snapshot(&job_id).await };
    match snap {
        Some(v) => Json(v).into_response(),
        None => (StatusCode::NOT_FOUND, "job not found").into_response(),
    }
}

async fn job_cancel(State(st): State<AppState>, AxPath(job_id): AxPath<String>) -> Response {
    match st.jobs.cancel(&job_id).await {
        Ok(v) => Json(v).into_response(),
        Err(jobs::CancelError::NotFound) => {
            (StatusCode::NOT_FOUND, "job not found").into_response()
        }
        Err(jobs::CancelError::Finished) => (StatusCode::CONFLICT, Json(json!({ "error": "job_finished", "job_id": job_id })),
        ).into_response(),
    }
}


/// Поток SSE-событий джобы: сначала терминал (если уже завершена) или последнее известное событие,
/// затем живые события до done/error/cancelled. Джоба из истории не удаляется — её может прочитать
/// следующий подписчик или GET /jobs/{id}.
fn sse_stream(sub: jobs::Subscription) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        let jobs::Subscription { first, terminal, mut rx } = sub;
        if let Some(ev) = first {
            yield sse_event(&ev);
            if terminal {
                return;
            }
        }
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let end = jobs::is_terminal_event(&ev);
                    yield sse_event(&ev);
                    if end {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break, // канал закрыт
            }
        }
    }
}

fn sse_event(ev: &Value) -> Result<Event, Infallible> {
    Ok(Event::default().data(serde_json::to_string(ev).unwrap_or_default()))
}





// ─── SPA fallback ───────────────────────────────────────────────────────────

async fn spa_fallback(State(st): State<AppState>, uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    spa::serve_spa(st.web_root.as_deref(), path).await
}

/// Where the window's language is kept between runs, in the data root.
const UI_LANG_FILE: &str = "ui-lang.txt";

/// The language the window shows, reported by the window so the server speaks it too, now and after a restart.
async fn set_ui_lang(State(st): State<AppState>, Json(body): Json<Value>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let lang = body.get("lang").and_then(Value::as_str).unwrap_or_default();
    i18n::set_language(lang).map_err(|detail| (StatusCode::BAD_REQUEST, Json(json!({ "code": "bad_lang", "detail": detail }))))?;
    let file = st.repo_root.join(UI_LANG_FILE);
    std::fs::write(&file, lang).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "code": "write_failed", "detail": format!("{}: {e}", file.display()) }))))?;
    Ok(Json(json!({ "lang": lang })))
}
