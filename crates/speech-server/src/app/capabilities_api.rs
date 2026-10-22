use crate::engines::{hw, models};
use crate::{setup, AppState};
use axum::extract::State;
use axum::Json;
use serde_json::json;
use serde_json::Value;

pub(super) async fn capabilities(State(st): State<AppState>) -> Json<Value> {
    let o = &st.opts;
    let ffmpeg = which_ffmpeg();
    // Текущий выбор вариантов (active.json) — фронт рисует по нему селекторы движка/модели/кванта ASR.
    // Без секретов: вместо ключа OpenRouter и пароля прокси — флаги or_key_set / proxy_password_set.
    let sel = models::public_selection(&st.models_root);
    // Языки контента = полный набор Whisper (99). Единый источник — dub_translate::WHISPER_LANGS.
    let langs: Vec<&str> = dub_translate::WHISPER_LANGS.iter().map(|(c, _)| *c).collect();
    Json(json!({
        "device": o.device,
        "tts_quant": o.tts_quant,
        "asr_model": o.asr_model,
        "asr_label": models::resolve_asr_choice(&st.repo_root, &st.models_root, &models::load_selection(&st.models_root)).label(),
        "models": {
            "asr": o.asr_model,
            "llm": o.mt_model_path.to_string_lossy(),
            "vision": o.mmproj_path.to_string_lossy(),
            "tts": o.tts_model,
        },
        "ffmpeg": ffmpeg,
        "languages": langs,
        "modes": ["speech","listening"],
        // Выбор ASR: движок (parakeet|nemotron|whisper), модель Whisper, квант Whisper (compute_type).
        "selection": sel,
        "asr_engines": ["parakeet","nemotron","whisper"],
        "whisper_models": ["tiny","base","small","medium","large-v3","large-v3-turbo"],
        // Кванты Whisper (compute_type): float16 / int8_float16 задействуют Tensor Cores на CUDA GPU.
        "whisper_computes": ["int8","int8_float16","int8_float32"],
        // Видимые лимиты RAM (настройки, не авто-магия): против OOM на слабой памяти. "0" = авто (дефолт).
        "llama_ubatches": ["0","512","256","128"],      // prefill-батч Gemma (меньше = меньше RAM)
        "higgs_ref_secs_opts": ["12","8","6","4"],       // длина реф-клипа клона (сек; <12 спасает 32ГБ)
    }))
}

pub(super) fn which_ffmpeg() -> bool {
    setup::ffmpeg_on_path()
}

/// GET /hw/snapshot — снимок GPU/VRAM/темп/мощность + RAM для монитора ресурсов.
pub(super) async fn hw_snapshot() -> Json<hw::HardwareSnapshot> {
    Json(tokio::task::spawn_blocking(hw::snapshot).await.unwrap_or_default())
}
