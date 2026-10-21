use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::listening::project as listening;
use crate::platform::{ids, time, workspace};
use crate::speech::project as speech;
use crate::text::pron_dict;

const SPEECH: &str = r#"{
  "kind": "speech", "version": 1, "name": "Старая озвучка", "created": 1700000123,
  "chapters": [{"id": "chapter-old", "blocks": [{
    "id": "block-old", "speaker": "Анна", "text": "Привет",
    "takes": [{"n": 1, "file": "old-1.flac", "duration": 1.25, "created": 1700000456, "key": "old-key"}]
  }]}],
  "dictionary": [{"id": "old-rule:ё", "find": "ИИ", "say": "искусственный интеллект"}]
}"#;

const LISTENING: &str = r#"{
  "kind": "listening", "version": 1, "name": "Старая запись", "created": 1700000123,
  "source": "source.wav", "speakers": {"speaker_0": "Анна"},
  "lines": [{"id": "line-old", "start": 0.5, "end": 1.75, "speaker": "speaker_0", "text": "Привет"}],
  "made": {"by": "subs", "at": 1700000456}
}"#;

fn fixture(workspace: &Path, pid: &str, json: &str) -> PathBuf {
    let path = workspace.join(pid).join("project.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, json).unwrap();
    path
}

fn short_hex(id: &str) -> bool {
    id.len() == 10 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[test]
fn old_projects_accept_short_uppercase_and_long_ids_without_rewriting_files() {
    let root = tempfile::tempdir().unwrap();
    for pid in ["a", "LegacyProjectABC123", &"A9".repeat(20)] {
        let speech_root = root.path().join("speech");
        let listening_root = root.path().join("listening");
        let speech_path = fixture(&speech_root, pid, SPEECH);
        let listening_path = fixture(&listening_root, pid, LISTENING);
        let spoken = crate::speech::load(&speech_root, pid).unwrap();
        assert_eq!(spoken.created, 1_700_000_123);
        assert_eq!(spoken.chapters[0].id, "chapter-old");
        assert_eq!(spoken.blocks().next().unwrap().takes[0].created, 1_700_000_456);
        assert_eq!(spoken.dictionary[0].id, "old-rule:ё");
        assert_eq!(spoken.settings.engine, "higgs");
        let heard = crate::listening::load(&listening_root, pid).unwrap();
        assert_eq!(heard.created, 1_700_000_123);
        assert_eq!(heard.made.as_ref().unwrap().at, 1_700_000_456);
        assert_eq!(heard.lines[0].id, "line-old");
        assert_eq!(heard.speaker_name("speaker_0"), "Анна");
        assert!(heard.settings.engine.is_empty());
        assert_eq!(fs::read_to_string(speech_path).unwrap(), SPEECH);
        assert_eq!(fs::read_to_string(listening_path).unwrap(), LISTENING);
    }
}

#[test]
fn saving_old_projects_keeps_the_layout_timestamps_and_revisions() {
    let root = tempfile::tempdir().unwrap();
    let spoken_pid = format!("LegacySpeech{}", ids::new_id());
    let heard_pid = format!("LegacyListen{}", ids::new_id());
    let speech_path = fixture(root.path(), &spoken_pid, SPEECH);
    let listening_path = fixture(root.path(), &heard_pid, LISTENING);
    let mut spoken = crate::speech::load(root.path(), &spoken_pid).unwrap();
    let mut heard = crate::listening::load(root.path(), &heard_pid).unwrap();
    spoken.name = "Озвучка после правки".into();
    heard.name = "Запись после правки".into();
    let spoken_rev = super::revisions::revision(&spoken_pid);
    let heard_rev = super::revisions::revision(&heard_pid);
    crate::speech::save(root.path(), &spoken_pid, &spoken).unwrap();
    crate::listening::save(root.path(), &heard_pid, &heard).unwrap();
    assert_eq!(crate::speech::load(root.path(), &spoken_pid).unwrap(), spoken);
    assert_eq!(crate::listening::load(root.path(), &heard_pid).unwrap(), heard);
    assert_eq!(super::revisions::revision(&spoken_pid), spoken_rev + 1);
    assert_eq!(super::revisions::revision(&heard_pid), heard_rev + 1);
    for path in [speech_path, listening_path] {
        let json: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(json["created"], 1_700_000_123_u64);
        assert_eq!(path.file_name().unwrap(), "project.json");
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }
    assert_eq!(heard.made.as_ref().unwrap().at, 1_700_000_456);
    assert_eq!(spoken.blocks().next().unwrap().takes[0].created, 1_700_000_456);
}

#[test]
fn invalid_project_ids_are_rejected_before_reading_outside_the_workspace() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let outside_speech = fixture(root.path(), "outsideSpeech", SPEECH);
    let outside_listening = fixture(root.path(), "outsideListen", LISTENING);
    let absolute = outside_speech.parent().unwrap().to_string_lossy().into_owned();
    for pid in [
        "",
        &"a".repeat(41),
        "..",
        "../outsideSpeech",
        "../outsideListen",
        "..\\outsideSpeech",
        "..\\outsideListen",
        "a/../../outsideSpeech",
        &absolute,
        "/outsideSpeech",
        "C:outsideSpeech",
        "a\\b",
        "%2e%2e%2foutsideSpeech",
        "проект",
        "a\0b",
    ] {
        let expected = format!("bad project id {pid}");
        assert_eq!(crate::speech::load(&workspace, pid).unwrap_err(), expected);
        assert_eq!(crate::listening::load(&workspace, pid).unwrap_err(), expected);
    }
    assert_eq!(fs::read_to_string(outside_speech).unwrap(), SPEECH);
    assert_eq!(fs::read_to_string(outside_listening).unwrap(), LISTENING);
    assert_eq!(fs::read_dir(&workspace).unwrap().count(), 0);
}

#[test]
fn valid_ids_keep_missing_wrong_kind_and_invalid_json_errors() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    let missing = t!("project-missing", pid = "Missing123".to_string());
    assert_eq!(crate::speech::load(root.path(), "Missing123").unwrap_err(), missing);
    assert_eq!(crate::listening::load(root.path(), "Missing123").unwrap_err(), missing);
    fixture(root.path(), "WrongForSpeech", LISTENING);
    fixture(root.path(), "WrongForListen", SPEECH);
    assert_eq!(
        crate::speech::load(root.path(), "WrongForSpeech").unwrap_err(),
        "project WrongForSpeech is not a speech project"
    );
    assert_eq!(
        crate::listening::load(root.path(), "WrongForListen").unwrap_err(),
        "project WrongForListen is not a listening project"
    );
    let broken = fixture(root.path(), "BrokenJSON", "{");
    assert!(crate::speech::load(root.path(), "BrokenJSON")
        .unwrap_err()
        .starts_with(&format!("{}:", broken.display())));
    assert!(crate::listening::load(root.path(), "BrokenJSON")
        .unwrap_err()
        .starts_with(&format!("{}:", broken.display())));
}

