//! Шумоподавление DPDFNet (`dpdfnet2_48khz_hr`, 48 кГц): порт `dpdfnet.enhance` (пакет `dpdfnet`, Ceva-IP) — STFT как
//! у librosa (`center`, `reflect`), модель по кадру с состоянием, предел подавления в дБ, обратное STFT.

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use ort::session::Session;
use ort::value::TensorRef;
use realfft::num_complex::Complex32;
use realfft::RealFftPlanner;

pub const SR: u32 = 48_000;
/// Сдвиг выхода модели относительно входа в кадрах: предел подавления смешивает с ним сдвинутый исходник.
const NOISY_FRAME_OFFSET: usize = 4;

pub fn model_path(models_root: &Path) -> PathBuf {
    models_root.join("dpdfnet").join("dpdfnet2_48khz_hr.onnx")
}

struct Model {
    session: Session,
    init_state: Vec<f32>,
    win: usize,
}

static MODEL: LazyLock<Mutex<Option<(PathBuf, Model)>>> = LazyLock::new(|| Mutex::new(None));

fn load(path: &Path) -> Result<Model, String> {
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let session = Session::builder().map_err(|e| err(&e))?.with_intra_threads(1).map_err(|e| err(&e))?.commit_from_file(path).map_err(|e| err(&e))?;
    let meta = session.metadata().map_err(|e| err(&e))?;
    let get = |k: &str| meta.custom(k).ok_or_else(|| format!("{}: no {k} in the model metadata", path.display()));
    let size = |k: &str| get(k)?.trim().parse::<usize>().map_err(|e| format!("{}: {k}: {e}", path.display()));
    let floats = |k: &str| -> Result<Vec<f32>, String> { get(k)?.split(',').map(|x| x.trim().parse::<f32>().map_err(|e| format!("{}: {k}: {e}", path.display()))).collect() };
    let (state, erb, spec) = (size("state_size")?, size("erb_norm_state_size")?, size("spec_norm_state_size")?);
    let mut init_state = vec![0f32; state];
    let (erb_init, spec_init) = (floats("erb_norm_init")?, floats("spec_norm_init")?);
    if erb_init.len() != erb || spec_init.len() != spec || erb + spec > state {
        return Err(format!("{}: the normalisation states do not match the metadata", path.display()));
    }
    init_state[..erb].copy_from_slice(&erb_init);
    init_state[erb..erb + spec].copy_from_slice(&spec_init);
    let win = (size("freq_bins")? - 1) * 2;
    drop(meta);
    Ok(Model { session, init_state, win })
}

fn vorbis(n: usize) -> Vec<f32> {
    let h = n as f64 / 2.0;
    (0..n)
        .map(|i| {
            let s = (0.5 * std::f64::consts::PI * (i as f64 + 0.5) / h).sin();
            (0.5 * std::f64::consts::PI * s * s).sin() as f32
        })
        .collect()
}

