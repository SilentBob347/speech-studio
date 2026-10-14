//! Текст блока → куски для синтеза. Паузы (`[pause 500ms]`, `[pause 1.5s]`, разметка Яндекс SpeechKit `sil<[300]>` и
//! `<[tiny|small|medium|large|huge]>`) режут блок на фразы, тишина между ними склеивается, а не синтезируется. Разметка
//! акцента `**слово**` и `<[accented]>`, фонемы `[[…]]` снимаются: Higgs их не понимает, и окно говорит об этом.
//! Ударение `+` перед гласной превращается в то, что понимает движок (для Higgs — снимается, пока проверка на слух
//! не покажет обратного). Остальное чистит `tts_text` Dub.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

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

static PAUSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\[\s*pause\s+(\d+(?:[.,]\d+)?)\s*(ms|s)?\s*\]|sil\s*<\[\s*(\d+)\s*\]>|<\[\s*(tiny|small|medium|large|huge)\s*\]>|\[\s*rate\s*:\s*(\d+(?:[.,]\d+)?)\s*\]").expect("pause")
});
static ACCENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*\n]{1,80})\*\*").expect("accent"));
static ACCENT_MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<\[\s*accented\s*\]>").expect("accented"));
static PHONEMES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\[\]]{1,120})\]\]").expect("phonemes"));
static ANGLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^<>\n]{1,48}>").expect("angle"));
static HIGGS_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^<\|(\w+):(\w+)\|>$").expect("higgs tag"));

/// Теги экспрессии Higgs: категория → значения (белый список HiggsAudio-Studio `director_worker.py`).
pub const HIGGS_TAGS: [(&str, &[&str]); 4] = [
    ("emotion", &["affection", "amusement", "anger", "arousal", "awe", "bitterness", "confusion", "contemplation", "contentment", "determination", "disgust", "elation", "enthusiasm", "fear", "helplessness", "longing", "pride", "relief", "sadness", "shame", "surprise"]),
    ("prosody", &["speed_very_slow", "speed_slow", "speed_fast", "speed_very_fast", "pitch_low", "pitch_high", "expressive_high", "expressive_low", "pause", "long_pause"]),
    ("style", &["singing", "shouting", "whispering"]),
    ("sfx", &["cough", "laughter", "crying", "screaming", "burping", "humming", "sigh", "sniff", "sneeze"]),
];

fn higgs_tag_known(cat: &str, value: &str) -> bool {
    HIGGS_TAGS.iter().any(|(c, values)| *c == cat && values.contains(&value))
}

static STRESS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\+([АаЕеЁёИиОоУуЫыЭэЮюЯяAaEeIiOoUuYy])").expect("stress"));

fn context_pause(word: &str) -> u32 {
    match word.to_ascii_lowercase().as_str() {
        "tiny" => 100,
        "small" => 250,
        "medium" => 500,
        "large" => 1000,
        _ => 2000,
    }
}

/// Что разметка блока просит, а движок не умеет: для честной пометки в окне.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct Ignored {
    pub accent: bool,
    pub phonemes: bool,
    pub stress: bool,
    pub unknown_tags: Vec<String>,
}

