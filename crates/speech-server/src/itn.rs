//! Numbers said in words written as digits in a transcript ("двадцать пять" → "25"), for its export: Russian by the
//! number words of every case and gender `num2words2` writes for the voicing (`normalize_ru`), read back; English,
//! Spanish, French, Portuguese, German, Italian, Dutch and Danish by `text2num`. A number below ten said alone stays a
//! word, as `text2num` keeps it ("один из нас"). The transcript itself and its word times stay as heard.

use std::collections::HashMap;
use std::sync::OnceLock;

use num_bigint::BigInt;

use crate::normalize_ru::{cardinal, ordinal, Case, Gender};

/// Numbers below this said alone stay words.
const ALONE_BELOW: f64 = 10.0;

#[derive(Clone, Copy, PartialEq)]
enum Part {
    /// 0–19, tens and hundreds: they add up inside a group of three digits.
    Small(u64),
    /// тысяча, миллион, миллиард: they close the group before them.
    Scale(u64),
    /// An ordinal's last word ("четвёртый", "тысячного"): it ends the number, written with its ending ("1984-й").
    Ordinal(u64),
}

/// The ending a Russian ordinal keeps after its digits: "-й", "-го", "-му", "-м", "-я", "-ю", "-е", "-х", "-ми".
fn ordinal_ending(word: &str) -> &'static str {
    let ends = |e: &[&str]| e.iter().any(|x| word.ends_with(x));
    if ends(&["ыми", "ими"]) {
        "-ми"
    } else if ends(&["ого", "его"]) {
        "-го"
    } else if ends(&["ому", "ему"]) {
        "-му"
    } else if ends(&["ых", "их"]) {
        "-х"
    } else if ends(&["ая", "яя"]) {
        "-я"
    } else if ends(&["ую", "юю"]) {
        "-ю"
    } else if ends(&["ое", "ее", "ые", "ие"]) {
        "-е"
    } else if ends(&["ым", "им", "ом", "ем"]) {
        "-м"
    } else {
        "-й"
    }
}

/// Every Russian number word in every case, gender and animacy, as `num2words2` writes them.
fn ru_words() -> &'static HashMap<String, Part> {
    static WORDS: OnceLock<HashMap<String, Part>> = OnceLock::new();
    WORDS.get_or_init(|| {
        let cases = [Case::Nom, Case::Gen, Case::Dat, Case::Acc, Case::Ins, Case::Loc];
        let genders = [Gender::Masc, Gender::Fem, Gender::Neut];
        let mut words = HashMap::new();
        let smalls = (0..20u64).chain((20..100).step_by(10)).chain((100..1000).step_by(100));
        for n in smalls {
            for case in cases {
                for gender in genders {
                    for animate in [false, true] {
                        let said = cardinal(&BigInt::from(n), case, gender, animate);
                        if !said.contains(' ') {
                            words.insert(said.to_lowercase().replace('ё', "е"), Part::Small(n));
                        }
                    }
                }
            }
        }
        let ordinals = (1..20u64).chain((20..100).step_by(10)).chain((100..1000).step_by(100)).chain([1_000, 1_000_000, 1_000_000_000]);
        for n in ordinals {
            for case in cases {
                for gender in genders {
                    for plural in [false, true] {
                        let said = ordinal(&BigInt::from(n), case, gender, plural);
                        if let Some(last) = said.split_whitespace().last() {
                            words.entry(last.to_lowercase().replace('ё', "е")).or_insert(Part::Ordinal(n));
                        }
                    }
                }
            }
        }
        for scale in [1_000u64, 1_000_000, 1_000_000_000] {
            for count in [1u64, 2, 5] {
                for case in cases {
                    let said = cardinal(&BigInt::from(count * scale), case, Gender::Masc, false);
                    if let Some(last) = said.split_whitespace().last() {
                        words.insert(last.to_lowercase().replace('ё', "е"), Part::Scale(scale));
                    }
                }
            }
        }
        words
    })
}

/// The digit count of a small part's place: hundreds 3, tens (20–90) 2, the rest 1.
fn place(n: u64) -> u8 {
    match n {
        100.. => 3,
        20..=99 => 2,
        _ => 1,
    }
}

