//! Пути данных студии для раздела «О программе».

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::AppState;

/// GET /app/paths — где лежат данные студии (раздел «О программе»).
pub async fn app_paths(State(st): State<AppState>) -> Json<Value> {
    Json(json!({
        "data_dir": st.repo_root.display().to_string(),
        "projects_dir": st.workspace.display().to_string(),
        "models_dir": st.models_root.display().to_string(),
    }))
}
