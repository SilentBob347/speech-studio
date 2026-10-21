//! Облачные модели речи и распознавания fal и Runware через `vs-providers` VideoSOS: каталог (копия при программе,
//! затем свежий в фоне; провайдер, который не ответил, держит прежний список и говорит почему), форма модели по её
//! схеме (кэш на сутки), оценка цены до запуска и цена после, ключи с проверкой. Запуск: ручка задачи провайдера
//! пишется на диск до первого опроса и стирается по готовности — после перезапуска тот же блок опрашивается, а не
//! отправляется второй раз. Каждый вызов попадает в журнал расходов; дневной лимит ограничивает агентов и программы.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use axum::extract::{Path as AxPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use vs_providers::{fal, price, runware, ModelEntry, ModelForm, Output, Polled, Provider, Role, Submitted, Task};

use crate::credentials;
use crate::speech_api::refuse;
use crate::AppState;

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
    http: vs_providers::http::Http,
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
    entries.into_iter().filter(|e| e.tasks.iter().any(|t| matches!(t, Task::Speech | Task::Transcribe))).collect()
}

static CLOUD: OnceLock<Arc<Cloud>> = OnceLock::new();

/// Облако процесса; каталог читается с диска при первом обращении.
pub fn cloud(repo_root: &Path) -> Arc<Cloud> {
    CLOUD.get_or_init(|| Cloud::open(&repo_root.join("cloud"))).clone()
}

/// Асинхронный вызов провайдера из синхронной задачи очереди.
fn block<F: std::future::Future>(work: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(work)),
        Err(_) => tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime for a provider call").block_on(work),
    }
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
        let http = vs_providers::http::Http::new(reqwest::Client::builder().timeout(Duration::from_secs(120))).expect("an HTTP client for the cloud providers");
        Arc::new(Self { dir: dir.to_path_buf(), providers: RwLock::new(providers), http })
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

    fn status(&self) -> Value {
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
        let read = match provider {
            Provider::Runware => runware::catalog(&self.http).await.map_err(|e| e.to_string()),
            Provider::Fal => {
                let (entries, failures) = fal::catalog(&self.http, 3).await;
                if entries.is_empty() { Err(failures.join("; ")) } else { Ok(entries) }
            }
            _ => return,
        };
        match read.map(ours) {
            Ok(entries) if !entries.is_empty() => {
                let fresh = ProviderCatalog { entries, updated: Some(chrono::Utc::now().timestamp_millis()), refreshing: false, error: None };
                if let Ok(text) = serde_json::to_string(&fresh) {
                    let _ = crate::voice_library::write_atomic(&self.dir.join(format!("{}.json", provider.as_str())), text.as_bytes());
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
        self.dir.join("forms").join(format!("{}-{}.json", provider.as_str(), &blake3::hash(id.as_bytes()).to_hex()[..24]))
    }

    /// Форма модели: с диска, если ей меньше суток, иначе у провайдера (а при его недоступности — прежняя с диска).
    pub async fn form(&self, provider: Provider, id: &str) -> Result<ModelForm, String> {
        let path = self.form_path(provider, id);
        let cached = || std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<CachedForm>(&t).ok()).filter(|c| c.version == FORM_VERSION);
        let fresh = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() < FORM_DAY);
        if fresh {
            if let Some(c) = cached() {
                return Ok(c.form);
            }
        }
        let read = self.read_form(provider, id).await;
        match read {
            Ok(form) => {
                if let Ok(text) = serde_json::to_string(&CachedForm { version: FORM_VERSION, form: form.clone() }) {
                    let _ = crate::voice_library::write_atomic(&path, text.as_bytes());
                }
                Ok(form)
            }
            Err(error) => cached().map(|c| c.form).ok_or(error),
        }
    }

    async fn read_form(&self, provider: Provider, id: &str) -> Result<ModelForm, String> {
        let entry = self.find(provider, id).ok_or_else(|| t!("cloud-unknown-model", provider = provider.as_str(), model = id.to_string()))?;
        match provider {
            Provider::Runware => {
                let url = entry.source["schema"].as_str().ok_or_else(|| t!("cloud-no-schema", model = id.to_string()))?;
                let schema: Value = self.http.get_json(url, None).await.map_err(|e| e.to_string())?;
                runware::form(id, &schema).map_err(|e| e.to_string())
            }
            Provider::Fal => {
                let document = fal::openapi(&self.http, id).await.map_err(|e| e.to_string())?;
                let mut form = fal::form(id, &document).map_err(|e| e.to_string())?;
                if let Some((key, _)) = credentials::provider_key(Provider::Fal) {
                    match fal::price(&self.http, &key, id).await {
                        Ok(rule) => form.price = rule,
                        Err(error) => form.price.note = Some(error.to_string()),
                    }
                }
                Ok(form)
            }
            other => Err(t!("cloud-unknown-model", provider = other.as_str(), model = id.to_string())),
        }
    }
}

