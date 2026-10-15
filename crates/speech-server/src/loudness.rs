//! Громкость речи: интегральная громкость ITU-R BS.1770 (замер Dub) и выравнивание к цели линейным усилением с
//! речевым лимитером пиков вместо компрессии; лимитер чуть снижает громкость, поэтому замер и усиление
//! повторяются, пока до цели не меньше 0,5 LU.

use crate::limiter;

/// Межсэмпловые пики выше выборочных: лимитер держит выборки на полдецибела ниже потолка true-peak.
const TRUE_PEAK_MARGIN_DB: f64 = 0.5;
/// Частота, до которой поднимается сигнал при замере истинного пика.
const TRUE_PEAK_RATE: u32 = 192_000;

/// Интегральная громкость ITU-R BS.1770 (LUFS) моно-сигнала: K-weighting (high-shelf + high-pass
/// биквады как в pyloudnorm) -> блоки 400мс с overlap 75% -> абсолютный гейт -70 + относительный -10.
/// None если блоков не осталось (слишком коротко/тихо).
pub fn integrated_lufs(x: &[f32], sr: u32) -> Option<f64> {
    let hs = biquad_high_shelf(1681.9744509555319, 0.7071752369554196, 4.0, sr as f64);
    let hp = biquad_high_pass(38.13547087613982, 0.5003270373253953, sr as f64);
    let y = apply_biquad(&apply_biquad(x, &hs), &hp);
    let block = (0.4 * sr as f64) as usize;
    let step = (0.1 * sr as f64) as usize;
    if block == 0 || step == 0 || y.len() < block {
        return None;
    }
    let mut zs: Vec<f64> = Vec::new();
    let mut i = 0;
    while i + block <= y.len() {
        let ms: f64 = y[i..i + block].iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>()
            / block as f64;
        zs.push(ms);
        i += step;
    }
    if zs.is_empty() {
        return None;
    }
    let loud = |z: f64| -0.691 + 10.0 * (z.max(1e-12)).log10();
    // абсолютный гейт -70 LUFS
    let abs_gated: Vec<f64> = zs.iter().copied().filter(|&z| loud(z) >= -70.0).collect();
    if abs_gated.is_empty() {
        return None;
    }
    let mean_abs = abs_gated.iter().sum::<f64>() / abs_gated.len() as f64;
    let rel_thr = loud(mean_abs) - 10.0;
    let rel_gated: Vec<f64> = abs_gated.into_iter().filter(|&z| loud(z) >= rel_thr).collect();
    if rel_gated.is_empty() {
        return None;
    }
    let mean_rel = rel_gated.iter().sum::<f64>() / rel_gated.len() as f64;
    Some(loud(mean_rel))
}

/// Биквад-фильтр прямой формы I (b/a нормированы на a0). Возвращает отфильтрованный сигнал.
fn apply_biquad(x: &[f32], c: &[f64; 5]) -> Vec<f32> {
    let [b0, b1, b2, a1, a2] = *c;
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut out = Vec::with_capacity(x.len());
    for &xn in x {
        let xn = xn as f64;
        let yn = b0 * xn + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        x2 = x1;
        x1 = xn;
        y2 = y1;
        y1 = yn;
        out.push(yn as f32);
    }
    out
}

