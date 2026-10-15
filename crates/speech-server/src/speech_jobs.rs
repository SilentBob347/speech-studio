//! Задачи Speech: синтез блоков и экспорт. Синтез перечитывает проект перед каждым блоком и пишет дубль сразу —
//! правки окна и агента во время озвучки не теряются, а окно видит блоки готовыми по одному. Сырой выход модели —
//! FLAC в `takes/`. Экспорт склеивает выбранные дубли с паузами (отрицательная — перекрытие), ставит громкость
//! пресета в два прохода и кодирует в нужный формат; блоки без звука или устаревшие экспорт не пропускает.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::speech_project::{self as sp, Block, SpeechProject, Take};
use crate::speech_text::{self as stx, Piece};
use crate::tts_host::{self, HiggsConfig};

pub const SR: u32 = 24_000;

#[derive(Clone)]
pub struct SpeechPaths {
    pub repo_root: PathBuf,
    pub workspace: PathBuf,
    pub voices_dir: PathBuf,
    pub models_root: PathBuf,
    pub higgs_dll: PathBuf,
    pub threads: i32,
}

pub fn higgs_config(p: &SpeechPaths) -> HiggsConfig {
    let sel = crate::models::load_selection(&p.models_root);
    let (model_root, quant) = crate::models::resolve_tts(&p.models_root, &sel);
    let backend = if crate::hw::gpu_report().nvidia { "cuda" } else { "cpu" };
    HiggsConfig { dll: p.higgs_dll.clone(), model_root, quant, backend: backend.into(), threads: p.threads }
}

pub fn ref_limit(models_root: &Path) -> f64 {
    crate::models::load_selection(models_root).get("higgs_ref_secs").and_then(Value::as_str).and_then(|v| v.parse::<f64>().ok()).unwrap_or(12.0)
}

/// The voice, when it is one of the project engine's own voices rather than a library voice.
fn preset_of<'a>(project: &SpeechProject, voice: Option<&'a str>) -> Option<&'a str> {
    let voice = voice?;
    crate::engines::get(&project.settings.engine).filter(|e| e.presets.iter().any(|p| p.id == voice)).map(|_| voice)
}

/// Ключи синтеза всех блоков проекта при нынешних голосах и настройках.
pub fn keys(p: &SpeechPaths, project: &SpeechProject) -> BTreeMap<String, String> {
    let engine = engine_tag(p, project);
    let limit = ref_limit(&p.models_root);
    let entries = dictionary(p, project);
    let compiled = crate::prep::compile_all(&entries);
    let mut tags: BTreeMap<Option<String>, String> = BTreeMap::new();
    project
        .blocks()
        .map(|b| {
            let voice = project.voice_of(b);
            let tag = tags
                .entry(voice.clone())
                .or_insert_with(|| match preset_of(project, voice.as_deref()) {
                    Some(preset) => format!("preset:{preset}"),
                    None => sp::voice_tag(&p.voices_dir, voice.as_deref()),
                })
                .clone();
            let prep = crate::prep::fingerprint(&b.text, &prep_opts(p, project, voice.as_deref(), &entries, project.settings.auto_stress), &compiled);
            (b.id.clone(), sp::synth_key(project, b, &tag, &engine, limit, &prep))
        })
        .collect()
}

/// Что за движок и модель озвучивают проект — часть ключа дубля.
fn engine_tag(p: &SpeechPaths, project: &SpeechProject) -> String {
    if project.settings.engine == "openrouter" {
        let (m, v) = crate::cloud_tts::model_and_voice(&p.models_root, &project.settings.cloud_model, &project.settings.cloud_voice).unwrap_or_default();
        format!("openrouter:{m}:{v}")
    } else if project.settings.engine == "google" {
        let g = crate::google_speech::setup(&p.models_root, &project.settings.cloud_model, &project.settings.cloud_voice);
        g.map(|g| format!("google:{}:{}", g.model, g.voice)).unwrap_or_else(|_| "google".into())
    } else if matches!(project.settings.engine.as_str(), "fal" | "runware") {
        let values = serde_json::to_string(&project.settings.cloud_values).unwrap_or_default();
        format!("{}:{}:{}", project.settings.engine, project.settings.cloud_model, &blake3::hash(values.as_bytes()).to_hex()[..10])
    } else if project.settings.engine == crate::models::DRAMA_TTS {
        crate::models::DRAMA_TTS.to_string()
    } else if project.settings.engine == crate::supertonic::ENGINE {
        format!("{}:int8", crate::supertonic::ENGINE)
    } else if crate::qwen_tts::is_engine(&project.settings.engine) {
        format!("{}:{}", project.settings.engine, crate::models::qwen_quant(&p.models_root))
    } else {
        format!("higgs:{}", higgs_config(p).quant)
    }
}

/// Записи словаря для проекта сверху вниз: его собственные, затем глобальные.
pub fn dictionary(p: &SpeechPaths, project: &SpeechProject) -> Vec<crate::pron_dict::Entry> {
    let mut all = project.dictionary.clone();
    all.extend(crate::pron_dict::load_global(&p.repo_root));
    all
}

pub fn prep_opts<'a>(p: &'a SpeechPaths, project: &'a SpeechProject, voice: Option<&'a str>, entries: &'a [crate::pron_dict::Entry], stress: bool) -> crate::prep::Opts<'a> {
    crate::prep::Opts {
        lang: &project.settings.lang,
        voice,
        engine: &project.settings.engine,
        entries,
        normalize: project.settings.normalize,
        stress,
        models_root: &p.models_root,
    }
}

fn takes_dir(p: &SpeechPaths, pid: &str) -> PathBuf {
    p.workspace.join(pid).join("takes")
}

fn seed_for(block: &str, n: u32) -> u64 {
    let h = blake3::hash(format!("{block}|{n}|{}", sp::now()).as_bytes());
    u64::from_le_bytes(h.as_bytes()[..8].try_into().expect("8 bytes"))
}

fn resample_to(samples: Vec<f32>, sr: i32) -> Vec<f32> {
    if sr as u32 == SR || sr <= 0 {
        return samples;
    }
    let ratio = SR as f64 / sr as f64;
    let n = (samples.len() as f64 * ratio) as usize;
    (0..n)
        .map(|i| {
            let x = i as f64 / ratio;
            let a = x.floor() as usize;
            let f = (x - a as f64) as f32;
            let s0 = samples.get(a).copied().unwrap_or(0.0);
            let s1 = samples.get(a + 1).copied().unwrap_or(s0);
            s0 + (s1 - s0) * f
        })
        .collect()
}

/// 10 ms fade-in and fade-out at a piece's edges (Dub's `timeline`): no click where pieces and silences join.
fn fade_edges(samples: &mut [f32], sr: u32) {
    let n = ((sr as usize) / 100).min(samples.len() / 2);
    let len = samples.len();
    for k in 0..n {
        let f = k as f32 / n as f32;
        samples[k] *= f;
        samples[len - 1 - k] *= f;
    }
}

/// A soft breath of 0.18–0.26 s (Dub's `generate_breath_sample`), at `level` RMS of the speech it comes before.
fn breath(sr: u32, seed: usize, level: f32) -> Vec<f32> {
    let n = ((0.18 + (seed % 5) as f64 * 0.02) * sr as f64) as usize;
    let mut state: u32 = (seed as u32).wrapping_add(12345);
    let (mut lp, mut hp) = (0.0f32, 0.0f32);
    (0..n)
        .map(|i| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = ((state >> 9) as f32 / 8388608.0) - 1.0;
            lp += 0.35 * (noise - lp);
            hp += 0.12 * (lp - hp);
            let at = i as f32 / n as f32;
            let env = if at < 0.35 { (at / 0.35).powf(1.5) } else { ((1.0 - at) / 0.65).powf(1.2) };
            (lp - hp) * env * 0.75 * level
        })
        .collect()
}

