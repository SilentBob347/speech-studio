//! Silero VAD v6: где в записи речь. Длинная запись режется по паузам на куски для распознавателя
//! (`windows`), а не по таймеру посреди слова.

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use ort::session::Session;
use ort::value::TensorRef;
use serde::{Deserialize, Serialize};

pub const SR: usize = 16_000;
pub const FRAME: usize = 512;
const CONTEXT: usize = 64;
/// Кусок для распознавателя не длиннее стольких секунд (окна Dub `WindowConfig::chunk_size`).
pub const MAX_WINDOW_SECS: f64 = 90.0;

pub fn model_path(models_root: &Path) -> PathBuf {
    models_root.join("silero-vad").join("silero_vad.onnx")
}

/// Настройки разметки (имена и значения по умолчанию — `get_speech_timestamps` Silero).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct VadConfig {
    pub threshold: f64,
    /// Речь кончается, когда вероятность падает ниже `threshold - hysteresis`.
    pub hysteresis: f64,
    pub min_speech_ms: u32,
    pub min_silence_ms: u32,
    pub pad_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        VadConfig { threshold: 0.5, hysteresis: 0.15, min_speech_ms: 250, min_silence_ms: 100, pad_ms: 30 }
    }
}

static SESSION: LazyLock<Mutex<Option<(PathBuf, Session)>>> = LazyLock::new(|| Mutex::new(None));

/// Вероятность речи на каждый кадр в 32 мс (16 кГц моно).
pub fn probabilities(model: &Path, samples: &[f32]) -> Result<Vec<f32>, String> {
    let mut slot = SESSION.lock().map_err(|_| "vad lock".to_string())?;
    if slot.as_ref().is_none_or(|(p, _)| p != model) {
        let err = |e: &dyn std::fmt::Display| format!("{}: {e}", model.display());
        let s = Session::builder().map_err(|e| err(&e))?.with_intra_threads(1).map_err(|e| err(&e))?.commit_from_file(model).map_err(|e| err(&e))?;
        *slot = Some((model.to_path_buf(), s));
    }
    let (_, session) = slot.as_mut().expect("vad session");
    let mut state = vec![0f32; 2 * 128];
    let mut input = vec![0f32; CONTEXT + FRAME];
    let sr = [SR as i64];
    let mut probs = Vec::with_capacity(samples.len() / FRAME + 1);
    for chunk in samples.chunks(FRAME) {
        input.copy_within(FRAME.., 0);
        input[CONTEXT..CONTEXT + chunk.len()].copy_from_slice(chunk);
        input[CONTEXT + chunk.len()..].fill(0.0);
        let out = session
            .run(ort::inputs![
                "input" => TensorRef::from_array_view((vec![1i64, (CONTEXT + FRAME) as i64], input.as_slice())).map_err(|e| e.to_string())?,
                "state" => TensorRef::from_array_view((vec![2i64, 1, 128], state.as_slice())).map_err(|e| e.to_string())?,
                "sr" => TensorRef::from_array_view((Vec::<i64>::new(), &sr[..])).map_err(|e| e.to_string())?,
            ])
            .map_err(|e| format!("vad: {e}"))?;
        let (_, p) = out["output"].try_extract_tensor::<f32>().map_err(|e| format!("vad: {e}"))?;
        probs.push(p[0]);
        let (_, s) = out["stateN"].try_extract_tensor::<f32>().map_err(|e| format!("vad: {e}"))?;
        let len = state.len();
        state.copy_from_slice(&s[..len]);
    }
    Ok(probs)
}

/// Отрезки речи в секундах по вероятностям кадров (`get_speech_timestamps` Silero без ограничения длины: длинное
/// режет `windows`).
pub fn speech(probs: &[f32], total_samples: usize, cfg: &VadConfig) -> Vec<(f64, f64)> {
    let ms = |v: u32| v as usize * SR / 1000;
    let (min_speech, min_silence, pad) = (ms(cfg.min_speech_ms), ms(cfg.min_silence_ms), ms(cfg.pad_ms));
    let off = (cfg.threshold - cfg.hysteresis).max(0.01);
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    let mut temp_end = 0usize;
    for (i, &p) in probs.iter().enumerate() {
        let (at, p) = (i * FRAME, p as f64);
        if p >= cfg.threshold {
            temp_end = 0;
            if start.is_none() {
                start = Some(at);
            }
            continue;
        }
        let Some(s) = start else { continue };
        if p < off {
            if temp_end == 0 {
                temp_end = at;
            }
            if at - temp_end >= min_silence {
                if temp_end - s > min_speech {
                    spans.push((s, temp_end));
                }
                start = None;
                temp_end = 0;
            }
        }
    }
    if let Some(s) = start {
        if total_samples.saturating_sub(s) > min_speech {
            spans.push((s, total_samples));
        }
    }
    for i in 0..spans.len() {
        if i == 0 {
            spans[0].0 = spans[0].0.saturating_sub(pad);
        }
        if i + 1 < spans.len() {
            let gap = spans[i + 1].0 - spans[i].1;
            let grow = if gap < 2 * pad { gap / 2 } else { pad };
            spans[i].1 += grow;
            spans[i + 1].0 -= grow;
        } else {
            spans[i].1 = (spans[i].1 + pad).min(total_samples);
        }
    }
    spans.into_iter().map(|(a, b)| (a as f64 / SR as f64, b as f64 / SR as f64)).collect()
}