#[test]
fn dictionary_imports_keep_new_ids_and_existing_ids_stay_opaque() {
    let bxd = "Version=1\nDelimiter=\u{3}\nID1\u{3}1\u{3}1\u{3}1\u{3}0\u{3}0\u{3}0\u{3}Xmas\u{3}Christmas\u{3}\n";
    let mut all_ids = HashSet::new();
    for (name, body) in [
        ("test.dic", "cat=kitten\ndog=puppy\n"),
        ("test.rex", "(cat)=kitten\n"),
        ("test.bxd", bxd),
    ] {
        let entries = pron_dict::import_balabolka(name, body.as_bytes()).unwrap();
        assert!(!entries.is_empty(), "{name}");
        pron_dict::validate(&entries).unwrap();
        for entry in entries {
            assert!(short_hex(&entry.id), "{name}: {}", entry.id);
            assert!(all_ids.insert(entry.id));
        }
    }
    assert!(short_hex(&pron_dict::new_id()));
    let root = tempfile::tempdir().unwrap();
    let existing = vec![pron_dict::Entry {
        id: "old-rule:ё_1".into(),
        find: "ИИ".into(),
        say: "искусственный интеллект".into(),
        ..Default::default()
    }];
    assert!(!workspace::is_pid(&existing[0].id));
    pron_dict::save_global(root.path(), &existing).unwrap();
    assert_eq!(pron_dict::load_global(root.path()), existing);
}

#[test]
fn project_constructors_use_short_ids_and_unix_seconds() {
    let before = time::unix_seconds();
    let spoken = speech::from_text("Новая озвучка", "Привет", "Анна");
    let heard = listening::ListenProject::new("Новая запись", "source.wav", "recording");
    let after = time::unix_seconds();
    assert!((before..=after).contains(&spoken.created));
    assert!((before..=after).contains(&heard.created));
    assert!(short_hex(&spoken.chapters[0].id));
    assert!(short_hex(&spoken.blocks().next().unwrap().id));
}
