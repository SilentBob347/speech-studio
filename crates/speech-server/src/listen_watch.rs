//! A folder of recordings recognised in the background: every sound or video file in it becomes a Listening recording,
//! is recognised through the studio's job queue and written in the chosen formats into the output folder. Once goes
//! through the folder; keep also takes the files that come into it later. What was done is kept on disk, and a
//! recording made before is found again by its file (`project_of_file`), so after the studio restarts the work goes on
//! where it stopped.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::listen_project as lp;
use crate::AppState;

/// The formats a folder's recordings are written in.
pub const FORMATS: [&str; 5] = ["srt", "vtt", "ass", "txt", "json"];
/// How often a watched folder is looked at again for new files.
const LOOK_AGAIN: Duration = Duration::from_secs(5);
/// A file unchanged this long is taken as fully written.
const SETTLE: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct WatchConfig {
    pub folder: String,
    /// Where the files go; empty — next to the recordings.
    pub out: String,
    pub formats: Vec<String>,
    /// Keep watching the folder for new files after going through it.
    pub keep: bool,
    /// The recogniser and language a new recording starts with (empty engine — the studio's).
    pub engine: String,
    pub lang: String,
    /// Subtitle rules of a broadcaster (`dub_captions::PROFILES`) and numbers as digits, as the export takes them.
    pub rules: String,
    pub digits: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Item {
    pub file: String,
    /// The file as it was (path, size, time): a changed file is recognised again.
    pub key: String,
    pub pid: Option<String>,
    /// queued, working, done or error.
    pub state: String,
    pub error: Option<String>,
    pub outputs: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
struct Saved {
    config: WatchConfig,
    items: Vec<Item>,
    /// A pass or a watch is under way, to go on with after a restart.
    on: bool,
}

fn file(repo_root: &Path) -> PathBuf {
    repo_root.join("listen-watch.json")
}

fn state() -> &'static Mutex<Saved> {
    static S: OnceLock<Mutex<Saved>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Saved::default()))
}

fn running() -> &'static AtomicBool {
    static R: AtomicBool = AtomicBool::new(false);
    &R
}

fn save(repo_root: &Path) -> Result<(), String> {
    let s = state().lock().unwrap_or_else(|p| p.into_inner()).clone();
    let text = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
    crate::voice_library::write_atomic(&file(repo_root), text.as_bytes())
}

fn check(c: &WatchConfig) -> Result<(), String> {
    if !Path::new(&c.folder).is_dir() {
        return Err(t!("watch-no-folder", folder = c.folder.clone()));
    }
    if c.formats.is_empty() {
        return Err(t!("watch-no-formats"));
    }
    if let Some(f) = c.formats.iter().find(|f| !FORMATS.contains(&f.as_str())) {
        return Err(t!("watch-bad-format", format = f.clone(), known = FORMATS.join(", ")));
    }
    if !c.rules.is_empty() && dub_captions::RulesProfile::by_name(&c.rules).is_none() {
        return Err(t!("listen-export-rules", rules = c.rules.clone(), known = dub_captions::PROFILES.iter().map(|p| p.name).collect::<Vec<_>>().join(", ")));
    }
    Ok(())
}

/// Goes through `config.folder` in the background (and keeps watching it with `keep`).
pub fn start(st: &AppState, config: WatchConfig) -> Result<Value, String> {
    check(&config)?;
    {
        let mut s = state().lock().unwrap_or_else(|p| p.into_inner());
        if s.config.folder != config.folder {
            s.items.clear();
        }
        // a new start tries again what failed
        s.items.retain(|i| i.state != "error");
        s.config = config;
        s.on = true;
    }
    save(&st.repo_root)?;
    if !running().swap(true, Ordering::SeqCst) {
        let st = st.clone();
        tokio::spawn(async move { work(st).await });
    }
    Ok(status())
}

/// Stops after the recording being recognised now; what is left waits for the next start.
pub fn stop(repo_root: &Path) -> Result<Value, String> {
    state().lock().unwrap_or_else(|p| p.into_inner()).on = false;
    save(repo_root)?;
    Ok(status())
}

