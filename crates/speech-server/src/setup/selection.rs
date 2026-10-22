//! Selection slots attached to component variants in the installation catalog.

/// Отобразить id компонента манифеста -> список (slot, значение) для записи выбора при скачивании.
/// Пусто — компонент не является переключаемым вариантом модели (движок/рантайм/OCR и т.п.).
/// ASR-варианты пишут ДВА слота: движок (asr_engine) + вариант этого движка (asr-квант / whisper-модель),
/// чтобы скачивание Whisper-модели сразу делало Whisper активным движком (и наоборот для Parakeet).
pub fn component_selection(id: &str) -> Vec<(&'static str, String)> {
    match id {
        "higgs" => vec![("tts", "q8_0".into())],
        "higgs-q6_k" => vec![("tts", "q6_k".into())],
        "higgs-q4_k_m" => vec![("tts", "q4_k_m".into())],
        "qwen3-tts" => vec![("qwen_tts_quant", "Q4_K_M".into())],
        "qwen3-tts-q8_0" => vec![("qwen_tts_quant", "Q8_0".into())],
        "parakeet" => vec![("asr_engine", "parakeet".into()), ("asr", "int8".into())],
        "parakeet-ultra" => vec![("asr_engine", "parakeet".into()), ("asr", "ultra-int8".into())],
        "asr-gguf-nemotron" => vec![("asr_engine", "nemotron".into())],
        "whisper-tiny" => vec![("asr_engine", "whisper".into()), ("whisper_model", "tiny".into())],
        "whisper-base" => vec![("asr_engine", "whisper".into()), ("whisper_model", "base".into())],
        "whisper-small" => vec![("asr_engine", "whisper".into()), ("whisper_model", "small".into())],
        "whisper-medium" => vec![("asr_engine", "whisper".into()), ("whisper_model", "medium".into())],
        "whisper-large-v3" => vec![("asr_engine", "whisper".into()), ("whisper_model", "large-v3".into())],
        "whisper-large-v3-turbo" => {
            vec![("asr_engine", "whisper".into()), ("whisper_model", "large-v3-turbo".into())]
        }
        "gemma" => vec![("mt", "q4_0".into())],
        "gemma-q5_0" => vec![("mt", "q5_0".into())],
        "gemma-q6_k" => vec![("mt", "q6_k".into())],
        "gemma-q8_0" => vec![("mt", "q8_0".into())],
        "roformer" => vec![("sep", "Q8_0".into())],
        "roformer-q5" => vec![("sep", "Q5_0".into())],
        "roformer-q4" => vec![("sep", "Q4_0".into())],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parakeet_components_map_to_their_asr_slot() {
        for (id, variant) in [("parakeet", "int8"), ("parakeet-ultra", "ultra-int8")] {
            let sel = component_selection(id);
            assert_eq!(
                sel,
                vec![("asr_engine", "parakeet".to_string()), ("asr", variant.to_string())],
                "{id}"
            );
        }
        assert_eq!(
            component_selection("asr-gguf-nemotron"),
            vec![("asr_engine", "nemotron".to_string())]
        );
    }
}
