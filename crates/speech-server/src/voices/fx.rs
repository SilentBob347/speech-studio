//! Voice effects carried over from Game Dub's mixers (`crates/loc-pipeline`): a radio (its compressor and the
//! 300–3400 Hz band, `cut-mix`), a phone (the same telephone band without the compressor) and a loudspeaker (the band
//! with Game Dub's echo, `audio::add_echo`). They are post-processing: the take stays as the model made it, and the
//! effect is applied where it sounds — export and listening.

use realfft::RealFftPlanner;

/// Every effect, in the order the window lists them.
pub const KINDS: [&str; 3] = ["radio", "phone", "loudspeaker"];

const COMPRESSOR: &str = "acompressor=threshold=-20dB:ratio=4:attack=5:release=100";
const BAND: &str = "highpass=f=300,lowpass=f=3400";
/// A loudspeaker's tail: Game Dub measured it on the original line; with no original, a hall's 0.8 s at −25 dB.
const LOUDSPEAKER_RT60: f64 = 0.8;
const LOUDSPEAKER_ECHO_DB: f64 = -25.0;

/// The samples with the effect `kind`; an unknown kind is refused.
pub fn apply(samples: &[f32], sr: u32, kind: &str) -> Result<Vec<f32>, String> {
    match kind {
        "radio" => crate::audio::media::filter_samples(samples, sr, &format!("{COMPRESSOR},{BAND}")),
        "phone" => crate::audio::media::filter_samples(samples, sr, BAND),
        "loudspeaker" => {
            let banded = crate::audio::media::filter_samples(samples, sr, BAND)?;
            add_echo(&banded, LOUDSPEAKER_RT60, LOUDSPEAKER_ECHO_DB, sr).ok_or_else(|| "the loudspeaker echo found no speech to follow".to_string())
        }
        other => Err(format!("no voice effect {other}: {}", KINDS.join(", "))),
    }
}

/// Convolution by FFT, full length `x.len() + h.len() - 1`.
fn convolve(x: &[f32], h: &[f64]) -> Option<Vec<f64>> {
    use realfft::num_complex::Complex;
    let full = x.len() + h.len() - 1;
    let len = full.next_power_of_two();
    let mut planner = RealFftPlanner::<f64>::new();
    let (fft, ifft) = (planner.plan_fft_forward(len), planner.plan_fft_inverse(len));
    let mut xb: Vec<f64> = x.iter().map(|v| *v as f64).collect();
    xb.resize(len, 0.0);
    let mut hb = h.to_vec();
    hb.resize(len, 0.0);
    let (mut xs, mut hs) = (fft.make_output_vec(), fft.make_output_vec());
    fft.process(&mut xb, &mut xs).ok()?;
    fft.process(&mut hb, &mut hs).ok()?;
    let mut ys: Vec<Complex<f64>> = xs.iter().zip(&hs).map(|(a, b)| a * b).collect();
    let last = ys.len() - 1;
    ys[0].im = 0.0;
    ys[last].im = 0.0;
    let mut y = ifft.make_output_vec();
    ifft.process(&mut ys, &mut y).ok()?;
    y.truncate(full);
    Some(y.into_iter().map(|v| v / len as f64).collect())
}

fn envelope(x: &[f32], sr: u32) -> Vec<f64> {
    let h = (sr as usize / 100).max(1);
    x.chunks(h).map(|c| 10.0 * (c.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / c.len() as f64 + 1e-12).log10()).collect()
}

/// The echo after the speech (10 ms envelope frames): RT60 — three times the time to fall 20 dB from the last speech
/// frame (by the slope to the minimum when the tail is shorter), the echo level — 0.5 s of tail against the speech, dB.
fn decay(x: &[f32], sr: u32) -> Option<(f64, f64)> {
    let env = envelope(x, sr);
    let peak = env.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let last = env.iter().rposition(|e| *e >= peak - 20.0)?;
    let tail = env.get(last..)?;
    if tail.len() < 6 {
        return None;
    }
    let t20 = tail.iter().position(|e| *e < env[last] - 20.0).map(|i| i as f64 * 0.01);
    let rt60 = match t20 {
        Some(t) => 3.0 * t,
        None => {
            let (i, m) = tail.iter().enumerate().fold((0, f64::INFINITY), |a, (i, e)| if *e < a.1 { (i, *e) } else { a });
            let drop = env[last] - m;
            if i == 0 || drop < 3.0 {
                return None;
            }
            60.0 * (i as f64 * 0.01) / drop
        }
    };
    let speech: f64 = env[..=last].iter().filter(|e| **e >= peak - 30.0).map(|e| 10f64.powf(e / 10.0)).sum();
    let echo: f64 = tail.iter().skip(1).take(50).map(|e| 10f64.powf(e / 10.0)).sum();
    Some((rt60.min(4.0), 10.0 * (echo / speech.max(1e-12) + 1e-12).log10()))
}

/// Game Dub's echo: a convolution with a synthetic tail (noise decaying exponentially, 10 ms pre-delay) whose length
/// and level are searched so that `decay` of the result gives `rt60` and `echo_db`. The dry signal is unchanged, the
/// tail lengthens it.
fn add_echo(x: &[f32], rt60: f64, echo_db: f64, sr: u32) -> Option<Vec<f32>> {
    let pre = sr as usize / 100;
    let err = |(rt, db): (f64, f64)| ((rt - rt60) / rt60).abs() + (db - echo_db).abs() / 10.0;
    let mut best: Option<(f64, Vec<f32>)> = None;
    for k in [1.0, 0.7, 1.4, 0.5, 2.0, 2.8] {
        let n = ((rt60 * k * sr as f64) as usize).max(1);
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut ir = vec![0f64; pre + n];
        for (i, v) in ir.iter_mut().enumerate().skip(pre) {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let u = (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0;
            *v = u * (-6.91 * (i - pre) as f64 / n as f64).exp();
        }
        let e: f64 = ir.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-12);
        ir.iter_mut().for_each(|v| *v /= e);
        let wet = convolve(x, &ir)?;
        for step in 0..=25 {
            let gain = 10f64.powf((-40.0 + 2.0 * step as f64) / 20.0);
            let out = (0..wet.len()).map(|i| (x.get(i).copied().unwrap_or(0.0) as f64 + gain * wet[i]) as f32).collect::<Vec<f32>>();
            if let Some(m) = decay(&out, sr) {
                let e = err(m);
                if best.as_ref().is_none_or(|b| e < b.0) {
                    best = Some((e, out));
                }
            }
        }
        if best.as_ref().is_some_and(|b| b.0 < 0.1) {
            break;
        }
    }
    best.map(|b| b.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_echo_lengthens_the_line_with_a_tail_of_the_asked_decay() {
        let sr = 24_000;
        let mut x: Vec<f32> = (0..sr as usize).map(|i| 0.5 * (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / sr as f32).sin()).collect();
        x.extend(vec![0.0f32; sr as usize]);
        let y = add_echo(&x, LOUDSPEAKER_RT60, LOUDSPEAKER_ECHO_DB, sr).unwrap();
        assert!(y.len() > x.len());
        let (rt, db) = decay(&y, sr).unwrap();
        assert!((rt - LOUDSPEAKER_RT60).abs() / LOUDSPEAKER_RT60 < 0.35 && (db - LOUDSPEAKER_ECHO_DB).abs() < 4.0, "{rt} {db}");
        assert!(apply(&x, sr, "echo").is_err());
    }
}