/// Звук одного блока: фразы синтезом, паузы тишиной. Возвращает сэмплы (SR), что разметки проигнорировано и сводку.
/// Блок облаком: фраза за фразой, паузы и темп — как у Higgs. OpenRouter — своим клиентом, fal и Runware — через
/// каталог (`cloud`), с ручкой задачи на диске до ответа и ценой в журнале расходов.
fn synth_block_cloud(p: &SpeechPaths, pid: &str, project: &SpeechProject, b: &Block, n: u32) -> Result<(Vec<f32>, Value, Take), String> {
    let provider = project.settings.engine.as_str();
    let google = if provider == "google" { Some(crate::google_speech::setup(&p.models_root, &project.settings.cloud_model, &project.settings.cloud_voice)?) } else { None };
    let (model, voice) = if let Some(g) = &google {
        (g.model.clone(), g.voice.clone())
    } else if provider == "openrouter" {
        crate::cloud_tts::model_and_voice(&p.models_root, &project.settings.cloud_model, &project.settings.cloud_voice)?
    } else {
        let model = project.settings.cloud_model.trim().to_string();
        if model.is_empty() {
            return Err(t!("cloud-no-model-chosen", provider = provider.to_string()));
        }
        let voice = project.settings.cloud_values.iter().find(|(k, _)| k.to_lowercase().contains("voice")).and_then(|(_, v)| v.as_str()).unwrap_or_default().to_string();
        (model, voice)
    };
    let entries = dictionary(p, project);
    let stress = project.settings.auto_stress && crate::stress::is_russian(&b.text, &project.settings.lang);
    let prepared = crate::prep::prepare(&b.text, &prep_opts(p, project, Some(voice.as_str()), &entries, stress)).map_err(|e| t!("stress-failed", detail = e))?;
    let (mut out, mut rate, started) = (Vec::new(), 1.0f64, std::time::Instant::now());
    let (mut cost, mut estimate) = (None::<f64>, None::<f64>);
    for (k, piece) in stx::pieces(&prepared.text).into_iter().enumerate() {
        match piece {
            Piece::Pause(ms) => out.extend(std::iter::repeat_n(0.0, (SR as u64 * ms as u64 / 1000) as usize)),
            Piece::Rate(r) => rate = r,
            Piece::Say(text) => {
                let say = stx::visible(&text);
                if say.trim().is_empty() {
                    continue;
                }
                let samples = if let Some(g) = &google {
                    let (samples, est) = crate::google_speech::phrase(&p.repo_root, &p.models_root, &p.workspace.join(pid), pid, g, &say, b.direction.trim())?;
                    estimate = match (estimate, est) { (Some(a), Some(b)) => Some(a + b), (a, b) => a.or(b) };
                    samples
                } else if provider == "openrouter" {
                    crate::cloud::check_limit(&p.repo_root, &p.models_root, None)?;
                    let samples = crate::cloud_tts::synth(&say, &model, &voice, b.direction.trim())?;
                    crate::cloud::record(&p.repo_root, crate::cloud::Spent { at: chrono::Utc::now().timestamp_millis(), day: chrono::Local::now().format("%Y-%m-%d").to_string(), project: pid.to_string(), provider: "openrouter".into(), model: model.clone(), task: "speech".into(), cost_source: "unknown".into(), origin: crate::mcp::current_actor(), ..Default::default() });
                    samples
                } else {
                    let handle = p.workspace.join(pid).join("cloud").join(format!("{}-{n}-{k}.json", b.id));
                    let run = crate::cloud::Run { repo_root: &p.repo_root, models_root: &p.models_root, project: pid, task: vs_providers::Task::Speech, handle_file: &handle };
                    let (samples, done) = crate::cloud::speak(&run, provider, &model, &project.settings.cloud_values, &say)?;
                    cost = match (cost, done.cost) { (Some(a), Some(b)) => Some(a + b), (a, b) => a.or(b) };
                    estimate = match (estimate, done.estimate) { (Some(a), Some(b)) => Some(a + b), (a, b) => a.or(b) };
                    samples
                };
                out.extend(if (rate - 1.0).abs() > 1e-3 { at_rate(&samples, rate)? } else { samples });
            }
        }
    }
    if out.is_empty() {
        return Err(t!("speech-block-silent"));
    }
    let fitted = fit_to_cue(project, b, out)?;
    let take = Take {
        n,
        created: sp::now(),
        source: "cloud".into(),
        synth_secs: started.elapsed().as_secs_f64(),
        duration: fitted.samples.len() as f64 / SR as f64,
        text: b.text.clone(),
        direction: b.direction.clone(),
        voice: voice.clone(),
        engine: format!("{provider}:{model}"),
        fit_stretch: fitted.stretch,
        fit_lead: fitted.lead,
        fit_over: fitted.over,
        cost,
        estimate,
        ..Default::default()
    };
    Ok((fitted.samples, serde_json::to_value(&prepared.ignored).unwrap_or(Value::Null), take))
}

/// Режим Batch Google: фразы всех блоков задачи, которые будут озвучены, одной пачкой заранее — синтез блоков потом
/// берёт их из кэша. Фразы собираются тем же разбором, что и в `synth_block_cloud`.
fn google_batch(p: &SpeechPaths, pid: &str, ids: &[String], force: bool, progress: &dyn Fn(Value)) -> Result<(), String> {
    let project = sp::load(&p.workspace, pid)?;
    let g = crate::google_speech::setup(&p.models_root, &project.settings.cloud_model, &project.settings.cloud_voice)?;
    if !g.batch {
        return Ok(());
    }
    let entries = dictionary(p, &project);
    let mut keys = keys(p, &project);
    let mut phrases = Vec::new();
    for b in project.blocks().filter(|b| ids.contains(&b.id)) {
        let state = sp::status(b, &keys.remove(&b.id).unwrap_or_default());
        if state == "skipped" || (!force && state == "ready") {
            continue;
        }
        let stress = project.settings.auto_stress && crate::stress::is_russian(&b.text, &project.settings.lang);
        let prepared = crate::prep::prepare(&b.text, &prep_opts(p, &project, Some(g.voice.as_str()), &entries, stress)).map_err(|e| t!("stress-failed", detail = e))?;
        for piece in stx::pieces(&prepared.text) {
            if let Piece::Say(text) = piece {
                let say = stx::visible(&text);
                if !say.trim().is_empty() {
                    phrases.push((say, b.direction.trim().to_string()));
                }
            }
        }
    }
    crate::google_speech::batch(&p.repo_root, &p.models_root, &p.workspace.join(pid), pid, &g, &phrases, progress)
}

fn synth_block(p: &SpeechPaths, pid: &str, project: &SpeechProject, b: &Block, n: u32) -> Result<(Vec<f32>, Value, Take), String> {
    if matches!(project.settings.engine.as_str(), "openrouter" | "google" | "fal" | "runware") {
        return synth_block_cloud(p, pid, project, b, n);
    }
    synth_block_with(p, pid, project, b, n, &dictionary(p, project))
}

