use std::path::PathBuf;

use serde_json::Value;

use crate::listening;
use crate::voices::profiles;

pub(crate) struct Paths {
    pub workspace: PathBuf,
    pub profiles: profiles::Paths,
}

pub(crate) fn matches(p: &Paths, pid: &str) -> Result<Value, String> {
    matches_with(p, pid, profiles::matches)
}

fn matches_with(
    p: &Paths,
    pid: &str,
    compare: impl FnOnce(&profiles::Paths, &[profiles::Speaker<'_>]) -> Result<Value, String>,
) -> Result<Value, String> {
    let prepared = listening::speakers_audio(&p.workspace, pid, profiles::MIN_SECS, profiles::MAX_SECS)?;
    let speakers: Vec<_> = prepared
        .samples
        .iter()
        .map(|(key, audio)| profiles::Speaker {
            key,
            name: prepared.names.get(key).map(String::as_str),
            audio,
        })
        .collect();
    compare(&p.profiles, &speakers)
}

pub(crate) fn remember(p: &Paths, pid: &str, speaker: &str) -> Result<Value, String> {
    remember_with(p, pid, speaker, profiles::remember)
}

fn remember_with(
    p: &Paths,
    pid: &str,
    speaker: &str,
    save: impl FnOnce(&profiles::Paths, &str, &str, &[f32]) -> Result<Value, String>,
) -> Result<Value, String> {
    let prepared = listening::speakers_audio(&p.workspace, pid, profiles::MIN_SECS, profiles::MAX_SECS)?;
    let name = prepared
        .names
        .get(speaker)
        .ok_or_else(|| format!("no speaker {speaker} in this recording"))?;
    let audio = prepared
        .samples
        .iter()
        .find(|(key, _)| key == speaker)
        .map(|(_, audio)| audio)
        .ok_or_else(|| t!("speaker-too-little", name = name.clone()))?;
    save(&p.profiles, pid, name, audio)
}

#[cfg(test)]
mod tests;
