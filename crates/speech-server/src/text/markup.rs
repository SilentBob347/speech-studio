//! Shared synthesis markup syntax and its visible text projection, independent of projects and engines.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

pub(crate) static PAUSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\[\s*pause\s+(\d+(?:[.,]\d+)?)\s*(ms|s)?\s*\]|sil\s*<\[\s*(\d+)\s*\]>|<\[\s*(tiny|small|medium|large|huge)\s*\]>|\[\s*rate\s*:\s*(\d+(?:[.,]\d+)?)\s*\]").expect("pause")
});
pub(crate) static ACCENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*\n]{1,80})\*\*").expect("accent"));
pub(crate) static ACCENT_MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<\[\s*accented\s*\]>").expect("accented"));
pub(crate) static PHONEMES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\[\]]{1,120})\]\]").expect("phonemes"));
pub(crate) static ANGLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^<>\n]{1,48}>").expect("angle"));
pub(crate) static STRESS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\+([АаЕеЁёИиОоУуЫыЭэЮюЯяAaEeIiOoUuYy])").expect("stress"));

/// Видимый текст без разметки синтеза: для субтитров, глав и поиска.
pub fn visible(text: &str) -> String {
    let t = PAUSE.replace_all(text, " ");
    let t = ACCENT.replace_all(&t, "$1");
    let t = ACCENT_MARK.replace_all(&t, "");
    let t = PHONEMES.replace_all(&t, "$1");
    let t = STRESS.replace_all(&t, "$1");
    let t = ANGLE.replace_all(&t, " ");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Пауза из разметки — не длиннее 10 с; SpeechKit ограничивает `sil` 7 с, наш тег — 10.
pub const MAX_PAUSE_MS: u32 = 10_000;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Piece {
    Say(String),
    Pause(u32),
    /// Темп следующих фраз до нового тега или конца блока.
    Rate(f64),
}

/// Темп `[rate:x]` — от четверти до учетверённого.
pub const RATE_RANGE: (f64, f64) = (0.25, 4.0);

pub(crate) static HIGGS_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^<\|(\w+):(\w+)\|>$").expect("higgs tag"));

fn context_pause(word: &str) -> u32 {
    match word.to_ascii_lowercase().as_str() {
        "tiny" => 100,
        "small" => 250,
        "medium" => 500,
        "large" => 1000,
        _ => 2000,
    }
}

/// Куски блока: фразы и паузы между ними; пустые фразы выпадают, соседние паузы складываются.
pub fn pieces(text: &str) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let push_say = |out: &mut Vec<Piece>, s: &str| {
        let s = s.trim();
        if !s.is_empty() {
            out.push(Piece::Say(s.to_string()));
        }
    };
    let mut last = 0;
    for c in PAUSE.captures_iter(text) {
        let m = c.get(0).expect("whole match");
        push_say(&mut out, &text[last..m.start()]);
        last = m.end();
        if let Some(r) = c.get(5) {
            let rate = r
                .as_str()
                .replace(',', ".")
                .parse::<f64>()
                .unwrap_or(1.0)
                .clamp(RATE_RANGE.0, RATE_RANGE.1);
            out.push(Piece::Rate(rate));
            continue;
        }
        let ms = if let Some(v) = c.get(1) {
            let n: f64 = v.as_str().replace(',', ".").parse().unwrap_or(0.0);
            match c.get(2).map(|u| u.as_str().to_ascii_lowercase()) {
                Some(u) if u == "s" => n * 1000.0,
                _ => n,
            }
        } else if let Some(v) = c.get(3) {
            v.as_str().parse::<f64>().unwrap_or(0.0)
        } else {
            context_pause(c.get(4).map(|m| m.as_str()).unwrap_or("medium")) as f64
        };
        let ms = (ms.round() as i64).clamp(0, MAX_PAUSE_MS as i64) as u32;
        if ms == 0 {
            continue;
        }
        match out.last_mut() {
            Some(Piece::Pause(p)) => *p = (*p + ms).min(MAX_PAUSE_MS),
            _ => out.push(Piece::Pause(ms)),
        }
    }
    push_say(&mut out, &text[last..]);
    out
}

