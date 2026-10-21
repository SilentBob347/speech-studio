//! Background link imports, persistent progress and publication of Listening projects.

use crate::integrations::url_import::{self as download, DownloadRequest, Downloaded, Progress};
use crate::ytdlp::{self, Quality, UrlError};
pub(crate) use download::Cookies;
use download::{check_lang, check_url, place_cookies};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const KEEP: usize = 20;
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FetchStatus {
    Downloading,
    Completed,
    Failed,
    Cancelled,
    /// Оборвана закрытием или падением студии; «Продолжить» докачивает с места.
    Interrupted,
}

/// Одна загрузка по ссылке.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Fetch {
    pub id: String,
    pub url: String,
    pub quality: Quality,
    /// Язык субтитров площадки, загруженных людьми; они становятся импортом субтитров проекта.
    pub subs_lang: Option<String>,
    pub cookies: bool,
    pub status: FetchStatus,
    /// probe | download | merge | extract | subtitles | project | done
    pub phase: String,
    pub title: Option<String>,
    pub duration: Option<f64>,
    pub downloaded: u64,
    /// Всего байт (сумма форматов, названная сайтом, или счётчики yt-dlp); null — неизвестно.
    pub total: Option<u64>,
    pub speed_bps: u64,
    pub eta_s: Option<u64>,
    /// Проект, в который легло видео.
    pub pid: Option<String>,
    pub subs_imported: bool,
    /// Субтитры выбраны, но не легли в проект: subs_missing (у видео нет таких) | subs_failed | subs_empty.
    pub warning: Option<String>,
    pub warning_detail: Option<String>,
    pub error_code: Option<String>,
    pub error: Option<String>,
    pub hint: Option<String>,
    pub tool_version: Option<String>,
    pub started_at: u64,
    pub updated_at: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Persistent {
    fetches: Vec<Fetch>,
}

/// Запрос загрузки.
#[derive(Clone, Debug)]
pub struct FetchRequest {
    pub url: String,
    pub quality: Quality,
    pub subs_lang: Option<String>,
    pub cookies: Option<Cookies>,
}

struct Inner {
    repo_root: PathBuf,
    root: PathBuf,
    state_path: PathBuf,
    workspace: PathBuf,
    list: Mutex<Vec<Fetch>>,
    stops: Mutex<HashMap<String, Arc<AtomicBool>>>,
    last_persist: Mutex<Instant>,
}

#[derive(Clone)]
pub struct Fetches {
    inner: Arc<Inner>,
}

fn persist(path: &Path, list: &[Fetch]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let body = serde_json::to_vec_pretty(&json!({ "fetches": list })).map_err(|e| e.to_string())?;
    dub_core::atomic::write(path, &body)
}

/// Загрузка, оставшаяся «идущей» в файле, оборвалась вместе с прошлым процессом.
fn recover_interrupted(list: &mut [Fetch]) -> bool {
    let mut any = false;
    for f in list.iter_mut().filter(|f| f.status == FetchStatus::Downloading) {
        f.status = FetchStatus::Interrupted;
        f.error_code = Some("interrupted".into());
        f.error = Some(t!("url-interrupted"));
        f.hint = Some(ytdlp::hint("interrupted").into());
        f.speed_bps = 0;
        f.eta_s = None;
        any = true;
    }
    any
}

/// Имя проекта из названия видео: без символов, запрещённых в именах файлов Windows.
fn project_name(title: &str, ext: &str) -> String {
    let clean: String = title
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let clean: String = clean.chars().take(120).collect();
    let clean = clean.trim_end_matches(['.', ' ']);
    format!("{}.{ext}", if clean.is_empty() { "video" } else { clean })
}

