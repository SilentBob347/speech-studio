//! Капабилити-манифест движков синтеза — данные, из которых окно строит контролы, а синтез проверяет запрос до
//! запуска: языки (из карточки модели), параметры с типом, диапазоном и умолчанием, режимы сида, теги экспрессии,
//! вид клона, продолжение, стриминг, что из разметки текста движок читает.

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct ParamSpec {
    pub key: &'static str,
    /// `float` или `int`.
    pub kind: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub default: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Reads {
    pub stress: bool,
    pub yo: bool,
    pub phonemes: bool,
    pub accent: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Engine {
    pub id: &'static str,
    pub name: &'static str,
    pub languages: &'static [&'static str],
    pub params: Vec<ParamSpec>,
    pub seed_modes: &'static [&'static str],
    pub tag_format: &'static str,
    pub tags: Vec<(&'static str, &'static [&'static str])>,
    /// `ref_text` — реф и его транскрипт; `timbre` — только тембр.
    pub clone: &'static str,
    pub continuation: bool,
    pub streaming: bool,
    pub deterministic_seed: bool,
    /// Сколько знаков блока движок берёт за раз; длиннее — режется на фразы.
    pub max_chars: usize,
    pub reads: Reads,
    /// Почему движок на этой машине не пойдёт (видеокарта, файлы); None — доступен.
    pub unavailable: Option<String>,
}

/// Языки Higgs Audio v3 — список `language` карточки `bosonai/higgs-audio-v3-tts-4b`.
const HIGGS_LANGS: &[&str] = &[
    "af", "ar", "as", "ast", "az", "ba", "be", "bg", "bn", "bs", "ca", "ceb", "ckb", "cs", "cy", "da", "de", "el", "en", "eo", "es",
    "et", "eu", "fa", "fi", "fr", "ga", "gl", "gu", "ha", "he", "hi", "hr", "ht", "hu", "hy", "id", "is", "it", "jv", "ka", "kab",
    "kam", "kea", "kk", "kn", "ko", "ky", "la", "lb", "lg", "ln", "lt", "luo", "lv", "mhr", "mi", "mk", "ml", "mn", "mr", "ms",
    "mt", "ne", "nl", "no", "nso", "ny", "oc", "om", "pa", "pl", "ps", "pt", "ro", "ru", "rw", "sd", "sk", "sl", "sn", "so",
    "sq", "sr", "sv", "sw", "ta", "te", "tg", "tl", "tr", "ug", "uk", "umb", "ur", "uz", "vi", "xh", "zh", "zu",
];

pub const MAX_TOKENS_LIMIT: u32 = 4096;

pub fn higgs() -> Engine {
    Engine {
        id: "higgs",
        name: "Higgs Audio v3",
        languages: HIGGS_LANGS,
        params: vec![
            ParamSpec { key: "temperature", kind: "float", min: 0.05, max: 2.0, step: 0.05, default: 1.0 },
            ParamSpec { key: "top_k", kind: "int", min: 1.0, max: 500.0, step: 1.0, default: 50.0 },
            ParamSpec { key: "top_p", kind: "float", min: 0.05, max: 1.0, step: 0.01, default: 0.95 },
            ParamSpec { key: "max_tokens", kind: "int", min: 0.0, max: MAX_TOKENS_LIMIT as f64, step: 64.0, default: 0.0 },
        ],
        seed_modes: &["random", "fixed", "inc", "dec"],
        tag_format: "<|{category}:{value}|>",
        tags: crate::speech_text::HIGGS_TAGS.to_vec(),
        clone: "ref_text",
        continuation: true,
        streaming: true,
        deterministic_seed: true,
        max_chars: 400,
        reads: Reads { stress: false, yo: true, phonemes: false, accent: false },
        unavailable: None,
    }
}

/// OpenRouter: языки и голоса — у выбранной модели; параметров выборки и клона нет.
pub fn openrouter() -> Engine {
    Engine {
        id: "openrouter",
        name: "OpenRouter",
        languages: &[],
        params: Vec::new(),
        seed_modes: &["random"],
        tag_format: "",
        tags: Vec::new(),
        clone: "none",
        continuation: false,
        streaming: false,
        deterministic_seed: false,
        max_chars: 1000,
        reads: Reads { stress: false, yo: true, phonemes: false, accent: false },
        unavailable: None,
    }
}

/// Fish Audio Drama 3: 83 языка по карточке `fishaudio/drama-3` (списка она не даёт — язык не ограничивается),
/// сэмплинг по умолчанию карточки (temperature 1.0, top-k 30, top-p 0.9), точечные теги `[laughs]`, клон только по
/// рефу с текстом; только видеокарты 24 ГБ+.
pub fn drama() -> Engine {
    Engine {
        id: crate::models::DRAMA_TTS,
        name: "Drama 3",
        languages: &[],
        params: vec![
            ParamSpec { key: "temperature", kind: "float", min: 0.05, max: 2.0, step: 0.05, default: 1.0 },
            ParamSpec { key: "top_k", kind: "int", min: 1.0, max: 500.0, step: 1.0, default: 30.0 },
            ParamSpec { key: "top_p", kind: "float", min: 0.05, max: 1.0, step: 0.01, default: 0.9 },
            ParamSpec { key: "max_tokens", kind: "int", min: 0.0, max: 2048.0, step: 64.0, default: 0.0 },
        ],
        seed_modes: &["random", "fixed", "inc", "dec"],
        tag_format: "[{value}]",
        tags: Vec::new(),
        clone: "ref_text",
        continuation: false,
        streaming: true,
        deterministic_seed: true,
        max_chars: 400,
        reads: Reads { stress: false, yo: true, phonemes: false, accent: false },
        unavailable: None,
    }
}

/// fal, Runware и Google Gemini: модель из каталога или настроек, голоса — у модели; языки — у модели.
pub fn catalog(id: &'static str, name: &'static str) -> Engine {
    Engine { id, name, ..openrouter() }
}

pub fn all() -> Vec<Engine> {
    vec![higgs(), drama(), openrouter(), catalog("google", "Google Gemini"), catalog("fal", "fal.ai"), catalog("runware", "Runware")]
}

/// Движки для окна: у Drama 3 — почему она здесь не пойдёт (видеокарта или нескачанные файлы).
pub fn for_window(models_root: &std::path::Path) -> Vec<Engine> {
    let mut all = all();
    for e in all.iter_mut().filter(|e| e.id == crate::models::DRAMA_TTS) {
        e.unavailable = crate::models::drama_unavailable().or_else(|| (!crate::models::drama_installed(models_root)).then(|| t!("render-drama-missing")));
    }
    all
}

pub fn get(id: &str) -> Option<Engine> {
    all().into_iter().find(|e| e.id == id)
}

/// Язык проекта годится движку; `auto` годится всегда — язык берётся по письму блока.
pub fn supports(engine: &Engine, lang: &str) -> bool {
    lang.is_empty() || lang == "auto" || engine.languages.is_empty() || engine.languages.contains(&lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn higgs_reads_russian_but_not_japanese() {
        let h = higgs();
        assert_eq!(h.languages.len(), 100);
        assert!(supports(&h, "ru") && supports(&h, "auto") && !supports(&h, "ja"));
    }
}
