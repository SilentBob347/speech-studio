use dub_llm::openrouter::OpenRouter;
use std::path::Path;

/// The OpenRouter voice model a studio starts with: speaks Russian, 30 voices in the catalog.
pub const DEFAULT_MODEL: &str = "google/gemini-3.1-flash-tts-preview";

/// The cloud voice settings with the defaults voicing uses where none is chosen, for the window; nothing is written.
pub fn with_defaults(mut sel: serde_json::Value) -> serde_json::Value {
    let Some(obj) = sel.as_object_mut() else { return sel };
    let mut fill = |k: &str, v: &str| {
        if obj.get(k).and_then(serde_json::Value::as_str).is_none_or(|x| x.trim().is_empty()) {
            obj.insert(k.into(), v.into());
        }
    };
    fill("or_tts_model", DEFAULT_MODEL);
    fill("google_tts_model", crate::integrations::google_tts::DEFAULT_MODEL);
    let model = obj.get("or_tts_model").and_then(serde_json::Value::as_str).unwrap_or(DEFAULT_MODEL).to_string();
    if let Some(v) = crate::integrations::cloud_voices::default_voice(&model) {
        if obj.get("or_tts_voice").and_then(serde_json::Value::as_str).is_none_or(|x| x.trim().is_empty()) {
            obj.insert("or_tts_voice".into(), v.into());
        }
    }
    sel
}

/// Модель и голос для проекта: свои, иначе из настроек облака; пустые — ошибка с тем, что выбрать.
pub fn model_and_voice(models_root: &Path, model: &str, voice: &str) -> Result<(String, String), String> {
    let model = if model.trim().is_empty() {
        crate::engines::models::openrouter_model(models_root, "tts")
    } else {
        model.trim().to_string()
    };
    let model = if model.is_empty() { DEFAULT_MODEL.to_string() } else { model };
    let voice = if voice.trim().is_empty() {
        crate::engines::models::openrouter_tts_voice(models_root)
    } else {
        voice.trim().to_string()
    };
    let voice = match voice.is_empty() {
        true => crate::integrations::cloud_voices::default_voice(&model).map(str::to_string).ok_or_else(|| t!("cloud-tts-no-voice"))?,
        false => voice,
    };
    Ok((model, voice))
}

pub fn synth(text: &str, model: &str, voice: &str, style: &str) -> Result<Vec<f32>, String> {
    let key = crate::engines::models::openrouter_key().ok_or_else(|| t!("cloud-tts-no-key"))?;
    crate::integrations::cloud_tts::synth(key, text, model, voice, style)
}

/// Потрачено ключом OpenRouter, USD (`usage` из `GET /key`). None — облако ни на одной стадии не выбрано,
/// ключа нет или OpenRouter не ответил (причина в лог).
pub fn total_usage_usd(models_root: &Path) -> Option<f64> {
    if !crate::engines::models::openrouter_any_on(models_root) {
        return None;
    }
    let key = crate::engines::models::openrouter_key()?;
    let usage = OpenRouter::new(Some(key)).and_then(|client| client.key_usage_usd());
    match usage {
        Ok(usage) => Some(usage),
        Err(e) => {
            tracing::warn!("OpenRouter spending was not read: {e:#}");
            None
        }
    }
}