/// Очистить 48-килогерцовый моно-сигнал. `limit_db` — насколько сильнее исходника можно приглушить шум
/// (`attn_limit_db`); `None` — без предела. `progress` получает долю готового.
pub fn denoise(model: &Path, input: &[f32], limit_db: Option<f32>, progress: &dyn Fn(f32)) -> Result<Vec<f32>, String> {
    let mut slot = MODEL.lock().map_err(|_| "dpdfnet lock".to_string())?;
    if slot.as_ref().is_none_or(|(p, _)| p != model) {
        *slot = Some((model.to_path_buf(), load(model)?));
    }
    let (_, m) = slot.as_mut().expect("dpdfnet model");
    let (win, hop) = (m.win, m.win / 2);
    let bins = win / 2 + 1;
    let window = vorbis(win);
    let mut x: Vec<f32> = input.to_vec();
    x.resize(input.len() + win, 0.0);
    let pad = win / 2;
    let n = x.len();
    let mut padded = Vec::with_capacity(n + 2 * pad);
    padded.extend((1..=pad).rev().map(|i| x[i.min(n - 1)]));
    padded.extend_from_slice(&x);
    padded.extend((0..pad).map(|i| x[(n - 2).saturating_sub(i)]));
    let frames = 1 + (padded.len() - win) / hop;
    let mut planner = RealFftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(win);
    let inv = planner.plan_fft_inverse(win);
    let mut buf = fwd.make_input_vec();
    let mut spec = fwd.make_output_vec();
    let mut noisy: Vec<Vec<Complex32>> = Vec::with_capacity(frames);
    let mut clean: Vec<Vec<Complex32>> = Vec::with_capacity(frames);
    let mut state = m.init_state.clone();
    let mut frame_ri = vec![0f32; bins * 2];
    for t in 0..frames {
        for (k, b) in buf.iter_mut().enumerate() {
            *b = padded[t * hop + k] * window[k];
        }
        fwd.process(&mut buf, &mut spec).map_err(|e| format!("stft: {e}"))?;
        for (k, c) in spec.iter().enumerate() {
            frame_ri[2 * k] = c.re;
            frame_ri[2 * k + 1] = c.im;
        }
        noisy.push(spec.clone());
        let out = m
            .session
            .run(ort::inputs![
                "spec" => TensorRef::from_array_view((vec![1i64, 1, bins as i64, 2], frame_ri.as_slice())).map_err(|e| e.to_string())?,
                "state_in" => TensorRef::from_array_view((vec![state.len() as i64], state.as_slice())).map_err(|e| e.to_string())?,
            ])
            .map_err(|e| format!("dpdfnet: {e}"))?;
        let (_, e) = out["spec_e"].try_extract_tensor::<f32>().map_err(|e| format!("dpdfnet: {e}"))?;
        clean.push((0..bins).map(|k| Complex32::new(e[2 * k], e[2 * k + 1])).collect());
        let (_, s) = out["state_out"].try_extract_tensor::<f32>().map_err(|e| format!("dpdfnet: {e}"))?;
        let len = state.len();
        state.copy_from_slice(&s[..len]);
        if t % 500 == 0 {
            progress(t as f32 / frames as f32);
        }
    }
    if let Some(db) = limit_db {
        let alpha = 10f32.powf(-db / 20.0);
        for t in 0..frames {
            for k in 0..bins {
                let reference = if t >= NOISY_FRAME_OFFSET { noisy[t - NOISY_FRAME_OFFSET][k] } else { Complex32::new(0.0, 0.0) };
                clean[t][k] = reference * alpha + clean[t][k] * (1.0 - alpha);
            }
        }
    }
    let total = padded.len();
    let mut out = vec![0f32; total];
    let mut norm = vec![0f32; total];
    let mut time = inv.make_output_vec();
    for (t, frame) in clean.iter_mut().enumerate() {
        frame[0].im = 0.0;
        frame[bins - 1].im = 0.0;
        inv.process(frame, &mut time).map_err(|e| format!("istft: {e}"))?;
        for k in 0..win {
            out[t * hop + k] += time[k] / win as f32 * window[k];
            norm[t * hop + k] += window[k] * window[k];
        }
    }
    let wave: Vec<f32> = (pad..total - pad).map(|i| if norm[i] > f32::MIN_POSITIVE { out[i] / norm[i] } else { out[i] }).collect();
    let wave = &wave[..((frames - 1) * hop).min(wave.len())];
    let mut result: Vec<f32> = wave.get(2 * win..).unwrap_or(&[]).to_vec();
    result.resize(input.len(), 0.0);
    progress(1.0);
    Ok(result)
}

/// Голос или запись без шума: файл в 48 кГц моно через ffmpeg, DPDFNet, запись WAV рядом в `dst`.
pub fn denoise_file(models_root: &Path, src: &Path, dst: &Path, limit_db: Option<f32>, progress: &dyn Fn(f32)) -> Result<(), String> {
    let model = model_path(models_root);
    if !model.is_file() {
        return Err(t!("denoise-missing"));
    }
    let tmp = tempfile::Builder::new().prefix("denoise-").tempdir().map_err(|e| e.to_string())?;
    let wav = tmp.path().join("in48.wav");
    crate::media::extract_audio(src, &wav, SR, 1)?;
    let (samples, _) = crate::wavio::read_mono_f32(&wav)?;
    let clean = denoise(&model, &samples, limit_db, progress)?;
    crate::wavio::write_mono_f32(dst, &clean, SR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs the DPDFNet model and a 48 kHz mono WAV: SPEECH_DENOISE_MODEL, SPEECH_DENOISE_IN, SPEECH_DENOISE_OUT"]
    fn denoise_a_file_for_comparison_with_the_python_package() {
        let var = |k: &str| PathBuf::from(std::env::var(k).unwrap());
        let (x, sr) = crate::wavio::read_mono_f32(&var("SPEECH_DENOISE_IN")).unwrap();
        assert_eq!(sr, SR);
        let limit = std::env::var("SPEECH_DENOISE_DB").ok().map(|v| v.parse::<f32>().unwrap());
        let y = denoise(&var("SPEECH_DENOISE_MODEL"), &x, limit, &|_| {}).unwrap();
        crate::wavio::write_mono_f32(&var("SPEECH_DENOISE_OUT"), &y, SR).unwrap();
    }
}
