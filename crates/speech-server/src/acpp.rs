//! The studio's audiocpp_server (audio.cpp, through Dub Studio's crate `acpp-server`: `SERVERS`, one process per
//! backend): CrisperWhisper recognition and Higgs Audio v3 voicing, with the models on disk now. The same server and
//! layout as Dub Studio's, so a model of one program serves the other.

use acpp_server::{AcppError, Backend, Flow, ModelEntry, Server, ServerConfig, SpeechRequest, VoiceRef};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const CRISPERWHISPER: &str = "crisperwhisper";
pub const HIGGS: &str = "higgs";

/// The CrisperWhisper folder under the models root (the Hugging Face repository as it is; audio.cpp reads the folder).
pub const CRISPERWHISPER_DIR: &str = "crisperwhisper2-turbo";

/// How long a start may take before the server answers `/health` (it loads no model at start).
const READY_WITHIN: Duration = Duration::from_secs(60);
/// The longest wait for the next piece of a phrase; the first one also waits for Higgs to load.
const SPEECH_STALL: Duration = Duration::from_secs(120);

/// The server program: env SPEECH_STUDIO_AUDIOCPP_SERVER, else `tools/audiocpp/audiocpp_server`.
pub fn exe(repo_root: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("SPEECH_STUDIO_AUDIOCPP_SERVER") {
        return PathBuf::from(p);
    }
    let name = if cfg!(windows) { "audiocpp_server.exe" } else { "audiocpp_server" };
    repo_root.join("tools").join("audiocpp").join(name)
}

pub fn backend(stage_backend: &str) -> Backend {
    if stage_backend == "cpu" { Backend::Cpu } else { Backend::Cuda }
}

/// The CrisperWhisper folder: env SPEECH_STUDIO_CRISPERWHISPER (a copy kept elsewhere, read only), else
/// `<models>/crisperwhisper2-turbo`.
pub fn crisperwhisper_dir(models_root: &Path) -> PathBuf {
    std::env::var_os("SPEECH_STUDIO_CRISPERWHISPER").map(PathBuf::from).unwrap_or_else(|| models_root.join(CRISPERWHISPER_DIR))
}

/// Whether CrisperWhisper is downloaded (the Hugging Face repository folder as it is).
pub fn crisperwhisper_installed(models_root: &Path) -> bool {
    crisperwhisper_dir(models_root).join("model.safetensors").is_file()
}

/// The Higgs GGUF of the quantization the settings pick.
pub fn higgs_gguf(models_root: &Path) -> PathBuf {
    let (dir, quant) = crate::models::resolve_tts(models_root, &crate::models::load_selection(models_root));
    dir.join(format!("{quant}.gguf"))
}

/// Every model the server can carry now. A model whose files are not on disk stays out: the server reads each
/// configured model's contract when it starts, and one missing file would stop all of them.
fn models(models_root: &Path) -> Vec<ModelEntry> {
    let mut out = Vec::new();
    if crisperwhisper_installed(models_root) {
        out.push(ModelEntry::crisperwhisper(CRISPERWHISPER, &crisperwhisper_dir(models_root)));
    }
    let higgs = higgs_gguf(models_root);
    if higgs.is_file() {
        out.push(ModelEntry::higgs_tts(HIGGS, &higgs));
    }
    out
}

fn config(repo_root: &Path, models_root: &Path, backend: Backend) -> ServerConfig {
    let name = match backend {
        Backend::Cuda => "cuda",
        Backend::Cpu => "cpu",
    };
    ServerConfig {
        exe: exe(repo_root),
        backend,
        device: 0,
        threads: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(8).min(16),
        models: models(models_root),
        data_dir: repo_root.join("logs").join(format!("audiocpp_server-{name}")),
        log: false,
    }
}

/// The running server of this backend carrying `model`: the one there is, or a new one in its place when the models
/// on disk changed or the process ended. An error when `model` is not on disk or the program is missing.
pub fn server(repo_root: &Path, models_root: &Path, backend: Backend, model: &str) -> Result<Arc<Server>, String> {
    let config = config(repo_root, models_root, backend);
    if !config.exe.is_file() {
        return Err(t!("acpp-no-server", path = config.exe.display().to_string()));
    }
    if !config.models.iter().any(|m| m.id == model) {
        return Err(t!("acpp-no-model", model = model.to_string()));
    }
    let server = acpp_server::SERVERS.get(&config, READY_WITHIN, &|_| {}).map_err(|e| localize(&e))?;
    tracing::info!("audiocpp_server {} ({backend:?}) carries {:?}", server.pid(), config.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>());
    Ok(server)
}

/// Frees the memory of these models on every running server; their next request loads them again.
pub fn unload(ids: &[&str]) {
    for (pid, e) in acpp_server::SERVERS.unload(ids) {
        tracing::error!("[ERROR] audiocpp_server {pid} did not unload {ids:?}: {e}");
    }
}

/// The server's error in the window's language; a cancel is the job's own cancel.
pub fn localize(e: &AcppError) -> String {
    match e {
        AcppError::Cancelled => crate::jobs::CANCELLED.to_string(),
        other => t!("acpp-error", error = other.to_string()),
    }
}

/// Higgs Audio v3 on the server; dropping it frees its memory there.
pub struct Higgs {
    server: Arc<Server>,
    rate: u32,
    cancel: AtomicBool,
}

impl Higgs {
    /// On the card when it can run CUDA 13, else on the processor.
    pub fn load(repo_root: &Path, models_root: &Path) -> Result<Higgs, String> {
        let gguf = higgs_gguf(models_root);
        if !gguf.is_file() {
            return Err(t!("render-higgs-model-missing", path = gguf.display().to_string()));
        }
        let backend = if crate::hw::gpu_report().cuda13_ok { Backend::Cuda } else { Backend::Cpu };
        let server = server(repo_root, models_root, backend, HIGGS)?;
        let rate = server.speech_rate(HIGGS).map_err(|e| localize(&e))?;
        Ok(Higgs { server, rate, cancel: AtomicBool::new(false) })
    }

    pub fn backend(&self) -> Backend {
        self.server.config().backend
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    /// Stops the phrase being spoken at its next piece.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// One phrase, every piece to `piece` as it comes; the whole phrase in the answer. Without a reference the
    /// model speaks in its own voice.
    pub fn speak(&self, text: &str, reference: Option<&Path>, reference_text: Option<&str>, options: Map<String, Value>, piece: &mut dyn FnMut(&[f32])) -> Result<Vec<f32>, String> {
        self.cancel.store(false, Ordering::SeqCst);
        let req = SpeechRequest {
            model: HIGGS.to_string(),
            input: text.to_string(),
            voice_ref: reference.map(|p| VoiceRef::Path(p.to_path_buf())),
            reference_text: reference_text.filter(|t| !t.trim().is_empty()).map(str::to_string),
            options,
            stall: SPEECH_STALL,
        };
        let mut all = Vec::new();
        self.server
            .speech_stream(&req, &self.cancel, &mut |d| {
                piece(d);
                all.extend_from_slice(d);
                Flow::Continue
            })
            .map_err(|e| localize(&e))?;
        Ok(all)
    }
}

impl Drop for Higgs {
    fn drop(&mut self) {
        unload(&[HIGGS]);
    }
}
