use std::fs;

use super::*;
use crate::listening::project::{Line, ListenProject, Word};
use crate::voices::library;

fn fixture(root: &Path) -> (String, std::path::PathBuf) {
    let workspace = root.join("workspace");
    let pid = crate::platform::ids::new_id();
    let mut project = ListenProject::new("Recording", "source.wav", "file");
    project.settings.lang = "ru".into();
    project.speakers = [("a".into(), "Alice".into()), ("b".into(), "Bob".into())].into();
    project.lines = vec![Line {
        id: "line1".into(),
        start: 0.5,
        end: 5.5,
        speaker: "a".into(),
        text: "Привет.".into(),
        words: vec![Word {
            text: "Привет.".into(),
            start: 0.5,
            end: 5.5,
            cut: false,
        }],
        ..Default::default()
    }];
    crate::listening::save(&workspace, &pid, &project).unwrap();
    let dir = crate::listening::project_dir(&workspace, &pid);
    fs::create_dir(dir.join("work")).unwrap();
    let samples: Vec<f32> = (0..96_000).map(|i| 0.2 * (i as f32 * 0.05).sin()).collect();
    crate::wavio::write_mono_f32(&dir.join("source.wav"), &samples, 16_000).unwrap();
    crate::wavio::write_mono_f32(&dir.join("work/audio16.wav"), &samples, 16_000).unwrap();
    (pid, workspace)
}

#[test]
fn selected_speaker_becomes_a_library_voice_with_the_same_reference_text_and_word_times() {
    let root = tempfile::tempdir().unwrap();
    let (pid, workspace) = fixture(root.path());
    let source = workspace.join(&pid).join("project.json");
    let before = fs::read(&source).unwrap();
    let voices = root.path().join("voices");
    let made = extract(&workspace, &voices, &root.path().join("models"), &pid, Some("a")).unwrap();
    assert_eq!(made["made"].as_array().unwrap().len(), 1);
    assert_eq!(made["made"][0]["speaker"], "a");
    assert_eq!(made["made"][0]["voice"], "Alice");
    assert_eq!(made["skipped"], json!([]));
    let voice = library::get(&voices, "Alice").unwrap();
    assert_eq!(voice.transcript.as_deref(), Some("Привет."));
    assert_eq!(voice.card.source, "listening");
    assert_eq!(voice.card.lang, "ru");
    assert!(voice.card.transcript_auto);
    assert!(library::audio_path(&voices, "Bob").is_none());
    let words = library::read_words(&voices, "Alice").unwrap();
    assert_eq!(words[0].word, "Привет.");
    assert!((words[0].start - 0.15).abs() < 1e-6);
    assert!((words[0].end - 5.15).abs() < 1e-6);
    assert_eq!(fs::read(source).unwrap(), before);
}

#[test]
fn insufficient_words_are_skipped_and_missing_prepared_audio_keeps_its_error() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    let (pid, workspace) = fixture(root.path());
    let voices = root.path().join("voices");
    assert_eq!(
        extract(&workspace, &voices, &root.path().join("models"), &pid, Some("b")).unwrap(),
        json!({"made":[],"skipped":["Bob"]})
    );
    assert!(!voices.exists());
    fs::remove_file(workspace.join(&pid).join("work/audio16.wav")).unwrap();
    assert_eq!(
        extract(&workspace, &voices, root.path(), &pid, None).unwrap_err(),
        t!("listen-transcribe-first")
    );
}
