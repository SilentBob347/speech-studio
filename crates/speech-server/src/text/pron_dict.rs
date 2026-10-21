//! Словарь произношения: замены, которые меняют только текст для синтеза. Записи глобальные (файл студии) и
//! проекта; у каждой — скоуп (язык, голос, движок), тип (`word` — слово или фраза с `*` по краям, как DIC
//! Balabolka; `regex` — регулярное выражение с `$N` и операторами регистра `\U \L \E \u \l`, как REX; `phonemes` —
//! фонемы, где движок их принимает), учёт регистра и границ слова. Порядок явный: запись выше срабатывает первой,
//! и то, что она заменила, нижние уже не трогают. Импорт словарей Balabolka `.dic`, `.rex`, `.bxd`.

use std::path::{Path, PathBuf};

use fancy_regex::{Captures, Regex};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Entry {
    pub id: String,
    pub enabled: bool,
    /// `word`, `regex` или `phonemes`.
    pub kind: String,
    pub find: String,
    pub say: String,
    pub case_sensitive: bool,
    /// Для `word`: только целым словом (по краям — не буквы и не цифры).
    pub whole_word: bool,
    /// Пусто — любой.
    pub lang: String,
    pub voice: String,
    pub engine: String,
    pub note: String,
    /// `manual` или имя импортированного файла.
    pub source: String,
    /// Проверено ли правило на слух: пусто — нет, `approved` — звучит верно, `fix` — исправить.
    pub review: String,
}

impl Default for Entry {
    fn default() -> Self {
        Entry {
            id: String::new(),
            enabled: true,
            kind: "word".into(),
            find: String::new(),
            say: String::new(),
            case_sensitive: false,
            whole_word: true,
            lang: String::new(),
            voice: String::new(),
            engine: String::new(),
            note: String::new(),
            source: "manual".into(),
            review: String::new(),
        }
    }
}

pub const KINDS: [&str; 3] = ["word", "regex", "phonemes"];

/// Где применяется подготовка текста: язык блока, его голос и движок.
#[derive(Clone, Copy, Debug)]
pub struct Scope<'a> {
    pub lang: &'a str,
    pub voice: Option<&'a str>,
    pub engine: &'a str,
}

impl Entry {
    pub fn applies(&self, scope: &Scope) -> bool {
        self.enabled
            && !self.find.is_empty()
            && (self.lang.is_empty() || self.lang.eq_ignore_ascii_case(scope.lang))
            && (self.voice.is_empty() || scope.voice == Some(self.voice.as_str()))
            && (self.engine.is_empty() || self.engine == scope.engine)
    }
}

/// Шаблон `word` в регулярное выражение: `*` по краю — сколько угодно букв, удвоенный спецсимвол — сам символ.
fn word_pattern(find: &str) -> (String, bool, bool) {
    let mut body = String::new();
    let chars: Vec<char> = find.chars().collect();
    let (mut lead, mut trail) = (false, false);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if "*#=$".contains(c) && chars.get(i + 1) == Some(&c) {
            body.push_str(&fancy_regex::escape(&c.to_string()));
            i += 2;
            continue;
        }
        if c == '*' {
            if i == 0 {
                lead = true;
            } else if i == chars.len() - 1 {
                trail = true;
            } else {
                body.push_str(r"[\p{L}\p{N}]*");
            }
            i += 1;
            continue;
        }
        body.push_str(&fancy_regex::escape(&c.to_string()));
        i += 1;
    }
    (body, lead, trail)
}

pub fn compile(e: &Entry) -> Result<Regex, String> {
    let flags = if e.case_sensitive { "" } else { "(?i)" };
    let pattern = match e.kind.as_str() {
        "regex" => format!("{flags}{}", e.find),
        _ => {
            let (body, lead, trail) = word_pattern(&e.find);
            let head = if lead { r"([\p{L}\p{N}]*)" } else { "()" };
            let tail = if trail { r"([\p{L}\p{N}]*)" } else { "()" };
            let edge = if e.whole_word { (r"(?<![\p{L}\p{N}])", r"(?![\p{L}\p{N}])") } else { ("", "") };
            format!("{flags}{}{head}{body}{tail}{}", edge.0, edge.1)
        }
    };
    Regex::new(&pattern).map_err(|err| format!("{}: {err}", e.find))
}

