use std::fs;

use serde_json::json;

use super::*;
use crate::listening::project::{Line, ListenProject};

fn fixture() -> (tempfile::TempDir, Paths, String) {
    let root = tempfile::tempdir().unwrap();
    let p = Paths {
        workspace: root.path().join("workspace"),
        profiles: profiles::Paths {
            repo_root: root.path().into(),
            voices_dir: root.path().join("voices"),
            models_root: root.path().join("models"),
        },
    };
    let pid = crate::platform::ids::new_id();
    let mut project = ListenProject::new("Test", "source.wav", "recording");
    project.speakers = [("a".into(), "Alice".into()), ("b".into(), "Bob".into())].into();
    project.lines = vec![
        Line {
            id: "a1".into(),
            start: 0.0,
            end: 2.0,
            speaker: "a".into(),
            ..Default::default()
        },
        Line {
            id: "b1".into(),
            start: 2.0,
            end: 3.0,
            speaker: "b".into(),
            ..Default::default()
        },
    ];
    crate::listening::save(&p.workspace, &pid, &project).unwrap();
    let work = p.workspace.join(&pid).join("work");
    fs::create_dir(&work).unwrap();
    crate::wavio::write_mono_f32(&work.join("audio16.wav"), &vec![0.25; 48_000], 16_000).unwrap();
    (root, p, pid)
}

#[test]
fn comparison_receives_only_prepared_speakers_and_returns_its_result() {
    let (_root, p, pid) = fixture();
    let expected = json!({"speakers":[{"speaker":"a","name":"Alice","best":null}],"threshold":0.5});
    let result = matches_with(&p, &pid, |paths, speakers| {
        assert_eq!(paths.repo_root, p.profiles.repo_root);
        assert_eq!(speakers.len(), 1);
        assert_eq!(speakers[0].key, "a");
        assert_eq!(speakers[0].name, Some("Alice"));
        assert_eq!(speakers[0].audio, vec![0.25; 32_000]);
        Ok(expected.clone())
    })
    .unwrap();
    assert_eq!(result, expected);
    assert!(!p.profiles.models_root.exists());
}

#[test]
fn remember_validates_the_speaker_and_duration_before_calling_profile_storage() {
    let _language = crate::i18n::test_language("ru");
    let (_root, p, pid) = fixture();
    assert_eq!(
        remember_with(&p, &pid, "missing", |_, _, _, _| panic!(
            "unknown speaker must not reach the engine"
        ))
        .unwrap_err(),
        "no speaker missing in this recording"
    );
    assert_eq!(
        remember_with(&p, &pid, "b", |_, _, _, _| panic!("short speaker must not reach the engine")).unwrap_err(),
        t!("speaker-too-little", name = "Bob".to_string())
    );
    let result = remember_with(&p, &pid, "a", |_, source, name, samples| {
        assert_eq!(source, pid);
        assert_eq!(name, "Alice");
        assert_eq!(samples.len(), 32_000);
        Ok(json!({"name":name,"recordings":1}))
    })
    .unwrap();
    assert_eq!(result, json!({"name":"Alice","recordings":1}));
    assert!(!p.profiles.models_root.exists());
}

#[test]
fn inference_and_storage_errors_are_returned_without_changing_the_recording() {
    let (_root, p, pid) = fixture();
    let source = p.workspace.join(&pid).join("project.json");
    let before = fs::read(&source).unwrap();
    assert_eq!(
        matches_with(&p, &pid, |_, _| Err("embedding failed".into())).unwrap_err(),
        "embedding failed"
    );
    assert_eq!(
        remember_with(&p, &pid, "a", |_, _, _, _| Err("profile write failed".into())).unwrap_err(),
        "profile write failed"
    );
    assert_eq!(fs::read(source).unwrap(), before);
}
