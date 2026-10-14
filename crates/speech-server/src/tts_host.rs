//! Higgs Audio на весь процесс: один загруженный экземпляр (модель, квант, устройство), загрузка при первом синтезе,
//! выгрузка по команде (перед другой тяжёлой моделью). Синтез — в отдельном потоке под сторожем: отмена задачи и
//! таймаут доходят до движка, зависший вызов DLL называется прямо. У Higgs нет своего потолка длины, поэтому
//! `max_new_tokens` считается по ожидаемой длине фразы; дефектный дубль (разнос, обрыв, тишина, гул) повторяется
//! с другой температурой, потому что сид движок может не учитывать.

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use audiocpp::AudiocppEngine;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::localize::Localize;

const GUARD_GRACE_SECS: u64 = 20;
pub const ENGINE_STUCK: &str = "ENGINE_STUCK";
/// Кодек Higgs выдаёт 25–75 токенов в секунду в зависимости от версии: потолок считается по верхней границе с запасом.
const TOKENS_PER_SEC: f64 = 75.0;
/// Движок принимает сид как небольшое беззнаковое целое.
pub const SEED_RANGE: u64 = 1 << 31;

#[derive(Clone, Debug, PartialEq)]
pub struct HiggsConfig {
    pub dll: PathBuf,
    pub model_root: PathBuf,
    pub quant: String,
    pub backend: String,
    pub threads: i32,
}

impl HiggsConfig {
    fn key(&self) -> String {
        format!("{}|{}|{}|{}", self.dll.display(), self.model_root.display(), self.quant, self.backend)
    }
}

static HOST: Mutex<Option<(String, Arc<AudiocppEngine>)>> = Mutex::new(None);

fn host() -> std::sync::MutexGuard<'static, Option<(String, Arc<AudiocppEngine>)>> {
    HOST.lock().unwrap_or_else(|p| p.into_inner())
}

/// Загруженный движок под эту конфигурацию; другая конфигурация выгружает прежнюю.
pub fn engine(cfg: &HiggsConfig) -> Result<Arc<AudiocppEngine>, String> {
    let mut slot = host();
    if let Some((key, e)) = slot.as_ref() {
        if *key == cfg.key() && e.is_model_loaded() {
            return Ok(e.clone());
        }
    }
    *slot = None;
    if cfg.backend == "cuda" {
        let gpu = crate::hw::gpu_report();
        if !gpu.cuda13_ok {
            let card = gpu.summary().unwrap_or_else(|| "-".to_string());
            return Err(match gpu.reason {
                Some("gpu_old") => t!("render-higgs-gpu-old", card = card, min = gpu.min_compute.clone()),
                Some("driver_old") => t!("render-higgs-driver-old", card = card, min = gpu.min_driver),
                _ => t!("render-higgs-no-gpu", card = card),
            });
        }
    }
    if !cfg.dll.is_file() {
        return Err(t!("tts-engine-missing", path = cfg.dll.display().to_string()));
    }
    let e = Arc::new(AudiocppEngine::load(&cfg.dll).map_err(|e| t!("render-higgs-load-failed", error = e.localize()))?);
    e.load_model(&cfg.model_root, &cfg.backend, 0, cfg.threads, Some(cfg.quant.as_str()))
        .map_err(|e| t!("render-higgs-model-failed", error = e.localize()))?;
    *slot = Some((cfg.key(), e.clone()));
    Ok(e)
}

/// Выгрузить Higgs и освободить видеопамять.
pub fn unload() {
    if let Some((_, e)) = host().take() {
        e.cancel();
    }
}

