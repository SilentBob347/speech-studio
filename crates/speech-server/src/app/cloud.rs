//! Облачные модели речи и распознавания fal и Runware через `vs-providers` VideoSOS: каталог (копия при программе,
//! затем свежий в фоне; провайдер, который не ответил, держит прежний список и говорит почему), форма модели по её
//! схеме (кэш на сутки), оценка цены до запуска и цена после, ключи с проверкой. Запуск: ручка задачи провайдера
//! пишется на диск до первого опроса и стирается по готовности — после перезапуска тот же блок опрашивается, а не
//! отправляется второй раз. Каждый вызов попадает в журнал расходов; дневной лимит ограничивает агентов и программы.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use super::spending::{check_limit, record, Spent};
use crate::integrations::cloud::{block, field_key, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use vs_providers::{price, ModelEntry, ModelForm, Output, Polled, Provider, Role, Submitted, Task};

use crate::credentials;

/// Провайдеры каталога здесь: OpenRouter идёт своим путём (каталог Dub).
pub const PROVIDERS: [Provider; 2] = [Provider::Fal, Provider::Runware];
/// Срок формы модели на диске.
const FORM_DAY: u64 = 24 * 3600;
const FORM_VERSION: u32 = 1;
const POLL_EVERY: Duration = Duration::from_millis(1500);

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct ProviderCatalog {
    pub entries: Vec<ModelEntry>,
    pub updated: Option<i64>,
    #[serde(skip_deserializing)]
    pub refreshing: bool,
    #[serde(skip_deserializing)]
    pub error: Option<String>,
}

pub struct Cloud {
    dir: PathBuf,
    providers: RwLock<HashMap<Provider, ProviderCatalog>>,
    client: Client,
}

#[derive(Serialize, Deserialize)]
struct CachedForm {
    version: u32,
    form: ModelForm,
}

fn shipped(provider: Provider) -> &'static str {
    match provider {
        Provider::Fal => include_str!("../../catalog/fal.json"),
        Provider::Runware => include_str!("../../catalog/runware.json"),
        _ => "{}",
    }
}

/// Модели речи и распознавания из списка провайдера.
fn ours(entries: Vec<ModelEntry>) -> Vec<ModelEntry> {
    entries
        .into_iter()
        .filter(|e| e.tasks.iter().any(|t| matches!(t, Task::Speech | Task::Transcribe)))
        .collect()
}

static CLOUD: OnceLock<Arc<Cloud>> = OnceLock::new();

/// Облако процесса; каталог читается с диска при первом обращении.
pub fn cloud(repo_root: &Path) -> Arc<Cloud> {
    CLOUD.get_or_init(|| Cloud::open(&repo_root.join("cloud"))).clone()
}

impl Cloud {
    fn open(dir: &Path) -> Arc<Self> {
        let _ = std::fs::create_dir_all(dir.join("forms"));
        let mut providers = HashMap::new();
        for provider in PROVIDERS {
            let cached = std::fs::read_to_string(dir.join(format!("{}.json", provider.as_str())))
                .ok()
                .and_then(|text| serde_json::from_str::<ProviderCatalog>(&text).ok())
                .filter(|c| !c.entries.is_empty())
                .or_else(|| serde_json::from_str::<ProviderCatalog>(shipped(provider)).ok())
                .unwrap_or_default();
            providers.insert(provider, cached);
        }
        let client = Client::new().expect("an HTTP client for the cloud providers");
        Arc::new(Self {
            dir: dir.to_path_buf(),
            providers: RwLock::new(providers),
            client,
        })
    }

    pub fn entries(&self, task: Option<Task>) -> Vec<ModelEntry> {
        let providers = self.providers.read().unwrap_or_else(|p| p.into_inner());
        PROVIDERS
            .iter()
            .flat_map(|p| providers.get(p).map(|c| c.entries.clone()).unwrap_or_default())
            .filter(|e| task.is_none_or(|t| e.tasks.contains(&t)))
            .collect()
    }

    pub fn find(&self, provider: Provider, id: &str) -> Option<ModelEntry> {
        let providers = self.providers.read().unwrap_or_else(|p| p.into_inner());
        providers.get(&provider)?.entries.iter().find(|e| e.id == id).cloned()
    }