/// A block voiced locally with these dictionary rules.
fn synth_block_with(p: &SpeechPaths, pid: &str, project: &SpeechProject, b: &Block, n: u32, entries: &[crate::pron_dict::Entry]) -> Result<(Vec<f32>, Value, Take), String> {
    let drama = project.settings.engine == crate::models::DRAMA_TTS;
    let manifest = crate::engines::get(&project.settings.engine);
    let presets: Vec<&str> = manifest.as_ref().map(|m| m.presets.iter().map(|p| p.id).collect()).unwrap_or_default();
    let voice = project.voice_of(b);
    if drama && voice.is_none() {
        return Err(t!("drama-needs-voice"));
    }
    if project.settings.engine == crate::qwen_tts::ENGINE_STRESS && voice.is_none() {
        return Err(t!("render-qwen-stress-needs-voice"));
    }
    // an engine with its own voices speaks one of them; a library voice there is cloned when the engine clones,
    // otherwise named, not quietly swapped
    let clones_too = manifest.as_ref().is_some_and(|m| m.clone != "none");
    let preset = match (presets.is_empty(), voice.as_deref()) {
        (true, _) => None,
        (false, None) => presets.first().map(|v| v.to_string()),
        (false, Some(v)) if presets.contains(&v) => Some(v.to_string()),
        (false, Some(_)) if clones_too => None,
        (false, Some(v)) => return Err(t!("speech-voice-not-preset", voice = v.to_string(), engine = manifest.as_ref().map_or("", |m| m.name).to_string())),
    };
    let engine = if drama {
        tts_host::drama(&p.models_root)?
    } else if project.settings.engine == crate::supertonic::ENGINE {
        tts_host::supertonic(&p.models_root)?
    } else if crate::qwen_tts::is_engine(&project.settings.engine) {
        tts_host::qwen(&p.models_root, preset.is_none(), project.settings.engine == crate::qwen_tts::ENGINE_STRESS)?
    } else {
        tts_host::engine(&higgs_config(p))?
    };
    let limit = if project.settings.engine == crate::qwen_tts::ENGINE_STRESS { ref_limit(&p.models_root).min(crate::qwen_tts::STRESS_REF_SECS) } else { ref_limit(&p.models_root) };
    let clones = preset.is_none();
    let continues = manifest.as_ref().is_some_and(|m| m.continuation);
    let continued = sp::continuation_source(project, b).filter(|(_, t)| clones && continues && t.duration > 0.5 && t.duration <= limit + 0.5);
    let reference = match (continued, voice.as_deref().filter(|_| clones)) {
        (Some((src, take)), _) => Some(continuation_ref(&takes_dir(p, pid), take, &src.text)?),
        (None, Some(v)) => Some(tts_host::prepare_ref(&p.voices_dir, v, limit)?),
        (None, None) => None,
    };
    let lang = if project.settings.lang == "auto" { "" } else { project.settings.lang.as_str() };
    let mut out: Vec<f32> = Vec::new();
    let mut ignored = stx::Ignored::default();
    let (mut seed, mut temperature, mut secs, mut retried) = (0u64, project.settings.params.temperature, 0.0, Vec::new());
    let fresh = seed_for(&b.id, n);
    let mut params = take_params(&project.settings.params, n);
    if let Some(seed) = REPEAT_SEED.with(|s| s.get()) {
        params.seed_mode = "fixed".into();
        params.seed = seed % tts_host::SEED_RANGE;
    }
    let mut rate = 1.0f64;
    let stress = project.settings.auto_stress && crate::stress::is_russian(&b.text, &project.settings.lang);
    let text = crate::prep::prepare(&b.text, &prep_opts(p, project, voice.as_deref(), entries, stress)).map_err(|e| t!("stress-failed", detail = e))?.text;
    let max_chars = manifest.as_ref().map_or(usize::MAX, |m| m.max_chars.max(1));
    let live = std::sync::Arc::new(crate::live::Phrase::begin(pid, &b.id));
    for (k, piece) in stx::pieces(&text).into_iter().enumerate() {
        match piece {
            Piece::Pause(ms) => {
                live.pause(ms, SR);
                out.extend(std::iter::repeat_n(0.0, (SR as u64 * ms as u64 / 1000) as usize))
            }
            Piece::Rate(r) => rate = r,
            Piece::Say(text) => {
                let retimed = (rate - 1.0).abs() > 1e-3;
                let (say, ig) = stx::for_engine(&text, lang, &project.settings.engine);
                ignored.accent |= ig.accent;
                ignored.phonemes |= ig.phonemes;
                ignored.stress |= ig.stress;
                if say.is_empty() {
                    continue;
                }
                for (j, (phrase, gap)) in stx::phrases(&say, max_chars).iter().enumerate() {
                    if *gap > 0 {
                        live.pause(*gap, SR);
                        out.extend(std::iter::repeat_n(0.0, (SR * gap / 1000) as usize));
                    }
                    let req = tts_host::Request {
                        text: phrase,
                        lang,
                        ref_wav: reference.as_ref().map(|(w, _)| w.as_path()),
                        ref_text: reference.as_ref().and_then(|(_, t)| t.as_deref()),
                        preset: preset.as_deref(),
                        instruct: Some(b.direction.trim()).filter(|d| !d.is_empty() && crate::qwen_tts::is_engine(&project.settings.engine)),
                        params: &params,
                        fresh_seed: fresh.wrapping_add(k as u64).wrapping_add((j as u64) << 32),
                        live: (!retimed).then(|| live.clone()),
                    };
                    let s = tts_host::synth(&engine, &req)?;
                    if (k == 0 && j == 0) || seed == 0 {
                        seed = s.seed;
                        temperature = s.temperature;
                    }
                    secs += s.seconds;
                    retried.extend(s.defects.iter().map(|d| d.to_string()));
                    let mut samples = resample_to(s.samples, s.sr);
                    fade_edges(&mut samples, SR);
                    if retimed {
                        let samples = at_rate(&samples, rate)?;
                        live.chunk(&samples, SR);
                        out.extend(samples);
                    } else {
                        out.extend(samples);
                    }
                }
            }
        }
    }
    ignored.stress = stx::has_stress(&b.text) && !manifest.as_ref().is_some_and(|m| m.reads.stress);
    if out.is_empty() {
        return Err(t!("speech-block-silent"));
    }
    let fitted = fit_to_cue(project, b, out)?;
    let out = fitted.samples;
    let take = Take {
        n,
        seed,
        temperature,
        created: sp::now(),
        source: "synth".into(),
        synth_secs: secs,
        retried,
        duration: out.len() as f64 / SR as f64,
        text: b.text.clone(),
        direction: b.direction.clone(),
        top_k: params.top_k,
        top_p: params.top_p,
        max_tokens: params.max_tokens,
        voice: preset.clone().or_else(|| voice.clone()).unwrap_or_default(),
        engine: engine_tag(p, project),
        fit_stretch: fitted.stretch,
        fit_lead: fitted.lead,
        fit_over: fitted.over,
        first_ms: live.first_ms(),
        ..Default::default()
    };
    live.end(true);
    Ok((out, serde_json::to_value(&ignored).unwrap_or(Value::Null), take))
}

struct Fitted {
    samples: Vec<f32>,
    stretch: f64,
    lead: f64,
    over: bool,
}

/// В проекте по таймкодам — уложить клип в его реплику (механизм подгонки Dub): до начала следующей реплики;
/// не влезает — начать раньше, в тишину перед репликой; ещё не влезает — ускорить до предела; дальше — пометить.
fn fit_to_cue(project: &SpeechProject, b: &Block, samples: Vec<f32>) -> Result<Fitted, String> {
    let (Some(start), Some(end)) = (b.start, b.end) else { return Ok(Fitted { samples, stretch: 1.0, lead: 0.0, over: false }) };
    if !project.settings.timeline {
        return Ok(Fitted { samples, stretch: 1.0, lead: 0.0, over: false });
    }
    let timed: Vec<&Block> = project.blocks().filter(|x| !x.hidden && x.start.is_some()).collect();
    let at = timed.iter().position(|x| x.id == b.id);
    let next_start = at.and_then(|i| timed.get(i + 1)).and_then(|x| x.start).unwrap_or(end.max(start) + 3600.0);
    let prev_end = at.and_then(|i| i.checked_sub(1)).and_then(|i| timed.get(i)).and_then(|x| x.end).unwrap_or(0.0);
    let rules = dub_core::fit::FitRules {
        speech_rate_on: false,
        qc_duration: true,
        max_stretch: project.settings.max_stretch,
        lead_into_silence: project.settings.lead_into_silence,
    };
    let slot = dub_core::fit::slot(start, end, start, next_start, &rules);
    let clip = samples.len() as f64 / SR as f64;
    let lead = dub_core::fit::lead(clip, &slot, (start - prev_end).max(0.0), &rules);
    let target = slot.target + lead;
    let needed = dub_core::fit::needed(clip, target);
    if needed <= 1.0 + 1e-6 {
        return Ok(Fitted { samples, stretch: 1.0, lead, over: false });
    }
    let stretch = needed.min(slot.cap);
    let samples = at_rate(&samples, stretch)?;
    Ok(Fitted { samples, stretch, lead, over: dub_core::fit::over(needed, slot.cap) })
}

/// Реф продолжения: дубль предыдущего блока (24 кГц моно) и его видимый текст.
fn continuation_ref(takes: &Path, take: &sp::Take, text: &str) -> Result<(PathBuf, Option<String>), String> {
    let src = takes.join(&take.file);
    let wav = takes.join(format!(".ref-{}.wav", take.file.trim_end_matches(".flac")));
    if !wav.is_file() {
        crate::media::extract_audio(&src, &wav, SR, 1)?;
    }
    let said = crate::speech_text::visible(text);
    Ok((wav, (!said.trim().is_empty()).then_some(said)))
}

/// Фраза в темпе `rate` (atempo ffmpeg, как подгонка длины в Dub): высота голоса не меняется.
fn at_rate(samples: &[f32], rate: f64) -> Result<Vec<f32>, String> {
    let dir = tempfile::Builder::new().prefix("rate-").tempdir().map_err(|e| e.to_string())?;
    let src = dir.path().join("in.wav");
    let dst = dir.path().join("out.wav");
    crate::wavio::write_mono_f32(&src, samples, SR)?;
    crate::media::time_stretch(&src, &dst, rate)?;
    let (out, sr) = crate::wavio::read_mono_f32(&dst)?;
    Ok(resample_to(out, sr as i32))
}

/// Параметры дубля `n`: при сиде «+1» / «−1» сид сдвигается на номер дубля и дальше фиксирован.
fn take_params(base: &tts_host::Params, n: u32) -> tts_host::Params {
    let mut p = base.clone();
    let range = tts_host::SEED_RANGE;
    let shift = u64::from(n.saturating_sub(1)) % range;
    match p.seed_mode.as_str() {
        "inc" => p.seed = (p.seed + shift) % range,
        "dec" => p.seed = (p.seed + range - shift) % range,
        _ => {}
    }
    p
}