pub fn loaded() -> bool {
    host().as_ref().is_some_and(|(_, e)| e.is_model_loaded())
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Params {
    pub temperature: f64,
    pub top_k: u32,
    pub top_p: f64,
    /// `fixed` — всегда `seed`; `inc` / `dec` — `seed` плюс / минус номер дубля; `random` — новый на каждый синтез.
    pub seed_mode: String,
    pub seed: u64,
    /// Потолок токенов ответа; 0 — по длине текста.
    pub max_tokens: u32,
}

impl Default for Params {
    fn default() -> Self {
        Params { temperature: 1.0, top_k: 50, top_p: 0.95, seed_mode: "random".into(), seed: 42, max_tokens: 0 }
    }
}

impl Params {
    pub fn checked(&self) -> Result<(), String> {
        if !(0.05..=2.0).contains(&self.temperature) {
            return Err(format!("temperature {} is outside 0.05..2.0", self.temperature));
        }
        if !(1..=500).contains(&self.top_k) {
            return Err(format!("top_k {} is outside 1..500", self.top_k));
        }
        if !(0.05..=1.0).contains(&self.top_p) {
            return Err(format!("top_p {} is outside 0.05..1.0", self.top_p));
        }
        if self.seed >= SEED_RANGE {
            return Err(format!("seed {} is outside 0..{SEED_RANGE}", self.seed));
        }
        if !matches!(self.seed_mode.as_str(), "fixed" | "random" | "inc" | "dec") {
            return Err(format!("seed_mode must be fixed, inc, dec or random, not {}", self.seed_mode));
        }
        if self.max_tokens > crate::engines::MAX_TOKENS_LIMIT {
            return Err(format!("max_tokens {} is above {}", self.max_tokens, crate::engines::MAX_TOKENS_LIMIT));
        }
        Ok(())
    }
}

/// Сколько секунд обычно звучит текст на этом языке (для потолка токенов и сторожа).
pub fn expected_secs(text: &str, lang: &str) -> f64 {
    let chars = text.chars().filter(|c| c.is_alphanumeric()).count() as f64;
    let lang = if lang.is_empty() || lang == "auto" { "ru" } else { lang };
    (chars / dub_core::fit::table_cps(lang)).max(0.6)
}

fn token_cap(expected: f64) -> u32 {
    (((expected * TOKENS_PER_SEC * 1.5).ceil() as u32) + 32).clamp(64, 2048)
}

/// Дефект синтеза по сэмплам — пороги Dub (подобраны на 149 клипах QC-прогона): разнос, обрыв, тишина, гул.
pub fn defect(samples: &[f32], sr: i32, chars: usize) -> Option<&'static str> {
    if sr <= 0 || samples.is_empty() {
        return Some("silence");
    }
    let srn = sr as usize;
    let dur = samples.len() as f64 / srn as f64;
    if chars >= 1 && dur > (chars as f64 * 0.4).max(6.0) {
        return Some("runaway");
    }
    if chars >= 4 && dur < chars as f64 * 0.045 {
        return Some("cutoff");
    }
    let w = srn / 40;
    if w == 0 || samples.len() < w * 4 {
        return None;
    }
    let rms: Vec<f64> = samples.chunks_exact(w).map(|c| (c.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / w as f64).sqrt()).collect();
    let peak = rms.iter().cloned().fold(0.0f64, f64::max);
    if peak < 0.02 {
        return Some("silence");
    }
    let silent = rms.iter().filter(|&&r| r < peak * 0.0316).count() as f64 / rms.len() as f64;
    let trough = rms.iter().cloned().filter(|&r| r > 1e-9).fold(peak, f64::min);
    let range = 20.0 * (peak / trough.max(1e-9)).log10();
    if range < 16.0 && silent < 0.03 {
        return Some("hum");
    }
    None
}

pub fn defect_name(kind: &str) -> String {
    match kind {
        "runaway" => t!("render-defect-runaway"),
        "cutoff" => t!("render-defect-cutoff"),
        "silence" => t!("render-defect-silence"),
        "hum" => t!("render-defect-hum"),
        other => other.to_string(),
    }
}

pub struct Synth {
    pub samples: Vec<f32>,
    pub sr: i32,
    pub seed: u64,
    pub temperature: f64,
    pub attempts: u32,
    pub defects: Vec<&'static str>,
    pub seconds: f64,
}

