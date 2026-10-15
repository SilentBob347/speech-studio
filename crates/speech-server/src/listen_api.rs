//! Маршруты Listening: запись в проект (файл, микрофон; ссылка — через загрузку по ссылке), транскрипт, распознавание
//! и монтаж по тексту задачами общей очереди, звук записи с Range, экспорт, проверка по эталонному тексту, проект
//! Speech из транскрипта и голоса из спикеров.

use std::path::Path;

use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::listen_jobs::{self, ListenPaths};
use crate::listen_project::{self as lp, ListenProject};
use crate::speech_api::{joined, refuse};
use crate::speech_project as sp;
use crate::{jobs, AppState};

/// Форматы, которые webview играет сам; остальное (mkv, avi, wma…) слушается через AAC-копию `work/play.m4a`.
const PLAYABLE: &[&str] = &["wav", "flac", "mp3", "m4a", "aac", "ogg", "opus", "mp4", "webm", "mov", "m4v"];

pub fn paths(st: &AppState) -> ListenPaths {
    ListenPaths { repo_root: st.repo_root.clone(), workspace: st.workspace.clone(), models_root: st.models_root.clone(), sortformer_onnx: st.sortformer_onnx.clone() }
}

fn saved(r: Result<Result<Value, String>, Response>, missing: StatusCode) -> Response {
    match r {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) if e.starts_with(crate::mcp::PROJECT_CHANGED) => refuse(StatusCode::CONFLICT, crate::mcp::PROJECT_CHANGED, e),
        Ok(Err(e)) => refuse(missing, "listen_failed", e),
        Err(r) => r,
    }
}

fn play_copy(dir: &Path, source: &str) -> Result<(), String> {
    let ext = Path::new(source).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if PLAYABLE.contains(&ext.as_str()) {
        return Ok(());
    }
    let work = dir.join("work");
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    crate::media::encode_preview_aac(&dir.join(source), &work.join("play.m4a"))
}

/// Проект вокруг файла, уже лежащего в папке `pid`.
pub(crate) fn make(workspace: &Path, pid: &str, name: &str, source: &str, origin: &str) -> Result<Value, String> {
    let dir = lp::dir(workspace, pid);
    play_copy(&dir, source)?;
    let project = lp::create_in(workspace, pid, name, source, origin)?;
    Ok(json!({ "pid": pid, "project": project }))
}

fn stem(file: &str) -> String {
    Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or(file).to_string()
}

/// POST /listen/upload (multipart, поле file) — новый проект из файла записи; файл пишется на диск по частям.
pub async fn upload(State(st): State<AppState>, mut multipart: axum::extract::Multipart) -> Response {
    let pid = sp::new_id();
    let dir = lp::dir(&st.workspace, &pid);
    let mut got: Option<(String, String)> = None;
    if let Ok(Some(mut field)) = multipart.next_field().await {
        let original = field.file_name().unwrap_or("record.wav").to_string();
        if !lp::is_media(&original) {
            return refuse(StatusCode::BAD_REQUEST, "not_media", t!("listen-not-media", file = original));
        }
        let ext = Path::new(&original).extension().and_then(|e| e.to_str()).unwrap_or("wav").to_ascii_lowercase();
        let source = format!("source.{ext}");
        if let Err(e) = tokio::fs::create_dir_all(&dir).await {
            return refuse(StatusCode::INTERNAL_SERVER_ERROR, "io", format!("{}: {e}", dir.display()));
        }
        let path = dir.join(&source);
        let mut out = match tokio::fs::File::create(&path).await {
            Ok(f) => f,
            Err(e) => return refuse(StatusCode::INTERNAL_SERVER_ERROR, "io", format!("{}: {e}", path.display())),
        };
        loop {
            match field.chunk().await {
                Ok(Some(bytes)) => {
                    if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut out, &bytes).await {
                        let _ = tokio::fs::remove_dir_all(&dir).await;
                        return refuse(StatusCode::INTERNAL_SERVER_ERROR, "io", format!("{}: {e}", path.display()));
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = tokio::fs::remove_dir_all(&dir).await;
                    return refuse(StatusCode::BAD_REQUEST, "bad_request", e.to_string());
                }
            }
        }
        got = Some((original, source));
    }
    let Some((original, source)) = got else { return refuse(StatusCode::BAD_REQUEST, "bad_request", "no file") };
    let ws = st.workspace.clone();
    let res = joined(tokio::task::spawn_blocking(crate::mcp::carry(move || {
        let made = make(&ws, &pid, &stem(&original), &source, &original);
        if made.is_err() {
            let _ = std::fs::remove_dir_all(lp::dir(&ws, &pid));
        }
        made
    }))
    .await);
    saved(res, StatusCode::BAD_REQUEST)
}

