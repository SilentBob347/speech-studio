//! yt-dlp downloads and probes. Completed media stays in the caller's download directory.

use crate::ytdlp::{self, Line, Quality, UrlError};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
/// cookies.txt больше этого не бывает.
const COOKIES_LIMIT: u64 = 1024 * 1024;
/// Проба ссылки (`-J`) дольше этого — сайт не отвечает.
const PROBE_TIMEOUT: Duration = Duration::from_secs(180);
/// Шаг субтитров площадки дольше этого — сайт не отвечает (при устаревших адресах yt-dlp заново читает страницу).
const SUBS_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Debug)]
pub enum Cookies {
    Path(PathBuf),
    Text(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DownloadWarning {
    pub code: &'static str,
    pub detail: String,
}

pub(crate) enum Progress {
    Phase(&'static str),
    Metadata {
        title: String,
        duration: Option<f64>,
        total: Option<u64>,
    },
    Bytes {
        downloaded: u64,
        total: Option<u64>,
        speed_bps: u64,
        eta_s: Option<u64>,
    },
    Warning(DownloadWarning),
}

pub(crate) struct DownloadRequest<'a> {
    pub url: &'a str,
    pub quality: Quality,
    pub subs_lang: Option<&'a str>,
    pub cookies: bool,
}

#[derive(Debug)]
pub(crate) struct Downloaded {
    pub media: PathBuf,
    pub title: String,
    pub subtitles: Option<PathBuf>,
    pub warning: Option<DownloadWarning>,
}
/// Проверенная ссылка: http(s) с хостом.
pub(crate) fn check_url(url: &str) -> Result<String, UrlError> {
    let url = url.trim();
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| UrlError::new("bad_url", t!("url-not-a-link", url = url.to_string(), error = e.to_string())))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none_or(str::is_empty) {
        return Err(UrlError::new("bad_url", t!("url-not-http", url = url.to_string())));
    }
    Ok(parsed.to_string())
}

/// Код языка субтитров как его пишет yt-dlp (en, pt-BR, zh-Hans): только буквы, цифры и дефис.
pub(crate) fn check_lang(lang: &str) -> Result<String, UrlError> {
    let lang = lang.trim();
    if lang.is_empty() || lang.len() > 24 || !lang.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err(UrlError::new("bad_url", t!("url-bad-subs-lang", lang = lang.to_string())));
    }
    Ok(lang.to_string())
}

/// Положить cookies.txt в `dest`: копия, а не исходный файл — yt-dlp дописывает в файл cookies свои.
pub(crate) fn place_cookies(c: &Cookies, dest: &Path) -> Result<(), UrlError> {
    let text = match c {
        Cookies::Path(p) => {
            let meta = std::fs::metadata(p).map_err(|e| UrlError::new("cookies_invalid", format!("{}: {e}", p.display())))?;
            if !meta.is_file() || meta.len() > COOKIES_LIMIT {
                return Err(UrlError::new(
                    "cookies_invalid",
                    t!("url-cookies-not-file", path = p.display().to_string()),
                ));
            }
            std::fs::read_to_string(p).map_err(|e| UrlError::new("cookies_invalid", format!("{}: {e}", p.display())))?
        }
        Cookies::Text(t) => t.clone(),
    };
    if text.trim().is_empty() || text.len() as u64 > COOKIES_LIMIT {
        return Err(UrlError::new("cookies_invalid", t!("url-cookies-empty-or-large")));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| UrlError::new("io", format!("{}: {e}", parent.display())))?;
    }
    std::fs::write(dest, text).map_err(|e| UrlError::new("io", format!("{}: {e}", dest.display())))
}

