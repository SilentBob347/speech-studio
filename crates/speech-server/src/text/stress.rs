//! Ударения, «ё» и омографы русского текста — RUAccent (`ruaccent/accentuator`, MIT): словарь 3,2 млн словоформ,
//! символьная сеть для слов вне словаря (int8) и сеть омографов tiny2 (int8). Шаги и их порядок — RUAccent
//! `process_all` без морфологического теггера (так же делает stressonnx): «ё» по словарю, омографы по контексту
//! предложения, ударения словом из словаря или сетью. Сеть «ё»-омографов в RUAccent есть только в fp32, поэтому
//! такие слова («все/всё») не меняются, а помечаются для ручного выбора.
//!
//! Ручные `+` и `ё` в тексте не перезаписываются; омограф определяется по тексту без ручных меток соседей
//! (урок issue #6 Silero Stress). Разметка синтеза (`<|…|>`, `[pause …]`, `[[…]]`, `<[…]>`) не трогается.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

const VOWELS: &str = "аеёиоуыэюя";
/// Омограф спорный, когда сеть отдала лучшему варианту меньше этого отрыва от второго.
const DOUBT_MARGIN: f32 = 0.2;

pub(crate) static MARKUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)<\|[^|\n]*\|>|\[\[[^\[\]]*\]\]|\[\s*pause[^\]]*\]|\[\s*rate\s*:[^\]]*\]|sil\s*<\[[^\]]*\]>|<\[[^\]]*\]>")
        .expect("markup")
});
static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\w]+|[^\w\s]+").expect("token"));

fn is_cyr(c: char) -> bool {
    matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')
}

pub(crate) fn is_vowel(c: char) -> bool {
    VOWELS.contains(c.to_lowercase().next().unwrap_or(c))
}

fn lower(s: &str) -> String {
    s.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect()
}

/// Как пословно решён текст: для подсветки в окне и трассировки подготовки.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct WordNote {
    /// Границы слова в результате, в символах.
    pub start: usize,
    pub end: usize,
    pub word: String,
    /// `manual`, `dictionary`, `model`, `omograph`, `yo`.
    pub source: &'static str,
    /// Варианты омографа с уверенностью сети (у `omograph`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<(String, f32)>,
    /// Спорный омограф: стоит проверить на слух и выбрать вручную.
    pub doubt: bool,
    /// «ё»-омограф, который не решается автоматически: вариант с «ё» для ручного выбора.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yo_choice: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Stressed {
    pub text: String,
    pub words: Vec<WordNote>,
}

pub(crate) trait Inference {
    /// Positions count Unicode characters in the unstressed word.
    fn accent_position(&self, word: &str) -> Result<Option<usize>, String>;
    fn omograph_probability(&self, context: &str, hypothesis: &str) -> Result<f32, String>;
}

pub(crate) struct Rules {
    accents: fst::Map<Vec<u8>>,
    omographs: HashMap<String, Vec<String>>,
    yo_words: HashMap<String, String>,
    yo_homographs: HashMap<String, String>,
}

fn read_gz(path: &Path) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(f)
        .read_to_end(&mut out)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(out)
}

fn json_gz<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&read_gz(path)?).map_err(|e| format!("{}: {e}", path.display()))
}

/// Позиция ударной гласной (в символах слова без `+`) из записи вида `молок+о`.
fn plus_pos(stressed: &str) -> Option<usize> {
    stressed.chars().position(|c| c == '+')
}

/// Словарь ударений как FST: слово → номер ударного символа. 3,2 млн пар в HashMap заняли бы сотни мегабайт.
fn build_accents(path: &Path) -> Result<fst::Map<Vec<u8>>, String> {
    let raw: HashMap<String, String> = json_gz(path)?;
    let mut pairs: Vec<(String, u64)> = raw.into_iter().filter_map(|(k, v)| plus_pos(&v).map(|p| (k, p as u64))).collect();
    pairs.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    pairs.dedup_by(|a, b| a.0 == b.0);
    let mut b = fst::MapBuilder::memory();
    for (k, v) in pairs {
        b.insert(k.as_bytes(), v).map_err(|e| format!("accents fst: {e}"))?;
    }
    let bytes = b.into_inner().map_err(|e| format!("accents fst: {e}"))?;
    fst::Map::new(bytes).map_err(|e| format!("accents fst: {e}"))
}