fn key_state(provider: Provider) -> Value {
    let (var, _) = credentials::provider_env(provider).unwrap_or(("", ""));
    json!({ "configured": credentials::provider_key(provider).is_some(), "source": credentials::provider_key(provider).map(|(_, s)| s), "environment_variable": var })
}

fn provider_of(name: &str) -> Result<Provider, String> {
    Provider::parse(name).filter(|p| PROVIDERS.contains(p)).ok_or_else(|| t!("cloud-unknown-provider", provider = name.to_string()))
}

// ---------------------------------------------------------------- spending

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Spent {
    pub at: i64,
    pub day: String,
    pub project: String,
    pub provider: String,
    pub model: String,
    pub task: String,
    pub estimate: Option<f64>,
    pub cost: Option<f64>,
    /// `provider` — цена из ответа, `estimate` — оценка до запуска, `unknown` — провайдер цену не назвал.
    pub cost_source: String,
    /// `window:<…>`, `agent`, `api` или `studio`.
    pub origin: String,
}

fn spend_path(repo_root: &Path) -> PathBuf {
    repo_root.join("cloud").join("spend.jsonl")
}

/// Записать вызов облака в журнал расходов.
pub fn record(repo_root: &Path, entry: Spent) {
    use std::io::Write;
    let path = spend_path(repo_root);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let line = match serde_json::to_string(&entry) {
        Ok(l) => l,
        Err(e) => return tracing::error!("[ERROR] spend record: {e}"),
    };
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            if let Err(e) = writeln!(f, "{line}") {
                tracing::error!("[ERROR] {}: {e}", path.display());
            }
        }
        Err(e) => tracing::error!("[ERROR] {}: {e}", path.display()),
    }
}

pub fn spent(repo_root: &Path) -> Vec<Spent> {
    std::fs::read_to_string(spend_path(repo_root)).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Сколько потрачено: за всё время и сегодня, по провайдерам, моделям и дням; `project` сужает до проекта.
pub fn summary(all: &[Spent], project: Option<&str>) -> Value {
    let rows: Vec<&Spent> = all.iter().filter(|s| project.is_none_or(|p| s.project == p)).collect();
    let usd = |s: &Spent| s.cost.or(s.estimate).unwrap_or(0.0);
    let group = |key: &dyn Fn(&Spent) -> String| {
        let mut m: std::collections::BTreeMap<String, (f64, usize, usize)> = Default::default();
        for s in &rows {
            let e = m.entry(key(s)).or_default();
            e.0 += usd(s);
            e.1 += 1;
            e.2 += usize::from(s.cost.is_none() && s.estimate.is_none());
        }
        m.into_iter().map(|(k, (usd, runs, unknown))| json!({ "key": k, "usd": usd, "runs": runs, "unpriced": unknown })).collect::<Vec<_>>()
    };
    let day = today();
    json!({
        "total": rows.iter().map(|s| usd(s)).fold(0.0, |a, b| a + b),
        "today": rows.iter().filter(|s| s.day == day).map(|s| usd(s)).fold(0.0, |a, b| a + b),
        "unpriced": rows.iter().filter(|s| s.cost.is_none() && s.estimate.is_none()).count(),
        "providers": group(&|s| s.provider.clone()),
        "models": group(&|s| format!("{}:{}", s.provider, s.model)),
        "days": group(&|s| s.day.clone()),
    })
}

/// Дневной лимит агентов и программ (USD, 0 — без лимита) из настроек студии.
fn daily_limit(models_root: &Path) -> f64 {
    crate::models::load_selection(models_root).get("cloud_daily_limit_usd").and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64())).unwrap_or(0.0)
}