static RECORDING: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// POST /listen/record/start {device?} — запись с микрофона в новый проект.
pub async fn record_start(State(st): State<AppState>, body: Option<Json<Value>>) -> Response {
    let device = body.and_then(|Json(v)| v.get("device").and_then(Value::as_str).map(str::to_string)).filter(|d| !d.is_empty());
    let pid = sp::new_id();
    let dir = lp::dir(&st.workspace, &pid);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, "io", format!("{}: {e}", dir.display()));
    }
    let path = dir.join("source.wav");
    match joined(tokio::task::spawn_blocking(move || crate::record::start(&path, device)).await) {
        Ok(Ok(())) => {
            *RECORDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(pid.clone());
            Json(json!({ "pid": pid })).into_response()
        }
        Ok(Err(e)) => {
            let _ = std::fs::remove_dir_all(&dir);
            refuse(StatusCode::CONFLICT, "record_failed", e)
        }
        Err(r) => r,
    }
}

/// POST /listen/record/stop — остановить запись: проект с ней готов к распознаванию.
pub async fn record_stop(State(st): State<AppState>) -> Response {
    let Some(pid) = RECORDING.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return refuse(StatusCode::CONFLICT, "not_recording", t!("listen-not-recording"));
    };
    let ws = st.workspace.clone();
    let res = joined(tokio::task::spawn_blocking(crate::mcp::carry(move || {
        crate::record::stop()?;
        make(&ws, &pid, &t!("listen-mic-name", time = chrono::Local::now().format("%d.%m %H:%M").to_string()), "source.wav", &t!("listen-mic-origin"))
    }))
    .await);
    saved(res, StatusCode::INTERNAL_SERVER_ERROR)
}

/// GET /listen/{pid} — проект.
pub async fn get(State(st): State<AppState>, AxPath(pid): AxPath<String>) -> Response {
    match lp::load(&st.workspace, &pid) {
        Ok(p) => Json(json!(p)).into_response(),
        Err(e) => refuse(StatusCode::NOT_FOUND, "no_project", e),
    }
}

/// PUT /listen/{pid} — сохранить правки транскрипта (тексты, спикеры, скрытие, пометки к вырезке, настройки).
pub async fn put(State(st): State<AppState>, AxPath(pid): AxPath<String>, Json(body): Json<Value>) -> Response {
    let ws = st.workspace.clone();
    let res = joined(tokio::task::spawn_blocking(crate::mcp::carry(move || -> Result<Value, String> {
        let mut project: ListenProject = serde_json::from_value(body).map_err(|e| e.to_string())?;
        let was = lp::load(&ws, &pid)?;
        project.kind = lp::KIND.into();
        project.source = was.source;
        lp::save(&ws, &pid, &project)?;
        Ok(json!(project))
    }))
    .await);
    saved(res, StatusCode::BAD_REQUEST)
}

