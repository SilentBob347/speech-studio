//! Shared synthesis markup syntax and its visible text projection, independent of projects and engines.

use std::sync::LazyLock;

use regex::Regex;

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

#[cfg(test)]
mod tests {
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
