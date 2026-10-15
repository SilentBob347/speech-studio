//! Проект Speech: `workspace/<pid>/project.json` вида `speech` — главы из блоков (спикер, голос, текст, указание,
//! пауза после, дубли), состав «спикер → голос» и настройки синтеза. Статус блока выводится, а не хранится: ключ
//! синтеза (текст для синтеза, голос и его файлы, движок, квант, параметры, язык) сравнивается с ключом выбранного
//! дубля — совпал `ready`, нет `stale`, дублей нет `pending`, последняя ошибка на этом ключе `failed`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::tts_host::Params;

pub const KIND: &str = "speech";
/// Поднимать при смене правил текста или синтеза, меняющей звучание уже озвученного.
const KEY_VERSION: &str = "speech-key-v1";
/// Длиннее этого блок режется по предложениям: длинный текст Higgs читает хуже и дольше ждать ошибку.
pub const BLOCK_CHARS: usize = 400;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Take {
    pub n: u32,
    pub file: String,
    pub duration: f64,
    pub seed: u64,
    pub temperature: f64,
    pub key: String,
    pub created: u64,
    pub source: String,
    pub synth_secs: f64,
    pub retried: Vec<String>,
    pub qc: Option<crate::qc::Check>,
    /// Текст блока, по которому озвучен дубль: к нему можно вернуться вместе с дублем.
    pub text: String,
    pub direction: String,
    /// Параметры, с которыми сделан дубль (для истории и повтора).
    pub top_k: u32,
    pub top_p: f64,
    pub max_tokens: u32,
    pub voice: String,
    pub engine: String,
    /// Подгонка под реплику: ускорение, на сколько секунд начат раньше, не влез и с ускорением.
    pub fit_stretch: f64,
    pub fit_lead: f64,
    pub fit_over: bool,
    /// Цена облачного дубля из ответа провайдера (USD), если он её назвал.
    pub cost: Option<f64>,
    /// Оценка цены облачного дубля до запуска (USD).
    pub estimate: Option<f64>,
    /// Сколько миллисекунд от начала синтеза до первого звука, у движков, что отдают звук потоком.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_ms: Option<u64>,
    /// Размер файла дубля при записи; 0 — не записан (дубли старых проектов), файл тогда не сверяется.
    #[serde(skip_serializing_if = "is_zero")]
    pub bytes: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct BlockError {
    pub key: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Block {
    pub id: String,
    pub speaker: String,
    pub voice: Option<String>,
    pub text: String,
    pub direction: String,
    pub pause_ms: Option<i64>,
    pub hidden: bool,
    pub takes: Vec<Take>,
    pub active: Option<u32>,
    pub pinned: Option<u32>,
    pub error: Option<BlockError>,
    /// Заметка к блоку: на синтез не влияет.
    pub note: String,
    /// Цвет маркера (`red`, `amber`, `green`, `blue`, `violet`) или пусто.
    pub marker: String,
    /// Время реплики субтитров, с: в проекте по таймкодам блок звучит в нём.
    pub start: Option<f64>,
    pub end: Option<f64>,
}

pub const MARKERS: [&str; 5] = ["red", "amber", "green", "blue", "violet"];

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Chapter {
    pub id: String,
    pub title: String,
    pub blocks: Vec<Block>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct QcSettings {
    /// Распознать каждый новый дубль и сравнить с текстом.
    pub asr_check: bool,
    pub strictness: String,
    /// Сколько дублей на блок; больше одного — лучший выбирается проверкой.
    pub takes: u32,
    /// Сверять голос дубля с рефом (WeSpeaker): непохожий — повтор и пометка.
    pub similarity: bool,
}

impl Default for QcSettings {
    fn default() -> Self {
        QcSettings { asr_check: false, strictness: "normal".into(), takes: 1, similarity: false }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub lang: String,
    pub engine: String,
    pub params: Params,
    pub voice: Option<String>,
    pub pause_same_ms: i64,
    pub pause_switch_ms: i64,
    pub loudness: String,
    pub qc: QcSettings,
    /// Русский текст перед озвучкой проходит разметку ударений и «ё» (stress.rs); движок берёт из неё то, что читает.
    pub auto_stress: bool,
    /// Русский текст перед озвучкой проходит нормализацию: числа, даты, единицы словами (normalize_ru.rs).
    pub normalize: bool,
    /// Блоки звучат во времени своих реплик (проект из субтитров), а не друг за другом.
    pub timeline: bool,
    /// Предел ускорения клипа под реплику (как в Dub).
    pub max_stretch: f64,
    /// Не влезающая реплика сначала начинается раньше, в тишину перед ней (до 1 с), и только потом ускоряется.
    pub lead_into_silence: bool,
    /// Паузы склейки — тихий room tone, а не цифровой ноль (у пресета ACX — всегда, с тишиной по краям).
    pub room_tone: bool,
    /// В паузу 0,4–1,8 с между блоками перед следующим ставится тихий вдох (Dub `breath_on`).
    pub breaths: bool,
    /// Сквозная интонация: блок берёт рефом предыдущий дубль того же голоса с его текстом (продолжение речи Higgs).
    pub continuation: bool,
    /// Движок `openrouter`: модель и голос (пусто — из «Облака»).
    pub cloud_model: String,
    pub cloud_voice: String,
    /// Значения формы облачной модели fal или Runware (голос, скорость…), кроме текста.
    pub cloud_values: serde_json::Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { lang: "auto".into(), engine: "higgs".into(), params: Params::default(), voice: None, pause_same_ms: 300, pause_switch_ms: 600, loudness: "video".into(), qc: QcSettings::default(), auto_stress: true, normalize: true, timeline: false, max_stretch: 1.25, lead_into_silence: true, room_tone: false, breaths: false, continuation: false, cloud_model: String::new(), cloud_voice: String::new(), cloud_values: serde_json::Map::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct SpeechProject {
    pub kind: String,
    pub version: u32,
    pub name: String,
    pub created: u64,
    pub settings: Settings,
    pub cast: BTreeMap<String, String>,
    pub chapters: Vec<Chapter>,
    /// Словарь произношения проекта: срабатывает раньше глобального.
    pub dictionary: Vec<crate::pron_dict::Entry>,
    /// Проект из книги: название, автор и файл обложки в папке проекта — в метаданные и обложку аудиокниги.
    pub book: Option<BookMeta>,
    /// Ключ инструмента агента «одним вызовом»: повторный вызов с теми же входами находит этот проект.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct BookMeta {
    pub title: String,
    pub author: String,
    pub cover: Option<String>,
}

impl Default for SpeechProject {
    fn default() -> Self {
        SpeechProject { kind: KIND.into(), version: 1, name: String::new(), created: 0, settings: Settings::default(), cast: BTreeMap::new(), chapters: Vec::new(), dictionary: Vec::new(), book: None, key: String::new() }
    }
}

/// Проект по таймкодам из реплик субтитров: реплика — блок; говорящий — из файла или рассказчик.
pub fn from_cues(name: &str, cues: &[crate::subimport::Cue], narrator: &str) -> SpeechProject {
    let blocks = cues
        .iter()
        .map(|c| Block {
            id: new_id(),
            speaker: c.speaker.clone().unwrap_or_else(|| narrator.to_string()),
            text: c.text.clone(),
            start: Some(c.start),
            end: Some(c.end),
            ..Default::default()
        })
        .collect();
    let mut project = SpeechProject { name: name.to_string(), created: now(), ..Default::default() };
    project.settings.timeline = true;
    project.chapters.push(Chapter { id: new_id(), title: String::new(), blocks });
    project
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn new_id() -> String {
    let mut id = uuid::Uuid::new_v4().simple().to_string();
    id.truncate(10);
    id
}

impl SpeechProject {
    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.chapters.iter().flat_map(|c| c.blocks.iter())
    }

    pub fn block_mut(&mut self, id: &str) -> Option<&mut Block> {
        self.chapters.iter_mut().flat_map(|c| c.blocks.iter_mut()).find(|b| b.id == id)
    }

    /// Голос блока: свой, иначе спикера по составу, иначе голос проекта; `None` — базовый голос движка.
    pub fn voice_of(&self, b: &Block) -> Option<String> {
        b.voice.clone().filter(|v| !v.is_empty()).or_else(|| self.cast.get(&b.speaker).cloned().filter(|v| !v.is_empty())).or_else(|| self.settings.voice.clone().filter(|v| !v.is_empty()))
    }

    /// Ид блоков и глав уникальны, у каждого блока есть ид, пустые главы допустимы, ссылки на дубли верны.
    pub fn validate(&self) -> Result<(), String> {
        if self.kind != KIND {
            return Err(format!("this is a {} project, not a speech one", self.kind));
        }
        self.settings.params.checked()?;
        if !(1.0..=2.0).contains(&self.settings.max_stretch) {
            return Err(format!("max_stretch {} is outside 1.0..2.0", self.settings.max_stretch));
        }
        crate::pron_dict::validate(&self.dictionary)?;
        let mut seen = std::collections::HashSet::new();
        for c in &self.chapters {
            if c.id.is_empty() || !seen.insert(format!("c:{}", c.id)) {
                return Err(format!("chapter id {:?} is empty or repeated", c.id));
            }
            for b in &c.blocks {
                if b.id.is_empty() || !seen.insert(format!("b:{}", b.id)) {
                    return Err(format!("block id {:?} is empty or repeated", b.id));
                }
                for n in [b.active, b.pinned].into_iter().flatten() {
                    if !b.takes.iter().any(|t| t.n == n) {
                        return Err(format!("block {} points at take {n}, which it does not have", b.id));
                    }
                }
                if let (Some(a), Some(z)) = (b.start, b.end) {
                    if !(a >= 0.0 && z > a) {
                        return Err(format!("block {} cue {a}..{z} is not a forward span", b.id));
                    }
                }
                if !b.marker.is_empty() && !MARKERS.contains(&b.marker.as_str()) {
                    return Err(format!("block {} marker {:?} is not one of {MARKERS:?}", b.id, b.marker));
                }
                if let Some(p) = b.pause_ms {
                    if !(-2000..=10_000).contains(&p) {
                        return Err(format!("block {} pause {p} ms is outside -2000..10000", b.id));
                    }
                }
            }
        }
        for p in [self.settings.pause_same_ms, self.settings.pause_switch_ms] {
            if !(-2000..=10_000).contains(&p) {
                return Err(format!("pause {p} ms is outside -2000..10000"));
            }
        }
        if !matches!(self.settings.qc.strictness.as_str(), "strict" | "normal" | "lenient") {
            return Err(format!("strictness {} is not one of strict, normal, lenient", self.settings.qc.strictness));
        }
        if !(1..=20).contains(&self.settings.qc.takes) {
            return Err(format!("takes per block {} is outside 1..20", self.settings.qc.takes));
        }
        if !matches!(self.settings.loudness.as_str(), "off" | "video" | "podcast" | "acx") {
            return Err(format!("loudness preset {} is not one of off, video, podcast, acx", self.settings.loudness));
        }
        Ok(())
    }
}

/// Начало видимого текста блока для строки прогресса.
pub fn visible_snippet(text: &str) -> String {
    let v = crate::speech_text::visible(text);
    let mut out: String = v.chars().take(40).collect();
    if v.chars().count() > 40 {
        out.push('…');
    }
    out
}

pub fn path(workspace: &Path, pid: &str) -> PathBuf {
    workspace.join(pid).join("project.json")
}

pub fn is_pid(pid: &str) -> bool {
    !pid.is_empty() && pid.len() <= 40 && pid.chars().all(|c| c.is_ascii_alphanumeric())
}

pub fn load(workspace: &Path, pid: &str) -> Result<SpeechProject, String> {
    if !is_pid(pid) {
        return Err(format!("bad project id {pid}"));
    }
    let p = path(workspace, pid);
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
    if v.get("kind").and_then(Value::as_str) != Some(KIND) {
        return Err(format!("project {pid} is not a speech project"));
    }
    serde_json::from_value(v).map_err(|e| format!("{}: {e}", p.display()))
}

/// Записать проект через ревизии (кто сохранил — окно, агент, задача — узнают все окна).
pub fn save(workspace: &Path, pid: &str, project: &SpeechProject) -> Result<(), String> {
    project.validate()?;
    let dir = workspace.join(pid);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    crate::mcp::save_with_revision(&dir, || {
        let tmp = dir.join("project.json.tmp");
        std::fs::write(&tmp, text.as_bytes()).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, dir.join("project.json")).map_err(|e| format!("{}: {e}", dir.display()))
    })
}

static DIALOG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:\[([^\[\]\n]{1,40})\]|([\p{L}][\p{L}\p{N} ._'-]{0,30}?)\s*:)\s+(\S.*)$").expect("dialog"));
static SENTENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^.!?…]+[.!?…]+[»”\x22)]*\s*|[^.!?…]+$").expect("sentence"));

/// Длинный абзац — по предложениям в куски не длиннее `BLOCK_CHARS`.
fn chunk(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.chars().count() <= BLOCK_CHARS {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for s in SENTENCE.find_iter(text).map(|m| m.as_str().trim()).filter(|s| !s.is_empty()) {
        if !cur.is_empty() && cur.chars().count() + 1 + s.chars().count() > BLOCK_CHARS {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(s);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Текст → блоки (спикер, текст). Диалог — когда не меньше половины строк начинаются с `Имя:` или `[Имя]` и
/// спикеров не больше восьми; иначе абзацы рассказчика, длинные — по предложениям.
pub fn split_text(text: &str, narrator: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let tagged: Vec<Option<(String, String)>> = lines
        .iter()
        .map(|l| DIALOG.captures(l).map(|c| (c.get(1).or_else(|| c.get(2)).map(|m| m.as_str().trim().to_string()).unwrap_or_default(), c[3].trim().to_string())))
        .collect();
    let speakers: std::collections::BTreeSet<&str> = tagged.iter().flatten().map(|(s, _)| s.as_str()).collect();
    let dialog = !lines.is_empty() && tagged.iter().filter(|t| t.is_some()).count() * 2 >= lines.len() && speakers.len() <= 8;
    if dialog {
        let mut out: Vec<(String, String)> = Vec::new();
        for (line, tag) in lines.iter().zip(tagged) {
            match tag {
                Some((s, t)) => out.extend(chunk(&t).into_iter().map(|c| (s.clone(), c))),
                None => match out.last_mut() {
                    Some(last) => {
                        last.1.push(' ');
                        last.1.push_str(line);
                    }
                    None => out.push((narrator.to_string(), line.to_string())),
                },
            }
        }
        return out;
    }
    text.split("\n\n")
        .flat_map(|p| {
            let p = p.split_whitespace().collect::<Vec<_>>().join(" ");
            chunk(&p).into_iter().filter(|c| !c.is_empty()).map(|c| (narrator.to_string(), c)).collect::<Vec<_>>()
        })
        .collect()
}

/// Новый проект из текста: одна глава, блоки по `split_text`.
pub fn from_text(name: &str, text: &str, narrator: &str) -> SpeechProject {
    let blocks = split_text(text, narrator)
        .into_iter()
        .map(|(speaker, text)| Block { id: new_id(), speaker, text, ..Default::default() })
        .collect();
    SpeechProject {
        name: name.to_string(),
        created: now(),
        chapters: vec![Chapter { id: new_id(), title: String::new(), blocks }],
        ..Default::default()
    }
}

/// Пакет: каждая непустая строка — отдельный клип (блок), без разбора диалогов и нарезки.
pub fn from_lines(name: &str, text: &str, narrator: &str) -> SpeechProject {
    let blocks = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| Block { id: new_id(), speaker: narrator.to_string(), text: l.to_string(), ..Default::default() })
        .collect();
    SpeechProject { name: name.to_string(), created: now(), chapters: vec![Chapter { id: new_id(), title: String::new(), blocks }], ..Default::default() }
}

/// Пакет из файлов: текст файла — блок, имя файла — заметка блока и имя его клипа при экспорте.
pub fn from_files(name: &str, files: &[(String, String)], narrator: &str) -> SpeechProject {
    let blocks = files
        .iter()
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(file, t)| Block { id: new_id(), speaker: narrator.to_string(), text: t.trim().to_string(), note: file.clone(), ..Default::default() })
        .collect();
    SpeechProject { name: name.to_string(), created: now(), chapters: vec![Chapter { id: new_id(), title: String::new(), blocks }], ..Default::default() }
}

/// Блок, чей дубль служит рефом продолжения: ближайший видимый выше с тем же голосом и выбранным дублем.
pub fn continuation_source<'a>(project: &'a SpeechProject, b: &Block) -> Option<(&'a Block, &'a Take)> {
    if !project.settings.continuation {
        return None;
    }
    let voice = project.voice_of(b);
    let blocks: Vec<&Block> = project.blocks().collect();
    let at = blocks.iter().position(|x| x.id == b.id)?;
    blocks[..at]
        .iter()
        .rev()
        .filter(|x| !x.hidden && !x.text.trim().is_empty())
        .find(|x| project.voice_of(x) == voice)
        .and_then(|x| x.active.and_then(|n| x.takes.iter().find(|t| t.n == n)).map(|t| (*x, t)))
}

/// Отпечаток голоса для ключа: имя, размер и время звука, текст рефа.
pub fn voice_tag(voices: &Path, name: Option<&str>) -> String {
    let Some(name) = name else { return "base".into() };
    let Some(audio) = crate::voice_library::audio_path(voices, name) else { return format!("missing:{name}") };
    let meta = std::fs::metadata(&audio).ok();
    let stamp = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let transcript = crate::voice_library::read_transcript(voices, name).unwrap_or_default();
    format!("{name}|{}|{stamp}|{}", meta.map(|m| m.len()).unwrap_or(0), blake3::hash(transcript.as_bytes()).to_hex())
}

/// Ключ синтеза блока: всё, от чего зависит звук. `engine` — движок и его модель с квантом.
pub fn synth_key(project: &SpeechProject, b: &Block, voice_tag: &str, engine: &str, ref_limit: f64, prep: &str) -> String {
    let p = &project.settings.params;
    let seed = if p.seed_mode == "fixed" { p.seed.to_string() } else { "random".into() };
    let mut h = blake3::Hasher::new();
    for part in [
        KEY_VERSION,
        b.text.trim(),
        b.direction.trim(),
        voice_tag,
        engine,
        &format!("{:.3}|{}|{:.3}|{seed}|{ref_limit:.1}", p.temperature, p.top_k, p.top_p),
        project.settings.lang.as_str(),
    ] {
        h.update(part.as_bytes());
        h.update(b"\x1f");
    }
    if project.settings.auto_stress && crate::stress::is_russian(&b.text, &project.settings.lang) {
        h.update(b"auto-stress\x1f");
    }
    if let Some((src, take)) = continuation_source(project, b) {
        h.update(format!("continue:{}:{}\x1f", src.id, take.file).as_bytes());
    }
    if !prep.is_empty() {
        h.update(prep.as_bytes());
        h.update(b"\x1f");
    }
    if let (true, Some(a), Some(z)) = (project.settings.timeline, b.start, b.end) {
        h.update(format!("cue:{a:.3}:{z:.3}:{:.2}:{}\x1f", project.settings.max_stretch, project.settings.lead_into_silence).as_bytes());
    }
    h.finalize().to_hex()[..20].to_string()
}

pub fn status(b: &Block, key: &str) -> &'static str {
    if b.text.trim().is_empty() || b.hidden {
        return "skipped";
    }
    if let Some(e) = &b.error {
        if e.key == key {
            return "failed";
        }
    }
    match b.active.and_then(|n| b.takes.iter().find(|t| t.n == n)) {
        None if b.takes.is_empty() => "pending",
        None => "stale",
        Some(t) if t.key == key => "ready",
        Some(_) => "stale",
    }
}

/// Проект для окна: сам проект и по каждому блоку его ключ и статус.
/// Прогноз до синтеза для блока по таймкодам: `fits`, `tight` (уложится ускорением) или `impossible` — по
/// оценке длины текста при темпе языка (механизм `dub_core::fit::verdict`).
pub fn fit_forecast(project: &SpeechProject, b: &Block) -> Option<&'static str> {
    let (true, Some(start), Some(end)) = (project.settings.timeline, b.start, b.end) else { return None };
    let timed: Vec<&Block> = project.blocks().filter(|x| !x.hidden && x.start.is_some()).collect();
    let next = timed.iter().position(|x| x.id == b.id).and_then(|i| timed.get(i + 1)).and_then(|x| x.start).unwrap_or(end + 3600.0);
    let rules = dub_core::fit::FitRules { speech_rate_on: false, qc_duration: true, max_stretch: project.settings.max_stretch, lead_into_silence: false };
    let slot = dub_core::fit::slot(start, end, start, next, &rules);
    let lang = if project.settings.lang == "auto" { if crate::stress::is_russian(&b.text, "auto") { "ru" } else { "en" } } else { project.settings.lang.as_str() };
    let est = dub_core::fit::estimate(&crate::speech_text::visible(&b.text), dub_core::fit::table_cps(lang));
    Some(match dub_core::fit::verdict(est, &slot, &rules) {
        dub_core::fit::Verdict::Fits => "fits",
        dub_core::fit::Verdict::Tight => "tight",
        dub_core::fit::Verdict::Impossible => "impossible",
    })
}

pub fn with_status(project: &SpeechProject, keys: &BTreeMap<String, String>) -> Value {
    let mut v = serde_json::to_value(project).unwrap_or(Value::Null);
    let st: serde_json::Map<String, Value> = project
        .blocks()
        .map(|b| {
            let key = keys.get(&b.id).cloned().unwrap_or_default();
            let mut entry = json!({ "state": status(b, &key), "key": key });
            if let Some(fit) = fit_forecast(project, b) {
                entry["fit"] = json!(fit);
            }
            (b.id.clone(), entry)
        })
        .collect();
    v["status"] = Value::Object(st);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_become_blocks_and_long_ones_are_cut_at_sentences() {
        let long = "Предложение номер один здесь. ".repeat(30);
        let blocks = split_text(&format!("Первый абзац.\n\nВторой абзац\nв две строки.\n\n{long}"), "Рассказчик");
        assert_eq!(blocks[0], ("Рассказчик".to_string(), "Первый абзац.".to_string()));
        assert_eq!(blocks[1].1, "Второй абзац в две строки.");
        assert!(blocks.len() > 3 && blocks[2..].iter().all(|(_, t)| t.chars().count() <= BLOCK_CHARS && t.ends_with('.')));
    }

    #[test]
    fn a_dialog_keeps_its_speakers() {
        let blocks = split_text("Анна: Привет!\n[Борис] Здравствуй.\nкак дела?\nАнна: Хорошо.", "Рассказчик");
        assert_eq!(blocks, vec![
            ("Анна".to_string(), "Привет!".to_string()),
            ("Борис".to_string(), "Здравствуй. как дела?".to_string()),
            ("Анна".to_string(), "Хорошо.".to_string()),
        ]);
    }

    #[test]
    fn a_colon_in_prose_does_not_make_a_dialog() {
        let blocks = split_text("Встреча в 12:00 у входа.\nВзять с собой: паспорт, билеты.\nНе опаздывать.\nВсё.", "Р");
        assert!(blocks.iter().all(|(s, _)| s == "Р"));
    }

    #[test]
    fn status_follows_the_key_of_the_active_take() {
        let mut b = Block { id: "b".into(), text: "Да".into(), ..Default::default() };
        assert_eq!(status(&b, "k1"), "pending");
        b.takes.push(Take { n: 1, key: "k1".into(), ..Default::default() });
        b.active = Some(1);
        assert_eq!(status(&b, "k1"), "ready");
        assert_eq!(status(&b, "k2"), "stale");
        b.error = Some(BlockError { key: "k2".into(), text: "x".into() });
        assert_eq!(status(&b, "k2"), "failed");
        b.hidden = true;
        assert_eq!(status(&b, "k2"), "skipped");
    }

    #[test]
    fn the_key_changes_with_what_changes_the_sound() {
        let mut p = from_text("t", "Привет.", "Р");
        let b = p.chapters[0].blocks[0].clone();
        let k = synth_key(&p, &b, "v1", "higgs:q8_0", 12.0, "");
        assert_eq!(k, synth_key(&p, &b, "v1", "higgs:q8_0", 12.0, ""));
        assert_ne!(k, synth_key(&p, &b, "v2", "higgs:q8_0", 12.0, ""));
        assert_ne!(k, synth_key(&p, &b, "v1", "higgs:q6_k", 12.0, ""));
        p.settings.params.temperature = 0.7;
        assert_ne!(k, synth_key(&p, &b, "v1", "higgs:q8_0", 12.0, ""));
        let mut b2 = b.clone();
        b2.pause_ms = Some(900);
        p.settings.params.temperature = 1.0;
        assert_eq!(k, synth_key(&p, &b2, "v1", "higgs:q8_0", 12.0, ""), "a pause is silence, it does not change the clip");
    }

    #[test]
    fn validation_refuses_repeated_ids_and_dangling_takes() {
        let mut p = from_text("t", "Раз.\n\nДва.", "Р");
        assert!(p.validate().is_ok());
        let id = p.chapters[0].blocks[0].id.clone();
        p.chapters[0].blocks[1].id = id;
        assert!(p.validate().is_err());
        let mut p = from_text("t", "Раз.", "Р");
        p.chapters[0].blocks[0].active = Some(3);
        assert!(p.validate().unwrap_err().contains("take 3"));
    }
}
