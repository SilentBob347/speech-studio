//! Recognition by transcribe.cpp (`dub_asr::tcpp`, shared with Dub Studio): Parakeet TDT v3 and Ultra, Nemotron 3.5.
//! Whisper stays on whisper-faster: the transcribe.cpp port of Whisper gives no word times.

pub use dub_asr::tcpp::{installed, knows, model, Model};
#[cfg(test)]
pub use dub_asr::tcpp::{ENGINE, MODELS};

/// The component of each model in the studio's manifest.
pub fn component(m: &Model) -> &'static str {
    match m.id {
        "parakeet-v3" => "parakeet",
        "parakeet-ultra" => "parakeet-ultra",
        _ => m.dir,
    }
}

/// The model the settings name: `asr_engine` nemotron, else Parakeet in its variant (`asr`: `ultra-int8` is Ultra,
/// else v3; the slot keeps the names of the ONNX builds it chose before, so a choice made then stays). None: Whisper
/// or the cloud.
pub fn chosen(engine: &str, variant: Option<&str>) -> Option<&'static Model> {
    match engine {
        "nemotron" => model("nemotron"),
        "" | "parakeet" => match variant {
            Some("ultra-int8") => model("parakeet-ultra"),
            _ => model("parakeet-v3"),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_name_their_model() {
        assert_eq!(chosen("", None).unwrap().id, "parakeet-v3");
        assert_eq!(chosen("parakeet", Some("ultra-int8")).unwrap().id, "parakeet-ultra");
        assert_eq!(chosen("parakeet", Some("int8")).unwrap().id, "parakeet-v3");
        assert_eq!(chosen("nemotron", None).unwrap().id, "nemotron");
        assert!(chosen("whisper", None).is_none());
    }
}
