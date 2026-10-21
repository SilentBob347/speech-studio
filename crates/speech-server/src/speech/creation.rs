use std::path::Path;

use super::project::{self, SpeechProject};
use crate::text::subimport::Cue;

pub(crate) struct TranscriptProject {
    project: SpeechProject,
}

pub(crate) fn from_transcript(name: &str, cues: &[Cue], narrator: &str) -> TranscriptProject {
    TranscriptProject {
        project: project::from_cues(name, cues, narrator),
    }
}

impl TranscriptProject {
    pub(crate) fn assign_voice(&mut self, speaker: &str, voice: &str) {
        self.project.cast.insert(speaker.to_string(), voice.to_string());
    }

    pub(crate) fn create(self, workspace: &Path) -> Result<String, String> {
        let pid = crate::platform::ids::new_id();
        project::save(workspace, &pid, &self.project)?;
        Ok(pid)
    }
}
