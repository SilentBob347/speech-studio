use super::project as lp;
use serde_json::{json, Value};

#[cfg(test)]
mod tests;

pub(super) static RECORDING: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// The recording the microphone is making now, if it is on.
pub fn recording() -> Option<String> {
    RECORDING.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
/// Stop the microphone and finalize the Listening project, including its live transcript.
pub(crate) async fn stop_recording(workspace: &std::path::Path) -> Result<Value, StopError> {
    let Some(pid) = RECORDING.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return Err(StopError::NotRecording);
    };
    let ws = workspace.to_path_buf();
    let res = tokio::task::spawn_blocking(crate::app::carry(move || {
        crate::voices::record::stop()?;
        let heard = crate::listening::live::finish();
        let project = super::create_from_source(
            &ws,
            &pid,
            &t!("listen-mic-name", time = chrono::Local::now().format("%d.%m %H:%M").to_string()),
            "source.wav",
            &t!("listen-mic-origin"),
        )?;
        let made = json!({ "pid": pid, "project": project });
        let heard = heard?;
        if heard.is_empty() {
            return Ok(made);
        }
        // the phrases heard live are the recording's first transcript, in one speaker's voice
        let mut project = crate::listening::load(&ws, &pid)?;
        project.speakers.insert("0".into(), t!("listen-speaker-n", n = 1));
        project.lines = heard
            .iter()
            .enumerate()
            .map(|(i, (start, end, text))| lp::Line {
                id: format!("live{i}"),
                start: *start,
                end: *end,
                speaker: "0".into(),
                words: lp::spread_words(text, *start, *end),
                text: text.clone(),
                approx: true,
                ..Default::default()
            })
            .collect();
        crate::listening::save(&ws, &pid, &project)?;
        Ok(json!({ "pid": pid, "project": project }))
    }))
    .await
    .map_err(|e| StopError::Join(e.to_string()))?;
    res.map_err(StopError::Failed)
}

#[derive(Debug)]
pub(crate) enum StopError {
    NotRecording,
    Join(String),
    Failed(String),
}
pub(crate) fn stop_response(result: Result<Value, StopError>) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse, Json};
    match result {
        Ok(value) => Json(value).into_response(),
        Err(StopError::NotRecording) => crate::app::refuse(StatusCode::CONFLICT, "not_recording", t!("listen-not-recording")),
        Err(StopError::Join(error)) => crate::app::refuse(StatusCode::INTERNAL_SERVER_ERROR, "internal", error),
        Err(StopError::Failed(error)) if error.starts_with(crate::app::PROJECT_CHANGED) => {
            crate::app::refuse(StatusCode::CONFLICT, crate::app::PROJECT_CHANGED, error)
        }
        Err(StopError::Failed(error)) => crate::app::refuse(StatusCode::INTERNAL_SERVER_ERROR, "listen_failed", error),
    }
}