/// Подстановка REX: `$N` и `\N` — группы, `\U…\E`, `\L…\E` — регистр до конца или до `\E`, `\u`, `\l` — следующая буква.
pub fn expand(caps: &Captures<'_, str>, template: &str) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        None,
        Upper,
        Lower,
    }
    let mut out = String::new();
    let mut mode = Mode::None;
    let mut next: Option<bool> = None;
    let mut push = |s: &str, out: &mut String, mode: Mode, next: &mut Option<bool>| {
        for ch in s.chars() {
            let ch: String = match (next.take(), mode) {
                (Some(true), _) => ch.to_uppercase().collect(),
                (Some(false), _) => ch.to_lowercase().collect(),
                (None, Mode::Upper) => ch.to_uppercase().collect(),
                (None, Mode::Lower) => ch.to_lowercase().collect(),
                (None, Mode::None) => ch.to_string(),
            };
            out.push_str(&ch);
        }
    };
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let digit_at = |k: usize| chars.get(k).and_then(|d| d.to_digit(10));
        if (c == '$' || c == '\\') && digit_at(i + 1).is_some() {
            let mut n = 0usize;
            let mut k = i + 1;
            while let Some(d) = digit_at(k) {
                n = n * 10 + d as usize;
                k += 1;
            }
            let group = caps.get(n).map(|m| m.as_str()).unwrap_or("");
            push(group, &mut out, mode, &mut next);
            i = k;
            continue;
        }
        if c == '\\' {
            match chars.get(i + 1) {
                Some('U') => mode = Mode::Upper,
                Some('L') => mode = Mode::Lower,
                Some('E') => mode = Mode::None,
                Some('u') => next = Some(true),
                Some('l') => next = Some(false),
                Some(other) => push(&other.to_string(), &mut out, mode, &mut next),
                None => push("\\", &mut out, mode, &mut next),
            }
            i += 2;
            continue;
        }
        push(&c.to_string(), &mut out, mode, &mut next);
        i += 1;
    }
    out
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: String,
    pub from: String,
    pub to: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Applied {
    pub text: String,
    pub hits: Vec<Hit>,
    /// Записи-фонемы, которые движок не принимает: что не сделано.
    pub ignored: Vec<String>,
    /// Записи с ошибкой в шаблоне.
    pub broken: Vec<(String, String)>,
}

/// Куски текста: свободные (их можно менять) и занятые (разметка синтеза и уже заменённое).
fn segments(text: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut last = 0;
    for m in super::stress::MARKUP.find_iter(text) {
        if m.start() > last {
            out.push((text[last..m.start()].to_string(), true));
        }
        out.push((m.as_str().to_string(), false));
        last = m.end();
    }
    if last < text.len() {
        out.push((text[last..].to_string(), true));
    }
    out
}