    pub(super) fn status(&self) -> Value {
        let providers = self.providers.read().unwrap_or_else(|p| p.into_inner());
        Value::Object(
            PROVIDERS
                .iter()
                .map(|p| {
                    let c = providers.get(p).cloned().unwrap_or_default();
                    (p.as_str().to_string(), json!({ "models": c.entries.len(), "updated": c.updated, "refreshing": c.refreshing, "error": c.error, "key": key_state(*p) }))
                })
                .collect(),
        )
    }

    fn set(&self, provider: Provider, change: impl FnOnce(&mut ProviderCatalog)) {
        let mut providers = self.providers.write().unwrap_or_else(|p| p.into_inner());
        change(providers.entry(provider).or_default());
    }

    /// Каталог провайдера заново; прежний список остаётся, пока новый не прочитан целиком.
    pub async fn refresh(self: &Arc<Self>, provider: Provider) {
        self.set(provider, |c| {
            c.refreshing = true;
            c.error = None;
        });
        if !PROVIDERS.contains(&provider) {
            return;
        }
        let read = self.client.catalog(provider).await;
        match read.map(ours) {
            Ok(entries) if !entries.is_empty() => {
                let fresh = ProviderCatalog {
                    entries,
                    updated: Some(chrono::Utc::now().timestamp_millis()),
                    refreshing: false,
                    error: None,
                };
                if let Ok(text) = serde_json::to_string(&fresh) {
                    let _ = crate::platform::fs::write_atomic(&self.dir.join(format!("{}.json", provider.as_str())), text.as_bytes());
                }
                self.set(provider, |c| *c = fresh);
            }
            Ok(_) => self.set(provider, |c| {
                c.refreshing = false;
                c.error = Some(t!("cloud-catalog-empty"));
            }),
            Err(error) => {
                tracing::warn!("[ERROR] {} catalogue: {error}", provider.as_str());
                self.set(provider, |c| {
                    c.refreshing = false;
                    c.error = Some(error);
                });
            }
        }
    }

    fn form_path(&self, provider: Provider, id: &str) -> PathBuf {
        self.dir.join("forms").join(format!(
            "{}-{}.json",
            provider.as_str(),
            &blake3::hash(id.as_bytes()).to_hex()[..24]
        ))
    }

    /// Форма модели: с диска, если ей меньше суток, иначе у провайдера (а при его недоступности — прежняя с диска).
    pub async fn form(&self, provider: Provider, id: &str) -> Result<ModelForm, String> {
        let path = self.form_path(provider, id);
        let cached = || {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<CachedForm>(&t).ok())
                .filter(|c| c.version == FORM_VERSION)
        };
        let fresh = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age.as_secs() < FORM_DAY);
        if fresh {
            if let Some(c) = cached() {
                return Ok(c.form);
            }
        }
        let read = self.read_form(provider, id).await;
        match read {
            Ok(form) => {
                if let Ok(text) = serde_json::to_string(&CachedForm {
                    version: FORM_VERSION,
                    form: form.clone(),
                }) {
                    let _ = crate::platform::fs::write_atomic(&path, text.as_bytes());
                }
                Ok(form)
            }
            Err(error) => cached().map(|c| c.form).ok_or(error),
        }
    }

    async fn read_form(&self, provider: Provider, id: &str) -> Result<ModelForm, String> {
        let entry = self
            .find(provider, id)
            .ok_or_else(|| t!("cloud-unknown-model", provider = provider.as_str(), model = id.to_string()))?;
        match provider {
            Provider::Runware => {
                let url = entry.source["schema"]
                    .as_str()
                    .ok_or_else(|| t!("cloud-no-schema", model = id.to_string()))?;
                let schema = self.client.schema(url).await?;
                self.client.runware_form(id, &schema)
            }
            Provider::Fal => {
                let mut form = self.client.fal_form(id).await?;
                if let Some((key, _)) = credentials::provider_key(Provider::Fal) {
                    match self.client.fal_price(&key, id).await {
                        Ok(rule) => form.price = rule,
                        Err(error) => form.price.note = Some(error.to_string()),
                    }
                }
                Ok(form)
            }
            other => Err(t!("cloud-unknown-model", provider = other.as_str(), model = id.to_string())),
        }
    }
    pub(super) async fn balance(&self, provider: Provider, key: &str) -> Result<Option<f64>, vs_providers::ProviderError> {
        self.client.balance(provider, key).await
    }
}