/// GET /listen/{pid}/audio — запись (или её AAC-копия для формата, который webview не играет) с Range.
pub async fn audio(State(st): State<AppState>, AxPath(pid): AxPath<String>, req: axum::http::Request<axum::body::Body>) -> Response {
    let project = match lp::load(&st.workspace, &pid) {
        Ok(p) => p,
        Err(e) => return refuse(StatusCode::NOT_FOUND, "no_project", e),
    };
    let dir = lp::dir(&st.workspace, &pid);
    let copy = dir.join("work").join("play.m4a");
    let path = if copy.is_file() { copy } else { dir.join(&project.source) };
    if !path.is_file() {
        return refuse(StatusCode::NOT_FOUND, "no_file", t!("listen-no-source"));
    }
    crate::serve_file_range(&path, req, None).await
}

fn enqueue_job(st: &AppState, pid: &str, kind: jobs::JobKind, run: fn(&ListenPaths, &str, &dyn Fn(Value)) -> Result<Value, String>) -> impl std::future::Future<Output = Response> {
    let p = paths(st);
    let (jobs, pid) = (st.jobs.clone(), pid.to_string());
    async move {
        if let Err(e) = lp::load(&p.workspace, &pid) {
            return refuse(StatusCode::NOT_FOUND, "no_project", e);
        }
        let pid2 = pid.clone();
        let job: jobs::JobFn = Box::new(move |progress: jobs::ProgressFn| run(&p, &pid2, &|ev| progress(ev)));
        match jobs.enqueue(jobs::JobMeta::new(kind, Some(&pid)), job).await {
            Ok(job_id) => Json(json!({ "job_id": job_id })).into_response(),
            Err(e) => crate::enqueue_error(e),
        }
    }
}

/// POST /listen/{pid}/transcribe — распознать запись задачей.
pub async fn transcribe(State(st): State<AppState>, AxPath(pid): AxPath<String>) -> Response {
    enqueue_job(&st, &pid, jobs::JobKind::Analyze, listen_jobs::transcribe).await
}

/// POST /listen/{pid}/cut — «Удалить отмеченное»: новая запись и новый проект без вырезанного.
pub async fn cut(State(st): State<AppState>, AxPath(pid): AxPath<String>) -> Response {
    enqueue_job(&st, &pid, jobs::JobKind::Remix, listen_jobs::cut).await
}

/// POST /listen/{pid}/export {format, speakers?, times?} — файл транскрипта в exports проекта: {file, text}.
pub async fn export(State(st): State<AppState>, AxPath(pid): AxPath<String>, Json(body): Json<Value>) -> Response {
    let format = body.get("format").and_then(Value::as_str).unwrap_or("srt").to_string();
    let o = crate::transcript_out::Opts {
        speakers: body.get("speakers").and_then(Value::as_bool).unwrap_or(true),
        times: body.get("times").and_then(Value::as_bool).unwrap_or(true),
    };
    let ws = st.workspace.clone();
    let res = joined(tokio::task::spawn_blocking(move || -> Result<Value, String> {
        let project = lp::load(&ws, &pid)?;
        let text = crate::transcript_out::render(&project, &format, &o)?;
        let dir = lp::dir(&ws, &pid).join("exports");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let base: String = project.name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { ' ' } else { c }).collect::<String>().trim().to_string();
        let file = format!("{}.{format}", if base.is_empty() { "transcript" } else { &base });
        crate::voice_library::write_atomic(&dir.join(&file), text.as_bytes())?;
        Ok(json!({ "file": file, "text": text }))
    })
    .await);
    saved(res, StatusCode::BAD_REQUEST)
}

/// POST /listen/{pid}/aligned {reference} — строки записи с текстом эталона по времени распознанных слов; окно
/// применяет их правкой проекта (её можно отменить).
pub async fn aligned(State(st): State<AppState>, AxPath(pid): AxPath<String>, Json(body): Json<Value>) -> Response {
    let reference = body.get("reference").and_then(Value::as_str).unwrap_or_default().to_string();
    if reference.trim().is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "no_reference", t!("listen-align-no-text"));
    }
    match lp::load(&st.workspace, &pid) {
        Ok(p) if p.lines.is_empty() => refuse(StatusCode::CONFLICT, "not_transcribed", t!("listen-transcribe-first")),
        Ok(p) => Json(json!({ "lines": listen_jobs::align_to_text(&p, &reference) })).into_response(),
        Err(e) => refuse(StatusCode::NOT_FOUND, "no_project", e),
    }
}

