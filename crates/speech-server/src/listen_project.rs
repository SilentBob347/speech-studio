//! Проект Listening: запись (`source.<ext>` в папке проекта) и её транскрипт — строки со спикером и таймкодами, слова
//! с таймкодами и пометкой «вырезать». Тот же `project.json` и те же ревизии, что у проекта Speech; вид — `listening`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const KIND: &str = "listening";
pub const MAX_SPEAKERS: u8 = 8;
const MEDIA: &[&str] = &["wav", "flac", "mp3", "m4a", "aac", "ogg", "opus", "wma", "mp4", "mkv", "webm", "mov", "avi", "m4v"];

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Word {
    pub text: String,
    pub start: f64,
    pub end: f64,
    /// Отмечено к удалению при монтаже по тексту.
    pub cut: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Line {
    pub id: String,
    pub start: f64,
    pub end: f64,
    /// Ключ спикера в `speakers` проекта.
    pub speaker: String,
    pub text: String,
    pub words: Vec<Word>,
    /// Таймкоды слов разложены по длине слов внутри фразы: распознаватель дал только фразу (облако, субтитры).
    pub approx: bool,
    /// Похоже на галлюцинацию распознавателя: `wordless`, `bracketed`, `credit`, `known_phrase`…
    pub flag: Option<String>,
    /// Чем проверялся голос на интервале: `vocals`, `diarization`, `mix`.
    pub evidence: Option<String>,
    /// Скрыта фильтром или пользователем: не идёт в экспорт и в Speech, возвращается одним щелчком.
    pub hidden: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// `""` — распознаватель из настроек студии; `parakeet`, `whisper`, `openrouter`, `fal`.
    pub engine: String,
    /// Модель OpenRouter; пусто — выбранная в «Облаке».
    pub cloud_model: String,
    pub lang: String,
    /// 0 — сколько услышит диаризация, 1 — без диаризации, 2–8 — ровно столько.
    pub speakers: u8,
    /// Отделить голос от музыки и шума перед распознаванием (Roformer).
    pub separate: bool,
    /// Скрывать галлюцинации распознавателя.
    pub filter: bool,
    /// Подсказка распознавателю: тема и термины записи. Уходит только в Whisper (`--initial_prompt`), вместе с
    /// написаниями из исправлений.
    pub prompt: String,
    /// Убрать шум (DPDFNet) перед распознаванием; `denoise_db` — предел подавления, 0 — без предела.
    pub denoise: bool,
    pub denoise_db: f64,
    /// Разметка речи Silero VAD: по ней длинная запись режется на куски для локального распознавателя.
    pub vad: crate::vad::VadConfig,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { engine: String::new(), cloud_model: String::new(), lang: "auto".into(), speakers: 0, separate: false, filter: true, prompt: String::new(), denoise: false, denoise_db: DENOISE_DB, vad: crate::vad::VadConfig::default() }
    }
}

/// Предел шумоподавления по умолчанию: речь остаётся живой, фон уходит заметно.
pub const DENOISE_DB: f64 = 18.0;

/// Подсказка Whisper: текст из настроек и, через запятую, верные написания из исправлений (без повторов).
pub fn whisper_prompt(project: &ListenProject) -> Option<String> {
    let mut terms: Vec<&str> = Vec::new();
    for f in &project.fixes {
        let say = f.say.trim();
        if !say.is_empty() && !terms.contains(&say) {
            terms.push(say);
        }
    }
    let text = project.settings.prompt.trim();
    let prompt = match (text.is_empty(), terms.is_empty()) {
        (_, true) => text.to_string(),
        (true, false) => terms.join(", "),
        (false, false) => format!("{text} {}", terms.join(", ")),
    };
    (!prompt.is_empty()).then_some(prompt)
}

/// «Как распознаватель коверкает термин» → термин; применяется к словам после распознавания.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Fix {
    pub heard: String,
    pub say: String,
}

