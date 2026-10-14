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
}

static PAUSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\[\s*pause\s+(\d+(?:[.,]\d+)?)\s*(ms|s)?\s*\]|sil\s*<\[\s*(\d+)\s*\]>|<\[\s*(tiny|small|medium|large|huge)\s*\]>").expect("pause")
});
static ACCENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*\n]{1,80})\*\*").expect("accent"));
static ACCENT_MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<\[\s*accented\s*\]>").expect("accented"));
static PHONEMES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\[\]]{1,120})\]\]").expect("phonemes"));
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
    let cleaned = crate::tts_text::tts_text(&t, &t, lang, &[]);
    (cleaned.text, ignored)
}

/// Видимый текст без разметки синтеза: для субтитров, глав и поиска.
pub fn visible(text: &str) -> String {
    let t = PAUSE.replace_all(text, " ");
    let t = ACCENT.replace_all(&t, "$1");
    let t = ACCENT_MARK.replace_all(&t, "");
    let t = PHONEMES.replace_all(&t, "$1");
    let t = STRESS.replace_all(&t, "$1");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
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
    fn neighbouring_pauses_add_up_and_stop_at_ten_seconds() {
        assert_eq!(pieces("[pause 6s][pause 6s] да"), vec![Piece::Pause(10_000), Piece::Say("да".into())]);
        assert_eq!(pieces("[PAUSE 200] да"), vec![Piece::Pause(200), Piece::Say("да".into())]);
        assert_eq!(pieces("просто текст"), vec![Piece::Say("просто текст".into())]);
    }

    #[test]
    fn markup_higgs_does_not_read_is_taken_off_and_reported() {
        let (t, ig) = for_higgs("Это **очень** важно, з+амок и [[zamok]] <[accented]>тут", "ru");
        assert_eq!(t, "Это очень важно, замок и zamok тут");
        assert_eq!(ig, Ignored { accent: true, phonemes: true, stress: true });
        let (t, ig) = for_higgs("Обычная фраза.", "ru");
        assert_eq!((t.as_str(), ig), ("Обычная фраза.", Ignored::default()));
    }

    #[test]
    fn the_visible_text_has_no_synthesis_markup() {
        assert_eq!(visible("Раз [pause 300ms] **два** з+амок sil<[100]>"), "Раз два замок");
    }
}