/// A dictionary rule tried by ear: the phrase as written (no dictionaries) and with this one rule, in the voice asked or
/// the project's, by the project's local engine. The audio is kept in the project's `tries/` by what it was made of.
pub fn try_rule(p: &SpeechPaths, pid: &str, phrase: &str, rule: crate::pron_dict::Entry, voice: Option<String>, progress: &dyn Fn(Value)) -> Result<Value, String> {
    let project = sp::load(&p.workspace, pid)?;
    if matches!(project.settings.engine.as_str(), "openrouter" | "google" | "fal" | "runware") {
        return Err(t!("dict-try-local-only"));
    }
    let b = Block { id: "try".into(), voice: voice.filter(|v| !v.trim().is_empty()), text: phrase.trim().to_string(), ..Default::default() };
    if b.text.is_empty() {
        return Err(t!("dict-try-empty"));
    }
    let rule = crate::pron_dict::Entry { enabled: true, ..rule };
    let dir = p.workspace.join(pid).join("tries");
    let voice_of = project.voice_of(&b).unwrap_or_default();
    let mut out = json!({});
    for (name, entries) in [("before", Vec::new()), ("after", vec![rule])] {
        let stress = project.settings.auto_stress && crate::stress::is_russian(&b.text, &project.settings.lang);
        let read = crate::prep::prepare(&b.text, &prep_opts(p, &project, Some(voice_of.as_str()).filter(|v| !v.is_empty()), &entries, stress)).map_err(|e| t!("stress-failed", detail = e))?.text;
        let made_of = format!("{read}|{voice_of}|{}|{}|{}", project.settings.engine, higgs_config(p).quant, serde_json::to_string(&project.settings.params).unwrap_or_default());
        let file = format!("{}.flac", &blake3::hash(made_of.as_bytes()).to_hex()[..20]);
        let path = dir.join(&file);
        if !path.is_file() {
            progress(json!({ "type": "progress", "stage": "speech_synth", "msg": if name == "before" { t!("dict-try-before") } else { t!("dict-try-after") } }));
            let (samples, _, _) = synth_block_with(p, pid, &project, &b, 1, &entries)?;
            write_flac(&samples, &path)?;
        }
        out[name] = json!({ "file": file, "text": read });
    }
    Ok(out)
}

fn write_flac(samples: &[f32], dst: &Path) -> Result<(), String> {
    let dir = dst.parent().expect("a folder");
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav = dir.join(format!(".{}.wav", sp::new_id()));
    crate::wavio::write_mono_f32(&wav, samples, SR)?;
    let made = crate::media::encode_audio(&wav, dst, "flac", false);
    let _ = std::fs::remove_file(&wav);
    made
}

/// Какие блоки озвучивать: `ids` — эти (нет — все), `from` — начиная с этого (Play until end), `force` — и готовые
/// (новый дубль), иначе только `pending`, `stale` и `failed`.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Scope {
    pub ids: Option<Vec<String>>,
    pub from: Option<String>,
    pub force: bool,
    /// Повтор с теми же параметрами: сид дубля, по которому повторяют.
    #[serde(default)]
    pub seed: Option<u64>,
}

/// Запрос озвучки, вставший в очередь, пока проект озвучивается.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Queued {
    pub n: u64,
    pub scope: Scope,
    pub added: u64,
}

/// Очереди озвучки по проектам. `None` — задача проекта уже заканчивается: новые запросы ставятся новой задачей.
pub static QUEUE: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, Option<Vec<Queued>>>>> = std::sync::LazyLock::new(Default::default);

pub fn queue_of(pid: &str) -> Vec<Queued> {
    QUEUE.lock().map(|q| q.get(pid).cloned().flatten().unwrap_or_default()).unwrap_or_default()
}

/// Встать в очередь работающей задачи проекта; `None` — задачи нет или она уже заканчивается.
pub fn try_queue(pid: &str, scope: &Scope) -> Option<u64> {
    let mut q = QUEUE.lock().ok()?;
    let list = q.get_mut(pid)?.as_mut()?;
    let n = list.iter().map(|x| x.n).max().unwrap_or(0) + 1;
    list.push(Queued { n, scope: scope.clone(), added: sp::now() });
    Some(n)
}

pub fn unqueue(pid: &str, n: Option<u64>) -> usize {
    let Ok(mut q) = QUEUE.lock() else { return 0 };
    let Some(Some(list)) = q.get_mut(pid) else { return 0 };
    let before = list.len();
    list.retain(|x| n.is_some_and(|n| x.n != n));
    before - list.len()
}