/// POST /listen/{pid}/check {reference} — распознанное против эталонного текста по словам.
pub async fn check(State(st): State<AppState>, AxPath(pid): AxPath<String>, Json(body): Json<Value>) -> Response {
    let reference = body.get("reference").and_then(Value::as_str).unwrap_or_default();
    match lp::load(&st.workspace, &pid) {
        Ok(p) => Json(listen_jobs::check_reference(&p, reference)).into_response(),
        Err(e) => refuse(StatusCode::NOT_FOUND, "no_project", e),
    }
}

/// POST /listen/{pid}/to-speech — проект Speech из транскрипта: реплики с теми же спикерами на таймкодах записи.
pub async fn to_speech(State(st): State<AppState>, AxPath(pid): AxPath<String>) -> Response {
    let (ws, voices_dir) = (st.workspace.clone(), st.voices_dir.clone());
    let res = joined(tokio::task::spawn_blocking(crate::mcp::carry(move || -> Result<Value, String> {
        let project = lp::load(&ws, &pid)?;
        let cues: Vec<crate::subimport::Cue> =
            project.shown().map(|l| crate::subimport::Cue { start: l.start, end: l.end, text: l.text.clone(), speaker: Some(project.speaker_name(&l.speaker)) }).collect();
        if cues.is_empty() {
            return Err(t!("listen-empty"));
        }
        let mut speech = sp::from_cues(&project.name, &cues, &t!("speech-narrator"));
        for name in project.speakers.values() {
            if crate::voice_library::audio_path(&voices_dir, name).is_some() {
                speech.cast.insert(name.clone(), name.clone());
            }
        }
        let new_pid = sp::new_id();
        sp::save(&ws, &new_pid, &speech)?;
        Ok(json!({ "pid": new_pid }))
    }))
    .await);
    saved(res, StatusCode::BAD_REQUEST)
}

/// POST /listen/{pid}/voices {speaker?} — голос из спикера записи (или всех): лучший кусок его речи по границам
/// слов — в галерею голосов, с текстом и словами куска.
pub async fn voices(State(st): State<AppState>, AxPath(pid): AxPath<String>, body: Option<Json<Value>>) -> Response {
    let only = body.and_then(|Json(v)| v.get("speaker").and_then(Value::as_str).map(str::to_string));
    let (ws, voices_dir, models_root) = (st.workspace.clone(), st.voices_dir.clone(), st.models_root.clone());
    let res = joined(tokio::task::spawn_blocking(crate::mcp::carry(move || voices_from_speakers(&ws, &voices_dir, &models_root, &pid, only.as_deref()))).await);
    saved(res, StatusCode::BAD_REQUEST)
}

fn profile_paths(st: &AppState) -> crate::speaker_profiles::Paths {
    crate::speaker_profiles::Paths { repo_root: st.repo_root.clone(), workspace: st.workspace.clone(), voices_dir: st.voices_dir.clone(), models_root: st.models_root.clone() }
}

/// POST /listen/{pid}/speakers/match — who each speaker of the recording is like among the named speakers and the
/// library voices, by the voice print.
pub async fn speakers_match(State(st): State<AppState>, AxPath(pid): AxPath<String>) -> Response {
    let p = profile_paths(&st);
    let res = joined(tokio::task::spawn_blocking(crate::mcp::carry(move || crate::speaker_profiles::matches(&p, &pid))).await);
    saved(res, StatusCode::BAD_REQUEST)
}

