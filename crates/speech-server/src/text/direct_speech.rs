//! Direct speech apart from the author's words, by the way books print it: a paragraph opening with a dash is a line
//! whose author's words stand between dashes (`— Привет, — сказал он. — Как дела?`), and a quotation is a line when a
//! colon leads into it, it opens the paragraph or the author's words follow it after a dash. A quotation inside the
//! narration that is none of these (a title, a word in quotes) stays the narrator's.

use std::sync::LazyLock;

use regex::Regex;

static DASH_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[—–]\s*|^-\s+").expect("dash start"));
static DASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+[—–-]\s+").expect("dash"));
static QUOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"«[^«»]+»|“[^“”]+”|"[^"]+""#).expect("quote"));
/// Marks right after a closing quote that end the line itself.
static AFTER_QUOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[.!?…]+").expect("after quote"));
/// The author's words after a quotation: a comma or the line's own mark, then a dash.
static AUTHOR_AFTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[,.!?…]*\s*[—–-]\s").expect("author after"));

fn has_letters(s: &str) -> bool {
    s.chars().any(char::is_alphabetic)
}

fn ends_with_any(s: &str, marks: &str) -> bool {
    s.trim_end_matches(['»', '”', '"', ')']).chars().last().is_some_and(|c| marks.contains(c))
}

fn starts_lower(s: &str) -> bool {
    s.chars().find(|c| c.is_alphabetic()).is_some_and(char::is_lowercase)
}

/// One paragraph cut into the speakers' parts: `true` for direct speech, `false` for the narrator. A paragraph with
/// no direct speech comes back whole as the narrator's.
pub fn split(paragraph: &str) -> Vec<(bool, String)> {
    let p = paragraph.trim();
    let mut out: Vec<(bool, String)> = Vec::new();
    let mut push = |speech: bool, s: &str| {
        let s = s.trim().trim_start_matches([',', ';', '—', '–', ' ']).trim();
        if s.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.0 == speech || !has_letters(s) => {
                last.1.push(' ');
                last.1.push_str(s);
            }
            _ => out.push((speech, s.to_string())),
        }
    };
    if let Some(m) = DASH_START.find(p) {
        let rest = &p[m.end()..];
        let mut speech = true;
        let mut cur = String::new();
        let mut last = 0;
        for d in DASH.find_iter(rest) {
            let next = &rest[d.end()..];
            let part = &rest[last..d.start()];
            let cur_with = if cur.is_empty() { part.to_string() } else { format!("{cur} — {part}") };
            let switch = if speech { ends_with_any(&cur_with, ",.!?…") && starts_lower(next) } else { ends_with_any(&cur_with, ",.:;!?…") };
            if switch {
                push(speech, &cur_with);
                cur.clear();
                speech = !speech;
            } else {
                cur = cur_with;
            }
            last = d.end();
        }
        let tail = &rest[last..];
        push(speech, &if cur.is_empty() { tail.to_string() } else { format!("{cur} — {tail}") });
        return out;
    }
    let mut last = 0;
    for q in QUOTE.find_iter(p) {
        let before = &p[last..q.start()];
        let after = &p[q.end()..];
        let is_line = before.trim().is_empty() && last == 0 || before.trim_end().ends_with(':') || AUTHOR_AFTER.is_match(after);
        if !is_line || !has_letters(q.as_str()) {
            continue;
        }
        let marks = AFTER_QUOTE.find(after).map_or(0, |m| m.end());
        push(false, before);
        push(true, &p[q.start()..q.end() + marks]);
        last = q.end() + marks;
    }
    push(false, &p[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(p: &str) -> Vec<(bool, String)> {
        split(p)
    }

    fn want(v: &[(bool, &str)]) -> Vec<(bool, String)> {
        v.iter().map(|(s, t)| (*s, t.to_string())).collect()
    }

    #[test]
    fn a_dialogue_dash_line_and_the_author_s_words_between_dashes() {
        assert_eq!(parts("— Привет, — сказал Олег. — Как дела?"), want(&[(true, "Привет,"), (false, "сказал Олег."), (true, "Как дела?")]));
        assert_eq!(parts("— Привет, — сказал он, — как дела?"), want(&[(true, "Привет,"), (false, "сказал он,"), (true, "как дела?")]));
        assert_eq!(parts("— Ты пришёл! — обрадовалась Анна."), want(&[(true, "Ты пришёл!"), (false, "обрадовалась Анна.")]));
        assert_eq!(parts("— Москва — большой город."), want(&[(true, "Москва — большой город.")]));
        assert_eq!(parts("- Да."), want(&[(true, "Да.")]));
    }

    #[test]
    fn a_quotation_is_a_line_after_a_colon_at_the_start_or_before_the_author_s_words() {
        assert_eq!(parts("Он сказал: «Я вернусь». И ушёл."), want(&[(false, "Он сказал:"), (true, "«Я вернусь»."), (false, "И ушёл.")]));
        assert_eq!(parts("«Я вернусь», — сказал он."), want(&[(true, "«Я вернусь»"), (false, "сказал он.")]));
        assert_eq!(parts("“Wait,” she said."), want(&[(true, "“Wait,”"), (false, "she said.")]));
    }

    #[test]
    fn narration_with_a_title_in_quotes_stays_whole() {
        let p = "Он читал «Войну и мир» до утра.";
        assert_eq!(parts(p), want(&[(false, p)]));
        assert_eq!(parts("Просто абзац без реплик."), want(&[(false, "Просто абзац без реплик.")]));
    }
}
