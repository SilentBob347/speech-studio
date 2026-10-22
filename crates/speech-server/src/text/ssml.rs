//! The part of SSML a script may carry, turned into the studio's own markup before anything else reads the text:
//! `<break>` becomes a pause, `<prosody rate>` a tempo, `<sub alias>` its alias, `<say-as interpret-as="characters">`
//! the letters apart; `<speak>`, `<p>`, `<s>` and the other wrappers come off, their words stay. `<emphasis>` stays:
//! Drama 3 reads it, and the other engines' adapters take it off.

use std::sync::LazyLock;

use regex::{Captures, Regex};

static BREAK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)<break\b([^>]*?)/?>"#).expect("break"));
static TIME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\btime\s*=\s*["']\s*(\d+(?:\.\d+)?)\s*(ms|s)\s*["']"#).expect("time"));
static STRENGTH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\bstrength\s*=\s*["']\s*([\w-]+)\s*["']"#).expect("strength"));
static PROSODY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<prosody\b[^>]*?\brate\s*=\s*["']\s*([^"']+?)\s*["'][^>]*>(.*?)</prosody>"#).expect("prosody"));
static SUB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<sub\b[^>]*?\balias\s*=\s*["']([^"']*)["'][^>]*>.*?</sub>"#).expect("sub"));
static SAY_AS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<say-as\b[^>]*?\binterpret-as\s*=\s*["']\s*(characters|spell-out)\s*["'][^>]*>(.*?)</say-as>"#).expect("say-as"));
static WRAPPER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</?(?:speak|p|s|prosody|say-as|voice|lang|mark|audio)\b[^>]*>").expect("wrapper"));
static OPENS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<(?:speak|break|prosody|sub|say-as)\b").expect("ssml"));

/// What was turned into what, for the trace of the preparation.
pub struct Hit {
    pub rule: &'static str,
    pub from: String,
    pub to: String,
}

fn pause_of(attrs: &str) -> String {
    if let Some(c) = TIME.captures(attrs) {
        return format!("[pause {}{}]", &c[1], c[2].to_ascii_lowercase());
    }
    let ms = match STRENGTH.captures(attrs).map(|c| c[1].to_ascii_lowercase()) {
        Some(s) if s == "none" => 0,
        Some(s) if s == "x-weak" => 100,
        Some(s) if s == "weak" => 250,
        Some(s) if s == "strong" => 750,
        Some(s) if s == "x-strong" => 1000,
        _ => 500,
    };
    if ms == 0 { String::new() } else { format!("[pause {ms}ms]") }
}

/// A prosody rate as a tempo: the SSML words, a percentage of the normal one, or a number.
fn rate_of(rate: &str) -> Option<f64> {
    let r = rate.trim().to_ascii_lowercase();
    let value = match r.as_str() {
        "x-slow" => 0.75,
        "slow" => 0.9,
        "medium" | "default" => 1.0,
        "fast" => 1.1,
        "x-fast" => 1.25,
        _ => match r.strip_suffix('%') {
            Some(p) => p.trim().parse::<f64>().ok()? / 100.0,
            None => r.parse::<f64>().ok()?,
        },
    };
    (value > 0.0).then_some(value)
}

/// The text with its SSML turned into the studio's markup, and each change; text without SSML comes back as it is.
pub fn to_markup(text: &str) -> (String, Vec<Hit>) {
    if !OPENS.is_match(text) {
        return (text.to_string(), Vec::new());
    }
    let mut hits = Vec::new();
    let mut note = |rule: &'static str, from: &str, to: &str| hits.push(Hit { rule, from: from.to_string(), to: to.to_string() });
    let t = SUB.replace_all(text, |c: &Captures| {
        note("sub", &c[0], &c[1]);
        c[1].to_string()
    });
    let t = SAY_AS.replace_all(&t, |c: &Captures| {
        let spelled = c[2].chars().filter(|ch| !ch.is_whitespace()).map(String::from).collect::<Vec<_>>().join(" ");
        note("say-as", &c[0], &spelled);
        spelled
    });
    let t = PROSODY.replace_all(&t, |c: &Captures| match rate_of(&c[1]) {
        Some(rate) => {
            let to = format!("[rate:{rate}]{}[rate:1]", &c[2]);
            note("prosody", &c[0], &to);
            to
        }
        None => c[2].to_string(),
    });
    let t = BREAK.replace_all(&t, |c: &Captures| {
        let to = pause_of(&c[1]);
        note("break", &c[0], &to);
        format!(" {to} ")
    });
    let t = WRAPPER.replace_all(&t, " ");
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    (t, hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssml_becomes_pauses_tempo_and_words() {
        let (t, hits) = to_markup(r#"<speak><p>Привет.<break time="700ms"/>Это <sub alias="Всемирная организация здравоохранения">ВОЗ</sub>, код <say-as interpret-as="characters">AB1</say-as>. <prosody rate="80%">Медленно</prosody><break strength="strong"/>конец.</p></speak>"#);
        assert_eq!(t, "Привет. [pause 700ms] Это Всемирная организация здравоохранения, код A B 1. [rate:0.8]Медленно[rate:1] [pause 750ms] конец.");
        assert_eq!(hits.iter().map(|h| h.rule).collect::<Vec<_>>(), vec!["sub", "say-as", "prosody", "break", "break"]);
    }

    #[test]
    fn text_without_ssml_and_our_own_tags_are_left_alone() {
        for t in ["Просто текст [pause 500ms] и <|emotion:anger|> тег.", "Шёпот <whisper>тут</whisper>, <emphasis>важно</emphasis>."] {
            assert_eq!(to_markup(t).0, t);
        }
    }
}