pub struct Request<'a> {
    pub text: &'a str,
    pub lang: &'a str,
    pub ref_wav: Option<&'a Path>,
    pub ref_text: Option<&'a str>,
    pub params: &'a Params,
    /// Сид случайного режима: вызывающий даёт разный на каждый дубль.
    pub fresh_seed: u64,
}

fn run_guarded(engine: &Arc<AudiocppEngine>, text: &str, ref_wav: Option<&Path>, ref_text: Option<&str>, opts: &str, timeout: Duration) -> Result<(Vec<f32>, i32), String> {
    let (tx, rx) = mpsc::channel();
    let eng = engine.clone();
    let (t, rw, rt, op) = (text.to_string(), ref_wav.map(|p| p.to_string_lossy().into_owned()), ref_text.map(str::to_string), opts.to_string());
    std::thread::spawn(move || {
        let r = match rw {
            Some(rw) => eng.voice_clone(&t, &rw, rt.as_deref(), &op),
            None => eng.tts(&t, &op),
        }
        .map_err(|e| e.localize());
        let _ = tx.send(r);
    });
    let ctl = crate::jobs::current();
    let deadline = Instant::now() + timeout;
    let cancelled = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left.min(Duration::from_millis(200))) {
            Ok(r) => return r,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(t!("render-synth-thread-ended")),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let cancelled = ctl.as_ref().is_some_and(|c| c.is_cancelled());
                if cancelled || left.is_zero() {
                    break cancelled;
                }
            }
        }
    };
    engine.cancel();
    match rx.recv_timeout(Duration::from_secs(GUARD_GRACE_SECS)) {
        Ok(_) if cancelled => Err(crate::jobs::CANCELLED.to_string()),
        Ok(_) => Err(t!("render-synth-timeout", seconds = timeout.as_secs())),
        Err(_) => {
            unload();
            Err(format!("{ENGINE_STUCK}: {}", t!("render-engine-stuck", seconds = timeout.as_secs() + GUARD_GRACE_SECS)))
        }
    }
}

/// Синтез фразы с повтором дефектного дубля: фиксированный сид — одна попытка, случайный — до трёх, каждая
/// с новым сидом и температурой выше на 0,15.
pub fn synth(engine: &Arc<AudiocppEngine>, req: &Request) -> Result<Synth, String> {
    req.params.checked()?;
    let expected = expected_secs(req.text, req.lang);
    let cap = if req.params.max_tokens > 0 { req.params.max_tokens as usize } else { token_cap(expected) as usize };
    let timeout = Duration::from_secs(((expected * 8.0).ceil() as u64).max(45));
    let chars = req.text.chars().filter(|c| c.is_alphanumeric()).count();
    let fixed = req.params.seed_mode != "random";
    let tries = if fixed { 1 } else { 3 };
    let mut defects = Vec::new();
    for attempt in 0..tries {
        let seed = if fixed { req.params.seed } else { req.fresh_seed.wrapping_add(attempt as u64 * 7919) } % SEED_RANGE;
        let temperature = (req.params.temperature + 0.15 * attempt as f64).min(2.0);
        let opts = json!({
            "temperature": temperature,
            "top_k": req.params.top_k,
            "top_p": req.params.top_p,
            "max_new_tokens": cap,
            "ras_win_len": 7,
            "return_audio_in_tokens": true,
            "seed": seed,
        })
        .to_string();
        let started = Instant::now();
        let (samples, sr) = run_guarded(engine, req.text, req.ref_wav, req.ref_text, &opts, timeout)?;
        let seconds = started.elapsed().as_secs_f64();
        match defect(&samples, sr, chars) {
            None => return Ok(Synth { samples, sr, seed, temperature, attempts: attempt + 1, defects, seconds }),
            Some(kind) => defects.push(kind),
        }
    }
    Err(t!("tts-defective", defects = defects.iter().map(|d| defect_name(d)).collect::<Vec<_>>().join(", ")))
}