pub fn status() -> Value {
    let s = state().lock().unwrap_or_else(|p| p.into_inner()).clone();
    let count = |state: &str| s.items.iter().filter(|i| i.state == state).count();
    json!({
        "config": s.config, "on": s.on, "running": running().load(Ordering::SeqCst), "items": s.items,
        "done": count("done"), "errors": count("error"), "queued": count("queued") + count("working"),
    })
}

/// At the studio's start: the folder's work that was under way goes on.
pub fn resume(st: &AppState) {
    let text = match std::fs::read_to_string(file(&st.repo_root)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            tracing::error!("[ERROR] {}: {e}", file(&st.repo_root).display());
            return;
        }
    };
    let saved: Saved = match serde_json::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("[ERROR] {}: {e}", file(&st.repo_root).display());
            return;
        }
    };
    let on = saved.on;
    *state().lock().unwrap_or_else(|p| p.into_inner()) = saved;
    if on && !running().swap(true, Ordering::SeqCst) {
        let st = st.clone();
        tokio::spawn(async move { work(st).await });
    }
}

fn media_in(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .map_err(|e| format!("{}: {e}", folder.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && lp::is_media(&p.to_string_lossy()))
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    Ok(files)
}

fn update(key: &str, f: impl FnOnce(&mut Item)) {
    let mut s = state().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(item) = s.items.iter_mut().find(|i| i.key == key) {
        f(item);
    }
}

/// Every file of the pass shows in the list from its start, so the count tells how much is left.
fn list_queued(files: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut s = state().lock().unwrap_or_else(|p| p.into_inner());
    for path in &files {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Ok(key) = crate::listen_api::file_key(path, "listen") else { continue };
        if !s.items.iter().any(|i| i.key == key) {
            s.items.retain(|i| i.file != name);
            s.items.push(Item { file: name, key, state: "queued".into(), ..Default::default() });
        }
    }
    files
}

/// A file still being copied into the folder is waited for until it stops changing; false when the work was stopped
/// meanwhile or the file went away.
async fn settled(path: &Path) -> bool {
    loop {
        let Ok(meta) = std::fs::metadata(path) else { return false };
        let age = meta.modified().ok().and_then(|m| m.elapsed().ok()).unwrap_or_default();
        if age >= SETTLE {
            return true;
        }
        if !state().lock().unwrap_or_else(|p| p.into_inner()).on {
            return false;
        }
        tokio::time::sleep(SETTLE - age).await;
    }
}

async fn work(st: AppState) {
    loop {
        let (config, on) = {
            let s = state().lock().unwrap_or_else(|p| p.into_inner());
            (s.config.clone(), s.on)
        };
        if !on {
            break;
        }
        let files = match media_in(Path::new(&config.folder)) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("[ERROR] watched folder: {e}");
                state().lock().unwrap_or_else(|p| p.into_inner()).on = false;
                break;
            }
        };
        let files = list_queued(files);
        for path in files {
            // a start with other settings begins a new pass with them
            let current = {
                let s = state().lock().unwrap_or_else(|p| p.into_inner());
                s.on && s.config == config
            };
            if !current {
                break;
            }
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !settled(&path).await {
                continue;
            }
            let key = match crate::listen_api::file_key(&path, "listen") {
                Ok(k) => k,
                Err(e) => {
                    let mut s = state().lock().unwrap_or_else(|p| p.into_inner());
                    s.items.retain(|i| i.file != name);
                    s.items.push(Item { file: name.clone(), key: format!("unreadable:{name}"), state: "error".into(), error: Some(e), ..Default::default() });
                    continue;
                }
            };
            {
                let mut s = state().lock().unwrap_or_else(|p| p.into_inner());
                if s.items.iter().any(|i| i.key == key && (i.state == "done" || i.state == "error")) {
                    continue;
                }
                if !s.items.iter().any(|i| i.key == key) {
                    s.items.retain(|i| i.file != name);
                    s.items.push(Item { file: name.clone(), key: key.clone(), state: "queued".into(), ..Default::default() });
                }
            }
            update(&key, |i| i.state = "working".into());
            if let Err(e) = save(&st.repo_root) {
                tracing::error!("[ERROR] watched folder state: {e}");
            }
            let done = one(&st, &config, &path).await;
            update(&key, |i| match done {
                Ok((pid, outputs)) => {
                    i.pid = Some(pid);
                    i.outputs = outputs;
                    i.state = "done".into();
                    i.error = None;
                }
                Err(e) => {
                    i.state = "error".into();
                    i.error = Some(e);
                }
            });
            if let Err(e) = save(&st.repo_root) {
                tracing::error!("[ERROR] watched folder state: {e}");
            }
        }
        {
            let mut s = state().lock().unwrap_or_else(|p| p.into_inner());
            if s.config != config {
                continue;
            }
            if !config.keep {
                s.on = false;
                break;
            }
        }
        tokio::time::sleep(LOOK_AGAIN).await;
    }
    running().store(false, Ordering::SeqCst);
    if let Err(e) = save(&st.repo_root) {
        tracing::error!("[ERROR] watched folder state: {e}");
    }
}