pub(crate) fn download(
    fetch: DownloadRequest<'_>,
    tool: &ytdlp::Tool,
    dir: &Path,
    stop: &Arc<AtomicBool>,
    progress: impl Fn(Progress),
) -> Result<Downloaded, UrlError> {
    std::fs::create_dir_all(&dir).map_err(|e| UrlError::new("io", format!("{}: {e}", dir.display())))?;
    let cookies = dir.join("cookies.txt");
    let with_cookies = |cmd: &mut std::process::Command| {
        if fetch.cookies {
            cmd.arg("--cookies").arg(&cookies);
        }
    };
    if fetch.cookies && !cookies.is_file() {
        return Err(UrlError::new("cookies_invalid", t!("url-cookies-gone")));
    }

    // Проба: свежие адреса форматов (у YouTube они живут часы), название, размер, субтитры.
    progress(Progress::Phase("probe"));
    let mut cmd = tool.command(&fetch.url, &dir)?;
    cmd.args(ytdlp::PROBE_ARGS).args(fetch.quality.args());
    with_cookies(&mut cmd);
    cmd.arg("--").arg(&fetch.url);
    let stop_probe = stop.clone();
    let out = ytdlp::run_captured(cmd, &move || stop_probe.load(Ordering::SeqCst), Some(PROBE_TIMEOUT))?;
    if !out.success {
        return Err(ytdlp::classify(&out.stderr));
    }
    let info: Value = serde_json::from_slice(&out.stdout).map_err(|e| {
        UrlError::new(
            "ytdlp_failed",
            t!(
                "url-probe-not-json",
                error = e.to_string(),
                stderr = ytdlp::last_error_line(&out.stderr)
            ),
        )
    })?;
    let probe = ytdlp::parse_probe(&info)?;
    let info_path = dir.join("info.json");
    std::fs::write(&info_path, &out.stdout).map_err(|e| UrlError::new("io", format!("{}: {e}", info_path.display())))?;
    let expected = probe.expected_bytes;
    if let (Some(need), Some(free)) = (expected, crate::setup::free_bytes(&dir)) {
        // Слияние видео и звука держит на диске и части, и итог.
        if free < need.saturating_mul(2) {
            return Err(UrlError::new(
                "disk_space",
                t!("url-disk-space", need = need.saturating_mul(2) / 1_000_000, free = free / 1_000_000),
            ));
        }
    }
    let mut warning = None;
    let mut subs_lang = fetch.subs_lang.map(str::to_string);
    if let Some(lang) = fetch.subs_lang {
        if !probe.subtitles.iter().any(|t| t.lang == lang) {
            let have: Vec<&str> = probe.subtitles.iter().map(|t| t.lang.as_str()).collect();
            let detail = if have.is_empty() {
                t!("url-no-subs-none", lang = lang.to_string())
            } else {
                t!("url-no-subs", lang = lang.to_string(), have = have.join(", "))
            };
            let w = DownloadWarning {
                code: "subs_missing",
                detail,
            };
            progress(Progress::Warning(w.clone()));
            warning = Some(w);
            subs_lang = None;
        }
    }
    progress(Progress::Metadata {
        title: probe.title.clone(),
        duration: probe.duration,
        total: expected,
    });

    // Загрузка по полученному описанию: форматы не выбираются второй раз с другой страницы.
    // Папка — через -P: знак % в её пути шаблон -o прочитал бы как поле.
    let mut cmd = tool.command(&fetch.url, &dir)?;
    cmd.arg("--load-info-json").arg(&info_path).args(fetch.quality.args());
    cmd.arg("-P").arg(&dir).args(["-o", "source.%(ext)s"]);
    with_cookies(&mut cmd);
    cmd.args([
        "--newline",
        "--progress",
        "--progress-template",
        ytdlp::PROGRESS_TEMPLATE,
        "--progress-template",
        ytdlp::POSTPROCESS_TEMPLATE,
        "--print",
        ytdlp::FILE_TEMPLATE,
    ]);
    let (media, stderr) = download_media(cmd, expected, stop, &progress)?;
    if stop.load(Ordering::SeqCst) {
        return Err(UrlError::new("cancelled", t!("url-stopped")));
    }
    let media = match media.filter(|p| p.is_file()) {
        Some(p) => p,
        None => find_media(&dir).ok_or_else(|| {
            UrlError::new(
                "ytdlp_failed",
                t!(
                    "url-no-media-file",
                    path = dir.display().to_string(),
                    stderr = ytdlp::last_error_line(&stderr)
                ),
            )
        })?,
    };

    // Субтитры площадки — отдельным вызовом после видео: в общем вызове yt-dlp пишет их до медиа, и их ошибка
    // бросает всю загрузку.
    let mut downloaded = after_media(dir, &media, &probe.title, subs_lang.as_deref(), stop, &progress, |lang| {
        let mut cmd = tool.command(&fetch.url, &dir)?;
        cmd.arg("--load-info-json")
            .arg(&info_path)
            .args(fetch.quality.args())
            .arg("--skip-download");
        cmd.args(["--write-subs", "--no-write-auto-subs", "--sub-langs", lang, "--convert-subs", "srt"]);
        cmd.arg("-P")
            .arg(&dir)
            .args(["-o", "source.%(ext)s", "-o", "subtitle:subs.%(ext)s"]);
        with_cookies(&mut cmd);
        let stop_subs = stop.clone();
        ytdlp::run_captured(cmd, &move || stop_subs.load(Ordering::SeqCst), Some(SUBS_TIMEOUT))
    })?;
    downloaded.warning = downloaded.warning.or(warning);
    Ok(downloaded)
}