/// Чем получен транскрипт.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Made {
    /// `parakeet`, `whisper:<модель>`, `openrouter:<модель>`, `subs`.
    pub by: String,
    pub at: u64,
    pub lang: String,
    pub separated: bool,
    pub hidden: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct ListenProject {
    pub kind: String,
    pub version: u32,
    pub name: String,
    pub created: u64,
    /// Файл записи в папке проекта.
    pub source: String,
    /// Откуда запись: имя загруженного файла, ссылка или «микрофон».
    pub origin: String,
    pub duration: f64,
    pub settings: Settings,
    /// Ключ спикера → имя.
    pub speakers: BTreeMap<String, String>,
    pub lines: Vec<Line>,
    pub fixes: Vec<Fix>,
    /// Эталонный текст для проверки озвучки.
    pub reference: String,
    pub made: Option<Made>,
    /// Ключ инструмента агента «одним вызовом» (файл и его размер и время): повторный вызов находит этот проект.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key: String,
}

impl ListenProject {
    pub fn new(name: &str, source: &str, origin: &str) -> Self {
        ListenProject { kind: KIND.into(), version: 1, name: name.to_string(), created: crate::speech_project::now(), source: source.to_string(), origin: origin.to_string(), ..Default::default() }
    }

    pub fn validate(&self) -> Result<(), String> {
        let (s, v) = (&self.settings, &self.settings.vad);
        if !(0.05..=0.95).contains(&v.threshold) || !(0.0..=0.5).contains(&v.hysteresis) {
            return Err(format!("speech threshold {} and hysteresis {} must be within 0.05..0.95 and 0..0.5", v.threshold, v.hysteresis));
        }
        if v.min_speech_ms > 5000 || v.min_silence_ms > 5000 || v.pad_ms > 1000 {
            return Err(format!("speech {} ms, silence {} ms, padding {} ms: at most 5000, 5000, 1000", v.min_speech_ms, v.min_silence_ms, v.pad_ms));
        }
        if !(0.0..=100.0).contains(&s.denoise_db) {
            return Err(format!("noise suppression limit {} dB is outside 0..100", s.denoise_db));
        }
        if self.kind != KIND {
            return Err(format!("kind {} is not {KIND}", self.kind));
        }
        if self.settings.speakers > MAX_SPEAKERS {
            return Err(format!("speakers {} is outside 0..{MAX_SPEAKERS}", self.settings.speakers));
        }
        if !matches!(self.settings.engine.as_str(), "" | "parakeet" | "whisper" | "openrouter" | "fal") {
            return Err(format!("engine {} is not one of parakeet, whisper, openrouter, fal", self.settings.engine));
        }
        if self.source.contains(['/', '\\']) || self.source.contains("..") {
            return Err(format!("source {} must be a file name in the project folder", self.source));
        }
        for l in &self.lines {
            if !(l.start.is_finite() && l.end.is_finite() && l.start >= 0.0 && l.end >= l.start) {
                return Err(format!("line {} has bad times {}..{}", l.id, l.start, l.end));
            }
        }
        Ok(())
    }

    /// Имя спикера для показа и экспорта.
    pub fn speaker_name(&self, key: &str) -> String {
        self.speakers.get(key).cloned().unwrap_or_else(|| key.to_string())
    }

    /// Строки, которые идут в экспорт и в Speech.
    pub fn shown(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter().filter(|l| !l.hidden && !l.text.trim().is_empty())
    }

