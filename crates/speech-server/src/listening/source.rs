use std::path::Path;

use super::project as lp;

/// Форматы, которые webview играет сам; остальное (mkv, avi, wma…) слушается через AAC-копию `work/play.m4a`.
const PLAYABLE: &[&str] = &["wav", "flac", "mp3", "m4a", "aac", "ogg", "opus", "mp4", "webm", "mov", "m4v"];

fn play_copy(dir: &Path, source: &str) -> Result<(), String> {
    let ext = Path::new(source)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if PLAYABLE.contains(&ext.as_str()) {
        return Ok(());
    }
    let work = dir.join("work");
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    crate::media::encode_preview_aac(&dir.join(source), &work.join("play.m4a"))
}

/// Проект вокруг файла, уже лежащего в папке `pid`.
pub(crate) fn create_from_source(workspace: &Path, pid: &str, name: &str, source: &str, origin: &str) -> Result<lp::ListenProject, String> {
    let dir = lp::dir(workspace, pid);
    play_copy(&dir, source)?;
    lp::create_in(workspace, pid, name, source, origin)
}
