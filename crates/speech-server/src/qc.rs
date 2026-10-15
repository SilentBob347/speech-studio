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
    /// Дефекты по словам распознавания: `tail` (звук после последнего слова), `stretched` (затянутое слово), `gap`
    /// (разрыв внутри фразы).
    pub issues: Vec<String>,
    /// The words recognition heard with their start and end in the take, seconds: the export's word timings.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub timed: Vec<(String, f64, f64)>,
}

impl Check {
    /// Прошёл всё, что проверялось.
    pub fn ok(&self) -> bool {
        self.passed && self.similar != Some(false) && self.issues.is_empty()
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

pub fn word_ops(expected: &[String], heard: &[String]) -> (usize, usize, usize) {
    word_diff(expected, heard).iter().fold((0, 0, 0), |(i, d, s), op| match op.op {
        "inserted" => (i + 1, d, s),
        "deleted" => (i, d + 1, s),
        "substituted" => (i, d, s + 1),
        _ => (i, d, s),
    })
}

/// Шаг выравнивания слов: `same`, `inserted` (лишнее услышанное), `deleted` (пропущенное), `substituted`.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct WordOp {
    pub op: &'static str,
    pub expected: Option<String>,
    pub heard: Option<String>,
}

/// Выравнивание задуманных и услышанных слов по Левенштейну, в порядке текста.
pub fn word_diff(expected: &[String], heard: &[String]) -> Vec<WordOp> {
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
    let mut ops = Vec::with_capacity(n.max(m));
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && d[i][j] == d[i - 1][j - 1] + usize::from(expected[i - 1] != heard[j - 1]) {
            let op = if expected[i - 1] == heard[j - 1] { "same" } else { "substituted" };
            ops.push(WordOp { op, expected: Some(expected[i - 1].clone()), heard: Some(heard[j - 1].clone()) });
            i -= 1;
            j -= 1;
        } else if i > 0 && d[i][j] == d[i - 1][j] + 1 {
            ops.push(WordOp { op: "deleted", expected: Some(expected[i - 1].clone()), heard: None });
            i -= 1;
        } else {
            ops.push(WordOp { op: "inserted", expected: None, heard: Some(heard[j - 1].clone()) });
            j -= 1;
        }
    }
    ops.reverse();
    ops
}

/// Звука после последнего распознанного слова больше этого — хвост (бормотание, шум, повтор без слов).
const TAIL_SECS: f64 = 1.0;
/// Слово звучит дольше этого и дольше `STRETCH_PER_LETTER` на букву — затянуто.
const STRETCH_SECS: f64 = 1.2;
const STRETCH_PER_LETTER: f64 = 0.3;
/// Тишина между словами длиннее этого (сверх пауз, заданных в тексте) — разрыв.
const GAP_SECS: f64 = 1.5;

/// Модель повторила кусок: два–четыре слова услышаны дважды подряд, а в тексте блока они подряд не повторяются.
pub fn repeated(expected: &[String], heard: &[String]) -> bool {
    let doubled = |words: &[String], n: usize| -> Vec<Vec<String>> {
        (0..words.len().saturating_sub(2 * n - 1)).filter(|&i| words[i..i + n] == words[i + n..i + 2 * n]).map(|i| words[i..i + n].to_vec()).collect()
    };
    (2..=4).any(|n| {
        let written = doubled(expected, n);
        doubled(heard, n).iter().any(|run| !written.contains(run))
    })
}

/// Дефекты дубля по таймингам слов распознавания; `pause_secs` — самая длинная пауза, заданная в тексте блока.
pub fn timing_issues(words: &[crate::ref_pick::Word], duration: f64, pause_secs: f64) -> Vec<String> {
    let mut issues = Vec::new();
    let Some(last) = words.last() else { return issues };
    if duration - last.end > TAIL_SECS {
        issues.push("tail".to_string());
    }
    if words.iter().any(|w| {
        let letters = w.word.chars().filter(|c| c.is_alphabetic()).count().max(1) as f64;
        let d = w.end - w.start;
        d > STRETCH_SECS && d > letters * STRETCH_PER_LETTER
    }) {
        issues.push("stretched".to_string());
    }
    if words.windows(2).any(|w| w[1].start - w[0].end > GAP_SECS + pause_secs) {
        issues.push("gap".to_string());
    }
    issues
}

