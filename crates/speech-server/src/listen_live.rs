//! Recognition while the microphone records: the recorder's sound (`record::tap`) is cut into utterances where the
//! speaker pauses (Silero VAD), and each is recognised as it ends by the recogniser a file of that language would get,
//! loaded once for the whole recording. The phrases become the recording's first transcript when it stops; full
//! recognition (speakers, exact words) runs as for any recording.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

use crate::localize::Localize;

/// A pause this long after speech ends the utterance.
const PAUSE_SECS: f64 = 0.7;
/// An utterance without a pause is cut at this length.
const LONGEST_SECS: f64 = 20.0;
/// Speech starts and ends this much wider than the detector heard it.
const EDGE_SECS: f64 = 0.15;
/// While a phrase goes on, what it says so far is recognised again this often.
const INTERIM_SECS: f64 = 1.5;

/// A phrase heard live: where it starts and ends in the recording, seconds, and what was said.
pub type Heard = (f64, f64, String);

struct Live {
    lines: Vec<Heard>,
    /// The phrase being said: where it started and what it says so far, until a pause ends it.
    partial: Option<(f64, String)>,
    error: Option<String>,
    /// The person turned the microphone off for a while: what it hears is taken as silence, the session goes on.
    muted: bool,
    /// The places in `lines` of what the person typed: their own words, never the studio's voice heard back.
    typed: std::collections::HashSet<usize>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

fn live() -> &'static Mutex<Option<Live>> {
    static L: OnceLock<Mutex<Option<Live>>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(None))
}

fn with<R>(f: impl FnOnce(&mut Live) -> R) -> Option<R> {
    live().lock().unwrap_or_else(|p| p.into_inner()).as_mut().map(f)
}

/// Starts recognising the recording that has just started, in `lang`.
pub fn start(repo_root: &Path, models_root: &Path, lang: &str, engine: &str) -> Result<(), String> {
    let choice = crate::asr_util::choice_of(repo_root, models_root, lang, &t!("listen-live-name"), engine)?;
    let vad = crate::vad::model_path(models_root);
    if !vad.is_file() {
        return Err(t!("listen-live-no-vad", path = vad.display().to_string()));
    }
    let (to, from) = channel::<Vec<f32>>();
    let stop = Arc::new(AtomicBool::new(false));
    *live().lock().unwrap_or_else(|p| p.into_inner()) = Some(Live { lines: Vec::new(), partial: None, error: None, muted: false, typed: Default::default(), stop: stop.clone(), worker: None });
    crate::record::tap(Some(to));
    std::env::set_var("DUB_ASR_BACKEND", crate::models::stage_backend(models_root, "asr_backend"));
    let lang = if lang == "auto" { String::new() } else { lang.to_string() };
    let worker = std::thread::spawn(move || {
        if let Err(e) = listen(choice, vad, &lang, from, &stop, &crate::record::rate) {
            tracing::error!("[ERROR] live recognition: {e}");
            with(|l| l.error = Some(e));
        }
    });
    with(|l| l.worker = Some(worker));
    Ok(())
}

/// A piece of the microphone as recognition takes it: silence while the person has it muted.
fn heard_as(mut chunk: Vec<f32>) -> Vec<f32> {
    if with(|l| l.muted).unwrap_or(false) {
        chunk.fill(0.0);
    }
    chunk
}

/// Whether the running session's microphone is muted; None with no session.
pub fn muted() -> Option<bool> {
    with(|l| l.muted)
}

/// Mutes or unmutes the microphone of the running session.
pub fn mute(muted: bool) -> Result<(), String> {
    with(|l| l.muted = muted).ok_or_else(|| t!("listen-live-not-running"))
}

/// A line the person typed instead of saying it, put after what was heard last.
pub fn type_line(text: &str) -> Result<(), String> {
    with(|l| {
        let at = l.lines.last().map(|line| line.1).unwrap_or(0.0);
        l.typed.insert(l.lines.len());
        l.lines.push((at, at, text.to_string()));
    })
    .ok_or_else(|| t!("listen-live-not-running"))
}

