//! Работа над голосом, которая идёт задачей очереди: транскрипт рефа (выбранный локальный ASR) и очистка голоса
//! от музыки и шума (Mel-Band Roformer).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::localize::Localize;
use crate::voice_library as lib;

pub struct VoicePaths {
    pub repo_root: PathBuf,
    pub models_root: PathBuf,
    pub voices_dir: PathBuf,
}

fn work_dir(voices: &Path, name: &str, what: &str) -> Result<tempfile::TempDir, String> {
    let base = voices.join(".work");
    std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    tempfile::Builder::new().prefix(&format!("{what}-")).tempdir_in(&base).map_err(|e| format!("{name}: {e}"))
}

/// Транскрипт рефа тем ASR, что выбран в настройках; пишет `<имя>.txt` и помечает транскрипт как распознанный.
pub fn transcribe(p: &VoicePaths, name: &str, progress: &dyn Fn(Value)) -> Result<Value, String> {
    let audio = lib::audio_path(&p.voices_dir, name).ok_or_else(|| t!("voice-missing", name = name.to_string()))?;
    let card = lib::read_card(&p.voices_dir, name)?;
    let work = work_dir(&p.voices_dir, name, "asr")?;
    let wav = work.path().join("ref-16k.wav");
    progress(json!({ "type": "progress", "stage": "voice_transcript", "msg": t!("voice-transcribing", name = name.to_string()) }));
    crate::media::to_16k_mono(&audio, &wav)?;
    crate::ensure_job_components(&p.repo_root, &p.models_root, false, false, &|ev| progress(ev))?;
    let mut sel = crate::models::load_selection(&p.models_root);
    let mut choice = crate::models::resolve_asr_choice(&p.repo_root, &p.models_root, &sel);
    let parakeet = matches!(choice, crate::models::AsrChoice::Parakeet(_));
    if parakeet && !card.lang.is_empty() && !crate::models::parakeet_knows(&card.lang) {
        if let Some(obj) = sel.as_object_mut() {
            obj.insert("asr_engine".into(), json!("whisper"));
        }
        choice = crate::models::resolve_asr_choice(&p.repo_root, &p.models_root, &sel);
        if matches!(choice, crate::models::AsrChoice::Parakeet(_)) {
            return Err(t!("voice-lang-needs-whisper", name = name.to_string(), lang = card.lang.clone()));
        }
    }
    std::env::set_var("DUB_ASR_BACKEND", crate::models::stage_backend(&p.models_root, "asr_backend"));
    let mut asr = crate::models::build_engine(&choice);
    let lang = if card.lang.is_empty() { "auto" } else { card.lang.as_str() };
    let segments = asr.transcribe(&wav, lang).map_err(|e| e.localize())?;
    let text = segments.iter().map(|s| s.text.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(t!("voice-no-speech", name = name.to_string()));
    }
    lib::write_transcript(&p.voices_dir, name, &text)?;
    let words: Vec<crate::ref_pick::Word> = segments
        .iter()
        .flat_map(|s| s.words.iter())
        .map(|w| crate::ref_pick::Word { word: w.word.clone(), start: w.start, end: w.end })
        .collect();
    lib::write_words(&p.voices_dir, name, &words)?;
    let mut card = lib::read_card(&p.voices_dir, name)?;
    card.transcript_auto = true;
    lib::write_card(&p.voices_dir, name, &card)?;
    Ok(json!({ "name": name, "transcript": text }))
}

/// Голос без музыки и шума: Roformer отделяет вокал, он заменяет звук голоса, исходник остаётся в `.originals`.
pub fn clean(p: &VoicePaths, name: &str, progress: &dyn Fn(Value)) -> Result<Value, String> {
    let audio = lib::audio_path(&p.voices_dir, name).ok_or_else(|| t!("voice-missing", name = name.to_string()))?;
    crate::ensure_job_components(&p.repo_root, &p.models_root, false, false, &|ev| progress(ev))?;
    let sel = crate::models::load_selection(&p.models_root);
    let cli = dub_sep::engine_cli(&p.repo_root, crate::models::stage_backend(&p.models_root, "sep_backend"));
    let model = crate::models::resolve_sep(&p.models_root, &sel);
    if !cli.is_file() || !model.is_file() {
        return Err(t!("voice-clean-missing"));
    }
    let work = work_dir(&p.voices_dir, name, "clean")?;
    let wav = work.path().join("mix.wav");
    progress(json!({ "type": "progress", "stage": "voice_clean", "msg": t!("voice-cleaning", name = name.to_string()) }));
    crate::media::to_wav(&audio, &wav)?;
    let stems = work.path().join("stems");
    let res = dub_sep::separate(&wav, &stems, &cli, &model).map_err(|e| e.localize())?;
    lib::replace_with_cleaned(&p.voices_dir, name, &res.vocals)?;
    Ok(json!({ "name": name, "cleaned": true }))
}

/// Можно ли чистить голос сейчас: движок и модель Roformer на месте.
pub fn clean_ready(p: &VoicePaths) -> bool {
    let sel = crate::models::load_selection(&p.models_root);
    dub_sep::engine_cli(&p.repo_root, crate::models::stage_backend(&p.models_root, "sep_backend")).is_file()
        && crate::models::resolve_sep(&p.models_root, &sel).is_file()
}

/// Куски рефа по словам распознавания, не длиннее лимита рефа движка (настройка `higgs_ref_secs`, 12 с).
pub fn ref_candidates(p: &VoicePaths, name: &str) -> Result<Value, String> {
    let audio = lib::audio_path(&p.voices_dir, name).ok_or_else(|| t!("voice-missing", name = name.to_string()))?;
    let words = lib::read_words(&p.voices_dir, name).ok_or_else(|| t!("voice-no-words", name = name.to_string()))?;
    let sel = crate::models::load_selection(&p.models_root);
    let max_len = sel.get("higgs_ref_secs").and_then(Value::as_str).and_then(|v| v.parse::<f64>().ok()).unwrap_or(12.0);
    let work = work_dir(&p.voices_dir, name, "pick")?;
    let wav = work.path().join("ref-16k.wav");
    crate::media::to_16k_mono(&audio, &wav)?;
    let (samples, sr) = crate::wavio::read_mono_f32(&wav)?;
    let found = crate::ref_pick::candidates(&words, &samples, sr, (max_len * 0.4).min(5.0), max_len, 5);
    Ok(json!({ "name": name, "max_len": max_len, "duration": samples.len() as f64 / sr.max(1) as f64, "candidates": found }))
}
