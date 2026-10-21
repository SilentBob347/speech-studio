//! Speech project persistence; keeps the existing writer and revision ordering.

use std::path::Path;
use std::sync::LazyLock;

use super::project::{SpeechProject, KIND};
use crate::platform::workspace::{is_pid, project_file as path};
use serde_json::Value;

pub fn load(workspace: &Path, pid: &str) -> Result<SpeechProject, String> {
    if !is_pid(pid) {
        return Err(format!("bad project id {pid}"));
    }
    let p = path(workspace, pid);
    let text = std::fs::read_to_string(&p).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            t!("project-missing", pid = pid.to_string())
        } else {
            format!("{}: {e}", p.display())
        }
    })?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
    if v.get("kind").and_then(Value::as_str) != Some(KIND) {
        return Err(format!("project {pid} is not a speech project"));
    }
    serde_json::from_value(v).map_err(|e| format!("{}: {e}", p.display()))
}

/// Записать проект через ревизии (кто сохранил — окно, агент, задача — узнают все окна).
pub fn save(workspace: &Path, pid: &str, project: &SpeechProject) -> Result<(), String> {
    super::validation::validate(project)?;
    let dir = workspace.join(pid);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    crate::app::revisions::save_with_revision(&dir, || {
        let tmp = dir.join("project.json.tmp");
        std::fs::write(&tmp, text.as_bytes()).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, dir.join("project.json")).map_err(|e| format!("{}: {e}", dir.display()))
    })
}

/// Only callers of `edit` share this read-modify-write lock; direct saves do not take it.
pub fn edit<R>(workspace: &Path, pid: &str, change: impl FnOnce(&mut SpeechProject) -> R) -> Result<R, String> {
    static WRITERS: LazyLock<std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<std::sync::Mutex<()>>>>> =
        LazyLock::new(Default::default);
    let lock = WRITERS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .entry(pid.to_string())
        .or_default()
        .clone();
    let _turn = lock.lock().unwrap_or_else(|p| p.into_inner());
    let mut project = load(workspace, pid)?;
    let out = change(&mut project);
    save(workspace, pid, &project)?;
    Ok(out)
}
