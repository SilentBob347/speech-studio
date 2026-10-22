//! Транскрипт Listening в файлы: SRT, WebVTT (спикер — тег `<v>`, когда их больше одного), ASS (спикер — поле Name),
//! TXT (со спикерами и таймкодами или без), JSON со словами. Скрытые и пустые строки не выходят.

use serde_json::{json, Value};

use crate::listening::project::ListenProject;


pub struct Opts {
    pub speakers: bool,
    pub times: bool,
    /// Subtitle rules of a broadcaster (`dub_captions::PROFILES`): lines laid out on screens by their
    /// line length and lines per screen, at the heard word times, and held to their shortest and longest showing.
    pub rules: Option<&'static dub_captions::RulesProfile>,
    /// Numbers said in words written as digits (`itn`), the transcript itself staying as heard.
    pub digits: bool,
}

/// A subtitle line: its time, who says it and what.
pub struct Row {
    pub start: f64,
    pub end: f64,
    pub speaker: String,
    pub text: String,
    /// The heard start and end of each word of `text`, when they are known.
    pub words: Vec<(f64, f64)>,
}

pub fn shown_rows(p: &ListenProject) -> Vec<Row> {
    p.shown()
        .map(|l| Row { start: l.start, end: l.end, speaker: p.speaker_name(&l.speaker), text: l.text.trim().to_string(), words: if l.approx { Vec::new() } else { l.words.iter().map(|w| (w.start, w.end)).collect() } })
        .collect()
}

/// The lines with numbers said in words written as digits, each in its language (`auto`: Russian by its letters);
/// answers also how many lines are in a language whose numbers are not known and stay as they are.
pub fn with_digits(rows: Vec<Row>, lang: &str) -> Result<(Vec<Row>, usize), String> {
    let mut unknown = 0;
    let mut out = Vec::with_capacity(rows.len());
    for mut r in rows {
        let line_lang = crate::text::prep::block_lang(&r.text, lang);
        if crate::text::itn::knows(&line_lang) {
            r.text = crate::text::itn::digits(&r.text, &line_lang)?;
            r.words = Vec::new();
        } else {
            unknown += 1;
        }
        out.push(r);
    }
    Ok((out, unknown))
}

/// Lines as screens of subtitles under a broadcaster's rules: a line goes onto a new screen where its words no longer
/// fit the screen's lines or would show longer than the rules allow (a screen changes when the voice reaches its first
/// word), each screen wrapped into balanced lines, cut after a sentence's end or a comma where one is near, then held
/// to the shortest and longest showing and small gaps bridged (dub-captions, SubtitleEdit's rules). Answers the
/// screens and how many of them still read faster than the rules allow.
pub fn by_rules(rows: &[Row], p: &dub_captions::RulesProfile, lang: &str) -> (Vec<Row>, usize) {
    use dub_captions::{balanced_wrap, normalize, violates_cps, NormalizeOpts, TimingSeg};
    let room = (p.max_line_len * p.max_lines.max(1)) as f64;
    let longest = p.max_display_ms as f64 / 1000.0;
    let mut segs = Vec::new();
    for r in rows {
        let words: Vec<&str> = r.text.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        let timed = (r.words.len() == words.len()).then_some(r.words.as_slice());
        let weight: Vec<f64> = words.iter().map(|w| w.chars().count() as f64 + 1.0).collect();
        let total: f64 = weight.iter().sum();
        // where word k starts: heard, or by the words' length inside the line
        let starts = |k: usize| -> f64 {
            match timed {
                Some(t) => t[k].0,
                None => r.start + (r.end - r.start) * weight[..k].iter().sum::<f64>() / total,
            }
        };
        let ends = |k: usize| -> f64 {
            match timed {
                Some(t) => t[k].1,
                None => r.start + (r.end - r.start) * weight[..=k].iter().sum::<f64>() / total,
            }
        };
        // as many screens as the words need, cut into near equal parts at word edges, so no word is left alone
        let fits = |r: &std::ops::Range<usize>| p.calc.count(&words[r.clone()].join(" ")) <= room;
        let need = (p.calc.count(&r.text) / room).ceil().max(1.0) as usize;
        // near each equal share the cut goes after a sentence's end, else after a comma, else at the nearest word
        let before: Vec<f64> = std::iter::once(0.0).chain(weight.iter().scan(0.0, |acc, w| { *acc += w; Some(*acc) })).collect();
        let pause = |k: usize| match words[k - 1].chars().last() {
            Some('.' | '!' | '?' | '…') => 0,
            Some(',' | ';' | ':' | '—') => 1,
            _ => 2,
        };
        let mut cuts = vec![0usize];
        for j in 1..need {
            let target = total * j as f64 / need as f64;
            let reach = total / need as f64 * 0.35;
            let from = cuts.last().copied().unwrap_or(0) + 1;
            let near = (from..words.len()).filter(|&k| (before[k] - target).abs() <= reach);
            let best = near.min_by(|&x, &y| (pause(x), (before[x] - target).abs()).partial_cmp(&(pause(y), (before[y] - target).abs())).unwrap_or(std::cmp::Ordering::Equal));
            let nearest = (from..words.len()).min_by(|&x, &y| (before[x] - target).abs().total_cmp(&(before[y] - target).abs()));
            if let Some(k) = best.or(nearest) {
                cuts.push(k);
            }
        }
        cuts.push(words.len());
        let mut screens: Vec<std::ops::Range<usize>> = Vec::new();
        for pair in cuts.windows(2) {
            let mut first = pair[0];
            for k in pair[0] + 1..pair[1] {
                if !fits(&(first..k + 1)) {
                    screens.push(first..k);
                    first = k;
                }
            }
            screens.push(first..pair[1]);
        }
        // a screen showing longer than the rules allow is halved at the word nearest its middle, both halves long enough
        let shortest = p.min_display_ms as f64 / 1000.0;
        let mut i = 0;
        while i < screens.len() {
            let s = screens[i].clone();
            let (a, b) = (starts(s.start), ends(s.end - 1));
            let mid = (a + b) / 2.0;
            let cut = (s.start + 1..s.end).filter(|&k| starts(k) - a >= shortest && b - starts(k) >= shortest).min_by(|&x, &y| (starts(x) - mid).abs().total_cmp(&(starts(y) - mid).abs()));
            match cut {
                Some(k) if b - a > longest => {
                    screens.splice(i..=i, [s.start..k, k..s.end]);
                }
                _ => i += 1,
            }
        }
        let count = screens.len();
        for (i, screen) in screens.into_iter().enumerate() {
            let a = if i == 0 { r.start } else { starts(screen.start) };
            let b = if i + 1 == count { r.end } else { starts(screen.end) };
            let text = balanced_wrap(&words[screen].join(" "), p, lang).join("\n");
            segs.push(TimingSeg::new(a, b.max(a), text).with_speaker(r.speaker.clone()));
        }
    }
    let segs = normalize(&segs, p, &NormalizeOpts::default());
    let fast = segs.iter().filter(|s| violates_cps(s, p)).count();
    let out = segs.into_iter().map(|s| Row { start: s.start, end: s.end, speaker: s.speaker.unwrap_or_default(), text: s.text, words: Vec::new() }).collect();
    (out, fast)
}

