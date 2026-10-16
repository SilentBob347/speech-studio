//! Where a failed take went wrong and how to mend it without voicing the whole block again. The words of the block,
//! sentence by sentence, are aligned with the words recognition heard and their times: a sentence with words missed,
//! extra, wrong or stretched is voiced again alone and put in its place; a tail after the last word is cut, a gap
//! inside is shortened. A defect of the whole sound, or a block of one sentence, is voiced again whole.

use crate::qc::Check;

/// Defects of the whole sound that no piece of the take can mend.
const WHOLE: [&str; 8] = ["runaway", "cutoff", "silence", "hum", "noise", "clipping", "whistle", "click"];
/// Sound kept around a cut, so the breath and the room of the take stay.
const MARGIN: f64 = 0.08;
/// What of a gap or a tail is kept.
const KEEP: f64 = 0.25;

#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// Take out `from..to` seconds.
    Cut { from: f64, to: f64 },
    /// Voice sentence `sentence` alone and put it over `from..to`.
    Revoice { sentence: usize, from: f64, to: f64 },
}

impl Edit {
    pub fn from(&self) -> f64 {
        match self {
            Edit::Cut { from, .. } | Edit::Revoice { from, .. } => *from,
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    /// The block's sentences as written, markup kept.
    pub sentences: Vec<String>,
    pub edits: Vec<Edit>,
    /// Nothing to mend in place: the block is voiced again whole.
    pub whole: bool,
}

struct Heard {
    word: String,
    /// Written in digits: the text reads it out, so any word of it may match.
    wild: bool,
    start: f64,
    end: f64,
}

fn heard(timed: &[(String, f64, f64)]) -> Vec<Heard> {
    let mut out = Vec::new();
    for (w, a, z) in timed {
        if w.chars().any(|c| c.is_ascii_digit()) {
            out.push(Heard { word: w.clone(), wild: true, start: *a, end: *z });
        } else {
            for t in crate::qc::normalize(w) {
                out.push(Heard { word: t, wild: false, start: *a, end: *z });
            }
        }
    }
    out
}

/// The words of each sentence as the check compares them, numbers read out.
fn expected(sentences: &[String], lang: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (k, s) in sentences.iter().enumerate() {
        let said = if lang == "ru" { crate::normalize_ru::normalize(s).text } else { s.clone() };
        out.extend(crate::qc::normalize(&said).into_iter().map(|w| (w, k)));
    }
    out
}

/// Levenshtein alignment: (expected index, heard index, whether they agree); a wild heard word agrees at half a cost.
fn align(e: &[(String, usize)], h: &[Heard]) -> Vec<(Option<usize>, Option<usize>, bool)> {
    let (n, m) = (e.len(), h.len());
    let sub = |i: usize, j: usize| if h[j].wild { 0.5 } else if e[i].0 == h[j].word { 0.0 } else { 1.0 };
    let mut d = vec![vec![0.0f64; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i as f64;
    }
    for j in 0..=m {
        d[0][j] = j as f64;
    }
    for i in 1..=n {
        for j in 1..=m {
            d[i][j] = (d[i - 1][j] + 1.0).min(d[i][j - 1] + 1.0).min(d[i - 1][j - 1] + sub(i - 1, j - 1));
        }
    }
    let (mut i, mut j) = (n, m);
    let mut out = Vec::new();
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && (d[i][j] - (d[i - 1][j - 1] + sub(i - 1, j - 1))).abs() < 1e-9 {
            out.push((Some(i - 1), Some(j - 1), h[j - 1].wild || e[i - 1].0 == h[j - 1].word));
            i -= 1;
            j -= 1;
        } else if i > 0 && (d[i][j] - (d[i - 1][j] + 1.0)).abs() < 1e-9 {
            out.push((Some(i - 1), None, false));
            i -= 1;
        } else {
            out.push((None, Some(j - 1), false));
            j -= 1;
        }
    }
    out.reverse();
    out
}

/// What to do with a take of `text` that failed `check`; `pause` is the longest pause the text asks for, seconds.
pub fn plan(text: &str, lang: &str, check: &Check, duration: f64, pause: f64) -> Plan {
    let sentences: Vec<String> = crate::speech_text::sentences(text).into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let mut plan = Plan { sentences, ..Default::default() };
    if check.similar == Some(false) || check.issues.iter().any(|i| WHOLE.contains(&i.as_str())) || check.timed.is_empty() {
        plan.whole = true;
        return plan;
    }
    let e = expected(&plan.sentences, lang);
    let h = heard(&check.timed);
    let steps = align(&e, &h);
    let count = plan.sentences.len();
    let mut bad = vec![false; count];
    let mut owner: Vec<Option<usize>> = vec![None; h.len()];
    let mut last = 0usize;
    for &(i, j, ok) in &steps {
        let k = i.map(|i| e[i].1).unwrap_or(last);
        last = k;
        if let Some(j) = j {
            owner[j] = Some(k);
        }
        if !ok && check.text_checked && !check.passed {
            bad[k] = true;
        }
    }
    let stretched = |w: &Heard| {
        let d = w.end - w.start;
        if w.wild {
            return false;
        }
        d > crate::qc::STRETCH_SECS && d > w.word.chars().filter(|c| c.is_alphabetic()).count().max(1) as f64 * crate::qc::STRETCH_PER_LETTER
    };
    for (j, w) in h.iter().enumerate() {
        if stretched(w) {
            bad[owner[j].unwrap_or(0)] = true;
        }
    }
    if check.issues.iter().any(|i| i == "repeat" || i == "garbled") && !bad.iter().any(|b| *b) {
        plan.whole = true;
        return plan;
    }
    for k in (0..count).filter(|&k| bad[k]) {
        let mine: Vec<usize> = (0..h.len()).filter(|&j| owner[j] == Some(k)).collect();
        let before = h.iter().enumerate().filter(|(j, _)| owner[*j].is_some_and(|o| o < k)).map(|(_, w)| w.end).fold(0.0, f64::max);
        let after = h.iter().enumerate().filter(|(j, _)| owner[*j].is_some_and(|o| o > k)).map(|(_, w)| w.start).fold(duration, f64::min);
        let (a, z) = match (mine.first(), mine.last()) {
            (Some(&f), Some(&l)) => (h[f].start, h[l].end),
            _ => (before, before),
        };
        let from = (a - MARGIN).max(before).max(0.0);
        let to = if k + 1 == count { duration } else { (z + MARGIN).min(after).min(duration) };
        plan.edits.push(Edit::Revoice { sentence: k, from, to });
    }
    if plan.edits.len() == count {
        plan.edits.clear();
        plan.whole = true;
        return plan;
    }
    let revoiced = |t: f64| plan.edits.iter().any(|e| matches!(e, Edit::Revoice { from, to, .. } if t >= *from && t <= *to));
    let mut cuts = Vec::new();
    for w in h.windows(2) {
        let (a, b) = (w[0].end, w[1].start);
        if b - a > crate::qc::GAP_SECS + pause && !revoiced(a) && !revoiced(b) {
            cuts.push(Edit::Cut { from: a + KEEP / 2.0, to: b - KEEP / 2.0 });
        }
    }
    if let Some(l) = h.last() {
        if duration - l.end > crate::qc::TAIL_SECS && !revoiced(l.end) {
            cuts.push(Edit::Cut { from: l.end + KEEP, to: duration });
        }
    }
    plan.edits.extend(cuts);
    plan.edits.sort_by(|a, b| a.from().total_cmp(&b.from()));
    plan
}

/// The take with each edit applied: a cut joins its sides, a revoiced sentence (`pieces`, by sentence) goes over its
/// span at the take's loudness; every join is a short crossfade.
pub fn splice(x: &[f32], sr: u32, edits: &[Edit], pieces: &std::collections::BTreeMap<usize, Vec<f32>>) -> Vec<f32> {
    let take_lufs = crate::loudness::integrated_lufs(x, sr);
    let at = |t: f64| ((t * sr as f64) as usize).min(x.len());
    let mut cur = x.to_vec();
    let mut order: Vec<&Edit> = edits.iter().collect();
    order.sort_by(|a, b| b.from().total_cmp(&a.from()));
    for edit in order {
        let (from, to, piece) = match edit {
            Edit::Cut { from, to } => (at(*from), at(*to), Vec::new()),
            Edit::Revoice { sentence, from, to } => {
                let mut p = pieces.get(sentence).cloned().unwrap_or_default();
                if let (Some(t), Some(l)) = (take_lufs, crate::loudness::integrated_lufs(&p, sr)) {
                    let g = 10f64.powf((t - l) / 20.0) as f32;
                    p.iter_mut().for_each(|s| *s *= g);
                }
                (at(*from), at(*to), p)
            }
        };
        let to = to.min(cur.len()).max(from.min(cur.len()));
        let mut out = cur[..from.min(cur.len())].to_vec();
        if !piece.is_empty() {
            crate::tts_trim::crossfade_join(&mut out, &piece, sr);
        }
        crate::tts_trim::crossfade_join(&mut out, &cur[to..], sr);
        cur = out;
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(timed: &[(&str, f64, f64)], passed: bool, issues: &[&str]) -> Check {
        Check {
            text_checked: true,
            passed,
            issues: issues.iter().map(|s| s.to_string()).collect(),
            timed: timed.iter().map(|(w, a, z)| (w.to_string(), *a, *z)).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn the_sentence_with_a_wrong_word_is_voiced_again_in_its_place() {
        let text = "Утро было тихим. Анна пила чай. Потом пришёл Олег.";
        let c = check(&[("утро", 0.0, 0.4), ("было", 0.4, 0.7), ("тихим.", 0.7, 1.2), ("анна", 1.6, 1.9), ("пила", 1.9, 2.2), ("кофе.", 2.2, 2.7), ("потом", 3.1, 3.4), ("пришёл", 3.4, 3.8), ("олег.", 3.8, 4.2)], false, &[]);
        let p = plan(text, "ru", &c, 4.5, 0.0);
        assert!(!p.whole);
        assert_eq!(p.sentences.len(), 3);
        match p.edits.as_slice() {
            [Edit::Revoice { sentence: 1, from, to }] => assert!((from - 1.52).abs() < 1e-9 && (to - 2.78).abs() < 1e-9, "{from} {to}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn numbers_heard_in_digits_and_a_long_tail_and_gap() {
        let text = "Мне 25 лет. Это было давно.";
        let c = check(&[("мне", 0.0, 0.3), ("25", 0.3, 1.0), ("лет.", 1.0, 1.3), ("это", 3.5, 3.7), ("было", 3.7, 4.0), ("давно.", 4.0, 4.5)], true, &["gap", "tail"]);
        let p = plan(text, "ru", &c, 6.0, 0.0);
        assert!(!p.whole);
        assert_eq!(p.edits, vec![Edit::Cut { from: 1.425, to: 3.375 }, Edit::Cut { from: 4.75, to: 6.0 }]);
    }

    #[test]
    fn a_defect_of_the_whole_sound_or_a_single_sentence_is_voiced_again_whole() {
        let c = check(&[("привет.", 0.0, 0.5)], true, &["clipping"]);
        assert!(plan("Привет. Как дела?", "ru", &c, 1.0, 0.0).whole);
        let c = check(&[("превет.", 0.0, 0.5)], false, &[]);
        assert!(plan("Привет.", "ru", &c, 1.0, 0.0).whole);
    }

    #[test]
    fn a_cut_joins_its_sides_and_a_piece_goes_in_its_place() {
        let sr = 1000;
        let x: Vec<f32> = (0..3000).map(|i| if (1000..2000).contains(&i) { 0.0 } else { 0.5 }).collect();
        let out = splice(&x, sr, &[Edit::Cut { from: 1.0, to: 2.0 }], &Default::default());
        assert!((out.len() as i64 - 2000).abs() <= 30);
        let mut pieces = std::collections::BTreeMap::new();
        pieces.insert(0, vec![0.25f32; 500]);
        let out = splice(&x, sr, &[Edit::Revoice { sentence: 0, from: 1.0, to: 2.0 }], &pieces);
        assert!((out.len() as i64 - 2500).abs() <= 60);
    }
}
