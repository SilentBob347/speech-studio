//! Ручки движков и облака: модели и голоса OpenRouter, локальные серверы моделей, прокси, пресеты, PATCH /engine/opts,
//! выбор модели.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::AppState;

// ─── GET /fonts ─────────────────────────────────────────────────────────────


// ─── GET /presets ───────────────────────────────────────────────────────────

// ─── GET /engine/openrouter/models?kind=llm|vision|tts|asr ──────────────────
// Модели каталога OpenRouter, подходящие стадии (llm = текст -> текст; vision = картинка -> текст;
// tts = синтез речи; asr = транскрипция): id, имя, цены (строки USD за токен, как у OpenRouter), контекст,
// голоса. Каталог — из кэша (память -> диск -> сеть), ключ не нужен.
pub async fn openrouter_models(State(st): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let kind = q.get("kind").map(String::as_str).unwrap_or("llm");
    let Some(capability) = dub_llm::openrouter::Capability::parse(kind) else {
        return (StatusCode::BAD_REQUEST, format!("unknown model kind: {kind:?}")).into_response();
    };
    let models_root = st.models_root.clone();
    match tokio::task::spawn_blocking(move || crate::integrations::openrouter::catalog(&models_root)).await.unwrap_or_else(|e| Err(e.to_string())) {
        Ok(cached) => {
            let models: Vec<Value> = cached.catalog().models_for(capability).map(model_view).collect();
            Json(json!({ "models": models, "refreshed_at": cached.refreshed_at })).into_response()
        }
        Err(e) => (StatusCode::BAD_GATEWAY, t!("openrouter-catalog-failed", error = e)).into_response(),
    }
}

/// Модель каталога для окна: без служебных полей чат-клиента.
fn model_view(model: &dub_llm::openrouter::CatalogModel) -> Value {
    json!({
        "id": model.id,
        "name": model.name,
        "context_length": model.context_length,
        "pricing": model.pricing,
        "input_modalities": model.input_modalities,
        "voices": model.voices,
    })
}

// ─── GET /engine/openrouter/catalog, POST /engine/openrouter/catalog/refresh ─
// Сводка каталога: сколько моделей по стадиям и когда он обновлялся; refresh — скачать заново.
fn catalog_summary(cached: &crate::integrations::openrouter::CachedCatalog) -> Value {
    use dub_llm::openrouter::Capability;
    let catalog = cached.catalog();
    let count = |capability| catalog.models_for(capability).count();
    json!({
        "refreshed_at": cached.refreshed_at,
        "total": cached.models.len(),
        "counts": {
            "llm": count(Capability::Text),
            "vision": count(Capability::Vision),
            "tts": count(Capability::Speech),
            "asr": count(Capability::Transcription),
        },
    })
}

pub async fn openrouter_catalog(State(st): State<AppState>) -> Response {
    let models_root = st.models_root.clone();
    match tokio::task::spawn_blocking(move || crate::integrations::openrouter::catalog(&models_root)).await.unwrap_or_else(|e| Err(e.to_string())) {
        Ok(cached) => Json(catalog_summary(&cached)).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, t!("openrouter-catalog-failed", error = e)).into_response(),
    }
}

pub async fn openrouter_catalog_refresh(State(st): State<AppState>) -> Response {
    let models_root = st.models_root.clone();
    match tokio::task::spawn_blocking(move || crate::integrations::openrouter::refresh(&models_root)).await.unwrap_or_else(|e| Err(e.to_string())) {
        Ok(cached) => Json(catalog_summary(&cached)).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, t!("openrouter-catalog-failed", error = e)).into_response(),
    }
}