/// High-shelf биквад (pyloudnorm K-weighting stage 1). Коэффы [b0,b1,b2,a1,a2] нормированы на a0.
fn biquad_high_shelf(fc: f64, q: f64, gain_db: f64, sr: f64) -> [f64; 5] {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * fc / sr;
    let (cw, sw) = (w0.cos(), w0.sin());
    let alpha = sw / (2.0 * q);
    let am = a - 1.0;
    let ap = a + 1.0;
    let sa = 2.0 * a.sqrt() * alpha;
    let b0 = a * (ap + am * cw + sa);
    let b1 = -2.0 * a * (am + ap * cw);
    let b2 = a * (ap + am * cw - sa);
    let a0 = ap - am * cw + sa;
    let a1 = 2.0 * (am - ap * cw);
    let a2 = ap - am * cw - sa;
    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

/// High-pass биквад (pyloudnorm K-weighting stage 2, RLB). Коэффы нормированы на a0.
fn biquad_high_pass(fc: f64, q: f64, sr: f64) -> [f64; 5] {
    let w0 = 2.0 * std::f64::consts::PI * fc / sr;
    let (cw, sw) = (w0.cos(), w0.sin());
    let alpha = sw / (2.0 * q);
    let b0 = (1.0 + cw) / 2.0;
    let b1 = -(1.0 + cw);
    let b2 = (1.0 + cw) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cw;
    let a2 = 1.0 - alpha;
    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

/// E2E-верификация капшенов без TTS/аудио: собрать ASS из Project и вжечь блюр+сабы в captioned.mp4.
/// Используется примером verify_captions (реальные кадры порт-vs-эталон). Порт captions.build+burn

/// Результат выравнивания: громкость до и после и сколько раз пришлось ограничить пики.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Levelled {
    pub before: f64,
    pub after: f64,
}

/// Истинный пик в dBTP: сигнал передискретизируется до 192 кГц и выше (как BS.1770 для 48 кГц ×4; звуку 24 кГц нужно
/// ×8) FFT-ресемплером rubato с ограничением полосы, берётся наибольший модуль.
pub fn true_peak_db(x: &[f32], sr: u32) -> Result<f64, String> {
    use audioadapter::Adapter;
    use rubato::Resampler;
    if x.len() < 2 {
        return Ok(x.first().map_or(-120.0, |v| 20.0 * (v.abs() as f64).max(1e-6).log10()));
    }
    let input = vec![x.iter().map(|&s| s as f64).collect::<Vec<_>>()];
    let adapter = audioadapter_buffers::direct::SequentialSliceOfVecs::new(&input, 1, x.len()).map_err(|e| format!("true peak buffer: {e}"))?;
    let factor = (TRUE_PEAK_RATE / sr.max(1)).max(4) as usize;
    let mut up = rubato::Fft::<f64>::new(sr as usize, sr as usize * factor, 1024, 1, rubato::FixedSync::Both).map_err(|e| format!("true peak resampler: {e}"))?;
    let out = up.process_all(&adapter, x.len(), None).map_err(|e| format!("true peak resampling: {e}"))?;
    let peak = (0..out.frames()).filter_map(|i| out.read_sample(0, i)).fold(0.0f64, |m, v| m.max(v.abs()));
    Ok(if peak <= 0.0 { -120.0 } else { 20.0 * peak.log10() })
}

/// Моно, которое играют обе колонки, звучит на 3,01 дБ громче замера одного канала (`ffmpeg-normalize --dual-mono`).
pub const DUAL_MONO_DB: f64 = 3.0103;

/// Привести моно-сигнал к `target` LUFS с истинным пиком не выше `peak_db` dBTP: ограничитель работает по отсчётам,
/// поэтому после него пик меряется по BS.1770, и при выходе за потолок порог опускается на превышение. С `dual_mono`
/// громкость меряется как у моно на двух колонках.
pub fn level(x: &mut [f32], sr: u32, target: f64, peak_db: f64, dual_mono: bool) -> Result<Levelled, String> {
    let integrated_lufs = |x: &[f32], sr: u32| integrated_lufs(x, sr).map(|l| if dual_mono { l + DUAL_MONO_DB } else { l });
    let before = integrated_lufs(x, sr).ok_or_else(|| t!("media-loudnorm-silent"))?;
    let mut limit_db = peak_db - TRUE_PEAK_MARGIN_DB;
    let mut now = before;
    for _ in 0..8 {
        let gain = 10f32.powf(((target - now) / 20.0) as f32);
        x.iter_mut().for_each(|v| *v *= gain);
        limiter::limit_mono(x, sr, 10f64.powf(limit_db / 20.0), &limiter::SPEECH);
        now = integrated_lufs(x, sr).ok_or_else(|| t!("media-loudnorm-silent"))?;
        let over = true_peak_db(x, sr)? - peak_db;
        if over > 0.0 {
            limit_db -= over + 0.05;
            continue;
        }
        if (now - target).abs() < 0.5 {
            break;
        }
    }
    let over = true_peak_db(x, sr)? - peak_db;
    if over > 0.0 {
        let gain = 10f32.powf((-(over + 0.05) / 20.0) as f32);
        x.iter_mut().for_each(|v| *v *= gain);
        now = integrated_lufs(x, sr).ok_or_else(|| t!("media-loudnorm-silent"))?;
    }
    Ok(Levelled { before, after: now })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speechlike(secs: f64, sr: u32, amp: f32) -> Vec<f32> {
        (0..(secs * sr as f64) as usize)
            .map(|i| {
                let t = i as f32 / sr as f32;
                let env = if (t * 3.0) as u32 % 2 == 0 { 1.0 } else { 0.15 };
                amp * env * ((t * 220.0 * std::f32::consts::TAU).sin() * 0.6 + (t * 1330.0 * std::f32::consts::TAU).sin() * 0.3 + if i % 97 == 0 { 0.9 } else { 0.0 })
            })
            .collect()
    }

    #[test]
    fn a_quiet_recording_reaches_the_target_without_passing_the_peak() {
        let sr = 24_000;
        let mut x = speechlike(8.0, sr, 0.05);
        let r = level(&mut x, sr, -14.0, -1.0, false).unwrap();
        assert!(r.before < -25.0, "{r:?}");
        assert!((r.after + 14.0).abs() < 0.6, "{r:?}");
        let tp = true_peak_db(&x, sr).unwrap();
        assert!(tp <= -1.0 + 1e-6, "true peak {tp}");
    }

    #[test]
    fn dual_mono_lands_three_decibels_lower_in_the_channel() {
        let sr = 24_000;
        let mut x = speechlike(8.0, sr, 0.05);
        let r = level(&mut x, sr, -16.0, -1.0, true).unwrap();
        assert!((r.after + 16.0).abs() < 0.6, "{r:?}");
        let channel = integrated_lufs(&x, sr).unwrap();
        assert!((channel - (-16.0 - DUAL_MONO_DB)).abs() < 0.6, "one channel {channel}");
    }

    #[test]
    fn silence_is_refused() {
        let mut x = vec![0.0f32; 48_000];
        assert!(level(&mut x, 24_000, -16.0, -1.0, false).is_err());
    }
}