impl Fetches {
    pub fn open(repo_root: &Path, workspace: &Path) -> Self {
        let root = workspace.join(".fetch");
        let state_path = root.join("fetches.json");
        let mut state: Persistent = match std::fs::read_to_string(&state_path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!(
                    "{} is unreadable ({e}); the previous link downloads are forgotten",
                    state_path.display()
                );
                Persistent::default()
            }),
            Err(_) => Persistent::default(),
        };
        if recover_interrupted(&mut state.fetches) {
            if let Err(e) = persist(&state_path, &state.fetches) {
                tracing::warn!("{}: {e}", state_path.display());
            }
        }
        Fetches {
            inner: Arc::new(Inner {
                repo_root: repo_root.to_path_buf(),
                root,
                state_path,
                workspace: workspace.to_path_buf(),
                list: Mutex::new(state.fetches),
                stops: Mutex::new(HashMap::new()),
                last_persist: Mutex::new(Instant::now()),
            }),
        }
    }

    fn dir(&self, id: &str) -> PathBuf {
        self.inner.root.join(id)
    }

    fn save(&self, list: &[Fetch]) {
        if let Err(e) = persist(&self.inner.state_path, list) {
            tracing::warn!("{}: {e}", self.inner.state_path.display());
        }
        *lock(&self.inner.last_persist) = Instant::now();
    }

    pub fn list(&self) -> Vec<Fetch> {
        lock(&self.inner.list).clone()
    }

    pub fn get(&self, id: &str) -> Option<Fetch> {
        lock(&self.inner.list).iter().find(|f| f.id == id).cloned()
    }

    /// Изменить загрузку `id`, пока она идёт; на диск — не чаще раза в пару секунд.
    fn update(&self, id: &str, change: impl FnOnce(&mut Fetch)) {
        let mut list = lock(&self.inner.list);
        let Some(f) = list.iter_mut().find(|f| f.id == id && f.status == FetchStatus::Downloading) else {
            return;
        };
        change(f);
        f.updated_at = ytdlp::now_s();
        if lock(&self.inner.last_persist).elapsed() > Duration::from_secs(2) {
            self.save(&list);
        }
    }

    /// Начать загрузку своим потоком. Нет yt-dlp или ffmpeg, негодная ссылка или cookies — отказ до старта.
    pub fn start(&self, req: FetchRequest) -> Result<Fetch, UrlError> {
        let url = check_url(&req.url)?;
        let subs_lang = req.subs_lang.as_deref().map(check_lang).transpose()?;
        let tool = ytdlp::tool(&self.inner.repo_root)?;
        ytdlp::update_in_background_if_due(&self.inner.repo_root);
        let mut id = uuid::Uuid::new_v4().simple().to_string();
        id.truncate(12);
        let id = format!("url{id}");
        let now = ytdlp::now_s();
        let fetch = Fetch {
            id: id.clone(),
            url,
            quality: req.quality,
            subs_lang,
            cookies: req.cookies.is_some(),
            status: FetchStatus::Downloading,
            phase: "probe".into(),
            title: None,
            duration: None,
            downloaded: 0,
            total: None,
            speed_bps: 0,
            eta_s: None,
            pid: None,
            subs_imported: false,
            warning: None,
            warning_detail: None,
            error_code: None,
            error: None,
            hint: None,
            tool_version: Some(tool.version.clone()),
            started_at: now,
            updated_at: now,
        };
        // Проверка «эта ссылка уже качается» и вставка — под одним замком: два запроса одной ссылки подряд не
        // становятся двумя загрузками.
        {
            let mut list = lock(&self.inner.list);
            if let Some(busy) = list.iter().find(|f| f.status == FetchStatus::Downloading && f.url == fetch.url) {
                return Err(UrlError::new("busy", t!("url-already-downloading", id = busy.id.clone())));
            }
            list.insert(0, fetch.clone());
            self.evict(&mut list);
            self.save(&list);
        }
        let dir = self.dir(&id);
        let placed = std::fs::create_dir_all(&dir)
            .map_err(|e| UrlError::new("io", format!("{}: {e}", dir.display())))
            .and_then(|()| req.cookies.as_ref().map_or(Ok(()), |c| place_cookies(c, &dir.join("cookies.txt"))));
        if let Err(e) = placed {
            self.drop_unstarted(&id, &dir);
            return Err(e);
        }
        self.spawn(&id, tool)?;
        Ok(fetch)
    }

    /// Убрать из списка загрузку, которая не началась (папка или cookies не легли).
    fn drop_unstarted(&self, id: &str, dir: &Path) {
        if dir.is_dir() {
            if let Err(e) = std::fs::remove_dir_all(dir) {
                tracing::warn!(
                    "the folder of a download that did not start, {}, was not removed: {e}",
                    dir.display()
                );
            }
        }
        let mut list = lock(&self.inner.list);
        list.retain(|f| f.id != id);
        self.save(&list);
    }

    /// Старые завершённые загрузки уходят из списка вместе с папками; идущие остаются всегда.
    fn evict(&self, list: &mut Vec<Fetch>) {
        while list.len() > KEEP {
            let Some(at) = list.iter().rposition(|f| f.status != FetchStatus::Downloading) else {
                break;
            };
            let old = list.remove(at);
            let dir = self.dir(&old.id);
            if dir.is_dir() {
                if let Err(e) = std::fs::remove_dir_all(&dir) {
                    tracing::warn!("the folder of an old download, {}, was not removed: {e}", dir.display());
                }
            }
        }
    }

    fn spawn(&self, id: &str, tool: ytdlp::Tool) -> Result<(), UrlError> {
        let stop = Arc::new(AtomicBool::new(false));
        lock(&self.inner.stops).insert(id.to_string(), stop.clone());
        let me = self.clone();
        let job = id.to_string();
        std::thread::Builder::new()
            .name("url-fetch".into())
            .spawn(move || me.run(&job, tool, stop))
            .map(|_| ())
            .map_err(|e| {
                self.finish(id, Err(UrlError::new("io", t!("url-thread-failed", error = e.to_string()))), false);
                UrlError::new("io", t!("url-thread-failed", error = e.to_string()))
            })
    }

    fn run(&self, id: &str, tool: ytdlp::Tool, stop: Arc<AtomicBool>) {
        let res = self.run_steps(id, &tool, &stop);
        self.finish(id, res, stop.load(Ordering::SeqCst));
    }

    /// Итог загрузки. Остановленная убирает свою папку; упавшая её оставляет — «Продолжить» докачает.
    fn finish(&self, id: &str, res: Result<(), UrlError>, stopped: bool) {
        lock(&self.inner.stops).remove(id);
        let mut list = lock(&self.inner.list);
        let Some(f) = list.iter_mut().find(|f| f.id == id) else { return };
        f.speed_bps = 0;
        f.eta_s = None;
        f.updated_at = ytdlp::now_s();
        let cancelled = stopped || res.as_ref().is_err_and(|e| e.code == "cancelled");
        match res {
            Ok(()) => {}
            Err(_) if cancelled => {
                f.status = FetchStatus::Cancelled;
                let dir = self.dir(id);
                if dir.is_dir() {
                    if let Err(e) = std::fs::remove_dir_all(&dir) {
                        tracing::warn!("the folder of a cancelled download, {}, was not removed: {e}", dir.display());
                    }
                }
            }
            Err(e) => {
                tracing::warn!("link download {}: {} ({})", f.url, e.detail, e.code);
                f.status = FetchStatus::Failed;
                f.hint = Some(e.hint().into());
                f.error_code = Some(e.code.into());
                f.error = Some(e.detail);
            }
        }
        // Пароль прокси живёт в хранилище, а не в папке загрузки: proxy.conf пишется заново к каждому запуску.
        let conf = self.dir(id).join(ytdlp::PROXY_CONF);
        match std::fs::remove_file(&conf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::error!("{} with the proxy password was not removed: {e}", conf.display()),
        }
        self.save(&list);
    }

    fn run_steps(&self, id: &str, tool: &ytdlp::Tool, stop: &Arc<AtomicBool>) -> Result<(), UrlError> {
        let fetch = self.get(id).ok_or_else(|| UrlError::new("not_found", id.to_string()))?;
        let downloaded = download::download(
            DownloadRequest {
                url: &fetch.url,
                quality: fetch.quality,
                subs_lang: fetch.subs_lang.as_deref(),
                cookies: fetch.cookies,
            },
            tool,
            &self.dir(id),
            stop,
            |progress| self.progress(id, progress),
        )?;
        self.import_download(id, downloaded, stop)
    }

    fn progress(&self, id: &str, progress: Progress) {
        self.update(id, |f| match progress {
            Progress::Phase(phase) => {
                f.phase = phase.into();
                f.speed_bps = 0;
                f.eta_s = None;
            }
            Progress::Metadata { title, duration, total } => {
                f.title = Some(title);
                f.duration = duration;
                f.total = total;
                f.phase = "download".into();
            }
            Progress::Bytes {
                downloaded,
                total,
                speed_bps,
                eta_s,
            } => {
                f.downloaded = downloaded;
                f.total = total;
                f.speed_bps = speed_bps;
                f.eta_s = eta_s;
                f.phase = "download".into();
            }
            Progress::Warning(w) => {
                f.warning = Some(w.code.into());
                f.warning_detail = Some(w.detail);
            }
        });
    }

    fn import_download(&self, id: &str, downloaded: Downloaded, stop: &Arc<AtomicBool>) -> Result<(), UrlError> {
        if stop.load(Ordering::SeqCst) {
            return Err(UrlError::new("cancelled", t!("url-stopped")));
        }
        self.update(id, |f| f.phase = "project".into());
        if let Some(w) = &downloaded.warning {
            self.progress(id, Progress::Warning(w.clone()));
        }
        let made = make_project(
            &self.inner.workspace,
            &downloaded.media,
            &downloaded.title,
            downloaded.subtitles.as_deref(),
        )?;
        self.completed(id, made);
        let dir = self.dir(id);
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!(
                "the download folder {} was not removed after moving into the project: {e}",
                dir.display()
            );
        }
        Ok(())
    }

    fn completed(&self, id: &str, made: Made) {
        // Once the project exists, a late cancellation must not orphan it from its import.
        {
            let mut list = lock(&self.inner.list);
            if let Some(f) = list.iter_mut().find(|f| f.id == id) {
                if let Some((code, detail)) = made.warning {
                    f.warning = Some(code.into());
                    f.warning_detail = Some(detail);
                }
                f.pid = Some(made.pid.clone());
                f.subs_imported = made.subs_imported;
                f.downloaded = f.total.unwrap_or(f.downloaded).max(f.downloaded);
                f.phase = "done".into();
                f.status = FetchStatus::Completed;
                f.error_code = None;
                f.error = None;
                f.hint = None;
                f.updated_at = ytdlp::now_s();
            }
            self.save(&list);
        }
        super::events::publish(json!({ "changed": "projects", "pid": made.pid, "by": "studio" }));
    }
    /// Остановить загрузку: процесс гасится со всеми потомками, её папка уходит.
    pub fn cancel(&self, id: &str) -> Result<Fetch, UrlError> {
        let stop = lock(&self.inner.stops).get(id).cloned();
        let mut list = lock(&self.inner.list);
        let f = list
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or_else(|| UrlError::new("not_found", t!("url-no-fetch", id = id.to_string())))?;
        if f.status != FetchStatus::Downloading {
            return Ok(f.clone());
        }
        if let Some(stop) = stop {
            stop.store(true, Ordering::SeqCst);
        }
        f.status = FetchStatus::Cancelled;
        f.speed_bps = 0;
        f.eta_s = None;
        f.updated_at = ytdlp::now_s();
        let out = f.clone();
        self.save(&list);
        Ok(out)
    }

    /// Продолжить прерванную или упавшую загрузку с теми же настройками; скачанное в её папке берётся с места.
    pub fn resume(&self, id: &str) -> Result<Fetch, UrlError> {
        let tool = ytdlp::tool(&self.inner.repo_root)?;
        let out = {
            let mut list = lock(&self.inner.list);
            let f = list
                .iter_mut()
                .find(|f| f.id == id)
                .ok_or_else(|| UrlError::new("not_found", t!("url-no-fetch", id = id.to_string())))?;
            match f.status {
                FetchStatus::Downloading => return Err(UrlError::new("running", t!("url-still-downloading", id = id.to_string()))),
                FetchStatus::Completed => return Ok(f.clone()),
                FetchStatus::Cancelled => {
                    return Err(UrlError::new("not_found", t!("url-cancelled-removed", id = id.to_string())));
                }
                FetchStatus::Failed | FetchStatus::Interrupted => {}
            }
            let url = f.url.clone();
            if let Some(busy) = list.iter().find(|x| x.status == FetchStatus::Downloading && x.url == url) {
                return Err(UrlError::new("busy", t!("url-already-downloading", id = busy.id.clone())));
            }
            let f = list
                .iter_mut()
                .find(|f| f.id == id)
                .ok_or_else(|| UrlError::new("not_found", t!("url-no-fetch", id = id.to_string())))?;
            f.status = FetchStatus::Downloading;
            f.phase = "probe".into();
            f.error_code = None;
            f.error = None;
            f.hint = None;
            f.warning = None;
            f.warning_detail = None;
            f.tool_version = Some(tool.version.clone());
            f.updated_at = ytdlp::now_s();
            let out = f.clone();
            self.save(&list);
            out
        };
        ytdlp::update_in_background_if_due(&self.inner.repo_root);
        self.spawn(id, tool)?;
        Ok(out)
    }

    /// Убрать загрузку из списка вместе с недокачанным. Идущую сначала отменяют.
    pub fn forget(&self, id: &str) -> Result<(), UrlError> {
        // Отменённая загрузка, чей поток ещё гасит yt-dlp: его файлы в папке ещё заняты.
        if lock(&self.inner.stops).contains_key(id) {
            return Err(UrlError::new("running", t!("url-still-stopping", id = id.to_string())));
        }
        let mut list = lock(&self.inner.list);
        let at = list
            .iter()
            .position(|f| f.id == id)
            .ok_or_else(|| UrlError::new("not_found", t!("url-no-fetch", id = id.to_string())))?;
        if list[at].status == FetchStatus::Downloading {
            return Err(UrlError::new("running", t!("url-cancel-first", id = id.to_string())));
        }
        let dir = self.dir(id);
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir).map_err(|e| UrlError::new("io", format!("{}: {e}", dir.display())))?;
        }
        list.remove(at);
        self.save(&list);
        Ok(())
    }
}