impl Rules {
    pub(crate) fn load(d: &Path) -> Result<Self, String> {
        let dict = d.join("dictionary");
        let mut omographs: HashMap<String, Vec<String>> = json_gz(&dict.join("omographs.json.gz"))?;
        omographs.insert("коса".into(), vec!["к+оса".into(), "кос+а".into()]);
        Ok(Self {
            accents: build_accents(&dict.join("accents.json.gz"))?,
            omographs,
            yo_words: json_gz(&dict.join("yo_words.json.gz"))?,
            yo_homographs: json_gz(&dict.join("yo_homographs.json.gz"))?,
        })
    }

    /// Знает ли разметчик слово: словарь ударений, омографы, «ё».
    pub fn knows(&self, word: &str) -> bool {
        let w = lower(word);
        let e = w.replace('ё', "е");
        self.accents.contains_key(w.as_bytes())
            || self.accents.contains_key(e.as_bytes())
            || self.omographs.contains_key(&w)
            || self.yo_words.contains_key(&e)
            || self.yo_homographs.contains_key(&e)
            || w.chars().filter(|c| is_vowel(*c)).count() < 2
    }

    fn dict_pos(&self, word: &str) -> Option<usize> {
        self.accents.get(word.as_bytes()).map(|v| v as usize)
    }

    /// Расставить ударения и «ё» в тексте блока.
    pub(crate) fn annotate(&self, text: &str, model: &impl Inference) -> Result<Stressed, String> {
        let plan = Plan::new(text);
        let mut words: Vec<Decision> = Vec::new();
        for sentence in plan.sentences() {
            self.decide_sentence(&plan, sentence, model, &mut words)?;
        }
        Ok(plan.render(&words))
    }

    fn decide_sentence(
        &self,
        plan: &Plan,
        sentence: (usize, usize),
        model: &impl Inference,
        out: &mut Vec<Decision>,
    ) -> Result<(), String> {
        let tokens = plan.tokens(sentence);
        let mut forms: Vec<String> = tokens.iter().map(|t| plan.clean_str(t.0, t.1)).collect();
        let mut decisions: Vec<Option<Decision>> = vec![None; tokens.len()];
        for (i, &(s, e)) in tokens.iter().enumerate() {
            if !plan.processable(s, e) {
                continue;
            }
            let lw = lower(&forms[i]);
            if plan.manual(s, e) {
                decisions[i] = Some(Decision::new(s, e, "manual"));
                continue;
            }
            if lw.contains('ё') {
                continue;
            }
            if let Some(yo) = self.yo_words.get(&lw) {
                forms[i] = fix_capital(&forms[i], yo);
                let mut d = Decision::new(s, e, "yo");
                d.yo = yo_positions(&lw, yo);
                decisions[i] = Some(d);
            } else if let Some(yo) = self.yo_homographs.get(&lw) {
                let mut d = Decision::new(s, e, "dictionary");
                d.yo_choice = Some(fix_capital(&forms[i], yo));
                decisions[i] = Some(d);
            }
        }
        for (i, &(s, e)) in tokens.iter().enumerate() {
            if !plan.processable(s, e) || plan.manual(s, e) {
                continue;
            }
            let lw = lower(&forms[i]);
            let Some(variants) = self.omographs.get(&lw) else { continue };
            let mut scored: Vec<(String, f32)> = Vec::new();
            if variants.len() == 1 {
                scored.push((variants[0].clone(), 1.0));
            } else {
                let context = omograph_context(&forms, i);
                for v in variants {
                    scored.push((v.clone(), model.omograph_probability(&context, v)?));
                }
            }
            let best = scored.iter().cloned().fold(None::<(String, f32)>, |m, v| match m {
                Some(b) if b.1 >= v.1 => Some(b),
                _ => Some(v),
            });
            let Some((chosen, p)) = best else { continue };
            let second = scored.iter().filter(|v| v.0 != chosen).map(|v| v.1).fold(0.0f32, f32::max);
            let bare: String = chosen.chars().filter(|c| *c != '+').collect();
            let mut d = decisions[i].take().unwrap_or_else(|| Decision::new(s, e, "omograph"));
            d.source = "omograph";
            d.stress = plus_pos(&chosen);
            d.yo.extend(yo_positions(&lw, &bare));
            d.doubt = scored.len() > 1 && p - second < DOUBT_MARGIN;
            d.variants = scored;
            decisions[i] = Some(d);
        }
        for (i, &(s, e)) in tokens.iter().enumerate() {
            if !plan.processable(s, e) || plan.manual(s, e) {
                continue;
            }
            if decisions[i].as_ref().is_some_and(|d| d.source == "omograph") {
                continue;
            }
            let lw = lower(&forms[i]);
            if lw.contains('ё') || lw.chars().filter(|c| is_vowel(*c)).count() < 2 {
                if let Some(d) = decisions[i].take() {
                    out.push(d);
                }
                continue;
            }
            let (pos, source) = match self.dict_pos(&lw) {
                Some(p) => (Some(p), "dictionary"),
                None => (model.accent_position(&lw)?, "model"),
            };
            let mut d = decisions[i].take().unwrap_or_else(|| Decision::new(s, e, source));
            if d.source != "yo" {
                d.source = source;
            }
            d.stress = pos.filter(|p| lw.chars().nth(*p).is_some_and(is_vowel));
            decisions[i] = Some(d);
        }
        out.extend(decisions.into_iter().flatten());
        Ok(())
    }
}

