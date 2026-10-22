//! Автокастинг облачных голосов по полу спикера + языку дубляжа. Справочник голосов (пол/возраст/русский)
//! собран роем из открытых доков провайдеров (voice_db.json, 271 голос / 12 моделей) и вшит в бинарь —
//! программа кастит сама, без обращения к сети. Мужскому спикеру — мужской голос, женскому — женский,
//! разным спикерам разные; для русского дубляжа берём только голоса, реально тянущие русский (voxtral/
//! deepgram/kokoro и пр. на русском не годятся — из справочника ru=false, отфильтруются).

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone)]
pub struct VoiceMeta {
    pub name: String,
    pub gender: String, // male | female | neutral | unknown
    pub age: String,    // child | teen | adult | elderly | unknown
    #[serde(default)]
    pub ru: bool, // годится ли голос для русской речи
}

#[derive(Deserialize)]
pub struct ModelVoices {
    #[serde(default)]
    pub supports_russian: bool,
    pub voices: Vec<VoiceMeta>,
}

/// Встроенный справочник голосов (model_id -> метаданные). Парсим один раз.
fn db() -> &'static HashMap<String, ModelVoices> {
    static DB: OnceLock<HashMap<String, ModelVoices>> = OnceLock::new();
    DB.get_or_init(|| serde_json::from_str(include_str!("voice_db.json")).unwrap_or_default())
}

fn catalog_model(model: &str) -> &str {
    match model.trim_start_matches("models/").trim_start_matches("google/") {
        "gemini-3.8-flash-lite-tts" | "gemini-3.8-flash-tts" => "google/gemini-3.1-flash-tts-preview",
        _ => model,
    }
}

/// Метаданные конкретного голоса модели (пол/возраст/русский). None -> нет в справочнике.
#[allow(dead_code)] // используется в тестах
pub fn voice_meta(model: &str, voice: &str) -> Option<&'static VoiceMeta> {
    db().get(catalog_model(model))?
        .voices
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case(voice))
}

/// Тянет ли модель русский (для предупреждения в UI при русском дубляже несовместимой моделью).
pub fn model_supports_russian(model: &str) -> Option<bool> {
    db().get(catalog_model(model)).map(|m| m.supports_russian)
}

/// The voice a model starts with when none is chosen: its first adult voice in the catalog.
pub fn default_voice(model: &str) -> Option<&'static str> {
    list(model)?.iter().find(|v| v.age != "child" && v.age != "teen").map(|v| v.name.as_str())
}

/// Список голосов модели с метаданными (для дропдауна в UI). Нет в справочнике -> None.
pub fn list(model: &str) -> Option<&'static Vec<VoiceMeta>> {
    db().get(catalog_model(model)).map(|m| &m.voices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_loads_and_has_known_models() {
        assert!(voice_meta("google/gemini-3.1-flash-tts-preview", "Kore").is_some());
        assert_eq!(voice_meta("google/gemini-3.1-flash-tts-preview", "Kore").unwrap().gender, "female");
        assert_eq!(voice_meta("google/gemini-3.1-flash-tts-preview", "Puck").unwrap().gender, "male");
        // voxtral русский не тянет
        assert_eq!(model_supports_russian("mistralai/voxtral-mini-tts-2603"), Some(false));
        assert_eq!(model_supports_russian("google/gemini-3.1-flash-tts-preview"), Some(true));
        assert_eq!(model_supports_russian("openai/gpt-audio"), Some(true));
    }

}