/// What live recognition has heard so far, and why it stopped when it did.
pub fn state() -> Value {
    match live().lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        Some(l) => json!({
            "running": !l.stop.load(Ordering::Relaxed),
            "lines": l.lines.iter().enumerate().map(|(n, (a, z, text))| json!({ "start": a, "end": z, "text": text, "typed": l.typed.contains(&n) })).collect::<Vec<_>>(),
            "partial": l.partial.as_ref().map(|(a, text)| json!({ "start": a, "text": text })),
            "error": l.error,
            "muted": l.muted,
        }),
        None => json!({ "running": false, "lines": [], "partial": null, "error": null, "muted": false }),
    }
}

pub fn finish() -> Result<Vec<Heard>, String> {
    crate::record::tap(None);
    let (stop, worker) = match live().lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        Some(l) => (l.stop.clone(), l.worker.take()),
        None => return Ok(Vec::new()),
    };
    stop.store(true, Ordering::Relaxed);
    let joined = worker.map(|w| w.join().is_ok()).unwrap_or(true);
    let Some(l) = live().lock().unwrap_or_else(|p| p.into_inner()).take() else {
        return Ok(Vec::new());
    };
    match (joined, l.error) {
        (false, _) => Err(t!("listen-live-broke")),
        (true, Some(e)) => Err(e),
        (true, None) => Ok(l.lines),
    }
}

/// Linear resampling to 16 kHz: enough for the speech detector, the recogniser gets the same.
pub(crate) fn to_16k(x: &[f32], sr: u32) -> Vec<f32> {
    if sr == 16_000 || x.is_empty() {
        return x.to_vec();
    }
    let n = (x.len() as f64 * 16_000.0 / sr as f64) as usize;
    (0..n)
        .map(|i| {
            let at = i as f64 * sr as f64 / 16_000.0;
            let k = at as usize;
            let f = (at - k as f64) as f32;
            let a = x.get(k).copied().unwrap_or(0.0);
            let b = x.get(k + 1).copied().unwrap_or(a);
            a + (b - a) * f
        })
        .collect()
}

