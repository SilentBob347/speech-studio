//! Транскрипт Listening в файлы: SRT, WebVTT (спикер — тег `<v>`, когда их больше одного), ASS (спикер — поле Name),
//! TXT (со спикерами и таймкодами или без), JSON со словами. Скрытые и пустые строки не выходят.

use serde_json::{json, Value};

use crate::listen_project::ListenProject;

pub const FORMATS: &[&str] = &["srt", "vtt", "ass", "txt", "json"];

pub struct Opts {
    pub speakers: bool,
    pub times: bool,
}

struct Row {
    start: f64,
    end: f64,
    speaker: String,
    text: String,
}

fn rows(p: &ListenProject) -> Vec<Row> {
    p.shown().map(|l| Row { start: l.start, end: l.end, speaker: p.speaker_name(&l.speaker), text: l.text.trim().to_string() }).collect()
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
    let rows = rows(p);
    let tagged = o.speakers && !rows.is_empty() && several(&rows);
    Ok(match format {
        "srt" => rows
            .iter()
            .enumerate()
            .map(|(i, r)| format!("{}\n{} --> {}\n{}{}\n", i + 1, srt_time(r.start), srt_time(r.end), if tagged { format!("{}: ", r.speaker) } else { String::new() }, r.text))
            .collect::<Vec<_>>()
            .join("\n"),
        "vtt" => {
            let mut out = String::from("WEBVTT\n");
            for r in &rows {
                let voice = if tagged { format!("<v {}>", vtt_text(&r.speaker)) } else { String::new() };
                out.push_str(&format!("\n{} --> {}\n{voice}{}\n", srt_time(r.start).replace(',', "."), srt_time(r.end).replace(',', "."), vtt_text(&r.text)));
            }
            out
        }
        "ass" => {
            let mut out = String::from(
                "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,64,&H00FFFFFF,&H000000FF,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,3,1,2,60,60,60,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
            );
            for r in &rows {
                let name = if o.speakers { r.speaker.replace(',', " ") } else { String::new() };
                out.push_str(&format!("Dialogue: 0,{},{},Default,{name},0,0,0,,{}\n", ass_time(r.start), ass_time(r.end), r.text.replace('\n', "\\N").replace('{', "(").replace('}', ")")));
            }
            out
        }
        "txt" => {
            let mut out: Vec<String> = Vec::new();
            for r in &rows {
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
        "json" => serde_json::to_string_pretty(&json_value(p)).map_err(|e| e.to_string())?,
        other => return Err(t!("listen-export-format", format = other.to_string())),
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
    use crate::listen_project::Line;

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
        let o = Opts { speakers: true, times: true };
        let srt = render(&p, "srt", &o).unwrap();
        assert!(srt.starts_with("1\n00:00:00,000 --> 00:00:01,500\nАнна: Привет <всем>\n"));
        assert!(srt.contains("01:01:01,250 --> 01:01:02,000") && !srt.contains("Скрыто"));
        let vtt = render(&p, "vtt", &o).unwrap();
        assert!(vtt.contains("<v Анна>Привет &lt;всем&gt;") && vtt.contains("01:01:01.250"));
        assert!(render(&p, "ass", &o).unwrap().contains("Dialogue: 0,1:01:01.25,1:01:02.00,Default,Борис,0,0,0,,Пока"));
        assert_eq!(render(&p, "txt", &o).unwrap(), "[0:00] Анна: Привет <всем>\n[1:01:01] Борис: Пока\n");
        assert_eq!(render(&p, "txt", &Opts { speakers: false, times: false }).unwrap(), "Привет <всем>\nПока\n");
        let json: Value = serde_json::from_str(&render(&p, "json", &o).unwrap()).unwrap();
        assert_eq!(json["lines"].as_array().unwrap().len(), 2);
    }
}
