//! Recognition by transcribe.cpp (`dub_asr::tcpp`, shared with Dub Studio): Parakeet TDT v3 and Ultra, Nemotron 3.5.
//! Whisper stays on whisper-faster: the transcribe.cpp port of Whisper gives no word times.

pub use dub_asr::tcpp::{chosen, installed, knows, model, Build, Model};
#[cfg(test)]
pub use dub_asr::tcpp::MODELS;

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
