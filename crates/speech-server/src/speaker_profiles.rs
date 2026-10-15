//! Who speaks in a recording, by the sound of the voice: each speaker's WeSpeaker embedding is set against the
//! speakers the user named before (the profiles) and the voices of the library.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::listen_project as lp;

/// The profile store, in the data root.
const FILE: &str = "speaker-profiles.json";
/// How much of a speaker's speech goes into one embedding.
const MAX_SECS: f64 = 30.0;
/// Less speech than this tells too little of the voice to match it.
const MIN_SECS: f64 = 2.0;
const SR: u32 = 16_000;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub embedding: Vec<f32>,
    /// How many recordings the profile was learnt from.
    pub recordings: u32,
    /// The recordings it was learnt from, so one recording counts once.
    #[serde(default)]
    pub pids: Vec<String>,
}

pub struct Paths {
    pub repo_root: PathBuf,
    pub workspace: PathBuf,
    pub voices_dir: PathBuf,
    pub models_root: PathBuf,
}

pub fn load(repo_root: &Path) -> Result<Vec<Profile>, String> {
    let file = repo_root.join(FILE);
    match std::fs::read(&file) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}: {e}", file.display())),
    }
}

fn save(repo_root: &Path, profiles: &[Profile]) -> Result<(), String> {
    let file = repo_root.join(FILE);
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(profiles).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("{}: {e}", file.display()))
}

fn normalized(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 1e-8 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

/// One more recording of the profile's person: the print moves towards it as much as one recording weighs.
fn learn(pr: &mut Profile, print: &[f32]) {
    let n = pr.recordings as f32;
    pr.embedding = normalized(pr.embedding.iter().zip(print).map(|(a, b)| (a * n + b) / (n + 1.0)).collect());
    pr.recordings += 1;
}

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (*x as f64) * (*y as f64)).sum()
}

fn embedder(p: &Paths) -> Result<dub_faces::VoiceEmbedder, String> {
    crate::ensure_job_components(&p.repo_root, &p.models_root, true, false, &|_| {})?;
    dub_faces::VoiceEmbedder::load(&dub_faces::wespeaker_path(&p.models_root)).map_err(|e| e.to_string())
}

/// Each speaker's speech of the recording, up to `MAX_SECS`, as 16 kHz samples; speakers with too little are left out.
fn speakers_audio(p: &Paths, pid: &str) -> Result<(lp::ListenProject, Vec<(String, Vec<f32>)>), String> {
    let project = lp::load(&p.workspace, pid)?;
    let dir = lp::dir(&p.workspace, pid);
    let clean = dir.join("work").join("vocals16.wav");
    let wav = if clean.is_file() { clean } else { dir.join("work").join("audio16.wav") };
    if !wav.is_file() {
        return Err(t!("listen-transcribe-first"));
    }
    let (samples, sr) = crate::wavio::read_mono_f32(&wav)?;
    if sr != SR {
        return Err(format!("{}: {sr} Hz, the voice print takes {SR} Hz", wav.display()));
    }
    let mut out = Vec::new();
    for key in project.speakers.keys() {
        let mut voice: Vec<f32> = Vec::new();
        for l in project.lines.iter().filter(|l| &l.speaker == key && !l.hidden) {
            let a = ((l.start * SR as f64) as usize).min(samples.len());
            let z = ((l.end * SR as f64) as usize).min(samples.len());
            voice.extend_from_slice(&samples[a..z]);
            if voice.len() as f64 >= MAX_SECS * SR as f64 {
                break;
            }
        }
        if voice.len() as f64 >= MIN_SECS * SR as f64 {
            out.push((key.clone(), voice));
        }
    }
    Ok((project, out))
}

