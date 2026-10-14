//! Обратная проверка озвучки: задуманный текст против услышанного распознаванием. Оба приводятся к одному виду
//! (нижний регистр, ё → е, без пунктуации и разметки, числа как есть), WER — по словам с разбивкой на вставки,
//! пропуски и замены; порог зависит от длины блока и строгости: в коротком блоке одно слово — уже много.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Check {
    /// Текст сверен распознаванием (иначе — только похожесть голоса).
    pub text_checked: bool,
    pub wer: f64,
    pub words: usize,
    pub inserted: usize,
    pub deleted: usize,
    pub substituted: usize,
    pub heard: String,
    pub passed: bool,
    /// Косинус голоса дубля и рефа (WeSpeaker); `similar` — не ниже порога «тот же голос».
    pub similarity: Option<f64>,
    pub similar: Option<bool>,
}

impl Check {
    /// Прошёл всё, что проверялось.
    pub fn ok(&self) -> bool {
        self.passed && self.similar != Some(false)
    }
}

pub fn normalize(text: &str) -> Vec<String> {
    let visible = crate::speech_text::visible(text);
    visible
        .to_lowercase()
        .replace('ё', "е")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Вставки, пропуски и замены по словам (Левенштейн с восстановлением операций).
pub fn word_ops(expected: &[String], heard: &[String]) -> (usize, usize, usize) {
    let (n, m) = (expected.len(), heard.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(expected[i - 1] != heard[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
        }
    }
    let (mut i, mut j) = (n, m);
    let (mut ins, mut del, mut sub) = (0, 0, 0);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && d[i][j] == d[i - 1][j - 1] + usize::from(expected[i - 1] != heard[j - 1]) {
            if expected[i - 1] != heard[j - 1] {
                sub += 1;
            }
            i -= 1;
            j -= 1;
        } else if i > 0 && d[i][j] == d[i - 1][j] + 1 {
            del += 1;
            i -= 1;
        } else {
            ins += 1;
            j -= 1;
        }
    }
    (ins, del, sub)
}

/// Сколько бы ни было слов, больше половины с лишним ошибок — уже не та фраза.
const MAX_THRESHOLD: f64 = 0.6;

/// Порог WER по строгости (`strict`, `normal`, `lenient`): в коротком блоке допускается слово-другое.
pub fn threshold(words: usize, strictness: &str) -> f64 {
    let (base, slack): (f64, f64) = match strictness {
        "strict" => (0.10, 1.0),
        "lenient" => (0.35, 3.0),
        _ => (0.20, 2.0),
    };
    base.max(slack / words.max(1) as f64).min(MAX_THRESHOLD)
}

pub fn check(expected: &str, heard: &str, strictness: &str) -> Check {
    let e = normalize(expected);
    let h = normalize(heard);
    let (inserted, deleted, substituted) = word_ops(&e, &h);
    let wer = if e.is_empty() { if h.is_empty() { 0.0 } else { 1.0 } } else { (inserted + deleted + substituted) as f64 / e.len() as f64 };
    Check { text_checked: true, wer, words: e.len(), inserted, deleted, substituted, heard: heard.to_string(), passed: wer <= threshold(e.len(), strictness), similarity: None, similar: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<String> {
        normalize(s)
    }

    #[test]
    fn the_same_words_pass_whatever_the_case_punctuation_and_yo() {
        let c = check("Ёлка, ЗЕЛЁНАЯ! <|emotion:awe|> [pause 300ms] стоит.", "елка зеленая стоит", "normal");
        assert_eq!((c.wer, c.passed), (0.0, true));
    }

    #[test]
    fn insertions_deletions_and_substitutions_are_counted_apart() {
        assert_eq!(word_ops(&w("раз два три четыре"), &w("раз два пять четыре шесть")), (1, 0, 1));
        assert_eq!(word_ops(&w("раз два три"), &w("раз три")), (0, 1, 0));
    }

    #[test]
    fn a_short_block_allows_a_word_and_a_long_one_a_share() {
        assert!(check("Да, конечно.", "да конечно нет", "normal").passed, "one extra word in two is within slack");
        let long = "раз два три четыре пять шесть семь восемь девять десять";
        assert!(!check(long, "раз два три четыре пять", "normal").passed);
        assert!(check(long, "раз два три четыре пять шесть семь восемь девять", "normal").passed);
        assert!(!check(long, "раз два три четыре пять шесть семь восемь", "strict").passed);
    }

    #[test]
    fn nothing_heard_from_a_text_fails() {
        let c = check("Привет всем.", "", "lenient");
        assert!(!c.passed && c.deleted == 2);
    }
}
