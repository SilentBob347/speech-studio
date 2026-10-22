use super::cloud::{cloud, key_state, provider_of, PROVIDERS};
use super::spending::{daily_limit, spent, summary};
use crate::integrations::cloud::field_key;
use crate::app::refuse;
use crate::platform::credentials;
use crate::AppState;
use axum::extract::{Path as AxPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use vs_providers::{price, Role, Task};

// ---------------------------------------------------------------- routes

#[derive(Deserialize)]
pub struct CatalogQuery {
    task: Option<String>,
}

/// GET /cloud/catalog?task=speech|transcribe — модели fal и Runware и состояние каталогов и ключей.
pub async fn catalog(State(st): State<AppState>, Query(q): Query<CatalogQuery>) -> Json<Value> {
    let c = cloud(&st.repo_root);
    let task = q.task.as_deref().and_then(Task::parse);
    Json(json!({ "models": c.entries(task), "providers": c.status() }))
}

/// POST /cloud/catalog/refresh — прочитать каталоги заново в фоне.
pub async fn refresh(State(st): State<AppState>) -> StatusCode {
    let c = cloud(&st.repo_root);
    for p in PROVIDERS {
        let c = c.clone();
        tokio::spawn(async move { c.refresh(p).await });
    }
    StatusCode::ACCEPTED
}

#[derive(Deserialize)]
pub struct FormQuery {
    provider: String,
    id: String,
}

/// GET /cloud/form?provider&id — поля модели, её цена и как она отвечает.
pub async fn form(State(st): State<AppState>, Query(q): Query<FormQuery>) -> Response {
    let provider = match provider_of(&q.provider) {
        Ok(p) => p,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, "unknown_provider", e),
    };
    match cloud(&st.repo_root).form(provider, &q.id).await {
        Ok(f) => Json(json!(f)).into_response(),
        Err(e) => refuse(StatusCode::BAD_GATEWAY, "form_failed", e),
    }
}

#[derive(Deserialize)]
pub struct EstimateBody {
    provider: String,
    id: String,
    #[serde(default)]
    values: Map<String, Value>,
    /// Сколько раз запуск повторится (блоков в проекте).
    #[serde(default = "one")]
    runs: u32,
    /// Знаков текста на все запуски, для моделей с ценой за знак.
    #[serde(default)]
    chars: u64,
}

fn one() -> u32 {
    1
}

/// POST /cloud/estimate — цена до запуска; null — узнаем после запуска.
pub async fn estimate(State(st): State<AppState>, Json(body): Json<EstimateBody>) -> Response {
    let provider = match provider_of(&body.provider) {
        Ok(p) => p,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, "unknown_provider", e),
    };
    let form = match cloud(&st.repo_root).form(provider, &body.id).await {
        Ok(f) => f,
        Err(e) => return refuse(StatusCode::BAD_GATEWAY, "form_failed", e),
    };
    let mut values = body.values.clone();
    if body.chars > 0 {
        if let Some(k) = field_key(&form, Role::Prompt, &["text", "prompt", "input"]) {
            values.insert(k, json!("x".repeat((body.chars / body.runs.max(1) as u64) as usize)));
        }
    }
    let one = price::estimate(&form, &values);
    Json(json!({ "usd": one.as_ref().map(|e| e.usd * body.runs.max(1) as f64), "exact": one.map(|e| e.exact), "note": form.price.note }))
        .into_response()
}

/// GET /cloud/keys — ключи fal и Runware: есть ли и откуда.
pub async fn keys() -> Json<Value> {
    Json(Value::Object(
        PROVIDERS.iter().map(|p| (p.as_str().to_string(), key_state(*p))).collect(),
    ))
}

/// PUT /cloud/keys/{provider} {api_key} — проверить ключ бесплатным запросом (баланс) и сохранить.
pub async fn set_key(State(st): State<AppState>, AxPath(name): AxPath<String>, Json(body): Json<Value>) -> Response {
    let provider = match provider_of(&name) {
        Ok(p) => p,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, "unknown_provider", e),
    };
    let key = body.get("api_key").and_then(Value::as_str).unwrap_or_default().trim().to_string();
    if key.is_empty() || key.contains(['\r', '\n']) {
        return refuse(StatusCode::BAD_REQUEST, "invalid_key", t!("cloud-key-one-line"));
    }
    let c = cloud(&st.repo_root);
    let checked = c.balance(provider, &key).await;
    let balance = match checked {
        Ok(b) => b,
        Err(vs_providers::ProviderError::Refused(why)) => return refuse(StatusCode::BAD_REQUEST, "key_refused", why),
        Err(e) => return refuse(StatusCode::BAD_GATEWAY, "key_check_failed", e.to_string()),
    };
    match credentials::store_provider_key(provider, Some(&key)) {
        Ok(_) => Json(json!({ "state": key_state(provider), "balance": balance })).into_response(),
        Err(e) => refuse(StatusCode::CONFLICT, "environment_key", e.to_string()),
    }
}

/// DELETE /cloud/keys/{provider}
pub async fn delete_key(AxPath(name): AxPath<String>) -> Response {
    let provider = match provider_of(&name) {
        Ok(p) => p,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, "unknown_provider", e),
    };
    match credentials::store_provider_key(provider, None) {
        Ok(_) => Json(key_state(provider)).into_response(),
        Err(e) => refuse(StatusCode::CONFLICT, "environment_key", e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct SpendQuery {
    project: Option<String>,
}

/// GET /cloud/spend?project= — расходы облака: всего, сегодня, по провайдерам, моделям и дням, и лимит.
pub async fn spend(State(st): State<AppState>, Query(q): Query<SpendQuery>) -> Json<Value> {
    let all = spent(&st.repo_root);
    let mut s = summary(&all, q.project.as_deref());
    s["daily_limit_usd"] = json!(daily_limit(&st.models_root));
    Json(s)
}

#[cfg(test)]
mod tests;