/// Отказ агенту или программе, если вызов выйдет за дневной лимит; вызов без оценки цены при лимите тоже отказ.
pub fn check_limit(repo_root: &Path, models_root: &Path, estimate: Option<f64>) -> Result<(), String> {
    check_limit_as(&crate::mcp::current_actor(), repo_root, models_root, estimate)
}

/// The same refusal for a caller named outright: the Assistant panel's conversation is an agent's.
pub fn check_limit_as(origin: &str, repo_root: &Path, models_root: &Path, estimate: Option<f64>) -> Result<(), String> {
    if !matches!(origin, "agent" | "api") {
        return Ok(());
    }
    let limit = daily_limit(models_root);
    if limit <= 0.0 {
        return Ok(());
    }
    let day = today();
    let used = spent(repo_root).iter().filter(|s| s.day == day && matches!(s.origin.as_str(), "agent" | "api")).map(|s| s.cost.or(s.estimate).unwrap_or(0.0)).fold(0.0, |a, b| a + b);
    match estimate {
        None => Err(t!("cloud-limit-unpriced", limit = format!("{limit:.2}"))),
        Some(e) if used + e > limit => Err(t!("cloud-limit-reached", used = format!("{used:.2}"), limit = format!("{limit:.2}"))),
        Some(_) => Ok(()),
    }
}

// ---------------------------------------------------------------- running