    /// Who speaks how much, over the lines that are shown: each speaker's talk time and its share of all talk, lines,
    /// words, pace in words a minute and longest stretch without another speaker, the most talkative first.
    pub fn speaker_stats(&self) -> serde_json::Value {
        #[derive(Default)]
        struct Tally {
            secs: f64,
            lines: usize,
            words: usize,
            longest: f64,
        }
        let mut tally: BTreeMap<String, Tally> = BTreeMap::new();
        let mut run: Option<(String, f64, f64)> = None;
        let close = |run: &Option<(String, f64, f64)>, tally: &mut BTreeMap<String, Tally>| {
            if let Some((who, a, z)) = run {
                let t = tally.entry(who.clone()).or_default();
                t.longest = t.longest.max(z - a);
            }
        };
        for l in self.shown() {
            let t = tally.entry(l.speaker.clone()).or_default();
            t.secs += (l.end - l.start).max(0.0);
            t.lines += 1;
            t.words += l.text.split_whitespace().count();
            match &mut run {
                Some((who, _, z)) if *who == l.speaker => *z = l.end,
                _ => {
                    close(&run, &mut tally);
                    run = Some((l.speaker.clone(), l.start, l.end));
                }
            }
        }
        close(&run, &mut tally);
        let talk: f64 = tally.values().map(|t| t.secs).sum();
        let mut rows: Vec<serde_json::Value> = tally
            .iter()
            .map(|(key, t)| {
                serde_json::json!({
                    "speaker": key, "name": self.speaker_name(key), "seconds": (t.secs * 10.0).round() / 10.0,
                    "share": if talk > 0.0 { (t.secs / talk * 1000.0).round() / 10.0 } else { 0.0 },
                    "lines": t.lines, "words": t.words,
                    "wpm": if t.secs > 0.0 { (t.words as f64 / (t.secs / 60.0)).round() } else { 0.0 },
                    "longest": (t.longest * 10.0).round() / 10.0,
                })
            })
            .collect();
        rows.sort_by(|a, b| b["seconds"].as_f64().partial_cmp(&a["seconds"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
        serde_json::json!({ "talk_seconds": (talk * 10.0).round() / 10.0, "duration": self.duration, "speakers": rows })
    }

    /// Отрезки записи, отмеченные к удалению. Подряд идущие вырезанные слова сливаются вместе с тишиной между ними;
    /// граница отрезка — середина паузы до соседнего оставленного слова: метки слов у распознавателя неточны на
    /// десятки миллисекунд, а рез по середине паузы не оставляет хвостов и не съедает соседей.
    pub fn cut_ranges(&self) -> Vec<(f64, f64)> {
        let mut words: Vec<&Word> = self.lines.iter().filter(|l| !l.hidden).flat_map(|l| l.words.iter()).collect();
        words.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut out: Vec<(f64, f64)> = Vec::new();
        let mut i = 0;
        while i < words.len() {
            if !words[i].cut {
                i += 1;
                continue;
            }
            let first = i;
            while i + 1 < words.len() && words[i + 1].cut {
                i += 1;
            }
            let (a, b) = (words[first].start, words[i].end);
            let a = if first > 0 { (words[first - 1].end.min(a) + a) / 2.0 } else { a };
            let b = if i + 1 < words.len() { (b + words[i + 1].start.max(b)) / 2.0 } else { b };
            out.push((a, b));
            i += 1;
        }
        merge_ranges(out)
    }
}

pub fn merge_ranges(mut ranges: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    ranges.retain(|(a, b)| b > a);
    ranges.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (a, b) in ranges {
        match out.last_mut() {
            Some(last) if a <= last.1 + 0.02 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// Что остаётся после вырезания `cuts` из записи длиной `total`.
pub fn keep_ranges(cuts: &[(f64, f64)], total: f64) -> Vec<(f64, f64)> {
    let mut keep = Vec::new();
    let mut at = 0.0;
    for &(a, b) in cuts {
        if a > at {
            keep.push((at, a.min(total)));
        }
        at = at.max(b);
    }
    if total > at {
        keep.push((at, total));
    }
    keep.retain(|(a, b)| b - a > 0.001);
    keep
}

/// Время исходной записи → время смонтированной (без вырезанного); внутри вырезанного — его начало.
pub fn remap(t: f64, cuts: &[(f64, f64)]) -> f64 {
    let mut removed = 0.0;
    for &(a, b) in cuts {
        if t >= b {
            removed += b - a;
        } else if t > a {
            return a - removed;
        } else {
            break;
        }
    }
    t - removed
}

pub fn is_media(name: &str) -> bool {
    Path::new(name).extension().and_then(|e| e.to_str()).is_some_and(|e| MEDIA.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn dir(workspace: &Path, pid: &str) -> PathBuf {
    workspace.join(pid)
}

pub fn load(workspace: &Path, pid: &str) -> Result<ListenProject, String> {
    if !crate::speech_project::is_pid(pid) {
        return Err(format!("bad project id {pid}"));
    }
    let p = crate::speech_project::path(workspace, pid);
    let text = std::fs::read_to_string(&p).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound { t!("project-missing", pid = pid.to_string()) } else { format!("{}: {e}", p.display()) }
    })?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
    if v.get("kind").and_then(Value::as_str) != Some(KIND) {
        return Err(format!("project {pid} is not a listening project"));
    }
    serde_json::from_value(v).map_err(|e| format!("{}: {e}", p.display()))
}

pub fn save(workspace: &Path, pid: &str, project: &ListenProject) -> Result<(), String> {
    project.validate()?;
    let dir = dir(workspace, pid);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    crate::mcp::save_with_revision(&dir, || crate::voice_library::write_atomic(&dir.join("project.json"), text.as_bytes()))
}

/// Новый проект вокруг файла, уже лежащего в его папке; длительность — по ffprobe. Субтитры площадки рядом
/// (`import_subs.srt`) сразу становятся транскриптом с приблизительными словами.
pub fn create_in(workspace: &Path, pid: &str, name: &str, source: &str, origin: &str) -> Result<ListenProject, String> {
    let d = dir(workspace, pid);
    let mut project = ListenProject::new(name, source, origin);
    project.duration = crate::media::probe(&d.join(source)).map(|m| m.duration).unwrap_or(0.0);
    if let Ok(text) = std::fs::read_to_string(d.join("import_subs.srt")) {
        let cues = crate::subimport::parse(&text, "srt");
        if !cues.is_empty() {
            project.lines = from_cues(&cues, &mut project.speakers);
            project.made = Some(Made { by: "subs".into(), at: crate::speech_project::now(), ..Default::default() });
        }
    }
    save(workspace, pid, &project)?;
    Ok(project)
}

/// Строки из субтитров: спикеры из меток субтитров, иначе один; слова — по длине внутри реплики.
pub fn from_cues(cues: &[crate::subimport::Cue], speakers: &mut BTreeMap<String, String>) -> Vec<Line> {
    cues.iter()
        .enumerate()
        .map(|(i, c)| {
            let name = c.speaker.clone().unwrap_or_default();
            let key = match speakers.iter().find(|(_, v)| **v == name && !name.is_empty()) {
                Some((k, _)) => k.clone(),
                None if name.is_empty() && speakers.contains_key("0") => "0".to_string(),
                None => {
                    let k = speakers.len().to_string();
                    speakers.insert(k.clone(), if name.is_empty() { t!("listen-speaker-n", n = (speakers.len() + 1) as i64) } else { name });
                    k
                }
            };
            Line { id: format!("l{i}"), start: c.start, end: c.end, speaker: key, words: spread_words(&c.text, c.start, c.end), text: c.text.clone(), approx: true, ..Default::default() }
        })
        .collect()
}

/// Слова фразы с таймкодами пропорционально числу букв — когда распознаватель дал только фразу.
pub fn spread_words(text: &str, start: f64, end: f64) -> Vec<Word> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let weight = |w: &str| w.chars().filter(|c| c.is_alphanumeric()).count().max(1) as f64;
    let total: f64 = words.iter().map(|w| weight(w)).sum();
    let span = (end - start).max(0.0);
    let mut at = start;
    words
        .iter()
        .map(|w| {
            let len = if total > 0.0 { span * weight(w) / total } else { 0.0 };
            let word = Word { text: (*w).to_string(), start: at, end: at + len, cut: false };
            at += len;
            word
        })
        .collect()
}

/// Файл записи проекта.
pub fn source_path(workspace: &Path, pid: &str, project: &ListenProject) -> Result<PathBuf, String> {
    let p = dir(workspace, pid).join(&project.source);
    if project.source.is_empty() || !p.is_file() {
        return Err(t!("listen-no-source"));
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whisper_prompt_joins_the_hint_and_the_corrected_spellings_once() {
        let mut p = ListenProject::default();
        assert_eq!(whisper_prompt(&p), None);
        p.fixes = vec![Fix { heard: "гит хаб".into(), say: "GitHub".into() }, Fix { heard: "гитхаб".into(), say: "GitHub".into() }, Fix { heard: "ку вен".into(), say: "Qwen".into() }];
        assert_eq!(whisper_prompt(&p).as_deref(), Some("GitHub, Qwen"));
        p.settings.prompt = " Подкаст о нейросетях. ".into();
        assert_eq!(whisper_prompt(&p).as_deref(), Some("Подкаст о нейросетях. GitHub, Qwen"));
    }

    fn w(text: &str, start: f64, end: f64, cut: bool) -> Word {
        Word { text: text.into(), start, end, cut }
    }

    #[test]
    fn cut_words_merge_into_ranges_and_times_shift_after_them() {
        let mut p = ListenProject::new("t", "source.wav", "");
        p.lines.push(Line { words: vec![w("a", 0.0, 0.5, false), w("b", 0.6, 1.0, true), w("c", 1.1, 1.5, true), w("d", 1.6, 2.0, false)], ..Default::default() });
        p.lines.push(Line { words: vec![w("e", 3.0, 3.5, true)], ..Default::default() });
        let cuts = p.cut_ranges();
        let near = |x: f64, y: f64| (x - y).abs() < 1e-9;
        assert_eq!(cuts.len(), 2);
        assert!(near(cuts[0].0, 0.55) && near(cuts[0].1, 1.55), "cuts go to the middle of the pauses around: {cuts:?}");
        assert!(near(cuts[1].0, 2.5) && near(cuts[1].1, 3.5), "the last word has no pause after it: {cuts:?}");
        let keep = keep_ranges(&cuts, 5.0);
        assert!(keep.len() == 3 && near(keep[1].0, 1.55) && near(keep[2].1, 5.0), "{keep:?}");
        assert!(near(remap(1.6, &cuts), 0.6));
        assert!(near(remap(4.0, &cuts), 2.0));
        assert!(near(remap(1.0, &cuts), 0.55), "inside a cut lands at its start");
    }

    #[test]
    fn spread_words_cover_the_phrase_by_letters() {
        let words = spread_words("ab abcd", 1.0, 4.0);
        assert_eq!(words.len(), 2);
        assert!((words[0].end - 2.0).abs() < 1e-9 && (words[1].end - 4.0).abs() < 1e-9);
    }

    #[test]
    fn speaker_stats_count_talk_share_pace_and_the_longest_stretch() {
        let mut p = ListenProject::default();
        p.speakers.insert("0".into(), "Анна".into());
        let line = |who: &str, a: f64, z: f64, text: &str| Line { speaker: who.into(), start: a, end: z, text: text.into(), ..Default::default() };
        p.lines = vec![line("0", 0.0, 6.0, "раз два три четыре пять шесть"), line("0", 6.0, 12.0, "семь восемь девять десять"), line("1", 12.0, 15.0, "да да да"), line("0", 15.0, 18.0, "ну")];
        p.lines.push(Line { hidden: true, ..line("1", 18.0, 30.0, "скрытая строка не считается") });
        let s = p.speaker_stats();
        assert_eq!(s["talk_seconds"], 18.0);
        let anna = &s["speakers"][0];
        assert_eq!((anna["name"].as_str(), anna["seconds"].as_f64(), anna["lines"].as_u64(), anna["words"].as_u64()), (Some("Анна"), Some(15.0), Some(3), Some(11)));
        assert_eq!(anna["share"], 83.3);
        assert_eq!(anna["wpm"], 44.0);
        assert_eq!(anna["longest"], 12.0);
        assert_eq!(s["speakers"][1]["name"], "1");
    }
}