// ─── POST /engine/openrouter/verify {key} ───────────────────────────────────
// Проверка ключа OpenRouter (GET /key) без сохранения. Ключ из формы, НЕ логируется. Ответ:
// {ok:true, data:{label, limit, usage, …}} | {ok:false, error}.
pub async fn openrouter_verify(Json(body): Json<Value>) -> Response {
    let key = body.get("key").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if key.is_empty() {
        return (StatusCode::BAD_REQUEST, t!("openrouter-empty-key")).into_response();
    }
    let res = tokio::task::spawn_blocking(move || {
        dub_llm::openrouter::OpenRouter::new(Some(key)).and_then(|client| client.check_key()).map_err(|e| format!("{e:#}"))
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    match res {
        Ok(dub_llm::openrouter::KeyCheck::Accepted(data)) => Json(json!({ "ok": true, "data": data })).into_response(),
        Ok(dub_llm::openrouter::KeyCheck::Rejected(detail)) => Json(json!({ "ok": false, "error": detail })).into_response(),
        Err(e) => Json(json!({ "ok": false, "error": e })).into_response(),
    }
}

// ─── GET /engine/server/models?url= — модели локального OpenAI-совместимого сервера ──────────────────────
// Сервер пользователя (Ollama, LM Studio, vLLM, llama-server) отвечает на GET <адрес>/v1/models списком
// {data:[{id}]}. Спрашиваем через сервер студии, чтобы окно не ходило на чужой адрес само. Без url — адрес из
// настроек. Ключ из хранилища уходит только на адрес, для которого его сохранили.
pub async fn server_models(State(st): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let url = q
        .get("url")
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| crate::engines::models::server_url(&st.models_root));
    let res = tokio::task::spawn_blocking(move || list_server_models(&url, crate::platform::credentials::local_server_key_for(&url)))
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
    match res {
        Ok(models) => Json(json!({ "models": models })).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": "server_unreachable", "detail": e }))).into_response(),
    }
}

/// id моделей сервера по адресу `url` (с `/v1` или без).
pub(crate) fn list_server_models(url: &str, key: Option<String>) -> Result<Vec<String>, String> {
    let base = dub_llm::server_base(url);
    if base.is_empty() {
        return Err(t!("llm-server-no-address"));
    }
    let endpoint = format!("{base}/v1/models");
    let client = dub_llm::net::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("http: {e}"))?;
    let mut request = client.get(&endpoint);
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    let response = request.send().map_err(|e| t!("llm-server-no-answer", base = base.clone(), reason = dub_llm::net::why(&e)))?;
    let status = response.status();
    if !status.is_success() {
        return Err(t!("llm-server-status", endpoint = endpoint.clone(), status = status.to_string()));
    }
    let body: Value = response.json().map_err(|e| t!("llm-server-not-json", endpoint = endpoint.clone(), error = e.to_string()))?;
    let models = body
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| t!("llm-server-not-model-list", endpoint = endpoint.clone()))?;
    Ok(models.iter().filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string)).collect())
}

// ─── POST /engine/proxy/test {mode?, kind?, url, password?} — проверить связность ──────────────────────────
// Отвечают ли HF (закачка моделей) и OpenRouter (облако) при таком прокси — до сохранения. mode по умолчанию
// custom (прежняя форма {url}); прямой доступ — mode=off, прокси Windows — mode=system. Ответ с любым
// HTTP-статусом считается: меряем транспорт. 15 с на каждый, оба параллельно, причины ошибок цепочкой.
pub async fn proxy_test(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let mode = match body.get("mode").and_then(Value::as_str) {
        None => dub_llm::net::ProxyMode::Custom,
        Some(mode) => match dub_llm::net::ProxyMode::parse(mode) {
            Some(mode) => mode,
            None => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_proxy_mode", "detail": mode }))).into_response(),
        },
    };
    let kind = match body.get("kind").and_then(Value::as_str) {
        None => crate::engines::models::proxy_kind(&crate::engines::models::load_selection(&st.models_root)),
        Some(kind) => match dub_llm::net::ProxyKind::parse(kind) {
            Some(kind) => kind,
            None => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_proxy_kind", "detail": kind }))).into_response(),
        },
    };
    let address = crate::app::secrets_api::proxy_test_address(
        &st.models_root,
        crate::platform::credentials::secrets_dir().as_deref(),
        body.get("url").and_then(Value::as_str).unwrap_or(""),
        body.get("password").and_then(Value::as_str),
    );
    let settings = dub_llm::net::ProxySettings { mode, kind, address: Some(address).filter(|a| !a.trim().is_empty()) };
    let res = tokio::task::spawn_blocking(move || dub_llm::net::test(settings).map_err(|e| format!("{e:#}")))
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
    match res {
        Ok(v) => {
            let hf = v["huggingface"].as_bool() == Some(true);
            let or = v["openrouter"].as_bool() == Some(true);
            Json(json!({
                "ok": hf && or,
                "hf": hf,
                "openrouter": or,
                "hf_error": v["huggingface_error"],
                "openrouter_error": v["openrouter_error"],
            }))
            .into_response()
        }
        Err(e) => Json(json!({ "ok": false, "error": e })).into_response(),
    }
}