fn after_media(
    dir: &Path,
    media: &Path,
    title: &str,
    subs_lang: Option<&str>,
    stop: &Arc<AtomicBool>,
    progress: &impl Fn(Progress),
    fetch_subs: impl FnOnce(&str) -> Result<ytdlp::Captured, UrlError>,
) -> Result<Downloaded, UrlError> {
    let mut warning = None;
    let subs = match subs_lang {
        Some(lang) => {
            progress(Progress::Phase("subtitles"));
            match fetch_subs(lang).and_then(|out| subs_file(&dir, out)) {
                Ok(p) => Some(p),
                Err(e) if e.code == "cancelled" => return Err(e),
                Err(e) => {
                    tracing::warn!("subtitles {lang} in {}: {} ({})", dir.display(), e.detail, e.code);
                    let detail = t!("url-subs-failed", lang = lang.to_string(), code = e.code, error = e.detail.clone());
                    let w = DownloadWarning {
                        code: "subs_failed",
                        detail,
                    };
                    progress(Progress::Warning(w.clone()));
                    warning = Some(w);
                    None
                }
            }
        }
        None => None,
    };
    if stop.load(Ordering::SeqCst) {
        return Err(UrlError::new("cancelled", t!("url-stopped")));
    }
    Ok(Downloaded {
        media: media.to_path_buf(),
        title: title.into(),
        subtitles: subs,
        warning,
    })
}

fn download_media(
    mut cmd: std::process::Command,
    expected: Option<u64>,
    stop: &Arc<AtomicBool>,
    progress: &impl Fn(Progress),
) -> Result<(Option<PathBuf>, String), UrlError> {
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| UrlError::new("io", t!("url-ytdlp-start", error = e.to_string())))?;
    let pid = child.id();
    let (tx, rx) = mpsc::channel::<(bool, String)>();
    let readers: Vec<_> = [
        child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .enumerate()
    .filter_map(|(i, pipe)| {
        let tx = tx.clone();
        pipe.map(|p| {
            ytdlp::lines_of(p, move |line| {
                let _ = tx.send((i == 1, line.to_string()));
            })
        })
    })
    .collect();
    drop(tx);
    let mut streams = ytdlp::Streams::default();
    let mut file: Option<PathBuf> = None;
    let mut stderr: Vec<String> = Vec::new();
    let mut killed = false;
    let mut handle = |is_err: bool, line: String, streams: &mut ytdlp::Streams| match ytdlp::parse_line(&line) {
        Line::Progress(p) => {
            let (done, total) = streams.apply(&p, expected);
            progress(Progress::Bytes {
                downloaded: done,
                total,
                speed_bps: p.speed.map_or(0, |s| s as u64),
                eta_s: p.eta,
            });
        }
        Line::Post(state, name) if state == "started" => {
            let phase = match name.as_str() {
                // Имена обработчиков yt-dlp — класс без FFmpeg и PP: FFmpegMergerPP -> Merger.
                "Merger" => "merge",
                "ExtractAudio" => "extract",
                _ => return,
            };
            progress(Progress::Phase(phase));
        }
        Line::File(p) => file = Some(p),
        Line::Post(..) => {}
        Line::Other => {
            if is_err && !line.trim().is_empty() {
                stderr.push(line);
                if stderr.len() > 200 {
                    stderr.remove(0);
                }
            }
        }
    };
    let status = loop {
        while let Ok((is_err, line)) = rx.try_recv() {
            handle(is_err, line, &mut streams);
        }
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {}
            Err(e) => {
                ytdlp::kill_tree(pid);
                return Err(UrlError::new("io", t!("url-ytdlp-wait", error = e.to_string())));
            }
        }
        if !killed && stop.load(Ordering::SeqCst) {
            ytdlp::kill_tree(pid);
            killed = true;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    for r in readers {
        let _ = r.join();
    }
    while let Ok((is_err, line)) = rx.try_recv() {
        handle(is_err, line, &mut streams);
    }
    let stderr = stderr.join("\n");
    if killed {
        return Err(UrlError::new("cancelled", t!("url-stopped")));
    }
    if !status.success() {
        return Err(ytdlp::classify(&stderr));
    }
    Ok((file, stderr))
}

/// Итоговый файл в папке загрузки, если yt-dlp не назвал его сам: `source.<ext>`, а не недокачанный `.part` и не
/// часть слияния `source.f137.mp4`.
fn find_media(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        name.strip_prefix("source.")
            .is_some_and(|ext| !ext.is_empty() && !ext.contains('.'))
            && p.is_file()
    })
}