/// One file: its recording (the one made before, when there is), recognised unless it is already, and written out.
async fn one(st: &AppState, config: &WatchConfig, path: &Path) -> Result<(String, Vec<String>), String> {
    let ws = st.workspace.clone();
    let owned = path.to_path_buf();
    let (engine, lang) = (config.engine.clone(), config.lang.clone());
    let pid = crate::speech_api::joined(
        tokio::task::spawn_blocking(move || -> Result<String, String> {
            let (pid, _) = crate::listen_api::project_of_file(&ws, &owned)?;
            let mut project = lp::load(&ws, &pid)?;
            if project.lines.is_empty() && (project.settings.engine != engine || project.settings.lang != lang) {
                project.settings.engine = engine;
                project.settings.lang = if lang.is_empty() { "auto".into() } else { lang };
                lp::save(&ws, &pid, &project)?;
            }
            Ok(pid)
        })
        .await,
    )
    .map_err(|_| t!("watch-broke"))??;
    let transcribed = !lp::load(&st.workspace, &pid)?.lines.is_empty();
    if !transcribed {
        let p = crate::listen_api::paths(st);
        let pid2 = pid.clone();
        let job: crate::jobs::JobFn = Box::new(move |progress: crate::jobs::ProgressFn| crate::listen_jobs::transcribe(&p, &pid2, &|ev| progress(ev)));
        let (_, done) = st.jobs.enqueue_awaitable(crate::jobs::JobMeta::new(crate::jobs::JobKind::Analyze, Some(&pid)), job).await.map_err(|e| match e {
            crate::jobs::EnqueueError::Conflict { kind, .. } => t!("watch-busy", kind = kind.as_str()),
            crate::jobs::EnqueueError::Store(e) => format!("job.json: {e}"),
        })?;
        done.await.map_err(|_| t!("watch-broke"))??;
    }
    let project = lp::load(&st.workspace, &pid)?;
    let out = if config.out.is_empty() { PathBuf::from(&config.folder) } else { PathBuf::from(&config.out) };
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let o = crate::transcript_out::Opts { speakers: true, times: true, rules: dub_captions::RulesProfile::by_name(&config.rules), digits: config.digits };
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| pid.clone());
    let mut outputs = Vec::new();
    for format in &config.formats {
        let text = crate::transcript_out::render(&project, format, &o)?;
        let target = out.join(format!("{stem}.{format}"));
        crate::voice_library::write_atomic(&target, text.as_bytes())?;
        outputs.push(target.to_string_lossy().into_owned());
    }
    Ok((pid, outputs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_needs_to_exist_and_known_formats_and_rules() {
        let dir = tempfile::tempdir().unwrap();
        let ok = WatchConfig { folder: dir.path().to_string_lossy().into_owned(), formats: vec!["srt".into(), "txt".into()], ..Default::default() };
        assert!(check(&ok).is_ok());
        assert!(check(&WatchConfig { folder: dir.path().join("none").to_string_lossy().into_owned(), ..ok.clone() }).is_err());
        assert!(check(&WatchConfig { formats: vec![], ..ok.clone() }).is_err());
        assert!(check(&WatchConfig { formats: vec!["doc".into()], ..ok.clone() }).is_err());
        assert!(check(&WatchConfig { rules: "Nobody".into(), ..ok.clone() }).is_err());
        for name in ["b.mp3", "A.wav", "notes.txt", "c.mkv"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let names: Vec<String> = media_in(dir.path()).unwrap().iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["A.wav", "b.mp3", "c.mkv"]);
    }
}
