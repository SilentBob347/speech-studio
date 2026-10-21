use std::collections::BTreeMap;
use std::path::Path;


const SR: u32 = 16_000;

pub(crate) struct SpeakersAudio {
    pub names: BTreeMap<String, String>,
    pub samples: Vec<(String, Vec<f32>)>,
}

/// Whole lines are appended until `max_secs` is reached; the last line is not cut. Hidden lines are excluded.
pub(crate) fn speakers_audio(workspace: &Path, pid: &str, min_secs: f64, max_secs: f64) -> Result<SpeakersAudio, String> {
    let project = crate::listening::load(workspace, pid)?;
    let dir = crate::listening::project_dir(workspace, pid);
    let clean = dir.join("work").join("vocals16.wav");
    let wav = if clean.is_file() {
        clean
    } else {
        dir.join("work").join("audio16.wav")
    };
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
            if voice.len() as f64 >= max_secs * SR as f64 {
                break;
            }
        }
        if voice.len() as f64 >= min_secs * SR as f64 {
            out.push((key.clone(), voice));
        }
    }
    Ok(SpeakersAudio {
        names: project.speakers,
        samples: out,
    })
}

#[cfg(test)]
mod tests;