/// Субтитры, сведённые yt-dlp в SRT: `subs.<язык>.srt`.
fn find_subs(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        name.starts_with("subs.") && name.ends_with(".srt") && p.is_file()
    })
}

/// Файл, который оставил шаг субтитров yt-dlp; его провал — код по тексту yt-dlp.
fn subs_file(dir: &Path, out: ytdlp::Captured) -> Result<PathBuf, UrlError> {
    if !out.success {
        return Err(ytdlp::classify(&out.stderr));
    }
    find_subs(dir).ok_or_else(|| {
        let said = [
            ytdlp::last_error_line(&out.stderr),
            ytdlp::last_error_line(&String::from_utf8_lossy(&out.stdout)),
        ]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_default();
        UrlError::new(
            "ytdlp_failed",
            t!("url-no-subs-file", path = dir.display().to_string(), stderr = said.to_string()),
        )
    })
}

/// Проба ссылки: `yt-dlp -J`, превью картинкой и что предложить (качества, субтитры площадки).
pub(crate) fn probe(repo_root: &Path, fetch_root: &Path, url: &str, cookies: Option<Cookies>) -> Result<Value, UrlError> {
    let url = check_url(url)?;
    let tool = ytdlp::tool(repo_root)?;
    let scratch = fetch_root.join(format!("probe-{}", uuid::Uuid::new_v4().simple()));
    let out = (|| {
        let mut cmd = tool.command(&url, &scratch)?;
        cmd.args(ytdlp::PROBE_ARGS);
        if let Some(c) = &cookies {
            let path = scratch.join("cookies.txt");
            place_cookies(c, &path)?;
            cmd.arg("--cookies").arg(&path);
        }
        cmd.arg("--").arg(&url);
        ytdlp::run_captured(cmd, &|| false, Some(PROBE_TIMEOUT))
    })();
    if scratch.is_dir() {
        if let Err(e) = std::fs::remove_dir_all(&scratch) {
            tracing::warn!("{}: {e}", scratch.display());
        }
    }
    let out = out?;
    if !out.success {
        return Err(ytdlp::classify(&out.stderr));
    }
    let info: Value = serde_json::from_slice(&out.stdout).map_err(|e| {
        UrlError::new(
            "ytdlp_failed",
            t!(
                "url-probe-not-json",
                error = e.to_string(),
                stderr = ytdlp::last_error_line(&out.stderr)
            ),
        )
    })?;
    let probe = ytdlp::parse_probe(&info)?;
    let mut v = serde_json::to_value(&probe).map_err(|e| UrlError::new("io", e.to_string()))?;
    let (data, problem) = match probe.thumbnail.as_deref().map(ytdlp::thumbnail_data) {
        Some(Ok(d)) => (Some(d), None),
        Some(Err(e)) => (None, Some(e)),
        None => (None, None),
    };
    v["thumbnail_data"] = data.into();
    v["thumbnail_error"] = problem.into();
    v["tool_version"] = tool.version.into();
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_languages_are_checked() {
        assert!(check_url("https://www.youtube.com/watch?v=abc").is_ok());
        for url in ["ftp://x/y", "C:\\video.mp4", ""] {
            assert_eq!(check_url(url).unwrap_err().code, "bad_url")
        }
        assert_eq!(check_lang("pt-BR").unwrap(), "pt-BR");
        for lang in ["en,ru", " ", "en/../x"] {
            assert!(check_lang(lang).is_err())
        }
    }

    #[test]
    fn failed_subtitles_return_media_with_an_explicit_warning() {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join("source.mp4");
        std::fs::write(&media, b"video").unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        for outcome in [
            Ok(ran(
                false,
                "ERROR: Unable to download video subtitles: HTTP Error 429: Too Many Requests",
            )),
            Err(UrlError::new("network", "no answer in 180 s")),
            Ok(ran(true, "")),
        ] {
            let mut asked = None;
            let warnings = std::cell::RefCell::new(Vec::new());
            let result = after_media(
                root.path(),
                &media,
                "clip",
                Some("en"),
                &stop,
                &|p| {
                    if let Progress::Warning(w) = p {
                        warnings.borrow_mut().push(w);
                    }
                },
                |lang| {
                    asked = Some(lang.to_string());
                    outcome
                },
            )
            .unwrap();
            assert_eq!(asked.as_deref(), Some("en"));
            let warning = result.warning.unwrap();
            assert_eq!(warning.code, "subs_failed");
            assert!(warning.detail.contains("en"));
            assert_eq!(*warnings.borrow(), [warning]);
            assert!(result.subtitles.is_none());
            assert_eq!(result.media, media);
            assert_eq!(std::fs::read(&media).unwrap(), b"video");
            assert_eq!(
                std::fs::read_dir(root.path()).unwrap().count(),
                1,
                "the downloader creates no project"
            );
        }
    }

    #[test]
    fn subtitles_follow_media_and_cancellation_stays_an_error() {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join("source.mp4");
        std::fs::write(&media, b"video").unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let result = after_media(root.path(), &media, "clip", Some("en"), &stop, &|_| {}, |_| {
            assert!(media.is_file());
            std::fs::write(root.path().join("subs.en.srt"), "1\n00:00:01,000 --> 00:00:02,500\nHello there\n\n").unwrap();
            Ok(ran(true, ""))
        })
        .unwrap();
        assert_eq!(result.subtitles, Some(root.path().join("subs.en.srt")));
        assert!(result.warning.is_none() && media.is_file());
        let result = after_media(root.path(), &media, "clip", None, &stop, &|_| {}, |_| {
            panic!("no subtitles requested")
        })
        .unwrap();
        assert!(result.subtitles.is_none() && result.warning.is_none());
        let error = after_media(root.path(), &media, "clip", Some("en"), &stop, &|_| {}, |_| {
            Err(UrlError::new("cancelled", "stopped"))
        })
        .unwrap_err();
        assert_eq!(error.code, "cancelled");
        stop.store(true, Ordering::SeqCst);
        assert_eq!(
            after_media(root.path(), &media, "clip", None, &stop, &|_| {}, |_| panic!("cancelled"))
                .unwrap_err()
                .code,
            "cancelled"
        );
        assert!(media.is_file());
    }

    fn ran(success: bool, stderr: &str) -> ytdlp::Captured {
        ytdlp::Captured {
            success,
            code: Some(i32::from(!success)),
            stdout: Vec::new(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn cookies_are_copied_and_bad_ones_refused() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("c.txt");
        std::fs::write(&src, "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tA\tB\n").unwrap();
        let dest = root.path().join("fetch").join("cookies.txt");
        place_cookies(&Cookies::Path(src.clone()), &dest).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), std::fs::read_to_string(&src).unwrap());
        assert_eq!(
            place_cookies(&Cookies::Path(root.path().join("none.txt")), &dest).unwrap_err().code,
            "cookies_invalid"
        );
        assert_eq!(
            place_cookies(&Cookies::Text("  ".into()), &dest).unwrap_err().code,
            "cookies_invalid"
        );
        place_cookies(&Cookies::Text("# Netscape HTTP Cookie File\n".into()), &dest).unwrap();
    }

    #[test]
    fn only_the_final_file_is_taken_for_the_project() {
        let root = tempfile::tempdir().unwrap();
        for name in ["source.mp4.part", "source.f137.mp4", "source.f140.m4a", "info.json", "cookies.txt"] {
            std::fs::write(root.path().join(name), b"x").unwrap();
        }
        assert_eq!(find_media(root.path()), None, "parts of a merge are not the video");
        std::fs::write(root.path().join("source.webm"), b"x").unwrap();
        assert_eq!(find_media(root.path()), Some(root.path().join("source.webm")));
    }
}