/// Применить записи сверху вниз; `phonemes_ok` — движок принимает фонемы `[[…]]`.
pub fn apply(text: &str, entries: &[Entry], scope: &Scope, phonemes_ok: bool) -> Applied {
    let mut segs = segments(text);
    let mut res = Applied::default();
    for e in entries.iter().filter(|e| e.applies(scope)) {
        if e.kind == "phonemes" && !phonemes_ok {
            res.ignored.push(e.find.clone());
            continue;
        }
        let re = match compile(e) {
            Ok(re) => re,
            Err(err) => {
                res.broken.push((e.id.clone(), err));
                continue;
            }
        };
        let mut next: Vec<(String, bool)> = Vec::new();
        for (seg, free) in segs {
            if !free {
                next.push((seg, false));
                continue;
            }
            let mut last = 0;
            for caps in re.captures_iter(seg.as_str()).flatten() {
                let m = caps.get(0).expect("whole");
                if m.as_str().is_empty() {
                    continue;
                }
                let to = match e.kind.as_str() {
                    "regex" => expand(&caps, &e.say),
                    "phonemes" => format!("[[{}]]", e.say),
                    _ => format!(
                        "{}{}{}",
                        caps.get(1).map(|g| g.as_str()).unwrap_or(""),
                        e.say,
                        caps.get(2).map(|g| g.as_str()).unwrap_or("")
                    ),
                };
                if m.start() > last {
                    next.push((seg[last..m.start()].to_string(), true));
                }
                res.hits.push(Hit { id: e.id.clone(), from: m.as_str().to_string(), to: to.clone() });
                next.push((to, false));
                last = m.end();
            }
            if last < seg.len() {
                next.push((seg[last..].to_string(), true));
            }
        }
        segs = next;
    }
    res.text = segs.into_iter().map(|(s, _)| s).collect();
    res
}

/// Сколько раз каждая запись сработала бы в тексте (без учёта порядка) — счётчик в окне словаря.
pub fn counts(text: &str, entries: &[Entry]) -> Vec<(String, usize)> {
    entries
        .iter()
        .map(|e| {
            let n = compile(e).map(|re| re.find_iter(text).flatten().filter(|m| !m.as_str().is_empty()).count()).unwrap_or(0);
            (e.id.clone(), n)
        })
        .collect()
}

pub fn global_path(repo_root: &Path) -> PathBuf {
    repo_root.join("dictionaries").join("pronunciation.json")
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct File {
    version: u32,
    entries: Vec<Entry>,
}

pub fn load_global(repo_root: &Path) -> Vec<Entry> {
    std::fs::read_to_string(global_path(repo_root)).ok().and_then(|t| serde_json::from_str::<File>(&t).ok()).map(|f| f.entries).unwrap_or_default()
}

pub fn save_global(repo_root: &Path, entries: &[Entry]) -> Result<(), String> {
    validate(entries)?;
    let path = global_path(repo_root);
    std::fs::create_dir_all(path.parent().expect("a folder")).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&File { version: 1, entries: entries.to_vec() }).map_err(|e| e.to_string())?;
    crate::platform::fs::write_atomic(&path, text.as_bytes())
}

/// Записи без пустого `find`, известного типа, с собирающимся шаблоном и уникальным id.
pub fn validate(entries: &[Entry]) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    for e in entries {
        if e.id.is_empty() || !ids.insert(e.id.as_str()) {
            return Err(t!("dict-bad-id", id = e.id.clone()));
        }
        if !KINDS.contains(&e.kind.as_str()) {
            return Err(t!("dict-bad-kind", kind = e.kind.clone()));
        }
        if e.find.trim().is_empty() {
            return Err(t!("dict-empty-find"));
        }
        if !["", "approved", "fix"].contains(&e.review.as_str()) {
            return Err(format!("review {} is not one of approved, fix or empty", e.review));
        }
        compile(e).map_err(|err| t!("dict-bad-pattern", detail = err))?;
    }
    Ok(())
}

pub fn new_id() -> String {
    crate::speech_project::new_id()
}

/// Импорт словаря Balabolka по расширению: `.dic` (шаблоны), `.rex` (регулярные выражения), `.bxd` (оба вида).
pub fn import_balabolka(file_name: &str, bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let text = decode(bytes);
    let ext = Path::new(file_name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let source = file_name.to_string();
    let entries = match ext.as_str() {
        "dic" => text.lines().filter_map(|l| dic_line(l, &source)).collect::<Vec<_>>(),
        "rex" => text.lines().filter_map(|l| rex_line(l, &source)).collect(),
        "bxd" => bxd(&text, &source),
        _ => return Err(t!("dict-import-unknown", name = file_name.to_string())),
    };
    let mut entries = entries;
    if ext == "dic" {
        entries.sort_by(|a, b| b.case_sensitive.cmp(&a.case_sensitive).then(b.find.chars().count().cmp(&a.find.chars().count())));
    }
    Ok(entries.into_iter().filter(|e| compile(e).is_ok()).collect())
}

fn decode(bytes: &[u8]) -> String {
    let b = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if bytes.starts_with(b"\xFF\xFE") {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| cp1251(c)).collect(),
    }
}

