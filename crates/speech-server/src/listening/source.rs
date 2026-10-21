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

pub(crate) fn stem(file: &str) -> String {
    Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or(file).to_string()
}

pub(crate) fn file_key(path: &Path, tool: &str) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let stamp = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    Ok(blake3::hash(format!("{tool}|{}|{}|{stamp}", path.display(), meta.len()).as_bytes()).to_hex()[..16].to_string())
}

/// The same path, size and modification time reuse the recording, including its edits and transcript.
pub(crate) fn project_of_file(ws: &Path, path: &Path) -> Result<(String, bool), String> {
    if !path.is_file() || !lp::is_media(&path.to_string_lossy()) {
        return Err(t!("listen-not-media", file = path.display().to_string()));
    }
    let key = file_key(path, "listen")?;
    let found = std::fs::read_dir(ws).map_err(|e| e.to_string())?.flatten().find_map(|e| {
        let pid = e.file_name().to_string_lossy().into_owned();
        lp::load(ws, &pid).ok().filter(|p| p.key == key).map(|_| pid)
    });
    if let Some(pid) = found {
        return Ok((pid, true));
    }
    let pid = crate::platform::ids::new_id();
    let dir = lp::dir(ws, &pid);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("wav").to_ascii_lowercase();
    let source = format!("source.{ext}");
    if let Err(e) = std::fs::copy(path, dir.join(&source)) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!("{}: {e}", path.display()));
    }
    let made = create_from_source(ws, &pid, &stem(&path.to_string_lossy()), &source, &path.to_string_lossy());
    if made.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    made?;
    let mut project = lp::load(ws, &pid)?;
    project.key = key;
    lp::save(ws, &pid, &project)?;
    Ok((pid, false))
}

#[cfg(test)]
mod tests;