fn listen(choice: crate::models::AsrChoice, vad: PathBuf, lang: &str, from: Receiver<Vec<f32>>, stop: &AtomicBool, rate: &dyn Fn() -> u32) -> Result<(), String> {
    let mut asr = crate::models::build_engine(&choice, None);
    let work = tempfile::Builder::new().prefix("live-").tempdir().map_err(|e| e.to_string())?;
    let mut pending: Vec<f32> = Vec::new();
    let mut offset = 0.0f64;
    let mut ended = false;
    let mut interim_at = 0.0f64;
    let wav = work.path().join("utterance.wav");
    let mut hear = |piece: &[f32]| -> Result<String, String> {
        let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&wav, spec).map_err(|e| e.to_string())?;
        for s in piece {
            w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).map_err(|e| e.to_string())?;
        }
        w.finalize().map_err(|e| e.to_string())?;
        let heard = asr.transcribe(&wav, if lang.is_empty() { "auto" } else { lang }).map_err(|e| e.localize())?;
        Ok(heard.iter().map(|s| s.text.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "))
    };
    while !ended {
        match from.recv_timeout(Duration::from_millis(300)) {
            Ok(chunk) => pending.extend(heard_as(chunk)),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => ended = true,
        }
        while let Ok(chunk) = from.try_recv() {
            pending.extend(heard_as(chunk));
        }
        ended |= stop.load(Ordering::Relaxed);
        let sr = rate().max(1);
        let audio = to_16k(&pending, sr);
        let secs = audio.len() as f64 / 16_000.0;
        if secs < 1.0 && !ended {
            continue;
        }
        let probs = crate::vad::probabilities(&vad, &audio)?;
        let spans = crate::vad::speech(&probs, audio.len(), &crate::vad::VadConfig::default());
        let Some(&(first, _)) = spans.first() else {
            // silence: keep only its last half second, where speech may be starting
            if secs > 5.0 {
                let keep = (0.5 * sr as f64) as usize;
                let drop = pending.len().saturating_sub(keep);
                pending.drain(..drop);
                offset += drop as f64 / sr as f64;
            }
            continue;
        };
        let last = spans.last().map(|s| s.1).unwrap_or(first);
        let cut = if ended || secs - last >= PAUSE_SECS {
            Some((last + EDGE_SECS).min(secs))
        } else if last - first >= LONGEST_SECS {
            Some(last)
        } else {
            None
        };
        let a = (first - EDGE_SECS).max(0.0);
        let Some(z) = cut else {
            // the phrase goes on: show what it says so far
            if secs - interim_at >= INTERIM_SECS && last - first >= INTERIM_SECS {
                interim_at = secs;
                let so_far = hear(&audio[(a * 16_000.0) as usize..])?;
                with(|l| l.partial = Some((offset + a, so_far)));
            }
            continue;
        };
        let text = hear(&audio[(a * 16_000.0) as usize..((z * 16_000.0) as usize).min(audio.len())])?;
        with(|l| l.partial = None);
        interim_at = 0.0;
        if !text.is_empty() {
            let line = (((offset + a) * 100.0).round() / 100.0, ((offset + z) * 100.0).round() / 100.0, text);
            with(|l| l.lines.push(line));
        }
        let used = ((z * sr as f64) as usize).min(pending.len());
        pending.drain(..used);
        offset += used as f64 / sr as f64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LIVE_WAV: a recording to feed as the microphone; SPEECH_STUDIO_ROOT: a studio root with Parakeet and Silero VAD.
    #[test]
    #[ignore]
    fn a_recording_fed_as_the_microphone_is_heard_phrase_by_phrase() {
        let root = PathBuf::from(std::env::var("SPEECH_STUDIO_ROOT").expect("SPEECH_STUDIO_ROOT"));
        let models = root.join("models");
        crate::set_ort_dylib_env(&root, &root);
        let (x, sr) = crate::wavio::read_mono_f32(Path::new(&std::env::var("LIVE_WAV").expect("LIVE_WAV"))).unwrap();
        let choice = crate::asr_util::choice(&root, &models, "ru", "test").unwrap();
        std::env::set_var("DUB_ASR_BACKEND", crate::models::stage_backend(&models, "asr_backend"));
        *live().lock().unwrap() = Some(Live { lines: Vec::new(), partial: None, error: None, muted: false, typed: Default::default(), stop: Arc::new(AtomicBool::new(false)), worker: None });
        let (to, from) = channel::<Vec<f32>>();
        let feeder = std::thread::spawn(move || {
            for chunk in x.chunks((sr / 10) as usize) {
                to.send(chunk.to_vec()).unwrap();
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let partials = std::thread::spawn(|| {
            let mut seen = Vec::new();
            for _ in 0..120 {
                if let Some(p) = live().lock().unwrap().as_ref().and_then(|l| l.partial.clone()) {
                    if seen.last() != Some(&p.1) {
                        seen.push(p.1);
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            seen
        });
        let stop = AtomicBool::new(false);
        listen(choice, crate::vad::model_path(&models), "ru", from, &stop, &move || sr).unwrap();
        feeder.join().unwrap();
        let seen = partials.join().unwrap();
        for p in &seen {
            eprintln!("… {p}");
        }
        assert!(!seen.is_empty(), "no text while the phrase went on");
        let lines = live().lock().unwrap().take().unwrap().lines;
        for l in &lines {
            eprintln!("{:.2}-{:.2} {}", l.0, l.1, l.2);
        }
        assert!(!lines.is_empty() && lines.iter().any(|l| l.2.contains("1945")), "{lines:?}");
        assert!(lines.windows(2).all(|w| w[0].1 <= w[1].0 + 0.01));
    }

    #[test]
    fn resampling_to_16k_keeps_the_length_in_time() {
        let x: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.01).sin()).collect();
        let y = to_16k(&x, 48_000);
        assert_eq!(y.len(), 16_000);
        assert!((y[100] - x[300]).abs() < 1e-3);
        assert_eq!(to_16k(&x[..10], 16_000).len(), 10);
    }
}