/// Есть ли в тексте ручные ударения `+`.
pub fn has_stress(text: &str) -> bool {
    STRESS.is_match(text)
}

/// Тишина между фразами длинного куска по типу границы: предложение — как в Supertonic `call`, запятая и слово короче.
pub const SENTENCE_GAP_MS: u32 = 300;
pub const COMMA_GAP_MS: u32 = 150;
pub const WORD_GAP_MS: u32 = 60;

/// Кусок текста фразами не длиннее `max` знаков — алгоритм Supertonic `chunk_text`: по предложениям, у слишком
/// длинного предложения по запятым, у слишком длинной части по словам. У каждой фразы — пауза перед ней в мс по типу
/// границы, на которой резали (у первой 0).
pub fn phrases(text: &str, max: usize) -> Vec<(String, u32)> {
    #[derive(Clone, Copy)]
    enum Cut {
        Sentence,
        Comma,
        Word,
    }
    let text = text.trim();
    let len = |s: &str| s.chars().count();
    if len(text) <= max {
        return vec![(text.to_string(), 0)];
    }
    let mut out: Vec<(String, Cut)> = Vec::new();
    let mut current = String::new();
    let flush = |current: &mut String, out: &mut Vec<(String, Cut)>, cut: Cut| {
        if !current.trim().is_empty() {
            out.push((current.trim().to_string(), cut));
        }
        current.clear();
    };
    for sentence in sentences(text) {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            continue;
        }
        if len(sentence) > max {
            flush(&mut current, &mut out, Cut::Sentence);
            for part in sentence.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                if len(part) > max {
                    flush(&mut current, &mut out, Cut::Comma);
                    let mut words = String::new();
                    for word in part.split_whitespace() {
                        if !words.is_empty() && len(&words) + len(word) + 1 > max {
                            flush(&mut words, &mut out, Cut::Word);
                        }
                        if !words.is_empty() {
                            words.push(' ');
                        }
                        words.push_str(word);
                    }
                    flush(&mut words, &mut out, Cut::Comma);
                } else {
                    if !current.is_empty() && len(&current) + len(part) + 2 > max {
                        flush(&mut current, &mut out, Cut::Comma);
                    }
                    if !current.is_empty() {
                        current.push_str(", ");
                    }
                    current.push_str(part);
                }
            }
            continue;
        }
        if !current.is_empty() && len(&current) + len(sentence) + 1 > max {
            flush(&mut current, &mut out, Cut::Sentence);
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(sentence);
    }
    flush(&mut current, &mut out, Cut::Sentence);
    let gap = |cut: Cut| match cut {
        Cut::Sentence => SENTENCE_GAP_MS,
        Cut::Comma => COMMA_GAP_MS,
        Cut::Word => WORD_GAP_MS,
    };
    out.iter()
        .enumerate()
        .map(|(k, (s, _))| (s.clone(), if k == 0 { 0 } else { gap(out[k - 1].1) }))
        .collect()
}

