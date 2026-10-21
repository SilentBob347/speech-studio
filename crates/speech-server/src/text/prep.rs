//! Чистые шаги подготовки текста и трассировка замен. Вызывающий код передаёт правила,
//! результат расстановки ударений и формат адаптации; здесь нет загрузки моделей и выбора движка.

use serde::Serialize;

use super::adapter::{Adapter, Ignored};
use super::markup::{self, Piece};
use super::pron_dict::{self, Entry, Scope};
use super::stress::Stressed;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Step {
    /// `ssml`, `dictionary`, `normalizer`, `stress`, `engine`.
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
    pub ignored: Ignored,
    pub ignored_phonemes: Vec<String>,
    pub broken: Vec<(String, String)>,
}

pub struct Opts<'a> {
    pub lang: &'a str,
    pub voice: Option<&'a str>,
    /// Идентификатор только для области действия словаря, без обращения к движку.
    pub engine: &'a str,
    /// Записи сверху вниз: проект, затем глобальные.
    pub entries: &'a [Entry],
    pub normalize: bool,
    /// Группы правил нормализации, выключенные в проекте (`normalize_ru::GROUPS`).
    pub normalize_off: &'a [String],
}

pub(crate) struct Draft {
    pub prepared: Prepared,
    pub lang: String,
    /// Предупреждение об исходных метках не учитывает метки, добавленные словарём или моделью.
    pub manual_stress: bool,
}

/// Язык блока для правил: язык проекта или, при `auto`, русский по кириллице.
pub fn block_lang(text: &str, lang: &str) -> String {
    if lang == "auto" || lang.is_empty() {
        if super::stress::is_russian(text, "auto") {
            "ru".into()
        } else {
            String::new()
        }
    } else {
        lang.to_string()
    }
}

pub fn prepare(text: &str, o: &Opts) -> Draft {
    let lang = block_lang(text, o.lang);
    let mut p = Prepared::default();
    let scope = Scope {
        lang: &lang,
        voice: o.voice,
        engine: o.engine,
    };
    let (text, ssml) = super::ssml::to_markup(text);
    let manual_stress = markup::has_stress(&text);
    p.steps.extend(ssml.into_iter().map(|h| Step {
        layer: "ssml",
        rule: h.rule.to_string(),
        from: h.from,
        to: h.to,
    }));
    let applied = pron_dict::apply(&text, o.entries, &scope, false);
    p.steps.extend(applied.hits.iter().map(|h| Step {
        layer: "dictionary",
        rule: h.id.clone(),
        from: h.from.clone(),
        to: h.to.clone(),
    }));
    p.ignored_phonemes = applied.ignored;
    p.broken = applied.broken;
    let mut t = applied.text;
    let russian = lang == "ru";
    if russian && o.normalize {
        let n = super::normalize_ru::normalize_with(&t, o.normalize_off);
        p.steps.extend(n.changes.into_iter().map(|c| Step {
            layer: "normalizer",
            rule: c.rule.to_string(),
            from: c.from,
            to: c.to,
        }));
        p.unknown = n.unknown;
        t = n.text;
    }
    p.text = t;
    Draft {
        prepared: p,
        lang,
        manual_stress,
    }
}

impl Draft {
    pub(crate) fn apply_stress(&mut self, s: Stressed) {
        for w in s
            .words
            .iter()
            .filter(|w| matches!(w.source, "omograph" | "yo" | "model") && w.word.contains(['+', 'ё', 'Ё']))
        {
            self.prepared.steps.push(Step {
                layer: "stress",
                rule: w.source.to_string(),
                from: w.word.replace('+', ""),
                to: w.word.clone(),
            });
        }
        self.prepared.text = s.text;
    }

    pub(crate) fn finish(mut self, adapter: Adapter) -> Prepared {
        let p = &mut self.prepared;
        let mut engine = Vec::new();
        for piece in markup::pieces(&p.text) {
            if let Piece::Say(say) = piece {
                let (out, ig) = adapter.apply(&say, &self.lang);
                p.ignored.accent |= ig.accent;
                p.ignored.phonemes |= ig.phonemes;
                p.ignored.unknown_tags.extend(ig.unknown_tags);
                engine.push(out);
            }
        }
        p.engine_text = engine.join(" ");
        self.prepared
    }
}

/// Отпечаток того, что подготовка сделает с блоком помимо самого текста: записи словаря, которые в нём срабатывают,
/// и включённые слои. Пустой — подготовка текст не трогает (ключ дубля остаётся прежним).
pub fn fingerprint(text: &str, o: &Opts, compiled: &[(Entry, fancy_regex::Regex)], stress_model: Option<&str>) -> String {
    let lang = block_lang(text, o.lang);
    let scope = Scope {
        lang: &lang,
        voice: o.voice,
        engine: o.engine,
    };
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
    if let Some(model) = stress_model.filter(|_| lang == "ru") {
        any = true;
        h.update(b"stress-");
        h.update(model.as_bytes());
    }
    if any {
        h.finalize().to_hex()[..12].to_string()
    } else {
        String::new()
    }
}

pub fn compile_all(entries: &[Entry]) -> Vec<(Entry, fancy_regex::Regex)> {
    entries
        .iter()
        .filter_map(|e| pron_dict::compile(e).ok().map(|re| (e.clone(), re)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts<'a>(entries: &'a [Entry]) -> Opts<'a> {
        Opts {
            lang: "auto",
            voice: None,
            engine: "higgs",
            entries,
            normalize: true,
            normalize_off: &[],
        }
    }

    #[test]
    fn dictionary_then_normalizer_and_each_layer_is_traced() {
        let entries = [Entry {
            id: "e1".into(),
            find: "ИИ".into(),
            say: "искусственный интеллект".into(),
            case_sensitive: true,
            ..Default::default()
        }];
        let p = prepare("ИИ за 5 минут [pause 300ms] прочтёт 2 книги.", &opts(&entries)).finish(Adapter::Higgs);
        assert_eq!(p.text, "искусственный интеллект за пять минут [pause 300ms] прочтёт две книги.");
        let layers: Vec<&str> = p.steps.iter().map(|s| s.layer).collect();
        assert_eq!(layers, vec!["dictionary", "normalizer", "normalizer"]);
        assert_eq!(p.engine_text, "искусственный интеллект за пять минут прочтёт две книги.");
    }

    #[test]
    fn english_text_is_left_to_the_engine() {
        let p = prepare("I have 5 apples.", &opts(&[])).finish(Adapter::Higgs);
        assert_eq!(p.text, "I have 5 apples.");
    }

    #[test]
    fn the_fingerprint_changes_only_when_preparation_touches_the_block() {
        let entries = [Entry {
            id: "e1".into(),
            find: "кот".into(),
            say: "котик".into(),
            ..Default::default()
        }];
        let compiled = compile_all(&entries);
        assert_eq!(fingerprint("Привет, мир", &opts(&entries), &compiled, None), "");
        assert_ne!(fingerprint("Привет, кот", &opts(&entries), &compiled, None), "");
        assert_ne!(fingerprint("Привет, 5 мир", &opts(&entries), &compiled, None), "");
        assert_ne!(fingerprint("Привет, мир", &opts(&entries), &compiled, Some("tiny2")), "");
    }
}
