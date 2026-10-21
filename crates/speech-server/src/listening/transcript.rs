use std::path::Path;

use crate::text::subimport::Cue;

pub(crate) struct TranscriptExport {
    pub name: String,
    pub cues: Vec<Cue>,
    pub speaker_names: Vec<String>,
}

pub(crate) fn export_transcript(workspace: &Path, pid: &str) -> Result<TranscriptExport, String> {
    let project = super::load(workspace, pid)?;
    let cues = project
        .shown()
        .map(|line| Cue {
            start: line.start,
            end: line.end,
            text: line.text.clone(),
            speaker: Some(project.speaker_name(&line.speaker)),
        })
        .collect();
    Ok(TranscriptExport {
        name: project.name,
        cues,
        speaker_names: project.speakers.into_values().collect(),
    })
}
