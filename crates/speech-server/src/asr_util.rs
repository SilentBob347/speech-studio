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

pub fn transcribe(repo_root: &Path, models_root: &Path, audio: &Path, lang: &str, what: &str) -> Result<Heard, String> {
    let choice = choice(repo_root, models_root, lang, what)?;
    run(&choice, models_root, audio, if lang == "auto" { "" } else { lang })
}

/// The recogniser for `lang`: the one chosen in the settings, Whisper for a language it does not know.
pub fn choice(repo_root: &Path, models_root: &Path, lang: &str, what: &str) -> Result<crate::models::AsrChoice, String> {
    choice_of(repo_root, models_root, lang, what, "")
}

/// The recogniser for `lang` with `engine` (parakeet, nemotron, whisper) in place of the settings' one; empty keeps it.
pub fn choice_of(repo_root: &Path, models_root: &Path, lang: &str, what: &str, engine: &str) -> Result<crate::models::AsrChoice, String> {
    let mut sel = crate::models::load_selection(models_root);
    if !engine.is_empty() {
        if let Some(obj) = sel.as_object_mut() {
            obj.insert("asr_engine".into(), json!(engine));
        }
    }
    let mut choice = crate::models::resolve_asr_choice(repo_root, models_root, &sel);
    let lang = if lang.is_empty() || lang == "auto" { "" } else { lang };
    if !choice.knows(lang) {
        if let Some(obj) = sel.as_object_mut() {
            obj.insert("asr_engine".into(), json!("whisper"));
        }
        choice = crate::models::resolve_asr_choice(repo_root, models_root, &sel);
        if !matches!(choice, crate::models::AsrChoice::Whisper { .. }) {
            return Err(t!("voice-lang-needs-whisper", name = what.to_string(), lang = lang.to_string()));
        }
    }
    Ok(choice)
}

/// Whisper when `transcribe` runs Parakeet or Nemotron, Parakeet when it runs Whisper, if that one is on this computer
/// and knows the language: a second opinion on a take its sound made suspect (Game Dub's checks heard a repeat one of
/// them missed). `None` when there is no such model.
pub fn second_opinion(repo_root: &Path, models_root: &Path, audio: &Path, lang: &str) -> Result<Option<Heard>, String> {
    let mut sel = crate::models::load_selection(models_root);
    let first = crate::models::resolve_asr_choice(repo_root, models_root, &sel);
    let lang = if lang.is_empty() || lang == "auto" { "" } else { lang };
    let first_whisper = matches!(first, crate::models::AsrChoice::Whisper { .. });
    if let Some(obj) = sel.as_object_mut() {
        obj.insert("asr_engine".into(), json!(if first_whisper { "parakeet" } else { "whisper" }));
    }
    let second = crate::models::resolve_asr_choice(repo_root, models_root, &sel);
    if matches!(second, crate::models::AsrChoice::Whisper { .. }) == first_whisper || !second.knows(lang) {
        return Ok(None);
    }
    run(&second, models_root, audio, lang).map(Some)
}

fn run(choice: &crate::models::AsrChoice, models_root: &Path, audio: &Path, lang: &str) -> Result<Heard, String> {
    let work = tempfile::Builder::new().prefix("asr-").tempdir().map_err(|e| e.to_string())?;
    let wav = work.path().join("in-16k.wav");
    crate::media::to_16k_mono(audio, &wav)?;
    std::env::set_var("DUB_ASR_BACKEND", crate::models::stage_backend(models_root, "asr_backend"));
    let mut asr = crate::models::build_engine(choice, None);
    let segments = asr.transcribe(&wav, if lang.is_empty() { "auto" } else { lang }).map_err(|e| e.localize())?;
    let text = segments.iter().map(|s| s.text.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    let words = segments.iter().flat_map(|s| s.words.iter()).map(|w| Word { word: w.word.clone(), start: w.start, end: w.end }).collect();
    Ok(Heard { text, words })
}
