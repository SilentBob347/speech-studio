use std::fs;
use std::path::Path;

use super::*;
use crate::listening::project::{Line, ListenProject};

fn line(speaker: &str, start: f64, end: f64, hidden: bool) -> Line {
    Line {
        id: crate::platform::ids::new_id(),
        speaker: speaker.into(),
        start,
        end,
        hidden,
        ..Default::default()
    }
}

fn fixture(root: &Path, lines: Vec<Line>) -> (String, std::path::PathBuf) {
    let pid = crate::platform::ids::new_id();
    let project = ListenProject {
        kind: crate::listening::project::KIND.into(),
        speakers: [
            ("a".into(), "Alice".into()),
            ("b".into(), "Bob".into()),
            ("few".into(), "Few".into()),
            ("hidden".into(), "Hidden".into()),
        ]
        .into(),
        lines,
        ..Default::default()
    };
    crate::listening::save(root, &pid, &project).unwrap();
    let work = root.join(&pid).join("work");
    fs::create_dir(&work).unwrap();
    (pid, work)
}

#[test]
fn samples_follow_stored_line_order_and_skip_hidden_short_and_unregistered_speakers() {
    let root = tempfile::tempdir().unwrap();
    let (pid, work) = fixture(
        root.path(),
        vec![
            line("a", 4.0, 5.5, false),
            line("a", 0.0, 0.5, false),
            line("a", 6.0, 9.0, true),
            line("b", 9.0, 20.0, false),
            line("few", 0.0, 1.99, false),
            line("hidden", 2.0, 5.0, true),
            line("unregistered", 0.0, 3.0, false),
        ],
    );
    let samples: Vec<f32> = (0..12 * SR).map(|i| (i % 251) as f32 / 1024.0).collect();
    crate::audio::wavio::write_mono_f32(&work.join("audio16.wav"), &samples, SR).unwrap();
    let before = fs::read(work.parent().unwrap().join("project.json")).unwrap();
    let prepared = speakers_audio(root.path(), &pid, 2.0, 30.0).unwrap();
    assert_eq!(prepared.names.len(), 4);
    assert_eq!(prepared.samples.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    let expected: Vec<_> = samples[4 * SR as usize..(5.5 * SR as f64) as usize]
        .iter()
        .chain(&samples[..SR as usize / 2])
        .copied()
        .collect();
    assert_eq!(prepared.samples[0].1, expected);
    assert_eq!(prepared.samples[1].1, samples[9 * SR as usize..]);
    assert_eq!(fs::read(work.parent().unwrap().join("project.json")).unwrap(), before);
}

#[test]
fn the_duration_target_keeps_the_last_line_whole_and_stops_before_the_next() {
    let root = tempfile::tempdir().unwrap();
    let (pid, work) = fixture(root.path(), vec![line("a", 0.0, 31.0, false), line("a", 31.0, 32.0, false)]);
    crate::audio::wavio::write_mono_f32(&work.join("audio16.wav"), &vec![0.25; 32 * SR as usize], SR).unwrap();
    let prepared = speakers_audio(root.path(), &pid, 2.0, 30.0).unwrap();
    assert_eq!(prepared.samples[0].1.len(), 31 * SR as usize);
}

#[test]
fn vocals_are_preferred_and_the_mix_is_used_only_when_vocals_are_absent() {
    let root = tempfile::tempdir().unwrap();
    let (pid, work) = fixture(root.path(), vec![line("a", 0.0, 2.0, false)]);
    crate::audio::wavio::write_mono_f32(&work.join("audio16.wav"), &vec![0.25; 2 * SR as usize], SR).unwrap();
    crate::audio::wavio::write_mono_f32(&work.join("vocals16.wav"), &vec![-0.5; 2 * SR as usize], SR).unwrap();
    assert!(speakers_audio(root.path(), &pid, 2.0, 30.0).unwrap().samples[0]
        .1
        .iter()
        .all(|&v| v == -0.5));
    fs::remove_file(work.join("vocals16.wav")).unwrap();
    assert!(speakers_audio(root.path(), &pid, 2.0, 30.0).unwrap().samples[0]
        .1
        .iter()
        .all(|&v| v == 0.25));
}

#[test]
fn invalid_vocals_report_the_error_instead_of_switching_to_the_mix() {
    let root = tempfile::tempdir().unwrap();
    let (pid, work) = fixture(root.path(), vec![line("a", 0.0, 2.0, false)]);
    crate::audio::wavio::write_mono_f32(&work.join("audio16.wav"), &vec![0.25; 2 * SR as usize], SR).unwrap();
    let vocals = work.join("vocals16.wav");
    crate::audio::wavio::write_mono_f32(&vocals, &[0.5; 10], 8_000).unwrap();
    assert_eq!(
        speakers_audio(root.path(), &pid, 2.0, 30.0).err().unwrap(),
        format!("{}: 8000 Hz, the voice print takes 16000 Hz", vocals.display())
    );
    fs::write(&vocals, b"broken WAV").unwrap();
    assert!(speakers_audio(root.path(), &pid, 2.0, 30.0)
        .err()
        .unwrap()
        .contains(&vocals.display().to_string()));
}

#[test]
fn missing_audio_and_project_errors_keep_their_causes() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    let (pid, _) = fixture(root.path(), Vec::new());
    assert_eq!(
        speakers_audio(root.path(), &pid, 2.0, 30.0).err().unwrap(),
        t!("listen-transcribe-first")
    );
    assert_eq!(
        speakers_audio(root.path(), "../outside", 2.0, 30.0).err().unwrap(),
        "bad project id ../outside"
    );
    assert_eq!(
        speakers_audio(root.path(), "Missing", 2.0, 30.0).err().unwrap(),
        t!("project-missing", pid = "Missing".to_string())
    );
}
