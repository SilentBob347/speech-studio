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

/// Теги Higgs в начало предложения (окрашивают всё предложение); остальные — по месту (`PROMPTING.md` модели).
pub const HIGGS_SENTENCE_TAGS: &[&str] = &["emotion", "style", "speed_very_slow", "speed_slow", "speed_fast", "speed_very_fast", "pitch_low", "pitch_high", "expressive_high", "expressive_low"];

/// Теги Supertonic 3 по карточке `Supertone/supertonic-3`: `<laugh>`, `<breath>`, `<sigh>` по месту.
pub const SUPERTONIC_TAGS: [(&str, &[&str]); 1] = [("event", &["laugh", "breath", "sigh"])];

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
    match engine {
        crate::models::DRAMA_TTS => for_drama(say, lang),
        crate::supertonic::ENGINE => for_plain(say, lang, true, SUPERTONIC_TAGS[0].1),
        crate::qwen_tts::ENGINE_STRESS => for_plain(say, lang, true, &[]),
        crate::qwen_tts::ENGINE => for_plain(say, lang, false, &[]),
        _ => for_higgs(say, lang),
    }
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
    let mut text = crate::tts_text::tts_text(&t, &t, lang, &[]).text.replace('\u{E002}', "\u{301}");
    for (i, tag) in tags.iter().enumerate() {
        text = text.replace(&format!("\u{E000}{i}\u{E001}"), tag);
    }
    (text, ignored)
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
    out.iter().enumerate().map(|(k, (s, _))| (s.clone(), if k == 0 { 0 } else { gap(out[k - 1].1) })).collect()
}

/// Предложения: конец — `.`, `!`, `?` или `…` с пробелом после, кроме сокращений.
fn sentences(text: &str) -> Vec<&str> {
    const ABBREVIATIONS: &[&str] = &["Dr.", "Mr.", "Mrs.", "Ms.", "Prof.", "Sr.", "Jr.", "St.", "etc.", "vs.", "i.e.", "e.g.", "т.е.", "т.д.", "т.п.", "др.", "г.", "гг.", "им.", "ул.", "св."];
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (i, &(at, c)) in chars.iter().enumerate() {
        if !matches!(c, '.' | '!' | '?' | '…') || !chars.get(i + 1).is_some_and(|&(_, n)| n.is_whitespace()) {
            continue;
        }
        let end = at + c.len_utf8();
        let head = &text[start..end];
        if c == '.' && ABBREVIATIONS.iter().any(|a| head.ends_with(a) && head[..head.len() - a.len()].chars().last().is_none_or(|p| !p.is_alphanumeric())) {
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
        let (text, ig) = for_drama("[laughs] Ну д+авай <|emotion:awe|> уже [sighs] <whisper>тихо</whisper> пойдём.", "ru");
        assert!(text.contains("[laughs]") && text.contains("[sighs]"), "{text}");
        assert!(text.contains("<whisper>тихо</whisper>"), "the scoped tag stays: {text}");
        assert!(!text.contains("<|") && !text.contains('+'), "{text}");
        assert!(ig.stress && ig.unknown_tags == vec!["<|emotion:awe|>".to_string()]);
    }


    #[test]
    fn supertonic_keeps_its_own_tags_and_drops_the_rest() {
        let (s, ig) = for_engine("Ну <laugh> хватит <|emotion:happy|> [laughs] <whisper>уже</whisper>.", "ru", crate::supertonic::ENGINE);
        assert!(s.contains("<laugh>") && !s.contains("<|") && !s.contains("[laughs]") && !s.contains("<whisper>"), "{s}");
        assert_eq!(ig.unknown_tags.len(), 4, "{ig:?}");
    }

    #[test]
    fn a_higgs_sound_stays_glued_to_its_onomatopoeia() {
        let (s, _) = for_higgs("<|emotion:elation|><|sfx:laughter|>Ха-ха, привет!", "ru");
        assert!(s.contains("<|sfx:laughter|>Ха-ха"), "{s}");
    }

    #[test]
    fn engines_without_own_markup_get_plain_text() {
        for engine in [crate::qwen_tts::ENGINE] {
            let (s, ignored) = for_engine("Он зашёл в з+амок <|emotion:happy|> [laughs] сразу.", "ru", engine);
            assert!(s.contains("замок") && !s.contains('\u{301}') && !s.contains('+') && !s.contains("<|") && !s.contains("[laughs]"), "{engine}: {s}");
            assert!(ignored.stress && ignored.unknown_tags.len() == 2, "{engine}: {ignored:?}");
        }
    }

    #[test]
    fn long_text_is_cut_into_phrases_by_sentences_then_commas_then_words() {
        let texts = |t: &str, max| phrases(t, max).into_iter().map(|(s, _)| s).collect::<Vec<_>>();
        assert_eq!(phrases("Коротко.", 20), vec![("Коротко.".to_string(), 0)]);
        assert_eq!(phrases("Раз два три. Четыре пять! Шесть семь восемь?", 26), vec![("Раз два три. Четыре пять!".to_string(), 0), ("Шесть семь восемь?".to_string(), SENTENCE_GAP_MS)]);
        assert_eq!(texts("Он пришёл в т.е. другой дом. Вот так.", 30), vec!["Он пришёл в т.е. другой дом.", "Вот так."]);
        assert_eq!(phrases("один два три четыре, пять шесть семь восемь", 22), vec![("один два три четыре".to_string(), 0), ("пять шесть семь восемь".to_string(), COMMA_GAP_MS)]);
        let long = "слово ".repeat(40);
        let cut = phrases(&long, 50);
        assert!(cut.len() > 1 && cut.iter().all(|(p, _)| p.chars().count() <= 50), "{cut:?}");
        assert!(cut.iter().skip(1).all(|(_, gap)| *gap == WORD_GAP_MS), "{cut:?}");
        assert_eq!(cut.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>().join(" "), long.trim());
    }

    #[test]
    fn supertonic_and_the_stress_adapter_read_an_acute_after_the_vowel() {
        for engine in [crate::qwen_tts::ENGINE_STRESS, crate::supertonic::ENGINE] {
            let (q, ignored) = for_engine("Он зашёл в з+амок <|emotion:happy|> [laughs] сразу.", "ru", engine);
            assert!(q.contains("за\u{301}мок"), "{engine}: {q}");
            assert!(!q.contains('+') && !q.contains("<|") && !q.contains("[laughs]") && !q.contains('\u{E002}'), "{engine}: {q}");
            assert!(!ignored.stress && ignored.unknown_tags.len() == 2, "{engine}: {ignored:?}");
        }
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
