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
    /// `""` — распознаватель из настроек студии; `parakeet`, `whisper`, `openrouter`.
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
}

impl Default for Settings {
    fn default() -> Self {
        Settings { engine: String::new(), cloud_model: String::new(), lang: "auto".into(), speakers: 0, separate: false, filter: true }
    }
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
}

impl ListenProject {
    pub fn new(name: &str, source: &str, origin: &str) -> Self {
        ListenProject { kind: KIND.into(), version: 1, name: name.to_string(), created: crate::speech_project::now(), source: source.to_string(), origin: origin.to_string(), ..Default::default() }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.kind != KIND {
            return Err(format!("kind {} is not {KIND}", self.kind));
        }
        if self.settings.speakers > MAX_SPEAKERS {
            return Err(format!("speakers {} is outside 0..{MAX_SPEAKERS}", self.settings.speakers));
        }
        if !matches!(self.settings.engine.as_str(), "" | "parakeet" | "whisper" | "openrouter") {
            return Err(format!("engine {} is not one of parakeet, whisper, openrouter", self.settings.engine));
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

    /// Отрезки записи, отмеченные к удалению: подряд идущие вырезанные слова одной строки сливаются вместе с тишиной
    /// между ними, соседние отрезки — тоже.
    pub fn cut_ranges(&self) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = Vec::new();
        for l in &self.lines {
            let mut run: Option<(f64, f64)> = None;
            for w in &l.words {
                if w.cut {
                    run = Some(run.map_or((w.start, w.end), |(a, _)| (a, w.end)));
                } else if let Some(r) = run.take() {
                    out.push(r);
                }
            }
            out.extend(run);
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
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
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

    fn w(text: &str, start: f64, end: f64, cut: bool) -> Word {
        Word { text: text.into(), start, end, cut }
    }

    #[test]
    fn cut_words_merge_into_ranges_and_times_shift_after_them() {
        let mut p = ListenProject::new("t", "source.wav", "");
        p.lines.push(Line { words: vec![w("a", 0.0, 0.5, false), w("b", 0.6, 1.0, true), w("c", 1.1, 1.5, true), w("d", 1.6, 2.0, false)], ..Default::default() });
        p.lines.push(Line { words: vec![w("e", 3.0, 3.5, true)], ..Default::default() });
        let cuts = p.cut_ranges();
        assert_eq!(cuts, vec![(0.6, 1.5), (3.0, 3.5)]);
        assert_eq!(keep_ranges(&cuts, 5.0), vec![(0.0, 0.6), (1.5, 3.0), (3.5, 5.0)]);
        assert!((remap(1.6, &cuts) - 0.7).abs() < 1e-9);
        assert!((remap(4.0, &cuts) - 2.6).abs() < 1e-9);
        assert!((remap(1.0, &cuts) - 0.6).abs() < 1e-9, "inside a cut lands at its start");
    }

    #[test]
    fn spread_words_cover_the_phrase_by_letters() {
        let words = spread_words("ab abcd", 1.0, 4.0);
        assert_eq!(words.len(), 2);
        assert!((words[0].end - 2.0).abs() < 1e-9 && (words[1].end - 4.0).abs() < 1e-9);
    }
}