fn ms(seconds: f64) -> u64 {
    (seconds * 1000.0).round().max(0.0) as u64
}

fn srt_time(seconds: f64) -> String {
    let ms = ms(seconds);
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms % 3_600_000 / 60_000, ms % 60_000 / 1000, ms % 1000)
}

fn ass_time(seconds: f64) -> String {
    let cs = (seconds * 100.0).round().max(0.0) as u64;
    format!("{}:{:02}:{:02}.{:02}", cs / 360_000, cs % 360_000 / 6000, cs % 6000 / 100, cs % 100)
}

/// Таймкод для TXT: `m:ss` или `h:mm:ss`.
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn vtt_text(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

fn several(rows: &[Row]) -> bool {
    rows.iter().any(|r| r.speaker != rows[0].speaker)
}

pub fn render(p: &ListenProject, format: &str, o: &Opts) -> Result<String, String> {
    if format == "json" {
        return serde_json::to_string_pretty(&json_value(p)).map_err(|e| e.to_string());
    }
    let rows = if o.digits { with_digits(shown_rows(p), &p.settings.lang)?.0 } else { shown_rows(p) };
    let rows = match o.rules.filter(|_| matches!(format, "srt" | "vtt" | "ass")) {
        Some(rules) => by_rules(&rows, rules, &p.settings.lang).0,
        None => rows,
    };
    render_rows(&rows, format, o).ok_or_else(|| t!("listen-export-format", format = format.to_string()))
}

/// Lines as srt, vtt, ass or txt; None for another format.
pub fn render_rows(rows: &[Row], format: &str, o: &Opts) -> Option<String> {
    let tagged = o.speakers && !rows.is_empty() && several(rows);
    Some(match format {
        "srt" => rows
            .iter()
            .enumerate()
            .map(|(i, r)| format!("{}\n{} --> {}\n{}{}\n", i + 1, srt_time(r.start), srt_time(r.end), if tagged { format!("{}: ", r.speaker) } else { String::new() }, r.text))
            .collect::<Vec<_>>()
            .join("\n"),
        "vtt" => {
            let mut out = String::from("WEBVTT\n");
            for r in rows {
                let voice = if tagged { format!("<v {}>", vtt_text(&r.speaker)) } else { String::new() };
                out.push_str(&format!("\n{} --> {}\n{voice}{}\n", srt_time(r.start).replace(',', "."), srt_time(r.end).replace(',', "."), vtt_text(&r.text)));
            }
            out
        }
        "ass" => {
            let mut out = String::from(
                "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,64,&H00FFFFFF,&H000000FF,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,3,1,2,60,60,60,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
            );
            for r in rows {
                let name = if o.speakers { r.speaker.replace(',', " ") } else { String::new() };
                out.push_str(&format!("Dialogue: 0,{},{},Default,{name},0,0,0,,{}\n", ass_time(r.start), ass_time(r.end), r.text.replace('\n', "\\N").replace('{', "(").replace('}', ")")));
            }
            out
        }
        "txt" => {
            let mut out: Vec<String> = Vec::new();
            for r in rows {
                let mut head = Vec::new();
                if o.times {
                    head.push(format!("[{}]", clock(r.start)));
                }
                if o.speakers {
                    head.push(format!("{}:", r.speaker));
                }
                out.push(if head.is_empty() { r.text.clone() } else { format!("{} {}", head.join(" "), r.text) });
            }
            out.join("\n") + "\n"
        }
        _ => return None,
    })
}

fn json_value(p: &ListenProject) -> Value {
    json!({
        "name": p.name,
        "duration": p.duration,
        "speakers": p.speakers,
        "lines": p.shown().map(|l| json!({
            "start": l.start, "end": l.end, "speaker": p.speaker_name(&l.speaker), "text": l.text,
            "words": l.words.iter().map(|w| json!({ "text": w.text, "start": w.start, "end": w.end })).collect::<Vec<_>>(),
            "approx": l.approx,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listening::project::Line;

    fn project() -> ListenProject {
        let mut p = ListenProject::new("t", "source.wav", "");
        p.speakers.insert("0".into(), "Анна".into());
        p.speakers.insert("1".into(), "Борис".into());
        p.lines = vec![
            Line { id: "l0".into(), start: 0.0, end: 1.5, speaker: "0".into(), text: "Привет <всем>".into(), ..Default::default() },
            Line { id: "l1".into(), start: 3661.25, end: 3662.0, speaker: "1".into(), text: "Пока".into(), ..Default::default() },
            Line { id: "l2".into(), start: 4000.0, end: 4001.0, speaker: "1".into(), text: "Скрыто".into(), hidden: true, ..Default::default() },
        ];
        p
    }

    #[test]
    fn every_format_names_speakers_and_skips_hidden_lines() {
        let p = project();
        let o = Opts { speakers: true, times: true, rules: None, digits: false };
        let srt = render(&p, "srt", &o).unwrap();
        assert!(srt.starts_with("1\n00:00:00,000 --> 00:00:01,500\nАнна: Привет <всем>\n"));
        assert!(srt.contains("01:01:01,250 --> 01:01:02,000") && !srt.contains("Скрыто"));
        let vtt = render(&p, "vtt", &o).unwrap();
        assert!(vtt.contains("<v Анна>Привет &lt;всем&gt;") && vtt.contains("01:01:01.250"));
        assert!(render(&p, "ass", &o).unwrap().contains("Dialogue: 0,1:01:01.25,1:01:02.00,Default,Борис,0,0,0,,Пока"));
        assert_eq!(render(&p, "txt", &o).unwrap(), "[0:00] Анна: Привет <всем>\n[1:01:01] Борис: Пока\n");
        assert_eq!(render(&p, "txt", &Opts { speakers: false, times: false, rules: None, digits: false }).unwrap(), "Привет <всем>\nПока\n");
        let json: Value = serde_json::from_str(&render(&p, "json", &o).unwrap()).unwrap();
        assert_eq!(json["lines"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_long_line_goes_on_screens_within_a_broadcasters_rules_and_without_rules_stays_as_heard() {
        let text = "Это очень длинная реплика, которую диктор произносит на одном дыхании, и она никак не помещается в две строки субтитра по правилам вещателя";
        let n = text.split_whitespace().count();
        let words: Vec<(f64, f64)> = (0..n).map(|k| (k as f64 * 0.5, k as f64 * 0.5 + 0.4)).collect();
        let row = Row { start: 0.0, end: n as f64 * 0.5, speaker: "A".into(), text: text.into(), words };
        let netflix = dub_captions::RulesProfile::by_name("Netflix (English)").unwrap();
        let (screens, _) = by_rules(std::slice::from_ref(&row), netflix, "ru");
        assert!(screens.len() > 1, "{}", screens.len());
        for s in &screens {
            let lines: Vec<&str> = s.text.lines().collect();
            assert!(lines.len() <= netflix.max_lines && lines.iter().all(|l| l.chars().count() <= netflix.max_line_len + 2), "{:?}", s.text);
            assert!(s.end - s.start <= netflix.max_display_ms as f64 / 1000.0 + 1e-6);
            assert!(s.end - s.start >= netflix.min_display_ms as f64 / 1000.0 - 1e-6, "a screen too short: {:?}", s.text);
        }
        let joined: String = screens.iter().map(|s| s.text.replace('\n', " ")).collect::<Vec<_>>().join(" ");
        assert_eq!(joined, text);
        let plain = render_rows(std::slice::from_ref(&row), "srt", &Opts { speakers: false, times: true, rules: None, digits: false }).unwrap();
        assert!(plain.contains(text));
    }
}