/// Куски блока: фразы и паузы между ними; пустые фразы выпадают, соседние паузы складываются.
pub fn pieces(text: &str) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut push_say = |out: &mut Vec<Piece>, s: &str| {
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
            let rate = r.as_str().replace(',', ".").parse::<f64>().unwrap_or(1.0).clamp(RATE_RANGE.0, RATE_RANGE.1);
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

/// Фраза для движка проекта: у каждого своя разметка экспрессии.
pub fn for_engine(say: &str, lang: &str, engine: &str) -> (String, Ignored) {
    if engine == crate::models::DRAMA_TTS { for_drama(say, lang) } else { for_higgs(say, lang) }
}

/// Точечный тег Drama 3: событие в квадратных скобках по месту — `[laughs]`, `[sighs]`.
static DRAMA_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([\p{L}][\p{L} '-]{0,38})\]").expect("drama tag"));

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
    let t = ANGLE.replace_all(&t, |c: &regex::Captures| {
        ignored.unknown_tags.push(c[0].to_string());
        String::new()
    });
    let mut tags: Vec<String> = Vec::new();
    let t = DRAMA_TAG.replace_all(&t, |c: &regex::Captures| {
        tags.push(c[0].to_string());
        format!("\u{E000}{}\u{E001}", tags.len() - 1)
    });
    let cleaned = crate::tts_text::tts_text(&t, &t, lang, &[]);
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
    let cleaned = crate::tts_text::tts_text(&t, &t, lang, &[]);
    let mut text = cleaned.text;
    for (i, tag) in tags.iter().enumerate() {
        text = text.replace(&format!("\u{E000}{i}\u{E001}"), tag);
    }
    (text, ignored)
}

/// Есть ли в тексте ручные ударения `+`.
pub fn has_stress(text: &str) -> bool {
    STRESS.is_match(text)
}

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

#[cfg(test)]
mod tests {
    #[test]
    fn drama_keeps_its_point_tags_and_drops_higgs_ones() {
        let (text, ig) = for_drama("[laughs] Ну д+авай <|emotion:awe|> уже [sighs] пойдём.", "ru");
        assert!(text.contains("[laughs]") && text.contains("[sighs]"), "{text}");
        assert!(!text.contains("<|") && !text.contains('+'), "{text}");
        assert!(ig.stress && ig.unknown_tags == vec!["<|emotion:awe|>".to_string()]);
    }

    use super::*;

    #[test]
    fn pauses_of_every_spelling_cut_the_block() {
        let p = pieces("Раз. [pause 500ms] Два, sil<[300]> три <[large]> четыре [pause 1,5s]");
        assert_eq!(p, vec![
            Piece::Say("Раз.".into()),
            Piece::Pause(500),
            Piece::Say("Два,".into()),
            Piece::Pause(300),
            Piece::Say("три".into()),
            Piece::Pause(1000),
            Piece::Say("четыре".into()),
            Piece::Pause(1500),
        ]);
    }

    #[test]
    fn rate_tags_split_the_block_and_are_clamped() {
        assert_eq!(pieces("Раз [rate:1.5] два [rate:9] три"), vec![
            Piece::Say("Раз".into()),
            Piece::Rate(1.5),
            Piece::Say("два".into()),
            Piece::Rate(4.0),
            Piece::Say("три".into()),
        ]);
        assert_eq!(visible("Раз [rate:0,8] два"), "Раз два");
    }

    #[test]
    fn neighbouring_pauses_add_up_and_stop_at_ten_seconds() {
        assert_eq!(pieces("[pause 6s][pause 6s] да"), vec![Piece::Pause(10_000), Piece::Say("да".into())]);
        assert_eq!(pieces("[PAUSE 200] да"), vec![Piece::Pause(200), Piece::Say("да".into())]);
        assert_eq!(pieces("просто текст"), vec![Piece::Say("просто текст".into())]);
    }

    #[test]
    fn markup_higgs_does_not_read_is_taken_off_and_reported() {
        let (t, ig) = for_higgs("Это **очень** важно, з+амок и [[zamok]] <[accented]>тут", "ru");
        assert_eq!(t, "Это очень важно, замок и zamok тут");
        assert_eq!(ig, Ignored { accent: true, phonemes: true, stress: true, unknown_tags: vec![] });
        let (t, ig) = for_higgs("Обычная фраза.", "ru");
        assert_eq!((t.as_str(), ig), ("Обычная фраза.", Ignored::default()));
    }

    #[test]
    fn known_higgs_tags_reach_the_engine_and_made_up_ones_are_cut_and_named() {
        let (t, ig) = for_higgs("<|emotion:anger|> Хватит! <|sfx:laughter|> Шучу. <|emotion:rage|> <b>жирный</b>", "ru");
        assert_eq!(t, "<|emotion:anger|> Хватит! <|sfx:laughter|> Шучу. жирный");
        assert_eq!(ig.unknown_tags, vec!["<|emotion:rage|>".to_string(), "<b>".to_string(), "</b>".to_string()]);
    }

    #[test]
    fn the_visible_text_has_no_synthesis_markup() {
        assert_eq!(visible("Раз [pause 300ms] **два** з+амок sil<[100]> <|emotion:joy|>три"), "Раз два замок три");
    }
}
