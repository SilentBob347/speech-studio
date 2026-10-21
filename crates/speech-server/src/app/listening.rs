use crate::listening::{ListenPaths, watching as watch};
use crate::AppState;

pub(crate) fn paths(st: &AppState) -> ListenPaths {
    ListenPaths {
        repo_root: st.repo_root.clone(),
        workspace: st.workspace.clone(),
        models_root: st.models_root.clone(),
        sortformer_onnx: st.sortformer_onnx.clone(),
    }
}

pub(crate) fn watch_dependencies(st: &AppState) -> watch::Dependencies {
    watch::Dependencies {
        paths: paths(st),
        jobs: st.jobs.clone(),
    }
}
