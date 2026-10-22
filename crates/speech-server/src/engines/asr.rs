//! ASR launch specifications and construction. Settings and preparation belong to the caller.

use std::path::{Path, PathBuf};

/// Выбор ASR-движка для одной джобы: модель transcribe.cpp (Parakeet, Nemotron) либо Whisper (бинарь + модель + квант + девайс).
#[derive(Debug, Clone)]
pub enum AsrChoice {
    /// A GGUF model run by the selected transcribe.cpp build.
    Ggml {
        models_root: PathBuf,
        build: crate::engines::tcpp_asr::Build,
        model: &'static crate::engines::tcpp_asr::Model,
    },
    Whisper {
        bin: PathBuf,
        model_dir: PathBuf,
        model: String,
        compute: String,
        device: String,
    },
}

impl AsrChoice {
    /// Participates in ASR cache keys: catalog tokens only, never absolute paths.
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
            AsrChoice::Whisper { model, compute, .. } => format!("Whisper {model} {compute}"),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            AsrChoice::Ggml { model, .. } => format!("transcribe.cpp ({})", model.file),
            AsrChoice::Whisper {
                model, compute, device, ..
            } => {
                format!("Whisper {model} (compute={compute}, device={device})")
            }
        }
    }

    /// Whether it transcribes `lang` (`""` or `auto` is any).
    pub fn knows(&self, lang: &str) -> bool {
        lang.is_empty()
            || lang == "auto"
            || match self {
                AsrChoice::Ggml { model, .. } => crate::engines::tcpp_asr::knows(model, lang),
                AsrChoice::Whisper { .. } => true,
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
    fn transcribe_turns(
        &mut self,
        wav: &Path,
        turns: &[dub_asr::Turn],
        lang: &str,
    ) -> Result<Vec<dub_asr::SpeakerSegment>, dub_asr::AsrError> {
        self.0.transcribe_turns(wav, turns, lang)
    }
    fn transcribe_many(&mut self, files: &[PathBuf], lang: &str) -> Vec<Result<String, dub_asr::AsrError>> {
        self.0.transcribe_many(files, lang)
    }
    fn transcribe_many_words(&mut self, files: &[PathBuf], lang: &str) -> Vec<Result<Vec<dub_asr::Word>, dub_asr::AsrError>> {
        self.0.transcribe_many_words(files, lang)
    }
}

pub fn build_engine(choice: &AsrChoice, prompt: Option<String>) -> Box<dyn dub_asr::AsrEngine> {
    match choice {
        AsrChoice::Ggml { models_root, build, model } => Box::new(Unloading(dub_asr::tcpp::TcppEngine::new(models_root, *build, model))),
        AsrChoice::Whisper {
            bin,
            model_dir,
            model,
            compute,
            device,
        } => Box::new(dub_asr::WhisperAsr::new(bin, model_dir, model, compute, device).with_prompt(prompt)),
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