/// Регистр исходного слова на форму из словаря той же длины.
fn fix_capital(source: &str, target: &str) -> String {
    let s: Vec<char> = source.chars().collect();
    let t: Vec<char> = target.chars().collect();
    if s.len() != t.len() {
        return target.to_string();
    }
    s.iter()
        .zip(t)
        .map(|(a, b)| {
            if a.is_uppercase() {
                b.to_uppercase().next().unwrap_or(b)
            } else {
                b.to_lowercase().next().unwrap_or(b)
            }
        })
        .collect()
}

/// Где в `with_yo` стоит «ё» на месте «е» исходного слова.
fn yo_positions(word: &str, with_yo: &str) -> Vec<usize> {
    word.chars()
        .zip(with_yo.chars())
        .enumerate()
        .filter(|(_, (a, b))| matches!(a, 'е' | 'Е') && matches!(b, 'ё' | 'Ё'))
        .map(|(i, _)| i)
        .collect()
}

/// RUAccent `delete_spaces_before_punc`.
fn delete_spaces_before_punc(text: &str) -> String {
    let mut t = text.to_string();
    for c in "!\"#$%&'()*,./:;<=>?@[\\]^_`{|}-".chars() {
        if c == '-' {
            t = t.replace(" -", "-").replace("- ", "-");
        }
        t = t.replace(&format!(" {c}"), &c.to_string());
    }
    t.replace('~', "-")
}

static SPACE_BEFORE_PUNCT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+([,.?!:;…])").expect("punct"));

/// Предложение с помеченным словом, как его готовит RUAccent для сети омографов.
fn omograph_context(forms: &[String], at: usize) -> String {
    let joined = forms
        .iter()
        .enumerate()
        .map(|(i, w)| if i == at { format!(" <w>{w}</w> ") } else { w.clone() })
        .collect::<Vec<_>>()
        .join(" ");
    SPACE_BEFORE_PUNCT
        .replace_all(&delete_spaces_before_punc(&joined), "$1")
        .into_owned()
}

#[derive(Clone, Debug)]
struct Decision {
    /// Границы слова в очищенном тексте.
    start: usize,
    end: usize,
    source: &'static str,
    stress: Option<usize>,
    yo: Vec<usize>,
    doubt: bool,
    variants: Vec<(String, f32)>,
    yo_choice: Option<String>,
}

impl Decision {
    fn new(start: usize, end: usize, source: &'static str) -> Self {
        Decision {
            start,
            end,
            source,
            stress: None,
            yo: Vec::new(),
            doubt: false,
            variants: Vec::new(),
            yo_choice: None,
        }
    }
}

/// Текст без ручных `+`: где они стояли, что из текста — разметка синтеза.
struct Plan {
    clean: Vec<char>,
    manual_stress: Vec<bool>,
    markup: Vec<bool>,
}