/// A library voice's embedding, kept by its file's size and time so it is worked out once.
fn voice_print(p: &Paths, emb: &mut dub_faces::VoiceEmbedder, name: &str) -> Result<Vec<f32>, String> {
    let audio = crate::voice_library::audio_path(&p.voices_dir, name).ok_or_else(|| t!("voice-missing", name = name.to_string()))?;
    let meta = std::fs::metadata(&audio).map_err(|e| format!("{}: {e}", audio.display()))?;
    let stamp = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let cache = p.voices_dir.join(".work").join("prints");
    std::fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    let tag = blake3::hash(format!("{name}|{}|{stamp}", meta.len()).as_bytes()).to_hex()[..16].to_string();
    let file = cache.join(format!("{tag}.json"));
    if let Ok(bytes) = std::fs::read(&file) {
        return serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", file.display()));
    }
    let tmp = tempfile::Builder::new().prefix("print-").tempdir().map_err(|e| e.to_string())?;
    let wav = tmp.path().join("16k.wav");
    crate::media::to_16k_mono(&audio, &wav)?;
    let (samples, _) = crate::wavio::read_mono_f32(&wav)?;
    let cut = samples.len().min((MAX_SECS * SR as f64) as usize);
    let print = emb.embed_samples(&samples[..cut]).map_err(|e| e.to_string())?;
    std::fs::write(&file, serde_json::to_vec(&print).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(print)
}

/// For each speaker of the recording, the most alike profile or library voice, and whether it is alike enough to be
/// the same person.
pub fn matches(p: &Paths, pid: &str) -> Result<Value, String> {
    let (project, voices) = speakers_audio(p, pid)?;
    let mut emb = embedder(p)?;
    let profiles = load(&p.repo_root)?;
    let (library, _) = crate::voice_library::list(&p.voices_dir);
    let mut prints: Vec<(String, &str, Vec<f32>)> = profiles.iter().map(|pr| (pr.name.clone(), "profile", pr.embedding.clone())).collect();
    for v in &library {
        prints.push((v.name.clone(), "voice", voice_print(p, &mut emb, &v.name)?));
    }
    let threshold = dub_faces::voice_cos_threshold() as f64;
    let mut out = Vec::new();
    for (key, audio) in &voices {
        let print = emb.embed_samples(audio).map_err(|e| e.to_string())?;
        let best = prints.iter().map(|(name, kind, e)| (name, *kind, cosine(&print, e))).max_by(|a, b| a.2.total_cmp(&b.2));
        out.push(json!({
            "speaker": key,
            "name": project.speakers.get(key),
            "best": best.map(|(name, kind, score)| json!({ "name": name, "kind": kind, "score": (score * 100.0).round() / 100.0, "same": score >= threshold })),
        }));
    }
    Ok(json!({ "speakers": out, "threshold": threshold }))
}

/// Keeps the voice of a recording's speaker under the name it has there: a known name learns from one more recording.
pub fn remember(p: &Paths, pid: &str, speaker: &str) -> Result<Value, String> {
    let (project, voices) = speakers_audio(p, pid)?;
    let name = project.speakers.get(speaker).cloned().ok_or_else(|| format!("no speaker {speaker} in this recording"))?;
    let audio = voices.iter().find(|(k, _)| k == speaker).map(|(_, a)| a).ok_or_else(|| t!("speaker-too-little", name = name.clone()))?;
    let print = embedder(p)?.embed_samples(audio).map_err(|e| e.to_string())?;
    let mut profiles = load(&p.repo_root)?;
    match profiles.iter_mut().find(|pr| pr.name == name) {
        Some(pr) if pr.pids.iter().any(|x| x == pid) => {}
        Some(pr) => {
            learn(pr, &print);
            pr.pids.push(pid.to_string());
        }
        None => profiles.push(Profile { name: name.clone(), embedding: print, recordings: 1, pids: vec![pid.to_string()] }),
    }
    save(&p.repo_root, &profiles)?;
    let recordings = profiles.iter().find(|pr| pr.name == name).map_or(1, |pr| pr.recordings);
    Ok(json!({ "name": name, "recordings": recordings }))
}

pub fn forget(repo_root: &Path, name: &str) -> Result<Value, String> {
    let mut profiles = load(repo_root)?;
    let before = profiles.len();
    profiles.retain(|pr| pr.name != name);
    if profiles.len() == before {
        return Err(format!("no speaker profile named {name}"));
    }
    save(repo_root, &profiles)?;
    Ok(json!({ "forgot": name }))
}

pub fn listing(repo_root: &Path) -> Result<Value, String> {
    Ok(json!({ "profiles": load(repo_root)?.iter().map(|pr| json!({ "name": pr.name, "recordings": pr.recordings })).collect::<Vec<_>>() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_learns_from_more_recordings_and_can_be_forgotten() {
        let d = tempfile::tempdir().unwrap();
        save(d.path(), &[Profile { name: "Anna".into(), embedding: vec![1.0, 0.0], recordings: 1, pids: vec!["a".into()] }]).unwrap();
        let mut profiles = load(d.path()).unwrap();
        learn(&mut profiles[0], &[0.0, 1.0]);
        assert_eq!(profiles[0].recordings, 2);
        assert!((cosine(&profiles[0].embedding, &[1.0, 0.0]) - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        learn(&mut profiles[0], &[0.0, 1.0]);
        assert!(cosine(&profiles[0].embedding, &[0.0, 1.0]) > cosine(&profiles[0].embedding, &[1.0, 0.0]));
        assert!(forget(d.path(), "Boris").is_err());
        forget(d.path(), "Anna").unwrap();
        assert!(load(d.path()).unwrap().is_empty());
    }
}