/// Байт кодировки Windows-1251 (старые словари Balabolka) в символ.
fn cp1251(c: u8) -> char {
    match c {
        0x00..=0x7F => c as char,
        0xC0..=0xFF => char::from_u32(0x0410 + (c as u32 - 0xC0)).unwrap_or('?'),
        0xA8 => 'Ё',
        0xB8 => 'ё',
        0xB9 => '№',
        0x96 => '–',
        0x97 => '—',
        0xAB => '«',
        0xBB => '»',
        0xA0 => '\u{a0}',
        _ => '?',
    }
}

/// Левая часть до первого неэкранированного `=`; в DIC `==` — сам знак.
fn split_rule(line: &str, doubled_escape: bool) -> Option<(String, String)> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut left = String::new();
    while i < chars.len() {
        let c = chars[i];
        if !doubled_escape && c == '\\' && chars.get(i + 1) == Some(&'=') {
            left.push_str("\\=");
            i += 2;
            continue;
        }
        if doubled_escape && c == '=' && chars.get(i + 1) == Some(&'=') {
            left.push_str("==");
            i += 2;
            continue;
        }
        if c == '=' {
            return Some((left, chars[i + 1..].iter().collect()));
        }
        left.push(c);
        i += 1;
    }
    None
}

fn dic_line(line: &str, source: &str) -> Option<Entry> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line.trim().is_empty() || (line.starts_with('#') && !line.starts_with("##")) {
        return None;
    }
    let (case_sensitive, body) = match line.strip_prefix('$') {
        Some(rest) if !line.starts_with("$$") => (true, rest),
        _ => (false, line),
    };
    let (find, say) = split_rule(body, true)?;
    Some(Entry { id: new_id(), kind: "word".into(), find: find.replace("==", "="), say, case_sensitive, whole_word: true, source: source.into(), ..Default::default() })
}

fn rex_line(line: &str, source: &str) -> Option<Entry> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line.trim().is_empty() || line.starts_with('#') {
        return None;
    }
    let (case_sensitive, body) = match line.strip_prefix('@') {
        Some(rest) => (true, rest),
        None => (false, line),
    };
    let (find, say) = split_rule(body, false)?;
    Some(Entry { id: new_id(), kind: "regex".into(), find: find.replace("\\=", "="), say, case_sensitive, whole_word: false, source: source.into(), ..Default::default() })
}