impl Plan {
    fn new(text: &str) -> Plan {
        let mut markup_bytes = vec![false; text.len()];
        for m in MARKUP.find_iter(text) {
            markup_bytes[m.start()..m.end()].iter_mut().for_each(|b| *b = true);
        }
        let src: Vec<(usize, char)> = text.char_indices().collect();
        let (mut clean, mut manual_stress, mut markup) = (Vec::new(), Vec::new(), Vec::new());
        let mut pending = false;
        for (k, &(b, c)) in src.iter().enumerate() {
            let in_markup = markup_bytes[b];
            let next_is_vowel = src.get(k + 1).is_some_and(|(_, n)| is_cyr(*n) && is_vowel(*n));
            if c == '+' && !in_markup && next_is_vowel {
                pending = true;
                continue;
            }
            clean.push(c);
            manual_stress.push(pending);
            markup.push(in_markup);
            pending = false;
        }
        Plan {
            clean,
            manual_stress,
            markup,
        }
    }

    fn clean_str(&self, s: usize, e: usize) -> String {
        self.clean[s..e].iter().collect()
    }

    /// Предложения очищенного текста (границы в символах), разметка — пробел.
    fn sentences(&self) -> Vec<(usize, usize)> {
        let n = self.clean.len();
        let mut out = Vec::new();
        let mut start = 0;
        let mut i = 0;
        while i < n {
            let c = self.clean[i];
            if !self.markup[i] && matches!(c, '.' | '!' | '?' | '…' | '\n') {
                let mut j = i + 1;
                while j < n && !self.markup[j] && matches!(self.clean[j], '.' | '!' | '?' | '…' | '"' | '»') {
                    j += 1;
                }
                if j >= n || self.clean[j].is_whitespace() {
                    out.push((start, j));
                    start = j;
                    i = j;
                    continue;
                }
            }
            i += 1;
        }
        if start < n {
            out.push((start, n));
        }
        out
    }

    /// Слова и знаки предложения (как в RUAccent), без разметки синтеза.
    fn tokens(&self, (s, e): (usize, usize)) -> Vec<(usize, usize)> {
        let text: String = self.clean[s..e]
            .iter()
            .enumerate()
            .map(|(k, c)| if self.markup[s + k] { ' ' } else { *c })
            .collect();
        let offsets: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
        let to_char = |b: usize| offsets.partition_point(|x| *x < b);
        TOKEN
            .find_iter(&text)
            .map(|m| (s + to_char(m.start()), s + to_char(m.end())))
            .collect()
    }

    fn processable(&self, s: usize, e: usize) -> bool {
        self.clean[s..e].iter().all(|c| is_cyr(*c))
    }

    fn manual(&self, s: usize, e: usize) -> bool {
        self.manual_stress[s..e].iter().any(|m| *m)
    }

    fn render(&self, decisions: &[Decision]) -> Stressed {
        let mut stress_at = vec![false; self.clean.len()];
        let mut yo_at = vec![false; self.clean.len()];
        for d in decisions {
            if let Some(p) = d.stress {
                if d.start + p < d.end {
                    stress_at[d.start + p] = true;
                }
            }
            for p in &d.yo {
                yo_at[d.start + p] = true;
            }
        }
        let mut text = String::new();
        let mut out_pos = vec![0usize; self.clean.len() + 1];
        let mut n = 0;
        for (i, c) in self.clean.iter().enumerate() {
            out_pos[i] = n;
            let stressed = self.manual_stress[i] || stress_at[i];
            let c = if yo_at[i] {
                if c.is_uppercase() {
                    'Ё'
                } else {
                    'ё'
                }
            } else {
                *c
            };
            if stressed && !(matches!(c, 'ё' | 'Ё') && !self.manual_stress[i]) {
                text.push('+');
                n += 1;
            }
            text.push(c);
            n += 1;
        }
        out_pos[self.clean.len()] = n;
        let chars: Vec<char> = text.chars().collect();
        let mut ordered: Vec<&Decision> = decisions.iter().collect();
        ordered.sort_by_key(|d| d.start);
        let words = ordered
            .into_iter()
            .map(|d| {
                let (start, end) = (out_pos[d.start], out_pos[d.end]);
                WordNote {
                    start,
                    end,
                    word: chars[start..end].iter().collect(),
                    source: d.source,
                    variants: d.variants.clone(),
                    doubt: d.doubt,
                    yo_choice: d.yo_choice.clone(),
                }
            })
            .collect();
        Stressed { text, words }
    }
}

