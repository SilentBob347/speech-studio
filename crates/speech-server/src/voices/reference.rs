use std::path::{Path, PathBuf};

use super::{library, ref_pick};

/// Реф голоса под лимит движка: короткий — как есть (WAV); длинный — лучший кусок по словам распознавания с его
/// текстом (у голоса с `rotate` — кусок по очереди для дубля `take`), а без таймингов — первые `limit` секунд без
/// текста (клон по одному звуку). Готовые куски кэшируются по размеру и времени файла.
pub(crate) fn prepare_reference(voices: &Path, name: &str, limit: f64, take: u32) -> Result<(PathBuf, Option<String>), String> {
    let lib = library::get(voices, name)?;
    let audio = library::audio_path(voices, name).ok_or_else(|| t!("voice-missing", name = name.to_string()))?;
    let meta = std::fs::metadata(&audio).map_err(|e| format!("{}: {e}", audio.display()))?;
    let stamp = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cache = voices.join(".work").join("refs");
    std::fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    let words = library::read_words(voices, name);
    let turn = if lib.card.rotate { take.saturating_sub(1) as usize } else { 0 };
    let tag =
        blake3::hash(format!("{name}|{}|{stamp}|{limit:.1}|{}|{turn}", meta.len(), words.is_some()).as_bytes()).to_hex()[..16].to_string();
    let wav = cache.join(format!("{tag}.wav"));
    let txt = cache.join(format!("{tag}.txt"));
    if wav.is_file() {
        return Ok((wav, std::fs::read_to_string(&txt).ok().filter(|t| !t.trim().is_empty())));
    }
    let duration = crate::media::duration(&audio)?;
    let (start, end, text) = if duration <= limit + 0.5 {
        (0.0, duration, lib.transcript.clone())
    } else if let Some(words) = words {
        let tmp = cache.join(format!("{tag}-16k.wav"));
        crate::media::to_16k_mono(&audio, &tmp)?;
        let (samples, sr) = crate::wavio::read_mono_f32(&tmp)?;
        let _ = std::fs::remove_file(&tmp);
        let pieces = ref_pick::candidates(
            &words,
            &samples,
            sr,
            (limit * 0.4).min(5.0),
            limit,
            if lib.card.rotate { 4 } else { 1 },
        );
        match pieces.get(turn % pieces.len().max(1)) {
            Some(c) => (c.start, c.end, Some(c.text.clone())),
            None => (0.0, limit, None),
        }
    } else {
        (0.0, limit, None)
    };
    crate::media::trim_ref(&audio, &wav, start, end)?;
    std::fs::write(&txt, text.as_deref().unwrap_or("")).map_err(|e| format!("{}: {e}", txt.display()))?;
    Ok((wav, text.filter(|t| !t.trim().is_empty())))
}

#[cfg(test)]
mod tests;
