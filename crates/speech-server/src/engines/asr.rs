//! ASR launch specifications and construction. Settings and preparation belong to the caller.

use std::path::{Path, PathBuf};

/// Выбор ASR-движка для одной джобы: модель transcribe.cpp (Parakeet, Nemotron) либо Whisper (бинарь + модель + квант + девайс).
#[derive(Debug, Clone)]
pub enum AsrChoice {
    /// A GGUF model run by transcribe.cpp on the GPU.
    Ggml { models_root: PathBuf, build: crate::engines::tcpp_asr::Build, model: &'static crate::engines::tcpp_asr::Model },
    /// CrisperWhisper 2.0 turbo in the studio's audiocpp_server on the recognition backend; the language of «auto» is
    /// heard by VoxLingua107 on the transcribe.cpp build of that backend.
    CrisperWhisper { repo_root: PathBuf, models_root: PathBuf, backend: &'static str, build: crate::engines::tcpp_asr::Build },
}

impl AsrChoice {
    /// Строка для лога `[models]` — видно, каким движком реально пойдёт транскрипция.
    /// ВАЖНО: участвует в param_hash ASR-стадии (analyze.rs) — только СТАБИЛЬНЫЕ токены (вариант
    /// каталога, не абсолютный путь), иначе кэш/чекпоинты инвалидируются от переноса репо между
    /// машинами/папками (ревью-находка).
    /// What the window names as the recogniser: the model, its quantization and the transcribe.cpp build, or Whisper's
    /// model and compute.
    pub fn label(&self) -> String {
        match self {
            AsrChoice::Ggml { model, build, .. } => {
                let quant = model.file.trim_end_matches(".gguf").rsplit('-').next().unwrap_or(model.file);
                let on = match build {
                    crate::engines::tcpp_asr::Build::Cuda => "CUDA",
                    crate::engines::tcpp_asr::Build::Vulkan => "Vulkan",
                    crate::engines::tcpp_asr::Build::Cpu => "CPU",
                };
                format!("{} {quant} ({on})", model.name)
            }
            AsrChoice::CrisperWhisper { backend, .. } => format!("CrisperWhisper 2.0 turbo ({})", if *backend == "cpu" { "CPU" } else { "CUDA" }),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            AsrChoice::Ggml { model, .. } => format!("transcribe.cpp ({})", model.file),
            AsrChoice::CrisperWhisper { backend, .. } => format!("CrisperWhisper 2.0 turbo (intended, {backend})"),
        }
    }

    /// Whether it transcribes `lang` (`""` or `auto` is any).
    pub fn knows(&self, lang: &str) -> bool {
        lang.is_empty()
            || lang == "auto"
            || match self {
                AsrChoice::Ggml { model, .. } => crate::engines::tcpp_asr::knows(model, lang),
                AsrChoice::CrisperWhisper { .. } => {
                    let primary = lang.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
                    acpp_server::language::whisper_code(&primary).is_ok()
                }
            }
    }
}

/// A transcribe.cpp recogniser that unloads the process's model when its holder lets it go: a Listening job or a take
/// check at its end, Live when it is switched off (it holds one recogniser for the whole conversation).
struct Unloading(dub_asr::tcpp::TcppEngine);

impl Drop for Unloading {
    fn drop(&mut self) {
        dub_asr::tcpp::unload();
    }
}

impl dub_asr::AsrEngine for Unloading {
    fn transcribe(&mut self, wav: &Path, lang: &str) -> Result<Vec<dub_asr::Segment>, dub_asr::AsrError> {
        self.0.transcribe(wav, lang)
    }
    fn transcribe_turns(&mut self, wav: &Path, turns: &[dub_asr::Turn], lang: &str) -> Result<Vec<dub_asr::SpeakerSegment>, dub_asr::AsrError> {
        self.0.transcribe_turns(wav, turns, lang)
    }
    fn transcribe_many(&mut self, files: &[PathBuf], lang: &str) -> Vec<Result<String, dub_asr::AsrError>> {
        self.0.transcribe_many(files, lang)
    }
    fn transcribe_many_words(&mut self, files: &[PathBuf], lang: &str) -> Vec<Result<Vec<dub_asr::Word>, dub_asr::AsrError>> {
        self.0.transcribe_many_words(files, lang)
    }
}

/// CrisperWhisper of a job: its weights leave the card when the job lets the recogniser go, the server stays.
struct ServerUnloading(dub_asr::acpp::AcppAsr);

impl Drop for ServerUnloading {
    fn drop(&mut self) {
        crate::engines::acpp::unload(&[crate::engines::acpp::CRISPERWHISPER]);
    }
}

impl dub_asr::AsrEngine for ServerUnloading {
    fn transcribe(&mut self, wav: &Path, lang: &str) -> Result<Vec<dub_asr::Segment>, dub_asr::AsrError> {
        self.0.transcribe(wav, lang)
    }
    fn transcribe_turns(&mut self, wav: &Path, turns: &[dub_asr::Turn], lang: &str) -> Result<Vec<dub_asr::SpeakerSegment>, dub_asr::AsrError> {
        self.0.transcribe_turns(wav, turns, lang)
    }
    fn transcribe_many(&mut self, files: &[PathBuf], lang: &str) -> Vec<Result<String, dub_asr::AsrError>> {
        self.0.transcribe_many(files, lang)
    }
    fn transcribe_many_words(&mut self, files: &[PathBuf], lang: &str) -> Vec<Result<Vec<dub_asr::Word>, dub_asr::AsrError>> {
        self.0.transcribe_many_words(files, lang)
    }
}

/// The recogniser of a choice; CrisperWhisper starts audiocpp_server, and its refusal is an error with the cause.
/// `prompt` — the topic and terms to expect; only CrisperWhisper takes one.
pub fn build_engine(choice: &AsrChoice, prompt: Option<String>) -> Result<Box<dyn dub_asr::AsrEngine>, String> {
    tracing::info!("recognition: {} [{}]", choice.label(), choice.describe());
    match choice {
        AsrChoice::Ggml { models_root, build, model } => Ok(Box::new(Unloading(dub_asr::tcpp::TcppEngine::new(models_root, *build, model)))),
        AsrChoice::CrisperWhisper { repo_root, models_root, backend, build } => {
            if crate::engines::models::load_selection(models_root).get("asr_backend").and_then(|v| v.as_str()) == Some("vulkan") {
                return Err(t!("asr-crisperwhisper-no-vulkan"));
            }
            let server = crate::engines::acpp::server(repo_root, models_root, crate::engines::acpp::backend(backend), crate::engines::acpp::CRISPERWHISPER)?;
            let language = dub_asr::acpp::Language::Whisper {
                engine_dir: dub_asr::tcpp::engine_dir(models_root, *build),
                langid: dub_asr::tcpp::helper_path(models_root, &dub_asr::tcpp::LANGID),
            };
            Ok(Box::new(ServerUnloading(dub_asr::acpp::AcppAsr::new(server, crate::engines::acpp::CRISPERWHISPER, language).with_prompt(prompt))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_labels_survive_relocation_and_distinguish_the_selected_build() {
        let model = crate::engines::tcpp_asr::model("parakeet-v3").unwrap();
        let spec = |root: &str, build| AsrChoice::Ggml {
            models_root: root.into(),
            model,
            build,
        };
        assert_eq!(
            spec("old/models", crate::engines::tcpp_asr::Build::Cpu).label(),
            spec("new/models", crate::engines::tcpp_asr::Build::Cpu).label()
        );
        assert_eq!(spec("models", crate::engines::tcpp_asr::Build::Cpu).label(), "Parakeet TDT v3 Q8_0 (CPU)");
        assert_ne!(
            spec("models", crate::engines::tcpp_asr::Build::Cpu).label(),
            spec("models", crate::engines::tcpp_asr::Build::Cuda).label()
        );
    }
}
