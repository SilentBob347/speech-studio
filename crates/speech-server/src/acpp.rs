//! The studio's audiocpp_server (audio.cpp, through Dub Studio's crate `acpp-server`: `SERVERS`, one process per
//! backend): CrisperWhisper recognition, with the models on disk now. The same server and layout as Dub Studio's, so a
//! model of one program serves the other.

use acpp_server::{AcppError, Backend, ModelEntry, Server, ServerConfig};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const CRISPERWHISPER: &str = "crisperwhisper";

/// The CrisperWhisper folder under the models root (the Hugging Face repository as it is; audio.cpp reads the folder).
pub const CRISPERWHISPER_DIR: &str = "crisperwhisper2-turbo";

/// How long a start may take before the server answers `/health` (it loads no model at start).
const READY_WITHIN: Duration = Duration::from_secs(60);

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

/// Every model the server can carry now. A model whose files are not on disk stays out: the server reads each
/// configured model's contract when it starts, and one missing file would stop all of them.
fn models(models_root: &Path) -> Vec<ModelEntry> {
    let mut out = Vec::new();
    if crisperwhisper_installed(models_root) {
        out.push(ModelEntry::crisperwhisper(CRISPERWHISPER, &crisperwhisper_dir(models_root)));
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

/// The server's error in the window's language.
pub fn localize(e: &AcppError) -> String {
    t!("acpp-error", error = e.to_string())
}