/// POST /listen/{pid}/speakers/remember {speaker} — keep this speaker's voice print under the name it has here.
pub async fn speaker_remember(State(st): State<AppState>, AxPath(pid): AxPath<String>, Json(body): Json<Value>) -> Response {
    let speaker = body.get("speaker").and_then(Value::as_str).unwrap_or_default().to_string();
    let p = profile_paths(&st);
    let res = joined(tokio::task::spawn_blocking(crate::mcp::carry(move || crate::speaker_profiles::remember(&p, &pid, &speaker))).await);
    saved(res, StatusCode::BAD_REQUEST)
}

/// GET /speaker-profiles — the named speakers the studio knows by voice.
pub async fn speaker_profiles(State(st): State<AppState>) -> Response {
    match crate::speaker_profiles::listing(&st.repo_root) {
        Ok(v) => Json(v).into_response(),
        Err(e) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "read_failed", e),
    }
}

/// DELETE /speaker-profiles/{name} — forget a named speaker's voice print.
pub async fn speaker_forget(State(st): State<AppState>, AxPath(name): AxPath<String>) -> Response {
    match crate::speaker_profiles::forget(&st.repo_root, &name) {
        Ok(v) => Json(v).into_response(),
        Err(e) => refuse(StatusCode::NOT_FOUND, "no_profile", e),
    }
}

fn voices_from_speakers(ws: &Path, voices_dir: &Path, models_root: &Path, pid: &str, only: Option<&str>) -> Result<Value, String> {
    let project = lp::load(ws, pid)?;
    let dir = lp::dir(ws, pid);
    let clean = dir.join("work").join("vocals16.wav");
    let pick_wav = if clean.is_file() { clean } else { dir.join("work").join("audio16.wav") };
    if !pick_wav.is_file() {
        return Err(t!("listen-transcribe-first"));
    }
    let src = lp::source_path(ws, pid, &project)?;
    let stems_vocals = dir.join("work").join("stems").join("vocals.wav");
    let cut_from = if stems_vocals.is_file() { stems_vocals } else { src };
    let (samples, sr) = crate::wavio::read_mono_f32(&pick_wav)?;
    let sel = crate::models::load_selection(models_root);
    let max_len = sel.get("higgs_ref_secs").and_then(Value::as_str).and_then(|v| v.parse::<f64>().ok()).unwrap_or(12.0);
    let mut made = Vec::new();
    let mut skipped = Vec::new();
    for (key, name) in project.speakers.iter().filter(|(k, _)| only.is_none_or(|o| o == k.as_str())) {
        let words: Vec<crate::ref_pick::Word> = project
            .lines
            .iter()
            .filter(|l| &l.speaker == key && !l.hidden && !l.approx)
            .flat_map(|l| l.words.iter().map(|w| crate::ref_pick::Word { word: w.text.clone(), start: w.start, end: w.end }))
            .collect();
        let Some(best) = crate::ref_pick::candidates(&words, &samples, sr, (max_len * 0.4).min(5.0), max_len, 1).into_iter().next() else {
            skipped.push(name.clone());
            continue;
        };
        let tmp = tempfile::Builder::new().prefix("spk-").suffix(".flac").tempfile().map_err(|e| e.to_string())?;
        crate::media::keep_audio_ranges(&cut_from, tmp.path(), &[(best.start, best.end)])?;
        let mut card = crate::voice_library::guess_card(name);
        card.source = "listening".into();
        card.transcript_auto = true;
        if !matches!(project.settings.lang.as_str(), "" | "auto") {
            card.lang = project.settings.lang.clone();
        } else if crate::stress::is_russian(&best.text, "auto") {
            card.lang = "ru".into();
        }
        let voice = crate::voice_library::import_audio(voices_dir, &crate::voice_library::sanitize(name), tmp.path(), card)?;
        crate::voice_library::write_transcript(voices_dir, &voice, best.text.trim())?;
        let local: Vec<crate::ref_pick::Word> =
            words.iter().filter(|w| w.start >= best.start - 1e-6 && w.end <= best.end + 1e-6).map(|w| crate::ref_pick::Word { word: w.word.clone(), start: w.start - best.start, end: w.end - best.start }).collect();
        crate::voice_library::write_words(voices_dir, &voice, &local)?;
        made.push(json!({ "speaker": key, "voice": voice, "seconds": best.duration }));
    }
    Ok(json!({ "made": made, "skipped": skipped }))
}