/// Куски для распознавателя: подряд идущие отрезки речи, кусок не длиннее `max`. Переполненный кусок кончается на
/// самой длинной паузе своей второй половины; отрезок речи длиннее `max` режется в самом тихом кадре второй половины.
pub fn windows(spans: &[(f64, f64)], probs: &[f32], max: f64) -> Vec<(f64, f64)> {
    let frame = FRAME as f64 / SR as f64;
    let mut pieces: Vec<(f64, f64)> = Vec::new();
    for &(a, b) in spans {
        let mut a = a;
        while b - a > max {
            let lo = ((a + max / 2.0) / frame) as usize;
            let hi = (((a + max) / frame) as usize).min(probs.len());
            let cut = (lo..hi).min_by(|&x, &y| probs[x].total_cmp(&probs[y])).map(|k| k as f64 * frame).unwrap_or(a + max);
            pieces.push((a, cut));
            a = cut;
        }
        pieces.push((a, b));
    }
    let mut out: Vec<(f64, f64)> = Vec::new();
    let mut i = 0;
    while i < pieces.len() {
        let start = pieces[i].0;
        let mut j = i;
        while j + 1 < pieces.len() && pieces[j + 1].1 - start <= max {
            j += 1;
        }
        if j + 1 < pieces.len() && j > i {
            let half = start + max / 2.0;
            let gap = |k: usize| pieces[k + 1].0 - pieces[k].1;
            if let Some(k) = (i..j).filter(|&k| pieces[k].1 >= half).max_by(|&x, &y| gap(x).total_cmp(&gap(y))) {
                if gap(k) > gap(j) {
                    j = k;
                }
            }
        }
        out.push((start, pieces[j].1));
        i = j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(pattern: &[(f32, usize)]) -> Vec<f32> {
        pattern.iter().flat_map(|&(p, n)| std::iter::repeat_n(p, n)).collect()
    }

    #[test]
    fn speech_follows_the_thresholds_and_drops_short_blips() {
        // 1 s silence, 2 s speech, a dip to 0.4 (above the off threshold 0.35) for 0.32 s, 1 s speech, 1 s silence,
        // a 64 ms blip, silence
        let probs = frames(&[(0.0, 31), (0.9, 62), (0.4, 10), (0.9, 31), (0.0, 31), (0.9, 2), (0.0, 31)]);
        let total = probs.len() * FRAME;
        let spans = speech(&probs, total, &VadConfig::default());
        assert_eq!(spans.len(), 1, "{spans:?}");
        let (a, b) = spans[0];
        assert!((a - (31.0 * 0.032 - 0.03)).abs() < 1e-6, "{a}");
        assert!((b - ((31.0 + 62.0 + 10.0 + 31.0) * 0.032 + 0.03)).abs() < 1e-6, "{b}");
    }

    #[test]
    #[ignore = "needs the Silero model and a 16 kHz mono WAV: SPEECH_VAD_MODEL, SPEECH_VAD_IN"]
    fn probabilities_of_a_file_for_comparison_with_the_python_wrapper() {
        let var = |k: &str| PathBuf::from(std::env::var(k).unwrap());
        let (x, _) = crate::wavio::read_mono_f32(&var("SPEECH_VAD_IN")).unwrap();
        let p = probabilities(&var("SPEECH_VAD_MODEL"), &x).unwrap();
        println!("PROBS {}", p.iter().map(|v| format!("{v:.5}")).collect::<Vec<_>>().join(","));
        println!("SPANS {:?}", speech(&p, x.len(), &VadConfig::default()));
    }

    #[test]
    fn windows_end_on_the_longest_pause_and_cut_a_long_span_where_it_is_quietest() {
        let spans = [(0.0, 20.0), (20.2, 50.0), (53.0, 70.0), (70.1, 100.0)];
        let w = windows(&spans, &[], 90.0);
        assert_eq!(w, vec![(0.0, 50.0), (53.0, 100.0)], "the 3 s pause wins over the last short one");
        let mut probs = vec![0.9f32; (200.0 / 0.032) as usize];
        probs[(70.0 / 0.032) as usize] = 0.1;
        let w = windows(&[(0.0, 120.0)], &probs, 90.0);
        assert_eq!(w.len(), 2);
        assert!((w[0].1 - 70.0).abs() < 0.04 && w[1] == (w[0].1, 120.0), "{w:?}");
    }
}
