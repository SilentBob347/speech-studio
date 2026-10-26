//! Renaming a library voice reaches the Speech projects that speak with it.

use super::AppState;

pub(crate) fn rename_voice(st: &AppState, from: &str, to: &str) -> Result<(), String> {
    let p = crate::speech::SpeechPaths {
        repo_root: st.repo_root.clone(),
        workspace: st.workspace.clone(),
        voices_dir: st.voices_dir.clone(),
        models_root: st.models_root.clone(),
    };
    crate::speech::rename_voice(&p, from, to, || crate::voices::rename(&st.voices_dir, from, to))
}
