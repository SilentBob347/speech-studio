//! The book's or the play's own tracks: sounds from this computer — files and whole folders — copied into the
//! project's `media/` and laid one after another on a track, and their place in the export's mix.

use std::path::{Path, PathBuf};

use crate::speech::project as sp;
use crate::speech::project::{SpeechProject, Track, TrackClip};

/// What a track takes from a folder: its sound files, by name.
const AUDIO: [&str; 10] = ["wav", "mp3", "flac", "ogg", "opus", "m4a", "aac", "wma", "aif", "aiff"];

pub fn media_dir(workspace: &Path, pid: &str) -> PathBuf {
    workspace.join(pid).join("media")
}

fn is_audio(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO.contains(&e.to_ascii_lowercase().as_str()))
}

/// The sound files `paths` name: a file as it is, a folder as its sound files sorted by name; anything else is refused
/// with its path.
pub fn expand(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut inside: Vec<PathBuf> = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file() && is_audio(p)).collect();
            if inside.is_empty() {
                return Err(t!("tracks-empty-folder", path = path.display().to_string()));
            }
            inside.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
            out.extend(inside);
        } else if path.is_file() && is_audio(path) {
            out.push(path.clone());
        } else {
            return Err(t!("tracks-not-audio", path = path.display().to_string()));
        }
    }
    Ok(out)
}

/// A name in `media/` for `name` that no file there has yet.
fn free_name(dir: &Path, name: &str) -> String {
    let path = Path::new(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "sound".into());
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let clean: String = stem.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string();
    let clean = if clean.is_empty() { "sound".to_string() } else { clean };
    (0..).map(|k| if k == 0 { format!("{clean}{ext}") } else { format!("{clean} ({k}){ext}") }).find(|n| !dir.join(n).exists()).unwrap_or_default()
}

/// Copies `files` into the project's `media/` and makes a clip of each, laid one after another from `at`.
pub fn clips_of(workspace: &Path, pid: &str, files: &[PathBuf], at: f64) -> Result<Vec<TrackClip>, String> {
    let dir = media_dir(workspace, pid);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut clips = Vec::new();
    let mut cursor = at.max(0.0);
    for src in files {
        let duration = crate::audio::media::duration(src)?;
        let name = free_name(&dir, &src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        std::fs::copy(src, dir.join(&name)).map_err(|e| format!("{}: {e}", src.display()))?;
        let label = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| name.clone());
        clips.push(TrackClip { id: sp::new_id(), file: name, label, start: cursor, duration, ..Default::default() });
        cursor += duration;
    }
    Ok(clips)
}

/// Puts `clips` on the track `track` (an id of the project's), or on a new one named `name`; answers the track's id.
/// `at` of None lays them after the track's last clip.
pub fn place(project: &mut SpeechProject, track: Option<&str>, name: &str, mut clips: Vec<TrackClip>, at: Option<f64>) -> Result<String, String> {
    let index = match track {
        Some(id) => project.tracks.iter().position(|t| t.id == id).ok_or_else(|| t!("tracks-no-track", track = id.to_string()))?,
        None => {
            project.tracks.push(Track { id: sp::new_id(), name: name.to_string(), ..Default::default() });
            project.tracks.len() - 1
        }
    };
    let track = &mut project.tracks[index];
    if at.is_none() {
        let end = track.clips.iter().map(|c| c.start + (c.kept()[1] - c.kept()[0])).fold(0.0, f64::max);
        let first = clips.first().map(|c| c.start).unwrap_or(0.0);
        clips.iter_mut().for_each(|c| c.start += end - first);
    }
    track.clips.extend(clips);
    Ok(track.id.clone())
}

/// Adds the tracks' clips into the mix at their places, each through its cut, its montage and its track's gain; a
/// muted track stays out. `stems` gets a track's sound under its name when it is asked for.
pub fn mix_in(workspace: &Path, pid: &str, project: &SpeechProject, sr: u32, mix: &mut Vec<f32>, work: &Path, mut stems: Option<&mut std::collections::BTreeMap<String, Vec<f32>>>) -> Result<(), String> {
    let dir = media_dir(workspace, pid);
    for (t, track) in project.tracks.iter().enumerate().filter(|(_, t)| !t.muted) {
        let gain = 10f32.powf(track.gain_db as f32 / 20.0);
        for (c, clip) in track.clips.iter().enumerate() {
            let wav = work.join(format!("track-{t}-{c}.wav"));
            crate::audio::media::extract_audio(&dir.join(&clip.file), &wav, sr, 1)?;
            let (samples, _) = crate::audio::wavio::read_mono_f32(&wav)?;
            let [a, z] = clip.kept();
            let from = ((a * sr as f64) as usize).min(samples.len());
            let to = ((z * sr as f64) as usize).clamp(from, samples.len());
            let mut piece = samples[from..to].to_vec();
            clip.mix.apply(&mut piece, sr);
            piece.iter_mut().for_each(|s| *s *= gain);
            let at = (clip.start * sr as f64) as usize;
            if mix.len() < at + piece.len() {
                mix.resize(at + piece.len(), 0.0);
            }
            for (k, s) in piece.iter().enumerate() {
                mix[at + k] += *s;
            }
            if let Some(stems) = stems.as_deref_mut() {
                let stem = stems.entry(format!("{} {}", t!("tracks-stem"), track.name)).or_default();
                if stem.len() < at + piece.len() {
                    stem.resize(at + piece.len(), 0.0);
                }
                for (k, s) in piece.iter().enumerate() {
                    stem[at + k] += *s;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_go_on_a_new_track_and_then_after_its_last_clip() {
        let mut p = SpeechProject::default();
        let clip = |d: f64| TrackClip { id: sp::new_id(), file: "a.wav".into(), duration: d, ..Default::default() };
        let id = place(&mut p, None, "Музыка", vec![clip(10.0)], Some(2.0)).unwrap();
        assert_eq!(p.tracks[0].name, "Музыка");
        let mut more = vec![clip(3.0), clip(4.0)];
        more[1].start = 3.0;
        place(&mut p, Some(&id), "", more, None).unwrap();
        let starts: Vec<f64> = p.tracks[0].clips.iter().map(|c| c.start).collect();
        assert_eq!(starts, [0.0, 10.0, 13.0]);
        assert!(place(&mut p, Some("nope"), "", vec![], None).is_err());
    }

    #[test]
    fn a_folder_gives_its_sound_files_by_name_and_other_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b.mp3", "A.wav", "notes.txt"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let got = expand(&[dir.path().to_path_buf()]).unwrap();
        let names: Vec<String> = got.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["A.wav", "b.mp3"]);
        assert!(expand(&[dir.path().join("notes.txt")]).is_err());
        assert_eq!(free_name(dir.path(), "A.wav"), "A (1).wav");
    }
}