thread_local! {
    /// Сид повтора «с теми же параметрами» для текущей задачи.
    static REPEAT_SEED: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Озвучить один блок `count` дублями и записать их в проект; номера новых дублей. Ошибка — в блок.
fn voice_block(p: &SpeechPaths, pid: &str, id: &str, count: u32, source: &str, progress: &dyn Fn(Value)) -> Result<Vec<u32>, String> {
    let mut made = Vec::new();
    for _ in 0..count {
        if crate::jobs::current().is_some_and(|c| c.is_cancelled()) {
            return Err(crate::jobs::CANCELLED.to_string());
        }
        let project = sp::load(&p.workspace, pid)?;
        let Some(b) = project.blocks().find(|b| b.id == id).cloned() else { return Ok(made) };
        let key = keys(p, &project).remove(id).unwrap_or_default();
        let n = b.takes.iter().map(|t| t.n).max().unwrap_or(0) + 1;
        let result = synth_block(p, pid, &project, &b, n).and_then(|(samples, ignored, mut take)| {
            let file = format!("{}-{n}.flac", b.id);
            write_flac(&samples, &takes_dir(p, pid).join(&file))?;
            take.file = file;
            take.key = key.clone();
            take.source = source.into();
            Ok((take, ignored))
        });
        let mut fresh = sp::load(&p.workspace, pid)?;
        let Some(block) = fresh.block_mut(id) else { return Ok(made) };
        match result {
            Ok((take, ignored)) => {
                block.takes.push(take);
                if block.pinned.is_none() {
                    block.active = Some(n);
                }
                block.error = None;
                sp::save(&p.workspace, pid, &fresh)?;
                made.push(n);
                progress(json!({ "type": "progress", "stage": "speech_block_done", "block": id, "ignored": ignored }));
            }
            Err(e) => {
                block.error = Some(sp::BlockError { key, text: e.clone() });
                sp::save(&p.workspace, pid, &fresh)?;
                return Err(e);
            }
        }
    }
    Ok(made)
}

/// Распознать новые дубли блоков, выбрать лучший по WER; блоки, где проверку не прошёл ни один, — в ответе.
/// Голос рефа и дубля в одном пространстве WeSpeaker: эмбеддинг 16 кГц моно.
fn voice_embedding(embedder: &mut dub_faces::VoiceEmbedder, audio: &Path) -> Result<Vec<f32>, String> {
    let dir = tempfile::Builder::new().prefix("emb-").tempdir().map_err(|e| e.to_string())?;
    let wav = dir.path().join("16k.wav");
    crate::media::to_16k_mono(audio, &wav)?;
    embedder.embed_wav(&wav).map_err(|e| e.to_string())
}

/// Проверить новые дубли блоков: текст — распознаванием (WER), голос — похожестью на реф; лучший по
/// «прошёл всё → WER → похожесть» становится активным; блоки, где не прошёл ни один, — в ответе.
fn check_takes(p: &SpeechPaths, pid: &str, made: &BTreeMap<String, Vec<u32>>, progress: &dyn Fn(Value)) -> Result<Vec<String>, String> {
    tts_host::make_room();
    let settings = sp::load(&p.workspace, pid)?.settings;
    let asr_on = settings.qc.asr_check || settings.qc.takes > 1;
    let sim_on = settings.qc.similarity;
    crate::ensure_job_components(&p.repo_root, &p.models_root, sim_on, false, &|ev| progress(ev))?;
    let mut embedder = if sim_on {
        Some(dub_faces::VoiceEmbedder::load(&dub_faces::wespeaker_path(&p.models_root)).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let threshold = dub_faces::voice_cos_threshold() as f64;
    let mut refs: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    let mut failed = Vec::new();
    for (i, (id, takes)) in made.iter().enumerate() {
        progress(json!({ "type": "progress", "stage": "speech_qc", "msg": t!("speech-qc-block", n = (i + 1) as i64, total = made.len() as i64), "pct": (i * 100 / made.len().max(1)) as i64 }));
        let project = sp::load(&p.workspace, pid)?;
        let Some(b) = project.blocks().find(|b| &b.id == id).cloned() else { continue };
        let lang = project.settings.lang.clone();
        let strictness = project.settings.qc.strictness.clone();
        let voice = project.voice_of(&b).filter(|v| preset_of(&project, Some(v)).is_none());
        let reference = match (&mut embedder, voice.as_deref()) {
            (Some(emb), Some(v)) => match refs.get(v) {
                Some(e) => Some(e.clone()),
                None => {
                    let audio = crate::voice_library::audio_path(&p.voices_dir, v).ok_or_else(|| t!("voice-missing", name = v.to_string()))?;
                    let e = voice_embedding(emb, &audio)?;
                    refs.insert(v.to_string(), e.clone());
                    Some(e)
                }
            },
            _ => None,
        };
        let mut checks: Vec<(u32, crate::qc::Check)> = Vec::new();
        for n in takes {
            let Some(take) = b.takes.iter().find(|t| t.n == *n) else { continue };
            let file = takes_dir(p, pid).join(&take.file);
            let mut check = if asr_on {
                let heard = crate::asr_util::transcribe(&p.repo_root, &p.models_root, &file, &lang, &b.id)?;
                let entries = dictionary(p, &project);
                let expected = crate::prep::prepare(&b.text, &prep_opts(p, &project, voice.as_deref(), &entries, false)).map_err(|e| t!("stress-failed", detail = e))?.text;
                let heard_said = if crate::prep::block_lang(&b.text, &lang) == "ru" { crate::normalize_ru::normalize(&heard.text).text } else { heard.text.clone() };
                let mut c = crate::qc::check(&expected, &heard_said, &strictness);
                c.heard = heard.text.clone();
                let pause = stx::pieces(&b.text).iter().filter_map(|x| if let Piece::Pause(ms) = x { Some(*ms as f64 / 1000.0) } else { None }).fold(0.0, f64::max);
                c.issues = crate::qc::timing_issues(&heard.words, take.duration, pause);
                if crate::qc::repeated(&crate::qc::normalize(&expected), &crate::qc::normalize(&heard_said)) {
                    c.issues.push("repeat".to_string());
                }
                c.timed = heard.words.iter().map(|w| (w.word.clone(), (w.start * 1000.0).round() / 1000.0, (w.end * 1000.0).round() / 1000.0)).collect();
                c
            } else {
                crate::qc::Check { passed: true, ..Default::default() }
            };
            if let (Some(emb), Some(r)) = (&mut embedder, &reference) {
                let e = voice_embedding(emb, &file)?;
                let cos: f64 = e.iter().zip(r).map(|(a, b)| (*a as f64) * (*b as f64)).sum();
                check.similarity = Some(cos);
                check.similar = Some(cos >= threshold);
            }
            checks.push((*n, check));
        }
        let mut fresh = sp::load(&p.workspace, pid)?;
        let Some(block) = fresh.block_mut(id) else { continue };
        for (n, c) in &checks {
            if let Some(t) = block.takes.iter_mut().find(|t| t.n == *n) {
                t.qc = Some(c.clone());
            }
        }
        let rank = |c: &crate::qc::Check| (!c.ok(), c.wer, -c.similarity.unwrap_or(0.0));
        let best = checks.iter().min_by(|a, b| rank(&a.1).partial_cmp(&rank(&b.1)).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((n, c)) = best {
            if block.pinned.is_none() {
                block.active = Some(*n);
            }
            if !c.ok() {
                failed.push(id.clone());
            }
        }
        sp::save(&p.workspace, pid, &fresh)?;
    }
    Ok(failed)
}

/// Задача озвучки проекта: свой запрос, затем всё, что встало в очередь, пока она шла.
pub fn synth(p: &SpeechPaths, pid: &str, scope: &Scope, progress: &dyn Fn(Value)) -> Result<Value, String> {
    if let Ok(mut q) = QUEUE.lock() {
        q.insert(pid.to_string(), Some(Vec::new()));
    }
    let mut current = scope.clone();
    let mut total = json!({ "done": 0, "failed": [], "skipped": 0, "qc_failed": [] });
    let result = loop {
        match synth_scope(p, pid, &current, progress) {
            Ok(v) => {
                total["done"] = json!(total["done"].as_u64().unwrap_or(0) + v["done"].as_u64().unwrap_or(0));
                total["skipped"] = json!(total["skipped"].as_u64().unwrap_or(0) + v["skipped"].as_u64().unwrap_or(0));
                for k in ["failed", "qc_failed"] {
                    if let (Some(all), Some(more)) = (total[k].as_array().cloned(), v[k].as_array()) {
                        total[k] = json!(all.into_iter().chain(more.iter().cloned()).collect::<Vec<_>>());
                    }
                }
            }
            Err(e) => break Err(e),
        }
        let next = QUEUE.lock().ok().and_then(|mut q| {
            let slot = q.get_mut(pid)?;
            let list = slot.as_mut()?;
            if list.is_empty() {
                *slot = None;
                None
            } else {
                Some(list.remove(0))
            }
        });
        match next {
            Some(item) => current = item.scope,
            None => break Ok(total.clone()),
        }
    };
    if let Ok(mut q) = QUEUE.lock() {
        q.remove(pid);
    }
    result
}

fn synth_scope(p: &SpeechPaths, pid: &str, scope: &Scope, progress: &dyn Fn(Value)) -> Result<Value, String> {
    REPEAT_SEED.with(|s| s.set(scope.seed));
    let project = sp::load(&p.workspace, pid)?;
    let qc = project.settings.qc.clone();
    let random_seed = project.settings.params.seed_mode != "fixed";
    let order: Vec<String> = project.blocks().map(|b| b.id.clone()).collect();
    let start = scope.from.as_ref().and_then(|f| order.iter().position(|id| id == f)).unwrap_or(0);
    let wanted: Vec<String> = order[start..].iter().filter(|id| scope.ids.as_ref().is_none_or(|ids| ids.contains(id))).cloned().collect();
    let engine = crate::engines::get(&project.settings.engine).ok_or_else(|| t!("speech-engine-unknown", engine = project.settings.engine.clone()))?;
    if !crate::engines::supports(&engine, &project.settings.lang) {
        return Err(t!("speech-lang-unsupported", lang = project.settings.lang.to_uppercase(), engine = engine.name.to_string()));
    }
    if project.settings.auto_stress && wanted.iter().any(|id| project.blocks().any(|b| &b.id == id && crate::stress::is_russian(&b.text, &project.settings.lang))) {
        crate::ensure_components(&p.repo_root, &[crate::stress::COMPONENT, "onnxruntime"], &|ev| progress(ev))?;
    }
    if project.settings.engine == "google" {
        google_batch(p, pid, &wanted, scope.force, progress)?;
    }
    let checking = qc.asr_check || qc.takes > 1 || qc.similarity;
    let (mut done, mut failed, mut skipped) = (0usize, Vec::new(), 0usize);
    let mut made: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let total = wanted.len();
    for (i, id) in wanted.iter().enumerate() {
        let project = sp::load(&p.workspace, pid)?;
        let Some(b) = project.blocks().find(|b| &b.id == id).cloned() else { continue };
        let key = keys(p, &project).remove(id).unwrap_or_default();
        let state = sp::status(&b, &key);
        if state == "skipped" || (!scope.force && state == "ready") {
            skipped += 1;
            continue;
        }
        progress(json!({ "type": "progress", "stage": "speech_synth", "msg": t!("speech-synth-block", n = (i + 1) as i64, total = total as i64, text = sp::visible_snippet(&b.text)), "pct": (i * 100 / total.max(1)) as i64 }));
        match voice_block(p, pid, id, qc.takes, if qc.takes > 1 { "multitake" } else { "synth" }, progress) {
            Ok(ns) => {
                done += 1;
                made.insert(id.clone(), ns);
            }
            Err(e) if e.starts_with(tts_host::ENGINE_STUCK) || e == crate::jobs::CANCELLED => return Err(e),
            Err(e) => failed.push(json!({ "block": id, "error": e })),
        }
    }
    let mut qc_failed = Vec::new();
    if checking && !made.is_empty() {
        qc_failed = check_takes(p, pid, &made, progress)?;
        if random_seed && !qc_failed.is_empty() {
            let mut again: BTreeMap<String, Vec<u32>> = BTreeMap::new();
            for id in &qc_failed {
                if let Ok(ns) = voice_block(p, pid, id, 1, "retry", progress) {
                    again.insert(id.clone(), ns);
                }
            }
            if !again.is_empty() {
                qc_failed = check_takes(p, pid, &again, progress)?;
            }
        }
        for id in &qc_failed {
            let mut fresh = sp::load(&p.workspace, pid)?;
            let key = keys(p, &fresh).remove(id).unwrap_or_default();
            if let Some(block) = fresh.block_mut(id) {
                let qc = block.active.and_then(|n| block.takes.iter().find(|t| t.n == n)).and_then(|t| t.qc.clone()).unwrap_or_default();
                let text = if qc.text_checked && !qc.passed {
                    t!("speech-qc-failed", heard = qc.heard.clone())
                } else {
                    t!("speech-qc-unlike", similarity = format!("{:.2}", qc.similarity.unwrap_or(0.0)))
                };
                block.error = Some(sp::BlockError { key, text });
                sp::save(&p.workspace, pid, &fresh)?;
            }
        }
    }
    if done == 0 && !failed.is_empty() {
        return Err(t!("speech-synth-all-failed", error = failed[0]["error"].as_str().unwrap_or("").to_string()));
    }
    Ok(json!({ "done": done, "failed": failed, "skipped": skipped, "qc_failed": qc_failed }))
}

/// Пауза между главами при сборке одним файлом (граница раздела у tts-audiobook-tool — 2–2,5 с).
const CHAPTER_PAUSE_MS: i64 = 2_000;
/// MP3 at a constant 192 kbps: ACX's rate, and plenty for speech.
const MP3_KBPS: u32 = 192;

/// Запас под сжатие: кодеки с потерями поднимают пики между отсчётами, поэтому для них потолок ниже на 1 дБ (AES TD1004).
const LOSSY_HEADROOM_DB: f64 = 1.0;

/// Потолок пиков перед кодированием в `format`.
pub fn peak_for(format: &str, tp: f64) -> f64 {
    if matches!(format, "wav" | "flac") { tp } else { tp - LOSSY_HEADROOM_DB }
}

/// Пресет громкости: интегральная громкость и потолок пиков; ACX — средний уровень −20 (RMS-цель ACX −23…−18), пики −3.
pub fn loudness(preset: &str) -> Option<(f64, f64, f64)> {
    match preset {
        "video" => Some((-14.0, -1.0, 11.0)),
        "podcast" => Some((-16.0, -1.0, 11.0)),
        "acx" => Some((-20.0, -3.0, 11.0)),
        _ => None,
    }
}

/// Что мешает экспорту: блоки без звука, устаревшие, упавшие, со стёртым файлом дубля. Пусто — можно.
pub fn export_check(p: &SpeechPaths, pid: &str, project: &SpeechProject) -> Vec<Value> {
    let keys = keys(p, project);
    project
        .blocks()
        .filter_map(|b| {
            let key = keys.get(&b.id).cloned().unwrap_or_default();
            match sp::status(b, &key) {
                "skipped" => None,
                "ready" => {
                    let on_disk = b.active.and_then(|n| b.takes.iter().find(|t| t.n == n)).is_some_and(|t| takes_dir(p, pid).join(&t.file).is_file());
                    (!on_disk).then(|| json!({ "block": b.id, "state": "missing" }))
                }
                state => Some(json!({ "block": b.id, "state": state })),
            }
        })
        .collect()
}

pub struct ExportOpts {
    pub format: String,
    pub chapter: Option<String>,
    /// Пакет: каждый блок — свой файл с громкостью по пресету.
    pub per_block: bool,
    /// Длинная книга — частями не длиннее стольких минут (по границам глав).
    pub part_minutes: Option<u32>,
}

/// Склеить проект в файл `exports/<имя>.<формат>`.
pub fn export(p: &SpeechPaths, pid: &str, opts: &ExportOpts, progress: &dyn Fn(Value)) -> Result<Value, String> {
    if !crate::media::EXPORT_FORMATS.contains(&opts.format.as_str()) {
        return Err(t!("media-bad-format", format = opts.format.clone()));
    }
    let project = sp::load(&p.workspace, pid)?;
    let problems = export_check(p, pid, &project);
    if !problems.is_empty() {
        return Err(t!("speech-export-not-ready", count = problems.len() as i64));
    }
    let blocks: Vec<&Block> = project
        .chapters
        .iter()
        .filter(|c| opts.chapter.as_ref().is_none_or(|id| &c.id == id))
        .flat_map(|c| c.blocks.iter())
        .filter(|b| !b.hidden && !b.text.trim().is_empty())
        .collect();
    if blocks.is_empty() {
        return Err(t!("speech-export-empty"));
    }
    let dir = p.workspace.join(pid);
    let work = tempfile::Builder::new().prefix("export-").tempdir_in(&dir).map_err(|e| e.to_string())?;
    if opts.per_block {
        return export_clips(p, pid, &project, &blocks, opts, work.path(), progress);
    }
    let mut mix: Vec<f32> = Vec::new();
    let mut cursor: i64 = 0;
    let mut marks = Vec::new();
    let mut prev_speaker: Option<&str> = None;
    let mut cues: Vec<(f64, f64, String, String)> = Vec::new();
    // each block's place in the file and the words recognition timed in its take, for synced text
    let mut timed: Vec<(f64, f64, String, String, Vec<(String, f64, f64)>)> = Vec::new();
    let chapter_of: std::collections::HashMap<&str, usize> = project.chapters.iter().enumerate().flat_map(|(ci, c)| c.blocks.iter().map(move |b| (b.id.as_str(), ci))).collect();
    let mut chapter_starts: Vec<(usize, f64)> = Vec::new();
    let mut prev_end: Option<usize> = None;
    for (i, b) in blocks.iter().enumerate() {
        progress(json!({ "type": "progress", "stage": "speech_export", "msg": t!("speech-export-block", n = (i + 1) as i64, total = blocks.len() as i64), "pct": (i * 90 / blocks.len()) as i64 }));
        let take = b.active.and_then(|n| b.takes.iter().find(|t| t.n == n)).ok_or_else(|| t!("speech-export-not-ready", count = 1))?;
        let src = takes_dir(p, pid).join(&take.file);
        let wav = work.path().join(format!("{i}.wav"));
        crate::media::extract_audio(&src, &wav, SR, 1)?;
        let (samples, _) = crate::wavio::read_mono_f32(&wav)?;
        if let Some(prev) = prev_speaker {
            let ms = b.pause_ms.unwrap_or(if prev == b.speaker { project.settings.pause_same_ms } else { project.settings.pause_switch_ms });
            let new_chapter = chapter_starts.last().is_some_and(|(last, _)| chapter_of.get(b.id.as_str()) != Some(last));
            let ms = if new_chapter { ms.max(CHAPTER_PAUSE_MS) } else { ms };
            cursor = (cursor + SR as i64 * ms / 1000).max(0);
        }
        let at = match (project.settings.timeline, b.start) {
            (true, Some(start)) => ((start - take.fit_lead).max(0.0) * SR as f64) as usize,
            _ => cursor as usize,
        };
        marks.push(json!({ "block": b.id, "start": at as f64 / SR as f64, "end": (at + samples.len()) as f64 / SR as f64 }));
        if let Some(ci) = chapter_of.get(b.id.as_str()) {
            if chapter_starts.last().is_none_or(|(last, _)| last != ci) {
                chapter_starts.push((*ci, at as f64 / SR as f64));
            }
        }
        cues.push((at as f64 / SR as f64, (at + samples.len()) as f64 / SR as f64, b.speaker.clone(), crate::speech_text::visible(&b.text)));
        let words = take.qc.as_ref().map(|q| q.timed.iter().map(|(w, a, z)| (w.clone(), at as f64 / SR as f64 + a, at as f64 / SR as f64 + z)).collect()).unwrap_or_default();
        timed.push((at as f64 / SR as f64, (at + samples.len()) as f64 / SR as f64, b.speaker.clone(), crate::speech_text::visible(&b.text), words));
        if mix.len() < at + samples.len() {
            mix.resize(at + samples.len(), 0.0);
        }
        if let Some(end) = prev_end.filter(|_| project.settings.breaths) {
            let gap = at.saturating_sub(end) as f64 / SR as f64;
            if (0.40..=1.80).contains(&gap) {
                let level = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt();
                let b = breath(SR, i, level);
                let margin = (0.04 * SR as f64) as usize;
                let b_at = at.saturating_sub(b.len() + margin).max(end + margin);
                for (k, s) in b.iter().enumerate().take(at.saturating_sub(b_at)) {
                    mix[b_at + k] += *s;
                }
            }
        }
        for (k, s) in samples.iter().enumerate() {
            mix[at + k] += *s;
        }
        prev_end = Some(at + samples.len());
        cursor = cursor.max((at + samples.len()) as i64);
        if let Some(own) = b.pause_ms.filter(|_| i + 1 == blocks.len()) {
            mix.resize(mix.len() + (SR as i64 * own.max(0) / 1000) as usize, 0.0);
        }
        prev_speaker = Some(b.speaker.as_str());
    }
    progress(json!({ "type": "progress", "stage": "speech_export", "msg": t!("speech-export-loudness"), "pct": 92 }));
    let levelled = match loudness(&project.settings.loudness) {
        Some((i, tp, _)) => Some(crate::loudness::level(&mut mix, SR, i, peak_for(&opts.format, tp))?),
        None => None,
    };
    let acx = project.settings.loudness == "acx";
    if acx {
        let head = 0.8;
        crate::acx::room_tone(&mut mix, SR, head, 2.0);
        chapter_starts.iter_mut().for_each(|c| c.1 += head);
        cues.iter_mut().for_each(|c| {
            c.0 += head;
            c.1 += head;
        });
        timed.iter_mut().for_each(|t| {
            t.0 += head;
            t.1 += head;
            t.4.iter_mut().for_each(|w| {
                w.1 += head;
                w.2 += head;
            });
        });
    } else if project.settings.room_tone {
        crate::acx::room_tone(&mut mix, SR, 0.0, 0.0);
    }
    let acx_report = if acx { Some(crate::acx::report(&mix, SR)?) } else { None };
    let exports = dir.join("exports");
    std::fs::create_dir_all(&exports).map_err(|e| format!("{}: {e}", exports.display()))?;
    let base = file_stem(&project.name, opts.chapter.as_deref().and_then(|id| project.chapters.iter().find(|c| c.id == id)).map(|c| c.title.as_str()));
    let total = mix.len() as f64 / SR as f64;
    let chapters: Vec<(f64, f64, String)> = chapter_starts
        .iter()
        .enumerate()
        .map(|(k, (ci, a))| {
            let z = chapter_starts.get(k + 1).map(|n| n.1).unwrap_or(total);
            let title = project.chapters[*ci].title.trim();
            (*a, z, if title.is_empty() { t!("speech-chapter-n", n = (k + 1) as i64) } else { title.to_string() })
        })
        .collect();
    let cuts = match opts.part_minutes.filter(|m| *m > 0 && total > *m as f64 * 60.0) {
        Some(m) => part_cuts(total, m as f64 * 60.0, &chapters.iter().map(|c| c.0).collect::<Vec<_>>(), &cues.iter().map(|c| c.0).collect::<Vec<_>>()),
        None => Vec::new(),
    };
    let bounds: Vec<(f64, f64)> = std::iter::once(0.0).chain(cuts.iter().copied()).zip(cuts.iter().copied().chain(std::iter::once(total))).collect();
    let voice = project.settings.voice.clone().unwrap_or_default();
    let mut outputs = Vec::new();
    let mut tag_problems: Vec<String> = Vec::new();
    for (k, (a, z)) in bounds.iter().enumerate() {
        let name = if bounds.len() > 1 { format!("{base} ({})", t!("speech-part-n", n = (k + 1) as i64)) } else { base.clone() };
        let within = |s: f64| s >= *a - 1e-6 && s < *z - 1e-6;
        let part_chapters: Vec<(f64, f64, String)> = chapters.iter().filter(|c| within(c.0)).map(|(s, e, t)| (s - a, e.min(*z) - a, t.clone())).collect();
        let part_cues: Vec<(f64, f64, String, String)> = cues.iter().filter(|c| within(c.0)).map(|(s, e, w, t)| (s - a, e - a, w.clone(), t.clone())).collect();
        let part_timed: Vec<(f64, f64, String, String, Vec<(String, f64, f64)>)> = timed
            .iter()
            .filter(|t| within(t.0))
            .map(|(s, e, w, t, words)| (s - a, e - a, w.clone(), t.clone(), words.iter().map(|(x, ws, we)| (x.clone(), ws - a, we - a)).collect()))
            .collect();
        let from = (a * SR as f64) as usize;
        let to = ((z * SR as f64) as usize).min(mix.len());
        let joined = work.path().join(format!("part-{k}.wav"));
        crate::wavio::write_mono_f32(&joined, &mix[from..to], SR)?;
        let file = format!("{name}.{}", opts.format);
        let target = exports.join(&file);
        let meta = crate::media::AudioMeta {
            title: if bounds.len() > 1 { format!("{} ({})", project.name, t!("speech-part-n", n = (k + 1) as i64)) } else { project.name.clone() },
            artist: project.book.as_ref().map(|b| b.author.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| voice.clone()),
            album: project.book.as_ref().map(|b| b.title.clone()).unwrap_or_default(),
            comment: format!("Speech Studio · {}", crate::engines::get(&project.settings.engine).map(|e| e.name).unwrap_or("")),
            genre: if project.book.is_some() { "Audiobook".into() } else { String::new() },
            chapters: if part_chapters.len() > 1 { part_chapters.clone() } else { Vec::new() },
            cover: project.book.as_ref().and_then(|b| b.cover.as_ref()).map(|c| dir.join(c)),
            lyrics: lrc(&part_timed),
        };
        progress(json!({ "type": "progress", "stage": "speech_export", "msg": t!("speech-export-encoding", n = (k + 1) as i64, total = bounds.len() as i64), "pct": 94 + (k * 5 / bounds.len()) as i64 }));
        match opts.format.as_str() {
            // FLAC and MP3 as the music studios write them: libFLAC 24-bit and LAME from audio-post, tags by lofty
            "flac" | "mp3" => {
                let part = mix[from..to].to_vec();
                let stereo = audio_post::Stereo::new(part.clone(), part, SR);
                let bytes = if opts.format == "mp3" {
                    let rate = if acx { 44_100 } else { 48_000 };
                    audio_post::encode::mp3(&stereo.resampled(rate).map_err(|e| e.to_string())?, MP3_KBPS)
                } else {
                    audio_post::encode::flac(&stereo)
                }
                .map_err(|e| e.to_string())?;
                std::fs::write(&target, bytes).map_err(|e| format!("{}: {e}", target.display()))?;
            }
            _ => crate::media::encode_audio_meta(&joined, &target, &opts.format, acx, &meta)?,
        }
        if crate::tagging::taggable(&target) {
            let cover = meta.cover.as_ref().and_then(|c| std::fs::read(c).ok().map(|bytes| (if c.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) { "image/png" } else { "image/jpeg" }.to_string(), bytes)));
            let tags = crate::tagging::TrackTags {
                title: meta.title.clone(),
                album: if meta.album.is_empty() { "Speech Studio".into() } else { meta.album.clone() },
                // as the studios name the engine the performer when nobody else is
                artist: if meta.artist.is_empty() { crate::engines::get(&project.settings.engine).map(|e| e.name.to_string()).unwrap_or_default() } else { meta.artist.clone() },
                genre: Some(meta.genre.clone()).filter(|g| !g.is_empty()),
                lyrics: Some(meta.lyrics.clone()).filter(|l| !l.is_empty()),
                bpm: None,
                cover,
            };
            // as in the studios a tag never loses the file: the problem is logged and named in the answer
            if let Err(e) = crate::tagging::write_tags(&target, &tags).and_then(|()| crate::tagging::write_chapters(&target, &part_chapters)) {
                tracing::error!("[ERROR] could not tag {}: {e:#}", target.display());
                tag_problems.push(format!("{file}: {e:#}"));
            }
        }
        let chapter_list = (part_chapters.len() > 1).then(|| format!("{name}.chapters.txt"));
        if let Some(list) = &chapter_list {
            let text: String = part_chapters.iter().map(|(s, _, t)| format!("{}\t{t}\n", hms(*s))).collect();
            std::fs::write(exports.join(list), text).map_err(|e| format!("{list}: {e}"))?;
        }
        let subtitles = format!("{name}.srt");
        std::fs::write(exports.join(&subtitles), srt(&part_cues)).map_err(|e| format!("{subtitles}: {e}"))?;
        let timings = format!("{name}.timings.json");
        let body = json!({ "file": file, "blocks": part_timed.iter().map(|(s, e, who, text, words)| json!({ "start": s, "end": e, "speaker": who, "text": text, "words": words.iter().map(|(w, ws, we)| json!({ "word": w, "start": ws, "end": we })).collect::<Vec<_>>() })).collect::<Vec<_>>() });
        std::fs::write(exports.join(&timings), serde_json::to_vec_pretty(&body).map_err(|e| e.to_string())?).map_err(|e| format!("{timings}: {e}"))?;
        outputs.push(json!({ "file": file, "subtitles": subtitles, "timings": timings, "chapters": chapter_list, "path": target.to_string_lossy(), "duration": z - a }));
    }
    let first = outputs.first().cloned().unwrap_or(Value::Null);
    Ok(json!({ "file": first["file"], "subtitles": first["subtitles"], "timings": first["timings"], "chapters": first["chapters"], "path": first["path"], "parts": if outputs.len() > 1 { Value::Array(outputs) } else { Value::Null }, "tag_problems": tag_problems, "acx": acx_report, "duration": total, "marks": marks, "lufs_before": levelled.map(|l| l.before), "lufs": levelled.map(|l| l.after) }))
}

/// Where a long export is cut into parts of at most `limit` seconds: at the last chapter start that keeps a part under
/// it, else at the last block start; a single block longer than the limit stays whole.
fn part_cuts(total: f64, limit: f64, chapters: &[f64], blocks: &[f64]) -> Vec<f64> {
    let mut cuts = Vec::new();
    let mut start = 0.0;
    while total - start > limit {
        let inside = |marks: &[f64]| marks.iter().copied().filter(|m| *m > start + 1e-6 && *m <= start + limit).fold(None, |best: Option<f64>, m| Some(best.map_or(m, |b| b.max(m))));
        let cut = inside(chapters).or_else(|| inside(blocks)).or_else(|| blocks.iter().copied().find(|m| *m > start + 1e-6));
        match cut {
            Some(c) => {
                cuts.push(c);
                start = c;
            }
            None => break,
        }
    }
    cuts
}

/// Пакет: каждый блок — отдельный файл `exports/<проект>/<NNN> <имя>.<формат>`, громкость — по пресету каждого.
fn export_clips(p: &SpeechPaths, pid: &str, project: &SpeechProject, blocks: &[&Block], opts: &ExportOpts, work: &Path, progress: &dyn Fn(Value)) -> Result<Value, String> {
    let base = file_stem(&project.name, None);
    let folder = p.workspace.join(pid).join("exports").join(&base);
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let mut files = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        progress(json!({ "type": "progress", "stage": "speech_export", "msg": t!("speech-export-block", n = (i + 1) as i64, total = blocks.len() as i64), "pct": (i * 100 / blocks.len()) as i64 }));
        let take = b.active.and_then(|n| b.takes.iter().find(|t| t.n == n)).ok_or_else(|| t!("speech-export-not-ready", count = 1))?;
        let wav = work.join(format!("{i}.wav"));
        crate::media::extract_audio(&takes_dir(p, pid).join(&take.file), &wav, SR, 1)?;
        let (mut samples, _) = crate::wavio::read_mono_f32(&wav)?;
        if let Some((target, tp, _)) = loudness(&project.settings.loudness) {
            crate::loudness::level(&mut samples, SR, target, peak_for(&opts.format, tp))?;
        }
        crate::wavio::write_mono_f32(&wav, &samples, SR)?;
        let label = if b.note.trim().is_empty() { crate::speech_text::visible(&b.text).chars().take(40).collect::<String>() } else { b.note.trim_end_matches(".txt").to_string() };
        let name = format!("{:03} {}.{}", i + 1, file_stem(&label, None), opts.format);
        let meta = crate::media::AudioMeta { title: crate::speech_text::visible(&b.text), artist: project.voice_of(b).unwrap_or_default(), album: project.name.clone(), ..Default::default() };
        crate::media::encode_audio_meta(&wav, &folder.join(&name), &opts.format, project.settings.loudness == "acx", &meta)?;
        files.push(format!("{base}/{name}"));
    }
    Ok(json!({ "files": files, "folder": folder.to_string_lossy() }))
}

fn hms(t: f64) -> String {
    let s = t.max(0.0).round() as u64;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn srt_time(t: f64) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// Synced text in LRC: a line per block at its start, word by word (enhanced LRC `<mm:ss.xx>`) where its take was timed.
fn lrc(timed: &[(f64, f64, String, String, Vec<(String, f64, f64)>)]) -> String {
    let at = |s: f64| {
        let cs = (s.max(0.0) * 100.0).round() as u64;
        format!("{:02}:{:02}.{:02}", cs / 6000, cs / 100 % 60, cs % 100)
    };
    timed
        .iter()
        .map(|(a, _, _, text, words)| {
            if words.is_empty() {
                format!("[{}]{}", at(*a), text.replace('\n', " "))
            } else {
                format!("[{}]{}", at(*a), words.iter().map(|(w, s, _)| format!("<{}>{w}", at(*s))).collect::<Vec<_>>().join(" "))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn srt(cues: &[(f64, f64, String, String)]) -> String {
    let many = cues.iter().map(|c| &c.2).collect::<std::collections::BTreeSet<_>>().len() > 1;
    cues.iter()
        .enumerate()
        .map(|(i, (a, z, who, text))| {
            let next = cues.get(i + 1).map(|c| c.0);
            let z = match next {
                Some(n) if n < *z && *z - n <= SUBTITLE_OVERLAP_TRIM => n.max(*a),
                _ => *z,
            };
            let line = if many { format!("{who}: {text}") } else { text.clone() };
            format!("{}\n{} --> {}\n{line}\n", i + 1, srt_time(*a), srt_time(z))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Перекрытие соседних субтитров не длиннее этого — следствие сдвига реплики в паузу, а не перебивания: конец
/// предыдущей подрезается до начала следующей, чтобы плеер не показывал две строки.
const SUBTITLE_OVERLAP_TRIM: f64 = 0.5;

/// Имя файла экспорта по проекту и главе: без символов, которых не любят файловые системы.
pub fn file_stem(project: &str, chapter: Option<&str>) -> String {
    let raw = match chapter.filter(|c| !c.trim().is_empty()) {
        Some(c) => format!("{project} - {c}"),
        None => project.to_string(),
    };
    let clean: String = raw.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { ' ' } else { c }).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean: String = clean.chars().take(100).collect();
    if clean.trim().is_empty() { "speech".into() } else { clean.trim().trim_end_matches('.').to_string() }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_long_book_is_cut_at_chapters_then_blocks() {
        // 3 h book, chapters at 0, 50, 100, 150 min; parts of at most 2 h cut at the 100 min chapter
        let ch = [0.0, 3000.0, 6000.0, 9000.0];
        let blocks: Vec<f64> = (0..108).map(|k| k as f64 * 100.0).collect();
        assert_eq!(part_cuts(10800.0, 7200.0, &ch, &blocks), vec![6000.0]);
        // no chapters: the last block start within the limit
        assert_eq!(part_cuts(10800.0, 7200.0, &[], &blocks), vec![7200.0]);
        // short enough: one part
        assert!(part_cuts(3000.0, 7200.0, &ch, &blocks).is_empty());
    }

    #[test]
    fn synced_text_is_lrc_with_words_where_timed() {
        let timed = vec![
            (1.5, 3.0, "A".to_string(), "Привет мир".to_string(), vec![("Привет".to_string(), 1.5, 2.0), ("мир".to_string(), 2.1, 2.6)]),
            (65.25, 67.0, "B".to_string(), "Без слов".to_string(), Vec::new()),
        ];
        assert_eq!(lrc(&timed), "[00:01.50]<00:01.50>Привет <00:02.10>мир\n[01:05.25]Без слов");
    }

    use super::*;

    #[test]
    fn a_short_overlap_is_trimmed_and_an_interruption_stays() {
        let cue = |a: f64, z: f64, who: &str| (a, z, who.to_string(), "x".to_string());
        let out = srt(&[cue(0.0, 3.0, "A"), cue(2.88, 5.0, "A"), cue(4.0, 6.0, "B")]);
        assert!(out.contains("00:00:00,000 --> 00:00:02,880"), "{out}");
        assert!(out.contains("00:00:02,880 --> 00:00:05,000"), "a second of overlap is an interruption: {out}");
        assert!((peak_for("mp3", -1.0) + 2.0).abs() < 1e-9 && (peak_for("flac", -1.0) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn export_names_drop_what_file_systems_refuse() {
        assert_eq!(file_stem("Глава: 1/2?", None), "Глава 1 2");
        assert_eq!(file_stem("Книга", Some("Пролог")), "Книга - Пролог");
        assert_eq!(file_stem("  ", None), "speech");
    }

    #[test]
    fn resampling_keeps_the_length_in_time() {
        let x = vec![0.5f32; 48_000];
        assert_eq!(resample_to(x.clone(), 48_000).len(), 24_000);
        assert_eq!(resample_to(x, 24_000).len(), 48_000);
    }

    #[test]
    fn loudness_presets_are_the_documented_ones() {
        assert_eq!(loudness("video"), Some((-14.0, -1.0, 11.0)));
        assert_eq!(loudness("acx").map(|l| l.1), Some(-3.0));
        assert_eq!(loudness("off"), None);
    }
}