/// Реф голоса под лимит движка: короткий — как есть (WAV); длинный — лучший кусок по словам распознавания с его
/// текстом, а без таймингов — первые `limit` секунд без текста (клон по одному звуку). Готовые куски кэшируются
/// по размеру и времени файла.
pub fn prepare_ref(voices: &Path, name: &str, limit: f64) -> Result<(PathBuf, Option<String>), String> {
    let lib = crate::voice_library::get(voices, name)?;
    let audio = crate::voice_library::audio_path(voices, name).ok_or_else(|| t!("voice-missing", name = name.to_string()))?;
    let meta = std::fs::metadata(&audio).map_err(|e| format!("{}: {e}", audio.display()))?;
    let stamp = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let cache = voices.join(".work").join("refs");
    std::fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    let tag = blake3::hash(format!("{name}|{}|{stamp}|{limit:.1}", meta.len()).as_bytes()).to_hex()[..16].to_string();
    let wav = cache.join(format!("{tag}.wav"));
    let txt = cache.join(format!("{tag}.txt"));
    if wav.is_file() {
        return Ok((wav, std::fs::read_to_string(&txt).ok().filter(|t| !t.trim().is_empty())));
    }
    let duration = crate::media::duration(&audio)?;
    let (start, end, text) = if duration <= limit + 0.5 {
        (0.0, duration, lib.transcript.clone())
    } else if let Some(words) = crate::voice_library::read_words(voices, name) {
        let tmp = cache.join(format!("{tag}-16k.wav"));
        crate::media::to_16k_mono(&audio, &tmp)?;
        let (samples, sr) = crate::wavio::read_mono_f32(&tmp)?;
        let _ = std::fs::remove_file(&tmp);
        match crate::ref_pick::candidates(&words, &samples, sr, (limit * 0.4).min(5.0), limit, 1).into_iter().next() {
            Some(c) => (c.start, c.end, Some(c.text)),
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
mod tests {
    use super::*;

    fn speech(secs: f64, sr: i32) -> Vec<f32> {
        let n = (secs * sr as f64) as usize;
        (0..n).map(|i| if (i / (sr as usize / 5)) % 2 == 0 { 0.3 * ((i as f32) * 0.05).sin() } else { 0.0 }).collect()
    }

    #[test]
    fn a_runaway_a_cut_off_and_silence_are_defects_and_speech_is_not() {
        let sr = 24_000;
        assert_eq!(defect(&speech(30.0, sr), sr, 10), Some("runaway"));
        assert_eq!(defect(&speech(0.2, sr), sr, 40), Some("cutoff"));
        assert_eq!(defect(&vec![0.001; sr as usize * 2], sr, 20), Some("silence"));
        assert_eq!(defect(&speech(2.0, sr), sr, 25), None);
    }

    #[test]
    fn a_steady_hum_is_a_defect() {
        let sr = 24_000;
        let hum: Vec<f32> = (0..sr as usize * 3).map(|i| 0.2 * ((i as f32) * 0.03).sin()).collect();
        assert_eq!(defect(&hum, sr, 30), Some("hum"));
    }

    #[test]
    fn the_token_cap_follows_the_expected_length() {
        assert_eq!(token_cap(0.6), 100);
        assert_eq!(token_cap(100.0), 2048);
        let short = expected_secs("Да.", "ru");
        let long = expected_secs(&"слово ".repeat(40), "ru");
        assert!(short < 1.0 && long > 15.0);
    }

    #[test]
    fn out_of_range_parameters_are_refused() {
        assert!(Params::default().checked().is_ok());
        assert!(Params { temperature: 5.0, ..Default::default() }.checked().is_err());
        assert!(Params { seed_mode: "sometimes".into(), ..Default::default() }.checked().is_err());
    }
}