/// Предложения: конец — `.`, `!`, `?` или `…` с пробелом после, кроме сокращений.
pub(crate) fn sentences(text: &str) -> Vec<&str> {
    const ABBREVIATIONS: &[&str] = &[
        "Dr.", "Mr.", "Mrs.", "Ms.", "Prof.", "Sr.", "Jr.", "St.", "etc.", "vs.", "i.e.", "e.g.", "т.е.", "т.д.", "т.п.", "др.", "г.",
        "гг.", "им.", "ул.", "св.",
    ];
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (i, &(at, c)) in chars.iter().enumerate() {
        if !matches!(c, '.' | '!' | '?' | '…') || !chars.get(i + 1).is_some_and(|&(_, n)| n.is_whitespace()) {
            continue;
        }
        let end = at + c.len_utf8();
        let head = &text[start..end];
        if c == '.'
            && ABBREVIATIONS
                .iter()
                .any(|a| head.ends_with(a) && head[..head.len() - a.len()].chars().last().is_none_or(|p| !p.is_alphanumeric()))
        {
            continue;
        }
        out.push(head);
        start = end;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

static COMMAND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\[\s*(pause|rate)\b[^\]\n]{0,40}\]").expect("command"));
static ENGINE_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<\|[^<>\n]{0,48}\|?>").expect("engine tag"));
static SCOPE_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<(/?)([a-z][a-z -]{0,30})>").expect("scope tag"));

/// Markup the text means but that would not work: a pause or tempo tag that does not parse (read aloud as words) or a
/// tempo outside `RATE_RANGE` (clamped), `**` or `[[…]]` left open, a span tag opened without its close or closed
/// without its opening, an engine tag that is not `<|category:value|>`. Each as the problem and the markup.
pub fn markup_problems(text: &str) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    for m in COMMAND.find_iter(text) {
        match PAUSE
            .captures(m.as_str())
            .filter(|c| c.get(0).is_some_and(|w| w.as_str() == m.as_str()))
        {
            None => out.push((
                if m.as_str().to_lowercase().contains("rate") {
                    "rate"
                } else {
                    "pause"
                },
                m.as_str().to_string(),
            )),
            Some(c) => {
                if let Some(r) = c.get(5) {
                    let rate: f64 = r.as_str().replace(',', ".").parse().unwrap_or(0.0);
                    if !(RATE_RANGE.0..=RATE_RANGE.1).contains(&rate) {
                        out.push(("rate_range", m.as_str().to_string()));
                    }
                }
            }
        }
    }
    if text.matches("**").count() % 2 == 1 {
        out.push(("accent", "**".to_string()));
    }
    if text.matches("[[").count() != text.matches("]]").count() {
        out.push(("phonemes", "[[ ]]".to_string()));
    }
    for m in ENGINE_TAG.find_iter(text) {
        if !HIGGS_TAG.is_match(m.as_str()) {
            out.push(("engine_tag", m.as_str().to_string()));
        }
    }
    let mut open: Vec<String> = Vec::new();
    for c in SCOPE_TAG.captures_iter(text) {
        let name = c[2].to_string();
        let closes = text.contains(&format!("</{name}>"));
        if c[1].is_empty() {
            if closes {
                open.push(name);
            }
        } else if let Some(at) = open.iter().rposition(|n| *n == name) {
            open.remove(at);
        } else {
            out.push(("scope", c[0].to_string()));
        }
    }
    out.extend(open.into_iter().map(|n| ("scope", format!("<{n}>"))));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_that_would_not_work_is_named() {
        use super::markup_problems;
        assert!(markup_problems(
            "Привет [pause 500ms] мир [rate:0.8]тише[rate:1] <whisper>шёпот</whisper> <|emotion:anger|> **да** [[a b]]."
        )
        .is_empty());
        assert_eq!(
            markup_problems("Раз [pause пять] два [rate:9] три."),
            vec![("pause", "[pause пять]".to_string()), ("rate_range", "[rate:9]".to_string())]
        );
        assert_eq!(
            markup_problems("**да и <|emotion anger|> <whisper>тихо"),
            vec![("accent", "**".to_string()), ("engine_tag", "<|emotion anger|>".to_string())]
        );
        assert_eq!(
            markup_problems("громко</whisper> и <laugh> смех"),
            vec![("scope", "</whisper>".to_string())]
        );
    }

    #[test]
    fn long_text_is_cut_into_phrases_by_sentences_then_commas_then_words() {
        let texts = |t: &str, max| phrases(t, max).into_iter().map(|(s, _)| s).collect::<Vec<_>>();
        assert_eq!(phrases("Коротко.", 20), vec![("Коротко.".to_string(), 0)]);
        assert_eq!(
            phrases("Раз два три. Четыре пять! Шесть семь восемь?", 26),
            vec![
                ("Раз два три. Четыре пять!".to_string(), 0),
                ("Шесть семь восемь?".to_string(), SENTENCE_GAP_MS)
            ]
        );
        assert_eq!(
            texts("Он пришёл в т.е. другой дом. Вот так.", 30),
            vec!["Он пришёл в т.е. другой дом.", "Вот так."]
        );
        assert_eq!(
            phrases("один два три четыре, пять шесть семь восемь", 22),
            vec![
                ("один два три четыре".to_string(), 0),
                ("пять шесть семь восемь".to_string(), COMMA_GAP_MS)
            ]
        );
        let long = "слово ".repeat(40);
        let cut = phrases(&long, 50);
        assert!(cut.len() > 1 && cut.iter().all(|(p, _)| p.chars().count() <= 50), "{cut:?}");
        assert!(cut.iter().skip(1).all(|(_, gap)| *gap == WORD_GAP_MS), "{cut:?}");
        assert_eq!(cut.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>().join(" "), long.trim());
    }

    #[test]
    fn pauses_of_every_spelling_cut_the_block() {
        let p = pieces("Раз. [pause 500ms] Два, sil<[300]> три <[large]> четыре [pause 1,5s]");
        assert_eq!(
            p,
            vec![
                Piece::Say("Раз.".into()),
                Piece::Pause(500),
                Piece::Say("Два,".into()),
                Piece::Pause(300),
                Piece::Say("три".into()),
                Piece::Pause(1000),
                Piece::Say("четыре".into()),
                Piece::Pause(1500),
            ]
        );
    }

    #[test]
    fn rate_tags_split_the_block_and_are_clamped() {
        assert_eq!(
            pieces("Раз [rate:1.5] два [rate:9] три"),
            vec![
                Piece::Say("Раз".into()),
                Piece::Rate(1.5),
                Piece::Say("два".into()),
                Piece::Rate(4.0),
                Piece::Say("три".into()),
            ]
        );
        assert_eq!(visible("Раз [rate:0,8] два"), "Раз два");
    }

    #[test]
    fn neighbouring_pauses_add_up_and_stop_at_ten_seconds() {
        assert_eq!(
            pieces("[pause 6s][pause 6s] да"),
            vec![Piece::Pause(10_000), Piece::Say("да".into())]
        );
        assert_eq!(pieces("[PAUSE 200] да"), vec![Piece::Pause(200), Piece::Say("да".into())]);
        assert_eq!(pieces("просто текст"), vec![Piece::Say("просто текст".into())]);
    }

    #[test]
    fn the_visible_text_has_no_synthesis_markup() {
        assert_eq!(
            visible("Раз [pause 300ms] **два** з+амок sil<[100]> <|emotion:joy|>три"),
            "Раз два замок три"
        );
    }
}