/// Russian number words in `text` as digits; the punctuation around a word stays with it.
fn ru(text: &str) -> String {
    let words = ru_words();
    let tokens: Vec<&str> = text.split(' ').collect();
    let core = |t: &str| t.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase().replace('ё', "е");
    let part = |t: &str| words.get(&core(t)).copied();
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        if part(tokens[i]).is_none() {
            out.push(tokens[i].to_string());
            i += 1;
            continue;
        }
        // a number: smalls add up in falling places, a scale closes the group, a smaller scale may follow
        let (mut total, mut group, mut last_place, mut last_scale) = (0u64, 0u64, 4u8, u64::MAX);
        let mut ending = "";
        let mut j = i;
        while j < tokens.len() {
            match part(tokens[j]) {
                Some(Part::Small(n)) => {
                    let p = place(n);
                    if p >= last_place || (group % 10 != 0 && n < 10) {
                        break;
                    }
                    group += n;
                    last_place = if n < 20 && n >= 10 { 1 } else { p };
                }
                Some(Part::Scale(s)) if s < last_scale => {
                    total += group.max(1) * s;
                    group = 0;
                    last_place = 4;
                    last_scale = s;
                }
                Some(Part::Ordinal(n)) if n >= 1000 && n < last_scale => {
                    total += group.max(1) * n;
                    group = 0;
                    ending = ordinal_ending(&core(tokens[j]));
                    j += 1;
                    break;
                }
                Some(Part::Ordinal(n)) if n < 1000 && place(n) < last_place && !(group % 10 != 0 && n < 10) => {
                    group += n;
                    ending = ordinal_ending(&core(tokens[j]));
                    j += 1;
                    break;
                }
                _ => break,
            }
            // a word that ends with punctuation ends the number with it
            if tokens[j].ends_with(|c: char| !c.is_alphanumeric()) {
                j += 1;
                break;
            }
            j += 1;
        }
        let value = total + group;
        let alone = j - i == 1;
        if alone && ((value as f64) < ALONE_BELOW || !ending.is_empty()) {
            out.push(tokens[i].to_string());
            i += 1;
            continue;
        }
        let lead: String = tokens[i].chars().take_while(|c| !c.is_alphanumeric()).collect();
        let tail: String = tokens[j - 1].chars().rev().take_while(|c| !c.is_alphanumeric()).collect::<Vec<_>>().into_iter().rev().collect();
        out.push(format!("{lead}{value}{ending}{tail}"));
        i = j;
    }
    out.join(" ")
}

/// Whether numbers in words can be written as digits in `lang`.
pub fn knows(lang: &str) -> bool {
    lang == "ru" || text2num::get_interpreter_for(lang).is_some()
}

/// `text` with the numbers said in words written as digits; a language neither reads is refused.
pub fn digits(text: &str, lang: &str) -> Result<String, String> {
    if lang == "ru" {
        return Ok(ru(text));
    }
    let interpreter = text2num::get_interpreter_for(lang).ok_or_else(|| t!("itn-no-lang", lang = lang.to_string()))?;
    Ok(text2num::replace_numbers_in_text(text, &interpreter, ALONE_BELOW))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_numbers_in_words_become_digits_and_a_small_one_alone_stays_a_word() {
        assert_eq!(digits("Мне двадцать пять лет.", "ru").unwrap(), "Мне 25 лет.");
        assert_eq!(digits("тысяча девятьсот восемьдесят четвёртый год, а не две тысячи двадцать шесть", "ru").unwrap(), "1984-й год, а не 2026");
        assert_eq!(digits("в двадцать пятом году и в двухтысячном", "ru").unwrap(), "в 25-м году и в двухтысячном");
        assert_eq!(digits("к тысяча девятьсот сорок пятому году", "ru").unwrap(), "к 1945-му году");
        assert_eq!(digits("в трёхстах метрах от двух миллионов пятисот тысяч", "ru").unwrap(), "в 300 метрах от 2500000");
        assert_eq!(digits("один из нас и пять шесть", "ru").unwrap(), "один из нас и пять шесть");
        assert_eq!(digits("сорок, пятьдесят", "ru").unwrap(), "40, 50");
        assert_eq!(digits("I have twenty five apples", "en").unwrap(), "I have 25 apples");
        assert!(digits("二十五", "zh").is_err());
        assert!(knows("fr") && knows("ru") && !knows("zh"));
    }
}