pub(super) fn key_state(provider: Provider) -> Value {
    let (var, _) = credentials::provider_env(provider).unwrap_or(("", ""));
    json!({ "configured": credentials::provider_key(provider).is_some(), "source": credentials::provider_key(provider).map(|(_, s)| s), "environment_variable": var })
}

pub(super) fn provider_of(name: &str) -> Result<Provider, String> {
    Provider::parse(name)
        .filter(|p| PROVIDERS.contains(p))
        .ok_or_else(|| t!("cloud-unknown-provider", provider = name.to_string()))
}

// ---------------------------------------------------------------- running

/// Что нужно знать о запуске для записи в журнал.
pub struct Run<'a> {
    pub repo_root: &'a Path,
    pub models_root: &'a Path,
    pub project: &'a str,
    pub task: Task,
    /// Файл ручки задачи провайдера: пока он есть, задача уже отправлена и только опрашивается.
    pub handle_file: &'a Path,
}

/// Результат облака: что вышло и сколько стоило.
pub struct Done {
    pub outputs: Vec<Output>,
    pub cost: Option<f64>,
    pub estimate: Option<f64>,
}

fn run(cloud: &Cloud, provider: Provider, id: &str, form: &ModelForm, values: &Map<String, Value>, r: &Run) -> Result<Done, String> {
    let (key, _) = credentials::provider_key(provider).ok_or_else(|| t!("cloud-no-key", provider = provider.as_str()))?;
    run_with(
        provider,
        id,
        form,
        values,
        r,
        || block(cloud.client.submit(provider, &key, id, form, values)).map_err(|e| e.to_string()),
        |handle| block(cloud.client.poll(provider, &key, handle)).map_err(|e| e.to_string()),
    )
}

