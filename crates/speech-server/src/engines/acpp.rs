//! The studio's audiocpp_server (audio.cpp, through Dub Studio's crate `acpp-server`: `SERVERS`, one process per
//! backend): CrisperWhisper recognition and Higgs Audio v3 voicing, with the models on disk now. The same server and
//! layout as Dub Studio's, so a model of one program serves the other.

use crate::engines::hw::CudaBuild;
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

/// The build of the server for this card: CUDA 13, else CUDA 12.9 (Maxwell to Volta, drivers 525-579); a machine
/// whose card runs neither (or that has none) takes the CUDA 13 build for its processor backend.
pub fn build() -> CudaBuild {
    crate::engines::hw::gpu_report().cuda_build.unwrap_or(CudaBuild::Cuda13)
}

/// The folder of a build under the program folder.
pub fn build_dir(build: CudaBuild) -> &'static str {
    match build {
        CudaBuild::Cuda13 => "tools/audiocpp",
        CudaBuild::Cuda12 => "tools/audiocpp-cuda12",
    }
}

/// The CUDA libraries the build's program needs at start and where the runtime component puts them (cudart is
/// linked into the CUDA 13 build); without them Windows refuses to start it (0xC0000135).
pub fn libraries(build: CudaBuild) -> &'static [&'static str] {
    match (build, cfg!(windows)) {
        (CudaBuild::Cuda13, true) => &["models/higgs-engine/cublas64_13.dll", "models/higgs-engine/cublasLt64_13.dll", "models/higgs-engine/cufft64_12.dll"],
        (CudaBuild::Cuda12, true) => &[
            "models/cuda12-runtime/cudart64_12.dll",
            "models/cuda12-runtime/cublas64_12.dll",
            "models/cuda12-runtime/cublasLt64_12.dll",
            "models/cuda12-runtime/cufft64_11.dll",
        ],
        (_, false) => &[],
    }
}

/// The server program: env SPEECH_STUDIO_AUDIOCPP_SERVER, else the program of this card's build.
pub fn exe(repo_root: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("SPEECH_STUDIO_AUDIOCPP_SERVER") {
        return PathBuf::from(p);
    }
    let name = if cfg!(windows) { "audiocpp_server.exe" } else { "audiocpp_server" };
    repo_root.join(build_dir(build())).join(name)
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
    let (dir, quant) = crate::engines::models::resolve_tts(models_root, &crate::engines::models::load_selection(models_root));
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
    if std::env::var_os("SPEECH_STUDIO_AUDIOCPP_SERVER").is_none() {
        let missing: Vec<&str> = libraries(build()).iter().copied().filter(|rel| !repo_root.join(rel).is_file()).collect();
        if !missing.is_empty() {
            return Err(t!("acpp-no-libraries", files = missing.join(", ")));
        }
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
        other => {
            let error = other.to_string();
            if out_of_memory(&error) {
                t!("acpp-out-of-memory", error = error)
            } else {
                t!("acpp-error", error = error)
            }
        }
    }
}

/// ggml names a graph or buffer it could not place in video memory this way.
fn out_of_memory(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    e.contains("failed to allocate") || e.contains("out of memory") || e.contains("cudamalloc")
}

/// Higgs Audio v3 on the server; dropping it frees its memory there.
pub struct Higgs {
    server: Arc<Server>,
    rate: u32,
    cancel: AtomicBool,
}

impl Higgs {
    /// On the card when it runs a CUDA build of the server (13 or 12.9), else on the processor.
    pub fn load(repo_root: &Path, models_root: &Path) -> Result<Higgs, String> {
        let gguf = higgs_gguf(models_root);
        if !gguf.is_file() {
            return Err(t!("render-higgs-model-missing", path = gguf.display().to_string()));
        }
        let backend = if crate::engines::hw::gpu_report().cuda_build.is_some() { Backend::Cuda } else { Backend::Cpu };
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

#[cfg(test)]
mod tests {
    use super::out_of_memory;

    #[test]
    fn a_graph_ggml_cannot_place_reads_as_out_of_memory() {
        assert!(out_of_memory("audiocpp_server answered 500 (internal): failed to allocate Higgs TTS AR prefill graph"));
        assert!(out_of_memory("cudaMalloc(1048576): CUDA error 2 (out of memory)"));
        assert!(!out_of_memory("audiocpp_server sent nothing for 120 s"));
    }
}