/// Сколько бы ни было слов, больше половины с лишним ошибок — уже не та фраза.
const MAX_THRESHOLD: f64 = 0.6;

/// Порог WER по строгости (`strict`, `normal`, `lenient`): в коротком блоке допускается слово-другое.
/// Музыка или шум вместо речи (open-tts-eval): в громкой части клипа (кадры VAD не тише −30 дБ от пика) речь по
/// Silero VAD — меньше половины. Меньше трети секунды громкого звука — судить не по чему.
pub fn non_speech(samples: &[f32], probs: &[f32], frame: usize) -> bool {
    let rms: Vec<f32> = samples.chunks(frame).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len().max(1) as f32).sqrt()).collect();
    let peak = rms.iter().cloned().fold(0.0f32, f32::max);
    let loud: Vec<usize> = (0..rms.len().min(probs.len())).filter(|&i| rms[i] >= peak * 0.0316).collect();
    if loud.len() < 10 {
        return false;
    }
    let speech = loud.iter().filter(|&&i| probs[i] >= 0.5).count();
    (speech as f64) < loud.len() as f64 * 0.5
}

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
    Check { text_checked: true, wer, words: e.len(), inserted, deleted, substituted, heard: heard.to_string(), passed: wer <= threshold(e.len(), strictness), similarity: None, similar: None, issues: Vec::new(), timed: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tail_a_stretched_word_and_a_gap_are_named() {
        let w = |word: &str, start: f64, end: f64| crate::ref_pick::Word { word: word.into(), start, end };
        assert!(timing_issues(&[w("да", 0.0, 0.3), w("нет", 0.4, 0.8)], 1.0, 0.0).is_empty());
        assert_eq!(timing_issues(&[w("да", 0.0, 0.3)], 2.0, 0.0), vec!["tail"]);
        assert_eq!(timing_issues(&[w("да", 0.0, 1.5)], 1.6, 0.0), vec!["stretched"]);
        assert_eq!(timing_issues(&[w("да", 0.0, 0.3), w("нет", 2.2, 2.5)], 2.6, 0.0), vec!["gap"]);
        assert!(timing_issues(&[w("да", 0.0, 0.3), w("нет", 2.2, 2.5)], 2.6, 1.0).is_empty(), "a written pause is no gap");
        let words = |t: &str| normalize(t);
        assert!(repeated(&words("Он пришёл домой и лёг спать."), &words("Он пришёл домой домой и лёг спать пришёл домой пришёл домой.")));
        assert!(!repeated(&words("Он пришёл домой и лёг спать."), &words("Он пришёл домой и лёг спать.")));
        assert!(!repeated(&words("Ну и ну, ну и ну!"), &words("Ну и ну ну и ну")), "a repeat the text has is no defect");
    }

    fn w(s: &str) -> Vec<String> {
        normalize(s)
    }

    #[test]
    fn the_same_words_pass_whatever_the_case_punctuation_and_yo() {
        let c = check("Ёлка, ЗЕЛЁНАЯ! <|emotion:awe|> [pause 300ms] стоит.", "елка зеленая стоит", "normal");
        assert_eq!((c.wer, c.passed), (0.0, true));
        let loud = vec![0.3f32; 512 * 20];
        assert!(non_speech(&loud, &[0.1; 20], 512), "loud and not speech");
        assert!(!non_speech(&loud, &[0.9; 20], 512));
        assert!(!non_speech(&loud[..512 * 5], &[0.1; 5], 512), "too short to judge");
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
