//! Текст блока → текст для движка, слоями: словарь произношения (проект, затем глобальный) → нормализация
//! русского → ударения и «ё» → адаптер движка. Видимый текст не меняется; каждый слой пишет, что заменил, — из
//! этого окно строит трассировку, а синтез берёт итоговый текст.

use std::path::Path;

use serde::Serialize;

use crate::pron_dict::{self, Entry, Scope};
use crate::speech_text as stx;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Step {
    /// `dictionary`, `normalizer`, `stress`, `engine`.
    pub layer: &'static str,
    pub rule: String,
    pub from: String,
    pub to: String,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Prepared {
    /// Текст после словаря, нормализации и ударений — с разметкой синтеза; его режет на куски `pieces`.
    pub text: String,
    /// Что увидит движок (адаптер применён к каждому куску, паузы — пробелом).
    pub engine_text: String,
    pub steps: Vec<Step>,
    pub unknown: Vec<String>,
    pub ignored: stx::Ignored,
    pub ignored_phonemes: Vec<String>,
    pub broken: Vec<(String, String)>,
}

pub struct Opts<'a> {
    pub lang: &'a str,
    pub voice: Option<&'a str>,
    pub engine: &'a str,
    /// Записи сверху вниз: проект, затем глобальные.
    pub entries: &'a [Entry],
    pub normalize: bool,
    pub stress: bool,
    pub models_root: &'a Path,
}

/// Язык блока для правил: язык проекта или, при `auto`, русский по кириллице.
pub fn block_lang(text: &str, lang: &str) -> String {
    if lang == "auto" || lang.is_empty() {
        if crate::stress::is_russian(text, "auto") { "ru".into() } else { String::new() }
    } else {
        lang.to_string()
    }
}

pub fn prepare(text: &str, o: &Opts) -> Result<Prepared, String> {
    let lang = block_lang(text, o.lang);
    let mut p = Prepared::default();
    let scope = Scope { lang: &lang, voice: o.voice, engine: o.engine };
    let applied = pron_dict::apply(text, o.entries, &scope, false);
    p.steps.extend(applied.hits.iter().map(|h| Step { layer: "dictionary", rule: h.id.clone(), from: h.from.clone(), to: h.to.clone() }));
    p.ignored_phonemes = applied.ignored;
    p.broken = applied.broken;
    let mut t = applied.text;
    let russian = lang == "ru";
    if russian && o.normalize {
        let n = crate::normalize_ru::normalize(&t);
        p.steps.extend(n.changes.into_iter().map(|c| Step { layer: "normalizer", rule: c.rule.to_string(), from: c.from, to: c.to }));
        p.unknown = n.unknown;
        t = n.text;
    }
    if russian && o.stress {
        let s = crate::stress::shared(o.models_root)?.annotate(&t)?;
        for w in s.words.iter().filter(|w| matches!(w.source, "omograph" | "yo" | "model") && w.word.contains(['+', 'ё', 'Ё'])) {
            p.steps.push(Step { layer: "stress", rule: w.source.to_string(), from: w.word.replace('+', ""), to: w.word.clone() });
        }
        t = s.text;
    }
    let mut engine = Vec::new();
    for piece in stx::pieces(&t) {
        if let stx::Piece::Say(say) = piece {
            let (out, ig) = stx::for_engine(&say, &lang, o.engine);
            p.ignored.accent |= ig.accent;
            p.ignored.phonemes |= ig.phonemes;
            p.ignored.unknown_tags.extend(ig.unknown_tags);
            engine.push(out);
        }
    }
    p.ignored.stress = stx::has_stress(text);
    p.engine_text = engine.join(" ");
    p.text = t;
    Ok(p)
}

/// Отпечаток того, что подготовка сделает с блоком помимо самого текста: записи словаря, которые в нём срабатывают,
/// и включённые слои. Пустой — подготовка текст не трогает (ключ дубля остаётся прежним).
pub fn fingerprint(text: &str, o: &Opts, compiled: &[(Entry, fancy_regex::Regex)]) -> String {
    let lang = block_lang(text, o.lang);
    let scope = Scope { lang: &lang, voice: o.voice, engine: o.engine };
    let mut h = blake3::Hasher::new();
    let mut any = false;
    for (e, re) in compiled.iter().filter(|(e, _)| e.applies(&scope)) {
        if re.is_match(text).unwrap_or(false) {
            any = true;
            for part in [&e.kind, &e.find, &e.say] {
                h.update(part.as_bytes());
                h.update(b"\x1f");
            }
            h.update(&[e.case_sensitive as u8, e.whole_word as u8]);
        }
    }
    if lang == "ru" && o.normalize && text.chars().any(|c| c.is_ascii_digit() || matches!(c, '$' | '€' | '£' | '₽' | '%')) {
        any = true;
        h.update(b"normalize-v1");
    }
    if any { h.finalize().to_hex()[..12].to_string() } else { String::new() }
}

pub fn compile_all(entries: &[Entry]) -> Vec<(Entry, fancy_regex::Regex)> {
    entries.iter().filter_map(|e| pron_dict::compile(e).ok().map(|re| (e.clone(), re))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts<'a>(entries: &'a [Entry]) -> Opts<'a> {
        Opts { lang: "auto", voice: None, engine: "higgs", entries, normalize: true, stress: false, models_root: Path::new(".") }
    }

    #[test]
    fn dictionary_then_normalizer_and_each_layer_is_traced() {
        let entries = [Entry { id: "e1".into(), find: "ИИ".into(), say: "искусственный интеллект".into(), case_sensitive: true, ..Default::default() }];
        let p = prepare("ИИ за 5 минут [pause 300ms] прочтёт 2 книги.", &opts(&entries)).unwrap();
        assert_eq!(p.text, "искусственный интеллект за пять минут [pause 300ms] прочтёт две книги.");
        let layers: Vec<&str> = p.steps.iter().map(|s| s.layer).collect();
        assert_eq!(layers, vec!["dictionary", "normalizer", "normalizer"]);
        assert_eq!(p.engine_text, "искусственный интеллект за пять минут прочтёт две книги.");
    }

    #[test]
    fn english_text_is_left_to_the_engine() {
        let p = prepare("I have 5 apples.", &opts(&[])).unwrap();
        assert_eq!(p.text, "I have 5 apples.");
    }

    #[test]
    fn the_fingerprint_changes_only_when_preparation_touches_the_block() {
        let entries = [Entry { id: "e1".into(), find: "кот".into(), say: "котик".into(), ..Default::default() }];
        let compiled = compile_all(&entries);
        assert_eq!(fingerprint("Привет, мир", &opts(&entries), &compiled), "");
        assert_ne!(fingerprint("Привет, кот", &opts(&entries), &compiled), "");
        assert_ne!(fingerprint("Привет, 5 мир", &opts(&entries), &compiled), "");
    }
}