// ─── Пресеты железа: GET /engine/presets (список + детект GPU + рекомендация + текущий), POST /engine/preset {id} ──

pub async fn presets_list(State(st): State<AppState>) -> Response {
    let models_root = st.models_root.clone();
    let lookup = move || {
        let rec = crate::engines::presets::recommend();
        let current = crate::engines::presets::current(&models_root, rec.recommended);
        (rec, current)
    };
    let (rec, current) = match tokio::task::spawn_blocking(lookup.clone()).await {
        Ok(found) => found,
        Err(_) => lookup(),
    };
    let list: Vec<Value> = crate::engines::presets::PRESETS
        .iter()
        .map(|p| json!({ "id": p.id, "title": (p.title)(), "subtitle": (p.subtitle)() }))
        .collect();
    Json(json!({ "presets": list, "hardware": rec, "current": current })).into_response()
}

pub async fn preset_apply(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let id = body.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    match crate::engines::presets::apply(&st.models_root, &id) {
        Ok(applied) => Json(json!({
            "ok": true,
            "id": id,
            "applied": applied.iter().map(|(k, v)| json!({ "key": k, "value": v })).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

// GET /engine/openrouter/voices?model=<id> — голоса TTS-модели с полом/возрастом/русским из встроенного
// справочника (voice_db, собран роем). Для дропдауна выбора голоса + автокастинга. Нет в справочнике ->
// пустой список (юзер введёт голос вручную).
pub async fn openrouter_voices(Query(q): Query<HashMap<String, String>>) -> Response {
    let model = q.get("model").cloned().unwrap_or_default();
    let voices = crate::integrations::cloud_voices::list(&model).cloned().unwrap_or_default();
    let ru = crate::integrations::cloud_voices::model_supports_russian(&model);
    Json(json!({ "voices": voices, "supportsRussian": ru })).into_response()
}

// ─── POST /engine/select ────────────────────────────────────────────────────
// Выбрать активный вариант модели (квант) для движка. Тело: {"id":"higgs-q6_k"} — id компонента из
// манифеста настроек. Пишем models/active.json; резолв при следующей генерации подхватит выбор без
// рестарта. Это ФАКТИЧЕСКИЙ свап-слот (в отличие от set_opts, который остался эхо-совместимостью).
pub async fn select_model(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    // Форма 1: {"key":"asr_engine","value":"whisper"} — прямая установка слота (движок/модель/квант ASR
    // и т.п.) из настроек, без скачивания. Ключ валидируем по белому списку.
    if let (Some(key), Some(val)) =
        (body.get("key").and_then(Value::as_str), body.get("value").and_then(Value::as_str))
    {
        if !crate::engines::models::is_selection_key(key) {
            return (StatusCode::BAD_REQUEST, format!("unknown selection key: {key:?}")).into_response();
        }
        if val.trim().is_empty() {
            return (StatusCode::BAD_REQUEST, "empty value").into_response();
        }
        if !crate::engines::models::is_selection_value(key, val) {
            return (StatusCode::BAD_REQUEST, format!("invalid value for {key}: {val:?}")).into_response();
        }
        if let Err(e) = crate::engines::models::set_selection(&st.models_root, key, val) {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("write selection: {e}")).into_response();
        }
        return Json(crate::engines::models::public_selection(&st.models_root)).into_response();
    }
    // Форма 2: {"id":"crisperwhisper-turbo"} — id компонента манифеста -> набор слотов (активация при скачивании
    // и переключение варианта). Пустой набор -> неизвестный компонент.
    let id = body.get("id").and_then(Value::as_str).unwrap_or("");
    let pairs = crate::setup::component_selection(id);
    if pairs.is_empty() {
        return (StatusCode::BAD_REQUEST, format!("unknown model component: {id:?}")).into_response();
    }
    for (engine, variant) in pairs {
        if let Err(e) = crate::engines::models::set_selection(&st.models_root, engine, &variant) {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("write selection: {e}")).into_response();
        }
    }
    Json(crate::engines::models::public_selection(&st.models_root)).into_response()
}

// ─── PUT /projects/{pid} ────────────────────────────────────────────────────
// Полная замена Project (undo/redo снапшот). Порт app.py.put_project: валидировать тело как Project,
// сохранить атомарно, вернуть Project.

// ─── GET /projects/{pid}/waveform?n=600 ─────────────────────────────────────
// Даунсэмпл-пики аудио (ffmpeg s16le 8kHz), кэш waveform.json. CPU, вне GPU-воркера. Порт app.py.

// ─── POST /projects/{pid}/remix?instruction= ────────────────────────────────
// Творческий ремикс: Gemma переписывает ВЕСЬ транскрипт по теме/инструкции, помечает всё dirty.
// Порт app.py.remix_project (использует translate.rewrite = flat_rewrite dub-translate).


// ─── GET /projects/{pid}/preview?t=&rev= ────────────────────────────────────
// ОДИН превью-кадр (PNG) через GPU-воркер (сериализовано). Порт app.py.preview (таймаут 300с).

// ─── GET /projects/{pid}/original?t= ────────────────────────────────────────
// Сырой кадр ОРИГИНАЛА (PNG) на t — для before/after. Порт app.py.original (source_frame, таймаут 60с).
// ВНИМАНИЕ: возвращает ОДИН PNG-кадр (как источник истины app.py), а не всё видео — фронт (ComparePane)
// использует это как <img src>.


#[cfg(test)]
mod provider_endpoint_tests {
    use super::*;
    use dub_llm::test_http::{serve, Reply};

    #[test]
    fn the_saved_server_key_goes_only_to_its_own_address() {
        let saved = serve(vec![Reply::json(200, r#"{"data":[{"id":"m"}]}"#)]);
        let foreign = serve(vec![Reply::json(200, r#"{"data":[{"id":"m"}]}"#)]);
        let dir = std::env::temp_dir().join(format!("speech-server-key-{}-{}", std::process::id(), uuid::Uuid::new_v4().simple()));
        crate::platform::credentials::store_local_server_key_in(&dir, &format!("{}/v1", saved.base()), Some("lm-secret")).unwrap();
        let key_for = |url: &str| crate::platform::credentials::local_server_key_in(&dir, url);

        list_server_models(&foreign.base(), key_for(&foreign.base())).unwrap();
        let sent = foreign.request(0).to_ascii_lowercase();
        assert!(!sent.contains("authorization:") && !sent.contains("lm-secret"), "{sent}");

        list_server_models(&saved.base(), key_for(&saved.base())).unwrap();
        assert!(saved.request(0).to_ascii_lowercase().contains("authorization: bearer lm-secret"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_local_server_lists_its_models_in_any_address_form() {
        let server = serve(vec![
            Reply::json(200, r#"{"object":"list","data":[{"id":"gemma3:12b"},{"id":"qwen2.5vl:7b"}]}"#),
            Reply::json(200, r#"{"data":[]}"#),
            Reply::json(200, r#"{"models":[]}"#),
            Reply::json(401, r#"{"error":"no key"}"#),
        ]);
        assert_eq!(list_server_models(&format!("{}/v1/", server.base()), Some("k".into())).unwrap(), ["gemma3:12b", "qwen2.5vl:7b"]);
        let first = server.request(0);
        assert!(first.starts_with("GET /v1/models "), "{first}");
        assert!(first.to_ascii_lowercase().contains("authorization: bearer k"));
        assert!(list_server_models(&server.base(), None).unwrap().is_empty());
        assert!(!server.request(1).to_ascii_lowercase().contains("authorization:"));
        assert!(list_server_models(&server.base(), None).unwrap_err().contains("data"));
        assert!(list_server_models(&server.base(), None).unwrap_err().contains("401"));
        assert!(list_server_models("  ", None).is_err());
    }
}
