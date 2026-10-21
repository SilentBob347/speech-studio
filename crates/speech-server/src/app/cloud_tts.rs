use dub_llm::openrouter::OpenRouter;
use std::path::Path;

/// Модель и голос для проекта: свои, иначе из настроек облака; пустые — ошибка с тем, что выбрать.
pub fn model_and_voice(models_root: &Path, model: &str, voice: &str) -> Result<(String, String), String> {
    let model = if model.trim().is_empty() {
        crate::models::openrouter_model(models_root, "tts")
    } else {
        model.trim().to_string()
    };
    if model.is_empty() {
        return Err(t!("cloud-tts-no-model"));
    }
    let voice = if voice.trim().is_empty() {
        crate::models::openrouter_tts_voice(models_root)
    } else {
        voice.trim().to_string()
    };
    if voice.is_empty() {
        return Err(t!("cloud-tts-no-voice"));
    }
    Ok((model, voice))
}

pub fn synth(text: &str, model: &str, voice: &str, style: &str) -> Result<Vec<f32>, String> {
    let key = crate::models::openrouter_key().ok_or_else(|| t!("cloud-tts-no-key"))?;
    crate::integrations::cloud_tts::synth(key, text, model, voice, style)
}

/// Потрачено ключом OpenRouter, USD (`usage` из `GET /key`). None — облако ни на одной стадии не выбрано,
/// ключа нет или OpenRouter не ответил (причина в лог).
pub fn total_usage_usd(models_root: &Path) -> Option<f64> {
    if !crate::models::openrouter_any_on(models_root) {
        return None;
    }
    let key = crate::models::openrouter_key()?;
    let usage = OpenRouter::new(Some(key)).and_then(|client| client.key_usage_usd());
    match usage {
        Ok(usage) => Some(usage),
        Err(e) => {
            tracing::warn!("OpenRouter spending was not read: {e:#}");
            None
        }
    }
}