fn run_with(
    provider: Provider,
    id: &str,
    form: &ModelForm,
    values: &Map<String, Value>,
    r: &Run,
    submit: impl FnOnce() -> Result<Submitted, String>,
    mut poll: impl FnMut(&Value) -> Result<Polled, String>,
) -> Result<Done, String> {
    let estimate = price::estimate(form, values).map(|e| e.usd);
    let handle: Option<Value> = std::fs::read_to_string(r.handle_file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let finish = |outputs: Vec<Output>, cost: Option<f64>| {
        let _ = std::fs::remove_file(r.handle_file);
        record(
            r.repo_root,
            Spent {
                at: chrono::Utc::now().timestamp_millis(),
                day: chrono::Local::now().format("%Y-%m-%d").to_string(),
                project: r.project.to_string(),
                provider: provider.as_str().into(),
                model: id.to_string(),
                task: r.task.as_str().into(),
                estimate,
                cost,
                cost_source: if cost.is_some() {
                    "provider"
                } else if estimate.is_some() {
                    "estimate"
                } else {
                    "unknown"
                }
                .into(),
                origin: crate::app::context::current_actor(),
            },
        );
        Done { outputs, cost, estimate }
    };
    let handle = match handle {
        Some(h) => h,
        None => {
            check_limit(r.repo_root, r.models_root, estimate)?;
            let submitted = submit().map_err(|e| t!("cloud-failed", provider = provider.as_str(), error = e.to_string()))?;
            match submitted {
                Submitted::Done { outputs, cost } => return Ok(finish(outputs, cost)),
                Submitted::Pending { handle } => {
                    if let Some(dir) = r.handle_file.parent() {
                        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                    }
                    crate::platform::fs::write_atomic(r.handle_file, handle.to_string().as_bytes())?;
                    handle
                }
            }
        }
    };
    let ctl = crate::jobs::current();
    loop {
        if ctl.as_ref().is_some_and(|c| c.is_cancelled()) {
            return Err(crate::jobs::CANCELLED.to_string());
        }
        let polled = poll(&handle).map_err(|e| t!("cloud-failed", provider = provider.as_str(), error = e.to_string()))?;
        match polled {
            Polled::Waiting { .. } => std::thread::sleep(POLL_EVERY),
            Polled::Done { outputs, cost } => return Ok(finish(outputs, cost)),
            Polled::Failed { message } => {
                let _ = std::fs::remove_file(r.handle_file);
                return Err(t!("cloud-failed", provider = provider.as_str(), error = message));
            }
        }
    }
}

/// Озвучить фразу облачной моделью: звук 24 кГц моно, цена и оценка. `values` — значения формы проекта; текст
/// встаёт в поле подсказки модели.
pub fn speak(r: &Run, provider: &str, id: &str, values: &Map<String, Value>, text: &str) -> Result<(Vec<f32>, Done), String> {
    let provider = provider_of(provider)?;
    let cloud = cloud(r.repo_root);
    let form = block(cloud.form(provider, id))?;
    let mut values = values.clone();
    let prompt = field_key(&form, Role::Prompt, &["text", "prompt", "input", "positivePrompt"])
        .ok_or_else(|| t!("cloud-no-text-field", model = id.to_string()))?;
    values.insert(prompt, json!(text));
    let done = run(&cloud, provider, id, &form, &values, r)?;
    let bytes = cloud.client.audio(&done.outputs)?;
    let dir = tempfile::Builder::new().prefix("cloud-").tempdir().map_err(|e| e.to_string())?;
    let src = dir.path().join("out.bin");
    let wav = dir.path().join("out.wav");
    std::fs::write(&src, &bytes).map_err(|e| e.to_string())?;
    crate::media::extract_audio(&src, &wav, 24_000, 1)?;
    let (samples, _) = crate::wavio::read_mono_f32(&wav)?;
    Ok((samples, done))
}

/// Распознать запись облачной моделью fal: текст целиком (фразы без слов).
pub fn transcribe(r: &Run, id: &str, audio: &Path, lang: &str) -> Result<(String, Done), String> {
    let provider = Provider::Fal;
    let cloud = cloud(r.repo_root);
    let form = block(cloud.form(provider, id))?;
    let (key, _) = credentials::provider_key(provider).ok_or_else(|| t!("cloud-no-key", provider = provider.as_str()))?;
    let media_key = form
        .fields
        .iter()
        .find(|f| {
            matches!(
                f.kind,
                vs_providers::FieldKind::Media {
                    media: vs_providers::MediaKind::Audio,
                    ..
                }
            ) || f.role == Some(Role::Audio)
        })
        .map(|f| f.key.clone())
        .ok_or_else(|| t!("cloud-no-audio-field", model = id.to_string()))?;
    let mut values = Map::new();
    if !std::path::Path::new(r.handle_file).is_file() {
        let bytes = std::fs::read(audio).map_err(|e| format!("{}: {e}", audio.display()))?;
        let payload = vs_providers::MediaPayload {
            bytes,
            mime: "audio/mpeg".into(),
            name: "audio.mp3".into(),
        };
        let url = block(cloud.client.upload(&key, &payload)).map_err(|e| t!("cloud-failed", provider = "fal", error = e.to_string()))?;
        values.insert(media_key, json!(url));
        if !matches!(lang, "" | "auto") {
            if let Some(f) = form.fields.iter().find(|f| f.key == "language") {
                values.insert(f.key.clone(), json!(lang));
            }
        }
    }
    let done = run(&cloud, provider, id, &form, &values, r)?;
    let text = done
        .outputs
        .iter()
        .find_map(|o| if let Output::Text { text } = o { Some(text.clone()) } else { None })
        .ok_or_else(|| t!("cloud-asr-empty"))?;
    Ok((text, done))
}

#[cfg(test)]
mod boundary_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_catalogues_hold_speech_and_recognition_models() {
        let fal: ProviderCatalog = serde_json::from_str(shipped(Provider::Fal)).unwrap();
        let runware: ProviderCatalog = serde_json::from_str(shipped(Provider::Runware)).unwrap();
        assert!(
            fal.entries.iter().any(|e| e.tasks.contains(&Task::Transcribe)) && fal.entries.iter().any(|e| e.tasks.contains(&Task::Speech))
        );
        assert!(!runware.entries.is_empty() && ours(runware.entries.clone()).len() == runware.entries.len());
    }
}
