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

/// Ключи синтеза всех блоков проекта при нынешних голосах и настройках.
pub fn keys(p: &SpeechPaths, project: &SpeechProject) -> BTreeMap<String, String> {
    let cfg = higgs_config(p);
    let engine = format!("higgs:{}", cfg.quant);
    let limit = ref_limit(&p.models_root);
    let entries = dictionary(p, project);
    let compiled = crate::prep::compile_all(&entries);
    let mut tags: BTreeMap<Option<String>, String> = BTreeMap::new();
    project
        .blocks()
        .map(|b| {
            let voice = project.voice_of(b);
            let tag = tags.entry(voice.clone()).or_insert_with(|| sp::voice_tag(&p.voices_dir, voice.as_deref())).clone();
            let prep = crate::prep::fingerprint(&b.text, &prep_opts(p, project, voice.as_deref(), &entries, project.settings.auto_stress), &compiled);
            (b.id.clone(), sp::synth_key(project, b, &tag, &engine, limit, &prep))
        })
        .collect()
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

/// Звук одного блока: фразы синтезом, паузы тишиной. Возвращает сэмплы (SR), что разметки проигнорировано и сводку.
fn synth_block(p: &SpeechPaths, project: &SpeechProject, b: &Block, n: u32) -> Result<(Vec<f32>, Value, Take), String> {
    let cfg = higgs_config(p);
    let engine = tts_host::engine(&cfg)?;
    let voice = project.voice_of(b);
    let reference = match voice.as_deref() {
        Some(v) => Some(tts_host::prepare_ref(&p.voices_dir, v, ref_limit(&p.models_root))?),
        None => None,
    };
    let lang = if project.settings.lang == "auto" { "" } else { project.settings.lang.as_str() };
    let mut out: Vec<f32> = Vec::new();
    let mut ignored = stx::Ignored::default();
    let (mut seed, mut temperature, mut secs, mut retried) = (0u64, project.settings.params.temperature, 0.0, Vec::new());
    let fresh = seed_for(&b.id, n);
    let params = take_params(&project.settings.params, n);
    let mut rate = 1.0f64;
    let entries = dictionary(p, project);
    let stress = project.settings.auto_stress && crate::stress::is_russian(&b.text, &project.settings.lang);
    let text = crate::prep::prepare(&b.text, &prep_opts(p, project, voice.as_deref(), &entries, stress)).map_err(|e| t!("stress-failed", detail = e))?.text;
    for (k, piece) in stx::pieces(&text).into_iter().enumerate() {
        match piece {
            Piece::Pause(ms) => out.extend(std::iter::repeat_n(0.0, (SR as u64 * ms as u64 / 1000) as usize)),
            Piece::Rate(r) => rate = r,
            Piece::Say(text) => {
                let (say, ig) = stx::for_higgs(&text, lang);
                ignored.accent |= ig.accent;
                ignored.phonemes |= ig.phonemes;
                ignored.stress |= ig.stress;
                if say.is_empty() {
                    continue;
                }
                let req = tts_host::Request {
                    text: &say,
                    lang,
                    ref_wav: reference.as_ref().map(|(w, _)| w.as_path()),
                    ref_text: reference.as_ref().and_then(|(_, t)| t.as_deref()),
                    params: &params,
                    fresh_seed: fresh.wrapping_add(k as u64),
                };
                let s = tts_host::synth(&engine, &req)?;
                if k == 0 || seed == 0 {
                    seed = s.seed;
                    temperature = s.temperature;
                }
                secs += s.seconds;
                retried.extend(s.defects.iter().map(|d| d.to_string()));
                let samples = resample_to(s.samples, s.sr);
                out.extend(if (rate - 1.0).abs() > 1e-3 { at_rate(&samples, rate)? } else { samples });
            }
        }
    }
    ignored.stress = stx::has_stress(&b.text);
    if out.is_empty() {
        return Err(t!("speech-block-silent"));
    }
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
        ..Default::default()
    };
    Ok((out, serde_json::to_value(&ignored).unwrap_or(Value::Null), take))
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
pub struct Scope {
    pub ids: Option<Vec<String>>,
    pub from: Option<String>,
    pub force: bool,
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
        let result = synth_block(p, &project, &b, n).and_then(|(samples, ignored, mut take)| {
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
fn check_takes(p: &SpeechPaths, pid: &str, made: &BTreeMap<String, Vec<u32>>, progress: &dyn Fn(Value)) -> Result<Vec<String>, String> {
    if crate::hw::total_vram() < 12 * 1024 * 1024 * 1024 {
        tts_host::unload();
    }
    crate::ensure_job_components(&p.repo_root, &p.models_root, false, false, &|ev| progress(ev))?;
    let mut failed = Vec::new();
    for (i, (id, takes)) in made.iter().enumerate() {
        progress(json!({ "type": "progress", "stage": "speech_qc", "msg": t!("speech-qc-block", n = (i + 1) as i64, total = made.len() as i64), "pct": (i * 100 / made.len().max(1)) as i64 }));
        let project = sp::load(&p.workspace, pid)?;
        let Some(b) = project.blocks().find(|b| &b.id == id).cloned() else { continue };
        let lang = project.settings.lang.clone();
        let strictness = project.settings.qc.strictness.clone();
        let mut checks: Vec<(u32, crate::qc::Check)> = Vec::new();
        for n in takes {
            let Some(take) = b.takes.iter().find(|t| t.n == *n) else { continue };
            let heard = crate::asr_util::transcribe(&p.repo_root, &p.models_root, &takes_dir(p, pid).join(&take.file), &lang, &b.id)?;
            let entries = dictionary(p, &project);
            let voice = project.voice_of(&b);
            let expected = crate::prep::prepare(&b.text, &prep_opts(p, &project, voice.as_deref(), &entries, false)).map(|x| x.text).unwrap_or_else(|_| b.text.clone());
            let heard_said = if crate::prep::block_lang(&b.text, &lang) == "ru" { crate::normalize_ru::normalize(&heard.text).text } else { heard.text.clone() };
            let mut check = crate::qc::check(&expected, &heard_said, &strictness);
            check.heard = heard.text.clone();
            checks.push((*n, check));
        }
        let mut fresh = sp::load(&p.workspace, pid)?;
        let Some(block) = fresh.block_mut(id) else { continue };
        for (n, c) in &checks {
            if let Some(t) = block.takes.iter_mut().find(|t| t.n == *n) {
                t.qc = Some(c.clone());
            }
        }
        let best = checks.iter().min_by(|a, b| (!a.1.passed, a.1.wer).partial_cmp(&(!b.1.passed, b.1.wer)).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((n, c)) = best {
            if block.pinned.is_none() {
                block.active = Some(*n);
            }
            if !c.passed {
                failed.push(id.clone());
            }
        }
        sp::save(&p.workspace, pid, &fresh)?;
    }
    Ok(failed)
}

pub fn synth(p: &SpeechPaths, pid: &str, scope: &Scope, progress: &dyn Fn(Value)) -> Result<Value, String> {
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
    let checking = qc.asr_check || qc.takes > 1;
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
                let heard = block.active.and_then(|n| block.takes.iter().find(|t| t.n == n)).and_then(|t| t.qc.as_ref()).map(|c| c.heard.clone()).unwrap_or_default();
                block.error = Some(sp::BlockError { key, text: t!("speech-qc-failed", heard = heard) });
                sp::save(&p.workspace, pid, &fresh)?;
            }
        }
    }
    if done == 0 && !failed.is_empty() {
        return Err(t!("speech-synth-all-failed", error = failed[0]["error"].as_str().unwrap_or("").to_string()));
    }
    Ok(json!({ "done": done, "failed": failed, "skipped": skipped, "qc_failed": qc_failed }))
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
    let mut mix: Vec<f32> = Vec::new();
    let mut cursor: i64 = 0;
    let mut marks = Vec::new();
    let mut prev_speaker: Option<&str> = None;
    for (i, b) in blocks.iter().enumerate() {
        progress(json!({ "type": "progress", "stage": "speech_export", "msg": t!("speech-export-block", n = (i + 1) as i64, total = blocks.len() as i64), "pct": (i * 90 / blocks.len()) as i64 }));
        let take = b.active.and_then(|n| b.takes.iter().find(|t| t.n == n)).ok_or_else(|| t!("speech-export-not-ready", count = 1))?;
        let src = takes_dir(p, pid).join(&take.file);
        let wav = work.path().join(format!("{i}.wav"));
        crate::media::extract_audio(&src, &wav, SR, 1)?;
        let (samples, _) = crate::wavio::read_mono_f32(&wav)?;
        if let Some(prev) = prev_speaker {
            let ms = b.pause_ms.unwrap_or(if prev == b.speaker { project.settings.pause_same_ms } else { project.settings.pause_switch_ms });
            cursor = (cursor + SR as i64 * ms / 1000).max(0);
        }
        let at = cursor as usize;
        marks.push(json!({ "block": b.id, "start": at as f64 / SR as f64 }));
        if mix.len() < at + samples.len() {
            mix.resize(at + samples.len(), 0.0);
        }
        for (k, s) in samples.iter().enumerate() {
            mix[at + k] += *s;
        }
        cursor = (at + samples.len()) as i64;
        if let Some(own) = b.pause_ms.filter(|_| i + 1 == blocks.len()) {
            mix.resize(mix.len() + (SR as i64 * own.max(0) / 1000) as usize, 0.0);
        }
        prev_speaker = Some(b.speaker.as_str());
    }
    progress(json!({ "type": "progress", "stage": "speech_export", "msg": t!("speech-export-loudness"), "pct": 92 }));
    let levelled = match loudness(&project.settings.loudness) {
        Some((i, tp, _)) => Some(crate::loudness::level(&mut mix, SR, i, tp)?),
        None => None,
    };
    let joined = work.path().join("joined.wav");
    crate::wavio::write_mono_f32(&joined, &mix, SR)?;
    let exports = dir.join("exports");
    std::fs::create_dir_all(&exports).map_err(|e| format!("{}: {e}", exports.display()))?;
    let base = file_stem(&project.name, opts.chapter.as_deref().and_then(|id| project.chapters.iter().find(|c| c.id == id)).map(|c| c.title.as_str()));
    let file = format!("{base}.{}", opts.format);
    let target = exports.join(&file);
    crate::media::encode_audio(&joined, &target, &opts.format, project.settings.loudness == "acx")?;
    Ok(json!({ "file": file, "path": target.to_string_lossy(), "duration": mix.len() as f64 / SR as f64, "marks": marks, "lufs_before": levelled.map(|l| l.before), "lufs": levelled.map(|l| l.after) }))
}

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
    use super::*;

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