#[cfg(test)]
mod projection_tests {
    use super::visible;

    #[test]
    fn pauses_and_rates_of_every_supported_spelling_leave_only_words() {
        for tag in [
            "[pause 500ms]",
            "[PAUSE 200]",
            "[pause 1.5s]",
            "[pause 1,5s]",
            "sil<[300]>",
            "<[tiny]>",
            "<[small]>",
            "<[medium]>",
            "<[large]>",
            "<[huge]>",
            "[rate:0.8]",
            "[rate:0,8]",
        ] {
            assert_eq!(visible(&format!("Раз {tag} два")), "Раз два", "{tag}");
        }
    }

    #[test]
    fn projection_keeps_unicode_words_and_punctuation_under_markup() {
        assert_eq!(
            visible("\t**З+амок**\u{a0}[[café]]\n<[accented]><whisper>你好</whisper> <|emotion:awe|>мир!"),
            "Замок café 你好 мир!"
        );
    }

    #[test]
    fn incomplete_markup_and_literal_operators_are_left_as_text() {
        for text in [
            "**открыто",
            "[[не закрыто",
            "[pause пять]",
            "<не закрыто",
            "C++ и 2+2",
            "[обычная ремарка]",
        ] {
            assert_eq!(visible(text), text, "{text}");
        }
    }

    #[test]
    fn empty_and_markup_only_text_has_no_visible_words() {
        for text in ["", " \t\n", "[pause 500ms] [rate:0.8] sil<[200]> <[accented]> <|sfx:laughter|>"] {
            assert!(visible(text).is_empty(), "{text}");
        }
    }
}
