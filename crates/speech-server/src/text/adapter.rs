//! Pure synthesis format adapters; the caller chooses the format and supplies its text.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use super::markup::{ACCENT, ACCENT_MARK, ANGLE, HIGGS_TAG, PHONEMES, STRESS};

#[derive(Clone, Copy)]
pub(crate) enum Adapter {
    Higgs,
    Drama,
    Plain { acute: bool, own: &'static [&'static str] },
}

impl Adapter {
    pub(crate) fn apply(self, say: &str, lang: &str) -> (String, Ignored) {
        match self {
            Self::Higgs => for_higgs(say, lang),
            Self::Drama => for_drama(say, lang),
            Self::Plain { acute, own } => for_plain(say, lang, acute, own),
        }
    }
}

/// Теги экспрессии Higgs: категория → значения (белый список HiggsAudio-Studio `director_worker.py`).
pub const HIGGS_TAGS: [(&str, &[&str]); 4] = [
    (
        "emotion",
        &[
            "affection",
            "amusement",
            "anger",
            "arousal",
            "awe",
            "bitterness",
            "confusion",
            "contemplation",
            "contentment",
            "determination",
            "disgust",
            "elation",
            "enthusiasm",
            "fear",
            "helplessness",
            "longing",
            "pride",
            "relief",
            "sadness",
            "shame",
            "surprise",
        ],
    ),
    (
        "prosody",
        &[
            "speed_very_slow",
            "speed_slow",
            "speed_fast",
            "speed_very_fast",
            "pitch_low",
            "pitch_high",
            "expressive_high",
            "expressive_low",
            "pause",
            "long_pause",
        ],
    ),
    ("style", &["singing", "shouting", "whispering"]),
    (
        "sfx",
        &[
            "cough",
            "laughter",
            "crying",
            "screaming",
            "burping",
            "humming",
            "sigh",
            "sniff",
            "sneeze",
        ],
    ),
];

/// Теги Higgs в начало предложения (окрашивают всё предложение); остальные — по месту (`PROMPTING.md` модели).
pub const HIGGS_SENTENCE_TAGS: &[&str] = &[
    "emotion",
    "style",
    "speed_very_slow",
    "speed_slow",
    "speed_fast",
    "speed_very_fast",
    "pitch_low",
    "pitch_high",
    "expressive_high",
    "expressive_low",
];

/// Теги Supertonic 3 по карточке `Supertone/supertonic-3`: `<laugh>`, `<breath>`, `<sigh>` по месту.
pub const SUPERTONIC_TAGS: [(&str, &[&str]); 1] = [("event", &["laugh", "breath", "sigh"])];

fn higgs_tag_known(cat: &str, value: &str) -> bool {
    HIGGS_TAGS.iter().any(|(c, values)| *c == cat && values.contains(&value))
}

/// Что разметка блока просит, а движок не умеет: для честной пометки в окне.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct Ignored {
    pub accent: bool,
    pub phonemes: bool,
    pub stress: bool,
    pub unknown_tags: Vec<String>,
}

/// Точечный тег Drama 3: событие в квадратных скобках по месту — `[laughs]`, `[sighs]`.
static DRAMA_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([\p{L}][\p{L} '-]{0,38})\]").expect("drama tag"));
/// Тег Drama 3 на отрезок: `<whisper>…</whisper>` задаёт подачу только обёрнутых слов.
static DRAMA_SCOPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"</?[a-z][a-z -]{0,30}>").expect("drama scope"));

/// Палитра Drama 3 — примеры приложения C техотчёта (Table 10): события по месту и подача отрезка; теги — свободный текст.
pub const DRAMA_TAGS: [(&str, &[&str]); 2] = [
    ("event", &["laughs", "sighs", "gasps", "clears throat", "hesitates", "pause"]),
    ("scope", &["whisper", "slow", "emphasis"]),
];

/// Фраза для Drama 3: ударения, фонемы и теги Higgs снимаются (что снято — в `Ignored`), точечные теги остаются.
pub fn for_drama(say: &str, lang: &str) -> (String, Ignored) {
    let mut ignored = Ignored::default();
    let t = ACCENT.replace_all(say, |c: &regex::Captures| {
        ignored.accent = true;
        c[1].to_string()
    });
    let t = ACCENT_MARK.replace_all(&t, |_: &regex::Captures| {
        ignored.accent = true;
        String::new()
    });
    let t = PHONEMES.replace_all(&t, |c: &regex::Captures| {
        ignored.phonemes = true;
        c[1].to_string()
    });
    let t = STRESS.replace_all(&t, |c: &regex::Captures| {
        ignored.stress = true;
        c[1].to_string()
    });
    let mut tags: Vec<String> = Vec::new();
    let t = DRAMA_SCOPE.replace_all(&t, |c: &regex::Captures| {
        tags.push(c[0].to_string());
        format!("\u{E000}{}\u{E001}", tags.len() - 1)
    });
    let t = ANGLE.replace_all(&t, |c: &regex::Captures| {
        ignored.unknown_tags.push(c[0].to_string());
        String::new()
    });
    let t = DRAMA_TAG.replace_all(&t, |c: &regex::Captures| {
        tags.push(c[0].to_string());
        format!("\u{E000}{}\u{E001}", tags.len() - 1)
    });
    let cleaned = super::tts::tts_text(&t, &t, lang, &[]);
    let mut text = cleaned.text;
    for (i, tag) in tags.iter().enumerate() {
        text = text.replace(&format!("\u{E000}{i}\u{E001}"), tag);
    }
    (text, ignored)
}