/// Поле формы с ролью или ключом.
fn field_key(form: &ModelForm, role: Role, names: &[&str]) -> Option<String> {
    form.fields.iter().find(|f| f.role == Some(role)).or_else(|| form.fields.iter().find(|f| names.contains(&f.key.as_str()))).map(|f| f.key.clone())
}

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
    let estimate = price::estimate(form, values).map(|e| e.usd);
    let handle: Option<Value> = std::fs::read_to_string(r.handle_file).ok().and_then(|t| serde_json::from_str(&t).ok());
    let finish = |outputs: Vec<Output>, cost: Option<f64>| {
        let _ = std::fs::remove_file(r.handle_file);
        record(r.repo_root, Spent {
            at: chrono::Utc::now().timestamp_millis(),
            day: today(),
            project: r.project.to_string(),
            provider: provider.as_str().into(),
            model: id.to_string(),
            task: r.task.as_str().into(),
            estimate,
            cost,
            cost_source: if cost.is_some() { "provider" } else if estimate.is_some() { "estimate" } else { "unknown" }.into(),
            origin: crate::mcp::current_actor(),
        });
        Done { outputs, cost, estimate }
    };
    let handle = match handle {
        Some(h) => h,
        None => {
            check_limit(r.repo_root, r.models_root, estimate)?;
            let submitted = block(async {
                match provider {
                    Provider::Fal => fal::submit(&cloud.http, &key, id, fal::build_input(values)).await,
                    Provider::Runware => runware::submit(&cloud.http, &key, runware::build_task(id, form, values, &uuid::Uuid::new_v4().to_string())).await,
                    _ => unreachable!("only catalog providers run here"),
                }
            })
            .map_err(|e| t!("cloud-failed", provider = provider.as_str(), error = e.to_string()))?;
            match submitted {
                Submitted::Done { outputs, cost } => return Ok(finish(outputs, cost)),
                Submitted::Pending { handle } => {
                    if let Some(dir) = r.handle_file.parent() {
                        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                    }
                    crate::voice_library::write_atomic(r.handle_file, handle.to_string().as_bytes())?;
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
        let polled = block(async {
            match provider {
                Provider::Fal => fal::poll(&cloud.http, &key, &handle).await,
                _ => runware::poll(&cloud.http, &key, &handle).await,
            }
        })
        .map_err(|e| t!("cloud-failed", provider = provider.as_str(), error = e.to_string()))?;
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

/// Байты первого звукового выхода.
fn audio_of(cloud: &Cloud, outputs: &[Output]) -> Result<Vec<u8>, String> {
    use base64::Engine;
    for o in outputs {
        match o {
            Output::Data { data_base64, .. } => return base64::engine::general_purpose::STANDARD.decode(data_base64).map_err(|e| e.to_string()),
            Output::Url { url, .. } => return block(cloud.http.get_bytes(url, None)).map(|(b, _)| b).map_err(|e| e.to_string()),
            Output::File { path, .. } => return std::fs::read(path).map_err(|e| format!("{}: {e}", path.display())),
            Output::Text { .. } => {}
        }
    }
    Err(t!("cloud-no-audio"))
}

/// Озвучить фразу облачной моделью: звук 24 кГц моно, цена и оценка. `values` — значения формы проекта; текст
/// встаёт в поле подсказки модели.
pub fn speak(r: &Run, provider: &str, id: &str, values: &Map<String, Value>, text: &str) -> Result<(Vec<f32>, Done), String> {
    let provider = provider_of(provider)?;
    let cloud = cloud(r.repo_root);
    let form = block(cloud.form(provider, id))?;
    let mut values = values.clone();
    let prompt = field_key(&form, Role::Prompt, &["text", "prompt", "input", "positivePrompt"]).ok_or_else(|| t!("cloud-no-text-field", model = id.to_string()))?;
    values.insert(prompt, json!(text));
    let done = run(&cloud, provider, id, &form, &values, r)?;
    let bytes = audio_of(&cloud, &done.outputs)?;
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
        .find(|f| matches!(f.kind, vs_providers::FieldKind::Media { media: vs_providers::MediaKind::Audio, .. }) || f.role == Some(Role::Audio))
        .map(|f| f.key.clone())
        .ok_or_else(|| t!("cloud-no-audio-field", model = id.to_string()))?;
    let mut values = Map::new();
    if !std::path::Path::new(r.handle_file).is_file() {
        let bytes = std::fs::read(audio).map_err(|e| format!("{}: {e}", audio.display()))?;
        let payload = vs_providers::MediaPayload { bytes, mime: "audio/mpeg".into(), name: "audio.mp3".into() };
        let url = block(fal::upload(&cloud.http, &key, &payload)).map_err(|e| t!("cloud-failed", provider = "fal", error = e.to_string()))?;
        values.insert(media_key, json!(url));
        if !matches!(lang, "" | "auto") {
            if let Some(f) = form.fields.iter().find(|f| f.key == "language") {
                values.insert(f.key.clone(), json!(lang));
            }
        }
    }
    let done = run(&cloud, provider, id, &form, &values, r)?;
    let text = done.outputs.iter().find_map(|o| if let Output::Text { text } = o { Some(text.clone()) } else { None }).ok_or_else(|| t!("cloud-asr-empty"))?;
    Ok((text, done))
}

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
    Json(json!({ "usd": one.as_ref().map(|e| e.usd * body.runs.max(1) as f64), "exact": one.map(|e| e.exact), "note": form.price.note })).into_response()
}

/// GET /cloud/keys — ключи fal и Runware: есть ли и откуда.
pub async fn keys() -> Json<Value> {
    Json(Value::Object(PROVIDERS.iter().map(|p| (p.as_str().to_string(), key_state(*p))).collect()))
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
    let checked = match provider {
        Provider::Fal => fal::balance(&c.http, &key).await,
        _ => runware::balance(&c.http, &key).await,
    };
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
mod tests {
    use super::*;

    fn row(day: &str, provider: &str, cost: Option<f64>, estimate: Option<f64>) -> Spent {
        Spent { day: day.into(), provider: provider.into(), project: "p".into(), model: "m".into(), cost, estimate, ..Default::default() }
    }

    #[test]
    fn spending_sums_the_real_cost_or_the_estimate_and_counts_the_unpriced() {
        let all = vec![row("2026-10-14", "fal", Some(0.02), Some(0.03)), row("2026-10-14", "runware", None, Some(0.01)), row("2026-10-13", "fal", None, None)];
        let s = summary(&all, None);
        assert!((s["total"].as_f64().unwrap() - 0.03).abs() < 1e-9);
        assert_eq!(s["unpriced"], 1);
        assert_eq!(s["days"].as_array().unwrap().len(), 2);
        assert_eq!(summary(&all, Some("other"))["total"].as_f64().map(f64::is_sign_negative), Some(false), "no negative zero");
    }

    #[test]
    fn the_shipped_catalogues_hold_speech_and_recognition_models() {
        let fal: ProviderCatalog = serde_json::from_str(shipped(Provider::Fal)).unwrap();
        let runware: ProviderCatalog = serde_json::from_str(shipped(Provider::Runware)).unwrap();
        assert!(fal.entries.iter().any(|e| e.tasks.contains(&Task::Transcribe)) && fal.entries.iter().any(|e| e.tasks.contains(&Task::Speech)));
        assert!(!runware.entries.is_empty() && ours(runware.entries.clone()).len() == runware.entries.len());
    }
}
