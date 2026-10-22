use std::path::Path;

use crate::{listening, speech, voices};

pub(crate) fn convert(workspace: &Path, voices_dir: &Path, pid: &str) -> Result<String, String> {
    let transcript = listening::export_transcript(workspace, pid)?;
    if transcript.cues.is_empty() {
        return Err(t!("listen-empty"));
    }
    let mut project = speech::from_transcript(&transcript.name, &transcript.cues, &t!("speech-narrator"));
    for name in &transcript.speaker_names {
        if voices::audio_path(voices_dir, name).is_some() {
            project.assign_voice(name, name);
        }
    }
    project.create(workspace)
}

#[cfg(test)]
mod tests;