/// BXD: `Version=1`, `Delimiter=<символ>`, строка описания, дальше правила `id␃тип␃вкл␃…␃найти␃заменить␃заметка`;
/// тип 1 — шаблон DIC, 2 — регулярное выражение REX.
fn bxd(text: &str, source: &str) -> Vec<Entry> {
    let mut delim = '\u{3}';
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(d) = line.strip_prefix("Delimiter=") {
            delim = d.chars().next().unwrap_or(delim);
            continue;
        }
        let f: Vec<&str> = line.split(delim).collect();
        if f.len() < 9 || !matches!(f[1], "1" | "2") {
            continue;
        }
        let regex = f[1] == "2";
        let (find, say, note) = (f[f.len() - 3], f[f.len() - 2], f[f.len() - 1]);
        let (case_sensitive, find) = match (regex, find.strip_prefix(if regex { '@' } else { '$' })) {
            (_, Some(rest)) => (true, rest),
            (_, None) => (false, find),
        };
        out.push(Entry {
            id: new_id(),
            enabled: f[2] == "1",
            kind: if regex { "regex" } else { "word" }.into(),
            find: find.to_string(),
            say: say.to_string(),
            case_sensitive,
            whole_word: !regex,
            note: note.to_string(),
            source: source.into(),
            ..Default::default()
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(kind: &str, find: &str, say: &str) -> Entry {
        Entry { id: find.into(), kind: kind.into(), find: find.into(), say: say.into(), ..Default::default() }
    }

    const ANY: Scope = Scope { lang: "ru", voice: None, engine: "higgs" };

    #[test]
    fn words_are_whole_case_free_and_wildcards_keep_the_rest() {
        let r = apply("Авто и автобиография, но не автомат. АВТО!", &[e("word", "авто", "афто")], &ANY, false);
        assert_eq!(r.text, "афто и автобиография, но не автомат. афто!");
        let r = apply("автобиография", &[e("word", "авто*", "афто")], &ANY, false);
        assert_eq!(r.text, "афтобиография");
    }

    #[test]
    fn the_upper_entry_wins_and_markup_is_not_touched() {
        let entries = [e("word", "крыса", "мышь"), e("word", "мышь", "хомяк")];
        let r = apply("крыса <|emotion:awe|> мышь", &entries, &ANY, false);
        assert_eq!(r.text, "мышь <|emotion:awe|> хомяк");
        let r = apply("<|emotion:awe|>", &[e("regex", "awe", "x")], &ANY, false);
        assert_eq!(r.text, "<|emotion:awe|>");
    }

    #[test]
    fn regex_groups_and_case_operators_work_like_balabolka() {
        let r = apply("test и Привет мир", &[e("regex", "(test)", r"\U$1\E!"), e("regex", r"(привет) (мир)", r"\u$2, \l$1")], &ANY, false);
        assert_eq!(r.text, "TEST! и Мир, привет");
    }

    #[test]
    fn scope_and_phonemes_are_respected() {
        let mut only_anna = e("word", "кот", "котик");
        only_anna.voice = "Anna".into();
        let r = apply("кот", &[only_anna.clone()], &ANY, false);
        assert_eq!(r.text, "кот");
        let r = apply("кот", &[only_anna], &Scope { voice: Some("Anna"), ..ANY }, false);
        assert_eq!(r.text, "котик");
        let r = apply("кот", &[e("phonemes", "кот", "k o t")], &ANY, false);
        assert_eq!((r.text.as_str(), r.ignored.len()), ("кот", 1));
    }

    #[test]
    fn balabolka_files_import() {
        let dic = "# comment\r\n$DOT=Департамент\r\nавто*=афто\r\n**=звёздочка\r\n";
        let d = import_balabolka("my.dic", dic.as_bytes()).unwrap();
        assert_eq!(d.len(), 3);
        assert!(d[0].case_sensitive && d[0].find == "DOT");
        let rex = "#c\n@(\\d+)\\=(\\d+)=$1 равно $2\n(test)=\\U$1\\E\n";
        let r = import_balabolka("x.rex", rex.as_bytes()).unwrap();
        assert_eq!((r[0].find.as_str(), r[0].say.as_str(), r[0].case_sensitive), ("(\\d+)=(\\d+)", "$1 равно $2", true));
        let bxd = "\u{feff}Version=1\r\nDelimiter=\u{3}\r\nT\u{3}A\u{3}D\r\nID1\u{3}1\u{3}1\u{3}1\u{3}0\u{3}0\u{3}0\u{3}Xmas\u{3}Christmas\u{3}\r\nID2\u{3}2\u{3}1\u{3}0\u{3}1\u{3}0\u{3}0\u{3}(www\\.)?(\\w+)\\.com\u{3}$2\u{3}note\r\n";
        let b = import_balabolka("s.bxd", bxd.as_bytes()).unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].kind.as_str(), b[1].kind.as_str(), b[1].say.as_str()), ("word", "regex", "$2"));
        assert_eq!(import_balabolka("a.dic", &[0xCA, 0xCE, 0xD2, b'=', 0xEA, 0xEE, 0xF2]).unwrap()[0].find, "КОТ");
    }
}
