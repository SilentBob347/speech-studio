use std::path::Path;

use serde_json::{json, Value};


pub(crate) fn extract(ws: &Path, voices_dir: &Path, models_root: &Path, pid: &str, only: Option<&str>) -> Result<Value, String> {
    let project = crate::listening::load(ws, pid)?;
    let dir = crate::listening::project_dir(ws, pid);
    let clean = dir.join("work").join("vocals16.wav");
    let pick_wav = if clean.is_file() {
        clean
    } else {
        dir.join("work").join("audio16.wav")
    };
    if !pick_wav.is_file() {
        return Err(t!("listen-transcribe-first"));
    }
    let src = crate::listening::source_path(ws, pid, &project)?;
    let stems_vocals = dir.join("work").join("stems").join("vocals.wav");
    let cut_from = if stems_vocals.is_file() { stems_vocals } else { src };
    let (samples, sr) = crate::wavio::read_mono_f32(&pick_wav)?;
    let sel = crate::models::load_selection(models_root);
    let max_len = sel
        .get("higgs_ref_secs")
        .and_then(Value::as_str)
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(12.0);
    let mut made = Vec::new();
    let mut skipped = Vec::new();
    for (key, name) in project.speakers.iter().filter(|(k, _)| only.is_none_or(|o| o == k.as_str())) {
        let words: Vec<crate::ref_pick::Word> = project
            .lines
            .iter()
            .filter(|l| &l.speaker == key && !l.hidden && !l.approx)
            .flat_map(|l| {
                l.words.iter().map(|w| crate::ref_pick::Word {
                    word: w.text.clone(),
                    start: w.start,
                    end: w.end,
                })
            })
            .collect();
        let Some(best) = crate::ref_pick::candidates(&words, &samples, sr, (max_len * 0.4).min(5.0), max_len, 1)
            .into_iter()
            .next()
        else {
            skipped.push(name.clone());
            continue;
        };
        let tmp = tempfile::Builder::new()
            .prefix("spk-")
            .suffix(".flac")
            .tempfile()
            .map_err(|e| e.to_string())?;
        crate::media::keep_audio_ranges(&cut_from, tmp.path(), &[(best.start, best.end)])?;
        let mut card = crate::voice_library::guess_card(name);
        card.source = "listening".into();
        card.transcript_auto = true;
        if !matches!(project.settings.lang.as_str(), "" | "auto") {
            card.lang = project.settings.lang.clone();
        } else if crate::stress::is_russian(&best.text, "auto") {
            card.lang = "ru".into();
        }
        let voice = crate::voice_library::import_audio(voices_dir, &crate::voice_library::sanitize(name), tmp.path(), card)?;
        crate::voice_library::write_transcript(voices_dir, &voice, best.text.trim())?;
        let local: Vec<crate::ref_pick::Word> = words
            .iter()
            .filter(|w| w.start >= best.start - 1e-6 && w.end <= best.end + 1e-6)
            .map(|w| crate::ref_pick::Word {
                word: w.word.clone(),
                start: w.start - best.start,
                end: w.end - best.start,
            })
            .collect();
        crate::voice_library::write_words(voices_dir, &voice, &local)?;
        made.push(json!({ "speaker": key, "voice": voice, "seconds": best.duration }));
    }
    Ok(json!({ "made": made, "skipped": skipped }))
}

#[cfg(test)]
mod tests;