struct Made {
    pid: String,
    subs_imported: bool,
    warning: Option<(&'static str, String)>,
}

/// Новый проект из скачанного файла: файл переезжает в workspace/<pid>/source.<ext> (тот же том — мгновенно),
/// имя проекта — название видео, субтитры площадки — import_subs.srt, как у загруженных вместе с видео.
fn make_project(workspace: &Path, media: &Path, title: &str, subs: Option<&Path>) -> Result<Made, UrlError> {
    let io = |what: &Path, e: std::io::Error| UrlError::new("io", format!("{}: {e}", what.display()));
    let mut warning = None;
    let subtitles = match subs {
        Some(src) => {
            let text = std::fs::read_to_string(src).map_err(|e| io(src, e))?;
            if crate::subimport::parse(&text, "srt").is_empty() {
                warning = Some(("subs_empty", t!("url-subs-empty", path = src.display().to_string())));
                None
            } else {
                Some(text)
            }
        }
        None => None,
    };
    let mut pid = uuid::Uuid::new_v4().simple().to_string();
    pid.truncate(12);
    let d = workspace.join(&pid);
    std::fs::create_dir(&d).map_err(|e| io(&d, e))?;
    let ext = media.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_ascii_lowercase();
    let dst = d.join(format!("source.{ext}"));
    let mut moved = false;
    let result = (|| -> Result<(), UrlError> {
        std::fs::rename(media, &dst).map_err(|e| io(&dst, e))?;
        moved = true;
        if let Some(text) = &subtitles {
            let path = d.join("import_subs.srt");
            std::fs::write(&path, text).map_err(|e| io(&path, e))?;
        }
        let name = project_name(title, &ext);
        let name = name.strip_suffix(&format!(".{ext}")).unwrap_or(&name);
        crate::listening::create_from_source(workspace, &pid, name, &format!("source.{ext}"), title).map_err(|e| UrlError::new("io", e))?;
        Ok(())
    })();
    if let Err(error) = result {
        // Never remove the project directory until its media has been restored for a retry.
        if moved {
            if let Err(restore) = std::fs::rename(&dst, media) {
                return Err(UrlError::new(
                    "io",
                    format!("{}; restore {} to {}: {restore}", error.detail, dst.display(), media.display()),
                ));
            }
        }
        if let Err(cleanup) = std::fs::remove_dir_all(&d) {
            return Err(UrlError::new("io", format!("{}; remove {}: {cleanup}", error.detail, d.display())));
        }
        return Err(error);
    }
    Ok(Made {
        pid,
        subs_imported: subtitles.is_some(),
        warning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn a_failed_import_keeps_the_downloaded_media_for_retry() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let dir = workspace.join(".fetch/url1");
        std::fs::create_dir_all(&dir).unwrap();
        let media = dir.join("source.mp4");
        std::fs::write(&media, b"downloaded media").unwrap();
        let error = make_project(&workspace, &media, "clip", Some(&dir.join("missing.srt")))
            .err()
            .unwrap();
        assert_eq!(error.code, "io");
        assert!(media.is_file(), "a failed import must remain resumable: {error}");
        assert_eq!(std::fs::read(&media).unwrap(), b"downloaded media");
        assert_eq!(std::fs::read_dir(&workspace).unwrap().count(), 1, "no orphaned project directory");
    }

    #[test]
    fn a_listening_creation_failure_restores_moved_media_and_removes_partial_outputs() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let dir = workspace.join(".fetch/url1");
        std::fs::create_dir_all(&dir).unwrap();
        let media = dir.join("source.mkv");
        std::fs::write(&media, b"not valid media for the required AAC preview").unwrap();
        let error = make_project(&workspace, &media, "clip", None).err().unwrap();
        assert_eq!(error.code, "io");
        assert_eq!(std::fs::read(&media).unwrap(), b"not valid media for the required AAC preview");
        assert_eq!(std::fs::read_dir(&workspace).unwrap().count(), 1);
    }

    #[test]
    fn completed_import_keeps_warnings_and_a_persistent_project_link() {
        let root = tempfile::tempdir().unwrap();
        let ws = root.path().join("workspace");
        let f = Fetches::open(root.path(), &ws);
        let media = downloaded(&f, "url1");
        let artifact = Downloaded {
            media,
            title: "My: clip".into(),
            subtitles: None,
            warning: Some(download::DownloadWarning {
                code: "subs_failed",
                detail: "rate_limited".into(),
            }),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let result = f.import_download("url1", artifact, &stop);
        f.finish("url1", result, false);
        let completed = f.get("url1").unwrap();
        assert_eq!(completed.status, FetchStatus::Completed);
        assert_eq!(completed.phase, "done");
        assert_eq!(completed.downloaded, 10);
        assert_eq!(completed.warning.as_deref(), Some("subs_failed"));
        assert_eq!(completed.warning_detail.as_deref(), Some("rate_limited"));
        assert!(!completed.subs_imported);
        assert!(ws.join(completed.pid.as_ref().unwrap()).join("source.mp4").is_file());
        assert!(!f.dir("url1").exists());
        assert_eq!(Fetches::open(root.path(), &ws).get("url1").unwrap(), completed);
    }

    #[test]
    fn cancellation_before_import_creates_no_project() {
        let root = tempfile::tempdir().unwrap();
        let ws = root.path().join("workspace");
        let f = Fetches::open(root.path(), &ws);
        let media = downloaded(&f, "url1");
        let stop = Arc::new(AtomicBool::new(true));
        let result = f.import_download(
            "url1",
            Downloaded {
                media,
                title: "clip".into(),
                subtitles: None,
                warning: None,
            },
            &stop,
        );
        assert_eq!(result.as_ref().unwrap_err().code, "cancelled");
        f.finish("url1", result, true);
        let cancelled = f.get("url1").unwrap();
        assert_eq!(cancelled.status, FetchStatus::Cancelled);
        assert!(cancelled.pid.is_none());
        assert_eq!(std::fs::read_dir(&ws).unwrap().count(), 1);
        assert!(!f.dir("url1").exists());
    }

    #[test]
    fn cancellation_after_project_creation_preserves_completed_state_and_its_event() {
        let root = tempfile::tempdir().unwrap();
        let ws = root.path().join("workspace");
        let f = Fetches::open(root.path(), &ws);
        let media = downloaded(&f, "url1");
        let stop = Arc::new(AtomicBool::new(false));
        lock(&f.inner.stops).insert("url1".into(), stop.clone());
        let made = make_project(&ws, &media, "clip", None).unwrap();
        let pid = made.pid.clone();
        f.cancel("url1").unwrap();
        assert!(stop.load(Ordering::SeqCst));
        let mut events = super::super::events::subscribe();
        f.completed("url1", made);
        f.finish("url1", Ok(()), true);
        let completed = f.get("url1").unwrap();
        assert_eq!(completed.status, FetchStatus::Completed);
        assert_eq!(completed.pid.as_deref(), Some(pid.as_str()));
        assert!(ws.join(&pid).join("project.json").is_file());
        assert_eq!(Fetches::open(root.path(), &ws).get("url1").unwrap(), completed);
        let mut seen = false;
        while let Ok(event) = events.try_recv() {
            let event: Value = serde_json::from_str(&event).unwrap();
            if event == json!({ "changed": "projects", "pid": pid, "by": "studio" }) {
                seen = true;
            }
        }
        assert!(seen);
    }

    #[test]
    fn project_names_keep_the_existing_sanitization() {
        assert_eq!(project_name("Клип: «часть 1/2»?", "mp4"), "Клип «часть 1 2».mp4");
        assert_eq!(project_name("  ...  ", "m4a"), "video.m4a");
        assert_eq!(project_name(&"x".repeat(300), "mp4").chars().count(), 124);
    }

    fn fetch(id: &str, status: FetchStatus) -> Fetch {
        Fetch {
            id: id.into(),
            url: "https://example.com/v".into(),
            quality: Quality::H720,
            subs_lang: Some("en".into()),
            cookies: false,
            status,
            phase: "download".into(),
            title: Some("t".into()),
            duration: None,
            downloaded: 5,
            total: Some(10),
            speed_bps: 7,
            eta_s: Some(3),
            pid: None,
            subs_imported: false,
            warning: None,
            warning_detail: None,
            error_code: None,
            error: None,
            hint: None,
            tool_version: None,
            started_at: 0,
            updated_at: 0,
        }
    }

    fn downloaded(f: &Fetches, id: &str) -> PathBuf {
        lock(&f.inner.list).push(fetch(id, FetchStatus::Downloading));
        let dir = f.dir(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("source.mp4"), b"not really a video").unwrap();
        dir.join("source.mp4")
    }

    #[test]
    fn a_download_running_at_exit_comes_back_interrupted_and_resumable() {
        let root = tempfile::tempdir().unwrap();
        let ws = root.path().join("workspace");
        persist(
            &ws.join(".fetch").join("fetches.json"),
            &[fetch("url1", FetchStatus::Downloading), fetch("url2", FetchStatus::Completed)],
        )
        .unwrap();
        let f = Fetches::open(root.path(), &ws);
        let list = f.list();
        assert_eq!(list[0].status, FetchStatus::Interrupted);
        assert_eq!(
            (list[0].error_code.as_deref(), list[0].speed_bps, list[0].downloaded),
            (Some("interrupted"), 0, 5),
            "what came is kept"
        );
        assert_eq!(list[1].status, FetchStatus::Completed);
        let again = Fetches::open(root.path(), &ws);
        assert_eq!(again.get("url1").unwrap().status, FetchStatus::Interrupted, "the state is on disk");
        assert_eq!(f.resume("url1").unwrap_err().code, "tool_missing", "resuming needs yt-dlp");
        std::fs::create_dir_all(ws.join(".fetch").join("url1")).unwrap();
        f.forget("url1").unwrap();
        assert!(
            f.get("url1").is_none() && !ws.join(".fetch").join("url1").exists(),
            "forgetting drops the partial download"
        );
        assert_eq!(f.forget("nope").unwrap_err().code, "not_found");
    }

    #[test]
    fn a_running_download_is_cancelled_and_not_forgotten_while_it_runs() {
        let root = tempfile::tempdir().unwrap();
        let ws = root.path().join("workspace");
        let f = Fetches::open(root.path(), &ws);
        lock(&f.inner.list).push(fetch("url9", FetchStatus::Downloading));
        let stop = Arc::new(AtomicBool::new(false));
        lock(&f.inner.stops).insert("url9".into(), stop.clone());
        assert_eq!(f.forget("url9").unwrap_err().code, "running");
        let c = f.cancel("url9").unwrap();
        assert_eq!((c.status, c.speed_bps), (FetchStatus::Cancelled, 0));
        assert!(stop.load(Ordering::SeqCst), "the process gets the stop");
        f.update("url9", |x| x.downloaded = 99);
        assert_eq!(f.get("url9").unwrap().downloaded, 5, "a stopped download takes no more progress");
        f.finish("url9", Err(UrlError::new("cancelled", t!("url-stopped"))), true);
        assert_eq!(f.get("url9").unwrap().status, FetchStatus::Cancelled);
        assert_eq!(f.resume("url9").unwrap_err().code, "tool_missing");
    }

    #[test]
    fn a_failure_keeps_its_code_and_what_to_do() {
        let root = tempfile::tempdir().unwrap();
        let f = Fetches::open(root.path(), &root.path().join("workspace"));
        lock(&f.inner.list).push(fetch("url3", FetchStatus::Downloading));
        f.finish("url3", Err(UrlError::new("geo_blocked", "not available in your country")), false);
        let x = f.get("url3").unwrap();
        assert_eq!((x.status, x.error_code.as_deref()), (FetchStatus::Failed, Some("geo_blocked")));
        assert!(x.hint.unwrap().contains("proxy"));
    }

    #[test]
    fn eviction_keeps_running_downloads() {
        let root = tempfile::tempdir().unwrap();
        let f = Fetches::open(root.path(), &root.path().join("workspace"));
        let mut list: Vec<Fetch> = (0..KEEP + 3)
            .map(|i| {
                fetch(
                    &format!("url{i}"),
                    if i % 2 == 0 {
                        FetchStatus::Downloading
                    } else {
                        FetchStatus::Completed
                    },
                )
            })
            .collect();
        f.evict(&mut list);
        assert!(list.len() <= KEEP || list.iter().all(|x| x.status == FetchStatus::Downloading));
        assert_eq!(
            list.iter().filter(|x| x.status == FetchStatus::Downloading).count(),
            (KEEP + 3).div_ceil(2)
        );
    }

    #[test]
    fn a_downloaded_file_becomes_a_project_with_the_sites_subtitles() {
        let root = tempfile::tempdir().unwrap();
        let fetch_root = root.path().join("workspace").join(".fetch");
        let dir = fetch_root.join("url5");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("source.mp4"), b"not really a video").unwrap();
        std::fs::write(dir.join("subs.en.srt"), "1\n00:00:01,000 --> 00:00:02,500\nHello there\n\n").unwrap();
        let subs = dir.join("subs.en.srt");
        let made = make_project(fetch_root.parent().unwrap(), &dir.join("source.mp4"), "My: clip", Some(&subs)).unwrap();
        let p = root.path().join("workspace").join(&made.pid);
        assert!(made.subs_imported && made.warning.is_none());
        assert!(
            p.join("source.mp4").is_file() && !dir.join("source.mp4").exists(),
            "the file moves, not copies"
        );
        assert!(p.join("import_subs.srt").is_file());
        let project: Value = serde_json::from_str(&std::fs::read_to_string(p.join("project.json")).unwrap()).unwrap();
        assert_eq!(
            (project["kind"].as_str(), project["name"].as_str(), project["source"].as_str()),
            (Some("listening"), Some("My clip"), Some("source.mp4"))
        );
        assert_eq!(
            project["made"]["by"].as_str(),
            Some("subs"),
            "platform subtitles become the transcript"
        );
        std::fs::write(dir.join("source.m4a"), b"x").unwrap();
        std::fs::write(dir.join("empty.srt"), "garbage").unwrap();
        let made = make_project(
            fetch_root.parent().unwrap(),
            &dir.join("source.m4a"),
            "a",
            Some(&dir.join("empty.srt")),
        )
        .unwrap();
        assert_eq!((made.subs_imported, made.warning.map(|w| w.0)), (false, Some("subs_empty")));
    }
}
