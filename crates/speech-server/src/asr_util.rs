//! Распознавание одного файла выбранным в настройках ASR: Parakeet, а язык, которого он не знает, — Whisper.
//! Общее для текста рефа голоса и проверки озвучки.

use std::path::Path;

use serde_json::json;

use crate::localize::Localize;
use crate::ref_pick::Word;

pub struct Heard {
    pub text: String,
    pub words: Vec<Word>,
}

/// Текст и слова записи `audio` (любой формат: перегоняется в 16 кГц моно рядом во временный файл).
pub fn transcribe(repo_root: &Path, models_root: &Path, audio: &Path, lang: &str, what: &str) -> Result<Heard, String> {
    let work = tempfile::Builder::new().prefix("asr-").tempdir().map_err(|e| e.to_string())?;
    let wav = work.path().join("in-16k.wav");
    crate::media::to_16k_mono(audio, &wav)?;
    let mut sel = crate::models::load_selection(models_root);
    let mut choice = crate::models::resolve_asr_choice(repo_root, models_root, &sel);
    let lang = if lang.is_empty() || lang == "auto" { "" } else { lang };
    if matches!(choice, crate::models::AsrChoice::Parakeet(_)) && !lang.is_empty() && !crate::models::parakeet_knows(lang) {
        if let Some(obj) = sel.as_object_mut() {
            obj.insert("asr_engine".into(), json!("whisper"));
        }
        choice = crate::models::resolve_asr_choice(repo_root, models_root, &sel);
        if matches!(choice, crate::models::AsrChoice::Parakeet(_)) {
            return Err(t!("voice-lang-needs-whisper", name = what.to_string(), lang = lang.to_string()));
        }
    }
    std::env::set_var("DUB_ASR_BACKEND", crate::models::stage_backend(models_root, "asr_backend"));
    let mut asr = crate::models::build_engine(&choice, None);
    let segments = asr.transcribe(&wav, if lang.is_empty() { "auto" } else { lang }).map_err(|e| e.localize())?;
    let text = segments.iter().map(|s| s.text.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    let words = segments.iter().flat_map(|s| s.words.iter()).map(|w| Word { word: w.word.clone(), start: w.start, end: w.end }).collect();
    Ok(Heard { text, words })
}