/// Русские слова текста (вне разметки синтеза), строчными, без ударений `+`.
pub fn cyrillic_words(text: &str) -> Vec<String> {
    let plan = Plan::new(text);
    plan.sentences()
        .into_iter()
        .flat_map(|s| plan.tokens(s))
        .filter(|(s, e)| plan.processable(*s, *e))
        .map(|(s, e)| lower(&plan.clean_str(s, e)))
        .collect()
}

/// Русский ли текст блока при языке проекта `lang` (`auto` — по доле кириллицы среди букв).
pub fn is_russian(text: &str, lang: &str) -> bool {
    match lang {
        "ru" => true,
        "auto" | "" => {
            let visible = super::markup::visible(text);
            let letters = visible.chars().filter(|c| c.is_alphabetic()).count();
            let cyr = visible.chars().filter(|c| is_cyr(*c)).count();
            letters > 0 && cyr * 2 > letters
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        Rules {
            accents: fst::Map::from_iter([("молоко", 5)]).unwrap(),
            omographs: HashMap::from([("замок".into(), vec!["з+амок".into(), "зам+ок".into()])]),
            yo_words: HashMap::from([("елка".into(), "ёлка".into())]),
            yo_homographs: HashMap::from([("все".into(), "всё".into())]),
        }
    }

    struct NoInference;

    impl Inference for NoInference {
        fn accent_position(&self, word: &str) -> Result<Option<usize>, String> {
            panic!("unexpected accent inference for {word}")
        }

        fn omograph_probability(&self, context: &str, hypothesis: &str) -> Result<f32, String> {
            panic!("unexpected omograph inference for {context}: {hypothesis}")
        }
    }

    #[test]
    fn dictionary_manual_marks_yo_and_markup_do_not_require_model_inference() {
        let result = rules()
            .annotate("Молоко Ёлка елка з+амок [[замок]] <|emotion:awe|> [pause 1.5s]", &NoInference)
            .unwrap();
        assert_eq!(result.text, "Молок+о Ёлка ёлка з+амок [[замок]] <|emotion:awe|> [pause 1.5s]");
        assert!(result.words.iter().any(|w| w.source == "dictionary" && w.word == "Молок+о"));
        assert!(result.words.iter().any(|w| w.source == "yo" && w.word == "ёлка"));
        assert!(result.words.iter().any(|w| w.source == "manual" && w.word == "з+амок"));
    }

    struct Prediction {
        accent: Result<Option<usize>, String>,
        omograph: Result<f32, String>,
        seen: std::cell::RefCell<Vec<(String, String)>>,
    }

    impl Inference for Prediction {
        fn accent_position(&self, _: &str) -> Result<Option<usize>, String> {
            self.accent.clone()
        }

        fn omograph_probability(&self, context: &str, hypothesis: &str) -> Result<f32, String> {
            self.seen.borrow_mut().push((context.into(), hypothesis.into()));
            self.omograph.clone()
        }
    }

    fn prediction(accent: Result<Option<usize>, String>, omograph: Result<f32, String>) -> Prediction {
        Prediction {
            accent,
            omograph,
            seen: Default::default(),
        }
    }

    #[test]
    fn model_positions_and_homograph_scores_keep_the_existing_annotation_contract() {
        let model = prediction(Ok(Some(5)), Ok(0.6));
        let result = rules().annotate("Это з+амок, замок и куздра, все.", &model).unwrap();
        assert!(result.text.contains("з+амок, з+амок и куздр+а, все."));
        let homo = result.words.iter().find(|w| w.source == "omograph").unwrap();
        assert_eq!(homo.variants, [("з+амок".into(), 0.6), ("зам+ок".into(), 0.6)]);
        assert!(homo.doubt);
        let yo = result.words.iter().find(|w| w.yo_choice.is_some()).unwrap();
        assert_eq!(yo.yo_choice.as_deref(), Some("всё"));
        assert!(model
            .seen
            .borrow()
            .iter()
            .all(|(context, _)| context.contains("Это замок,") && context.contains("<w>замок</w>") && !context.contains('+')));
    }

    #[test]
    fn inference_errors_propagate_and_uncertain_or_invalid_positions_do_not_add_marks() {
        assert_eq!(
            rules()
                .annotate("куздра", &prediction(Err("accent failed".into()), Ok(1.0)))
                .unwrap_err(),
            "accent failed"
        );
        assert_eq!(
            rules()
                .annotate("замок", &prediction(Ok(None), Err("omograph failed".into())))
                .unwrap_err(),
            "omograph failed"
        );
        for pos in [None, Some(2), Some(usize::MAX)] {
            let result = rules().annotate("куздра", &prediction(Ok(pos), Ok(1.0))).unwrap();
            assert_eq!(result.text, "куздра");
            assert_eq!(result.words[0].source, "model");
        }
    }

    #[test]
    fn dictionary_loading_and_known_words_work_without_onnx_or_a_tokenizer() {
        use std::io::Write;

        let root = tempfile::tempdir().unwrap();
        let dict = root.path().join("dictionary");
        std::fs::create_dir(&dict).unwrap();
        for (name, json) in [
            ("accents", r#"{"молоко":"молок+о"}"#),
            ("omographs", r#"{"замок":["з+амок","зам+ок"]}"#),
            ("yo_words", r#"{"елка":"ёлка"}"#),
            ("yo_homographs", r#"{"все":"всё"}"#),
        ] {
            let file = std::fs::File::create(dict.join(format!("{name}.json.gz"))).unwrap();
            let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            gz.write_all(json.as_bytes()).unwrap();
            gz.finish().unwrap();
        }
        let rules = Rules::load(root.path()).unwrap();
        for word in ["МОЛОКО", "замок", "коса", "елка", "Ёлка", "все", "кот"] {
            assert!(rules.knows(word), "{word}")
        }
        assert!(!rules.knows("куздра"));
        assert_eq!(rules.annotate("Молоко елка", &NoInference).unwrap().text, "Молок+о ёлка");
        assert!(!root.path().join("nn").exists());
    }

    #[test]
    fn manual_pluses_leave_the_clean_text_and_come_back_in_place() {
        let plan = Plan::new("Это з+амок, а не <|emotion:awe|> зам+ок.");
        let clean: String = plan.clean.iter().collect();
        assert_eq!(clean, "Это замок, а не <|emotion:awe|> замок.");
        assert_eq!(plan.render(&[]).text, "Это з+амок, а не <|emotion:awe|> зам+ок.");
    }

    #[test]
    fn markup_is_neither_a_word_nor_a_sentence_end() {
        let plan = Plan::new("Раз. [pause 1.5s] Два <|emotion:anger|> три! [[каша]] [rate:0,8]");
        let words: Vec<String> = plan
            .sentences()
            .into_iter()
            .flat_map(|s| plan.tokens(s))
            .map(|(s, e)| plan.clean_str(s, e))
            .collect();
        assert_eq!(words, vec!["Раз", ".", "Два", "три", "!"]);
    }

    #[test]
    fn decisions_put_plus_before_the_vowel_and_yo_keeps_its_case() {
        let plan = Plan::new("Елка и молоко");
        let mut a = Decision::new(0, 4, "yo");
        a.yo = vec![0];
        let mut b = Decision::new(7, 13, "dictionary");
        b.stress = Some(5);
        let s = plan.render(&[a, b]);
        assert_eq!(s.text, "Ёлка и молок+о");
        assert_eq!((s.words[1].start, s.words[1].end, s.words[1].word.as_str()), (7, 14, "молок+о"));
    }

    #[test]
    fn the_omograph_context_is_the_ruaccent_one() {
        let forms: Vec<String> = ["Старинный", "замок", "стоял", "на", "холме", "."]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(omograph_context(&forms, 1), "Старинный <w>замок</w>  стоял на холме.");
    }

    #[test]
    fn russian_is_found_by_cyrillic_share() {
        assert!(is_russian("Привет, world", "auto"));
        assert!(!is_russian("Hello мир and everyone", "auto"));
        assert!(!is_russian("Привет", "en"));
    }
}
