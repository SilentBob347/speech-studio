//! Listening project persistence and file availability, outside the project model.

use super::project::{ListenProject, KIND};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn project_dir(workspace: &Path, pid: &str) -> PathBuf {
    workspace.join(pid)
}

pub fn load(workspace: &Path, pid: &str) -> Result<ListenProject, String> {
    if !crate::platform::workspace::is_pid(pid) {
        return Err(format!("bad project id {pid}"));
    }
    let p = crate::platform::workspace::project_file(workspace, pid);
    let text = std::fs::read_to_string(&p).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            t!("project-missing", pid = pid.to_string())
        } else {
            format!("{}: {e}", p.display())
        }
    })?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
    if v.get("kind").and_then(Value::as_str) != Some(KIND) {
        return Err(format!("project {pid} is not a listening project"));
    }
    serde_json::from_value(v).map_err(|e| format!("{}: {e}", p.display()))
}

pub fn save(workspace: &Path, pid: &str, project: &ListenProject) -> Result<(), String> {
    project.validate()?;
    let dir = project_dir(workspace, pid);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    crate::app::save_with_revision(&dir, || {
        crate::platform::fs::write_atomic(&dir.join("project.json"), text.as_bytes())
    })
}

/// Файл записи проекта.
pub fn source_path(workspace: &Path, pid: &str, project: &ListenProject) -> Result<PathBuf, String> {
    let p = project_dir(workspace, pid).join(&project.source);
    if project.source.is_empty() || !p.is_file() {
        return Err(t!("listen-no-source"));
    }
    Ok(p)
}