/// Карточка проекта Listening для списка проектов.
pub fn summary(project: &ListenProject) -> Value {
    json!({
        "name": project.name,
        "created": project.created,
        "duration": project.duration,
        "lines": project.shown().count(),
        "speakers": project.speakers.len(),
        "transcribed": project.made.is_some(),
    })
}

/// Ключ файла для инструментов агента: путь, размер и время изменения.
pub fn file_key(path: &Path, tool: &str) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let stamp = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    Ok(blake3::hash(format!("{tool}|{}|{}|{stamp}", path.display(), meta.len()).as_bytes()).to_hex()[..16].to_string())
}

/// POST /listen/from-path {path} — проект Listening файла этого компьютера (для агента): тот же файл с теми же
/// размером и временем находит прежний проект.
pub async fn from_path(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let path = std::path::PathBuf::from(body.get("path").and_then(Value::as_str).unwrap_or_default());
    let ws = st.workspace.clone();
    let res = joined(tokio::task::spawn_blocking(crate::mcp::carry(move || -> Result<Value, String> {
        if !path.is_file() || !lp::is_media(&path.to_string_lossy()) {
            return Err(t!("listen-not-media", file = path.display().to_string()));
        }
        let key = file_key(&path, "listen")?;
        let found = std::fs::read_dir(&ws).map_err(|e| e.to_string())?.flatten().find_map(|e| {
            let pid = e.file_name().to_string_lossy().into_owned();
            lp::load(&ws, &pid).ok().filter(|p| p.key == key).map(|_| pid)
        });
        if let Some(pid) = found {
            return Ok(json!({ "pid": pid, "existing": true }));
        }
        let pid = sp::new_id();
        let dir = lp::dir(&ws, &pid);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("wav").to_ascii_lowercase();
        let source = format!("source.{ext}");
        if let Err(e) = std::fs::copy(&path, dir.join(&source)) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!("{}: {e}", path.display()));
        }
        let made = make(&ws, &pid, &stem(&path.to_string_lossy()), &source, &path.to_string_lossy());
        if made.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        made?;
        let mut project = lp::load(&ws, &pid)?;
        project.key = key;
        lp::save(&ws, &pid, &project)?;
        Ok(json!({ "pid": pid, "existing": false }))
    }))
    .await);
    saved(res, StatusCode::BAD_REQUEST)
}

/// POST /separate {path} — голос файла отдельно от музыки и шума (Roformer) задачей; готовый — сразу путь.
pub async fn separate_file(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let path = std::path::PathBuf::from(body.get("path").and_then(Value::as_str).unwrap_or_default());
    if !path.is_file() || !lp::is_media(&path.to_string_lossy()) {
        return refuse(StatusCode::BAD_REQUEST, "not_media", t!("listen-not-media", file = path.display().to_string()));
    }
    let key = match file_key(&path, "separate") {
        Ok(k) => k,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, "bad_request", e),
    };
    let out = st.workspace.join(".separate").join(&key);
    let vocals = out.join("vocals.flac");
    if vocals.is_file() {
        return Json(json!({ "done": true, "vocals": vocals, "instrumental": out.join("instrumental.flac") })).into_response();
    }
    let (repo_root, models_root) = (st.repo_root.clone(), st.models_root.clone());
    let job: jobs::JobFn = Box::new(move |progress: jobs::ProgressFn| crate::listen_jobs::separate_to(&repo_root, &models_root, &path, &out, &|ev| progress(ev)));
    match st.jobs.enqueue(jobs::JobMeta::new(jobs::JobKind::Separate, None), job).await {
        Ok(job_id) => Json(json!({ "done": false, "job_id": job_id })).into_response(),
        Err(e) => crate::enqueue_error(e),
    }
}