/// Фраза для Higgs: разметка, которую он не понимает, снимается; что снято — в `Ignored`.
pub fn for_higgs(say: &str, lang: &str) -> (String, Ignored) {
    let mut ignored = Ignored::default();
    let t = ACCENT.replace_all(say, |c: &regex::Captures| {
        ignored.accent = true;
        c[1].to_string()
    });
    let t = ACCENT_MARK.replace_all(&t, |_: &regex::Captures| {
        ignored.accent = true;
        String::new()
    });
    let t = PHONEMES.replace_all(&t, |c: &regex::Captures| {
        ignored.phonemes = true;
        c[1].to_string()
    });
    let t = STRESS.replace_all(&t, |c: &regex::Captures| {
        ignored.stress = true;
        c[1].to_string()
    });
    let mut tags: Vec<String> = Vec::new();
    let t = ANGLE.replace_all(&t, |c: &regex::Captures| {
        let whole = &c[0];
        match HIGGS_TAG.captures(whole) {
            Some(h) if higgs_tag_known(&h[1], &h[2]) => {
                tags.push(whole.to_string());
                format!("\u{E000}{}\u{E001}", tags.len() - 1)
            }
            _ => {
                ignored.unknown_tags.push(whole.to_string());
                String::new()
            }
        }
    });
    let cleaned = super::tts::tts_text(&t, &t, lang, &[]);
    let mut text = cleaned.text;
    for (i, tag) in tags.iter().enumerate() {
        text = text.replace(&format!("\u{E000}{i}\u{E001}"), tag);
    }
    (text, ignored)
}

/// Фраза для движка без разметки Higgs и Drama (Supertonic, Qwen3-TTS): теги, фонемы и выделение снимаются, кроме
/// своих тегов движка `own` вида `<laugh>`; что снято — в `Ignored`. С `acute` ударение «+» перед гласной становится
/// знаком U+0301 после неё — так его читают Supertonic 3 и Qwen3-TTS с адаптером ударений; без него ударение снимается.
pub fn for_plain(say: &str, lang: &str, acute: bool, own: &[&str]) -> (String, Ignored) {
    let mut ignored = Ignored::default();
    let t = ACCENT.replace_all(say, |c: &regex::Captures| {
        ignored.accent = true;
        c[1].to_string()
    });
    let t = ACCENT_MARK.replace_all(&t, |_: &regex::Captures| {
        ignored.accent = true;
        String::new()
    });
    let t = PHONEMES.replace_all(&t, |c: &regex::Captures| {
        ignored.phonemes = true;
        c[1].to_string()
    });
    let t = STRESS.replace_all(&t, |c: &regex::Captures| {
        if acute {
            format!("{}\u{E002}", &c[1])
        } else {
            ignored.stress = true;
            c[1].to_string()
        }
    });
    let mut tags: Vec<String> = Vec::new();
    let t = ANGLE.replace_all(&t, |c: &regex::Captures| {
        let whole = &c[0];
        if own.iter().any(|o| whole[1..whole.len() - 1].trim() == *o) {
            tags.push(whole.to_string());
            return format!("\u{E000}{}\u{E001}", tags.len() - 1);
        }
        ignored.unknown_tags.push(whole.to_string());
        String::new()
    });
    let t = DRAMA_TAG.replace_all(&t, |c: &regex::Captures| {
        ignored.unknown_tags.push(c[0].to_string());
        String::new()
    });
    let mut text = super::tts::tts_text(&t, &t, lang, &[]).text.replace('\u{E002}', "\u{301}");
    for (i, tag) in tags.iter().enumerate() {
        text = text.replace(&format!("\u{E000}{i}\u{E001}"), tag);
    }
    (text, ignored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drama_keeps_its_point_tags_and_drops_higgs_ones() {
        let (text, ig) = for_drama(
            "[laughs] Ну д+авай <|emotion:awe|> уже [sighs] <whisper>тихо</whisper> пойдём.",
            "ru",
        );
        assert!(text.contains("[laughs]") && text.contains("[sighs]"), "{text}");
        assert!(text.contains("<whisper>тихо</whisper>"), "the scoped tag stays: {text}");
        assert!(!text.contains("<|") && !text.contains('+'), "{text}");
        assert!(ig.stress && ig.unknown_tags == vec!["<|emotion:awe|>".to_string()]);
    }

    #[test]
    fn a_higgs_sound_stays_glued_to_its_onomatopoeia() {
        let (s, _) = for_higgs("<|emotion:elation|><|sfx:laughter|>Ха-ха, привет!", "ru");
        assert!(s.contains("<|sfx:laughter|>Ха-ха"), "{s}");
    }

    #[test]
    fn markup_higgs_does_not_read_is_taken_off_and_reported() {
        let (t, ig) = for_higgs("Это **очень** важно, з+амок и [[zamok]] <[accented]>тут", "ru");
        assert_eq!(t, "Это очень важно, замок и zamok тут");
        assert_eq!(
            ig,
            Ignored {
                accent: true,
                phonemes: true,
                stress: true,
                unknown_tags: vec![]
            }
        );
        let (t, ig) = for_higgs("Обычная фраза.", "ru");
        assert_eq!((t.as_str(), ig), ("Обычная фраза.", Ignored::default()));
    }

    #[test]
    fn known_higgs_tags_reach_the_engine_and_made_up_ones_are_cut_and_named() {
        let (t, ig) = for_higgs(
            "<|emotion:anger|> Хватит! <|sfx:laughter|> Шучу. <|emotion:rage|> <b>жирный</b>",
            "ru",
        );
        assert_eq!(t, "<|emotion:anger|> Хватит! <|sfx:laughter|> Шучу. жирный");
        assert_eq!(
            ig.unknown_tags,
            vec!["<|emotion:rage|>".to_string(), "<b>".to_string(), "</b>".to_string()]
        );
    }
}
