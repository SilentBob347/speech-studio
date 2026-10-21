//! Озвучка через OpenRouter (`/audio/speech`) — `dub_llm::openrouter`, как облачный TTS Dub. Модель и голос — из
//! проекта, иначе из выбора в «Облаке»; указание блока уходит стилем. Ответ любого формата — WAV 24 кГц моно.

use std::path::Path;

use dub_llm::openrouter::{OpenRouter, SpeechAudio};

pub const SR: u32 = 24_000;

/// Модель и голос для проекта: свои, иначе из настроек облака; пустые — ошибка с тем, что выбрать.
pub fn model_and_voice(models_root: &Path, model: &str, voice: &str) -> Result<(String, String), String> {
    let model = if model.trim().is_empty() { crate::models::openrouter_model(models_root, "tts") } else { model.trim().to_string() };
    if model.is_empty() {
        return Err(t!("cloud-tts-no-model"));
    }
    let voice = if voice.trim().is_empty() { crate::models::openrouter_tts_voice(models_root) } else { voice.trim().to_string() };
    if voice.is_empty() {
        return Err(t!("cloud-tts-no-voice"));
    }
    Ok((model, voice))
}

/// Одна фраза облаком → сэмплы 24 кГц моно.
pub fn synth(text: &str, model: &str, voice: &str, style: &str) -> Result<Vec<f32>, String> {
    let key = crate::models::openrouter_key().ok_or_else(|| t!("cloud-tts-no-key"))?;
    let client = OpenRouter::new(Some(key)).map_err(|e| t!("cloud-tts-failed", error = format!("{e:#}")))?;
    let (bytes, mime) = match client.speech_with_style(model, text, voice, style).map_err(|e| t!("cloud-tts-failed", error = format!("{e:#}")))? {
        SpeechAudio::Wav(wav) => (wav, "audio/wav".to_string()),
        SpeechAudio::Encoded { mime, bytes } => (bytes, mime),
    };
    if bytes.len() < 200 {
        return Err(t!("cloud-tts-too-short", bytes = bytes.len()));
    }
    let dir = tempfile::Builder::new().prefix("cloud-tts-").tempdir().map_err(|e| e.to_string())?;
    let ext = if mime.contains("mpeg") || mime.contains("mp3") { "mp3" } else if mime.contains("ogg") || mime.contains("opus") { "ogg" } else if mime.contains("flac") { "flac" } else { "wav" };
    let src = dir.path().join(format!("in.{ext}"));
    let wav = dir.path().join("out.wav");
    std::fs::write(&src, &bytes).map_err(|e| e.to_string())?;
    crate::media::extract_audio(&src, &wav, SR, 1)?;
    crate::wavio::read_mono_f32(&wav).map(|(s, _)| s)
}
