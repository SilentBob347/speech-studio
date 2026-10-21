use std::{fs, path::Path};

use serde_json::{json, Value};

use super::{context::RequestContext, events, revisions};

#[derive(Clone, Copy, Debug)]
enum Kind {
    Speech,
    Listening,
}

impl Kind {
    fn save(self, ws: &Path, pid: &str, name: &str, valid: bool) -> Result<(), String> {
        match self {
            Self::Speech => {
                let mut p = crate::speech::from_text(name, "Текст.", "Анна");
                if !valid {
                    p.settings.params.temperature = 9.0;
                }
                crate::speech::save(ws, pid, &p)
            }
            Self::Listening => {
                let mut p = crate::listening::ListenProject::new(name, "source.wav", "recording");
                if !valid {
                    p.source = "../outside.wav".into();
                }
                crate::listening::save(ws, pid, &p)
            }
        }
    }

    fn load(self, ws: &Path, pid: &str) -> Result<Value, String> {
        match self {
            Self::Speech => crate::speech::load(ws, pid).map(|p| serde_json::to_value(p).unwrap()),
            Self::Listening => crate::listening::load(ws, pid).map(|p| serde_json::to_value(p).unwrap()),
        }
    }
}

fn pid() -> String {
    crate::platform::ids::new_id()
}

fn changes(rx: &mut tokio::sync::broadcast::Receiver<String>, pid: &str) -> Vec<Value> {
    let mut found = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(text) => {
                let event: Value = serde_json::from_str(&text).unwrap();
                if event["pid"] == pid {
                    found.push(event);
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(e) => panic!("{e}"),
        }
    }
    found
}

#[test]
fn invalid_models_are_refused_before_creating_a_directory_or_revision() {
    let root = tempfile::tempdir().unwrap();
    let mut rx = events::subscribe();
    for kind in [Kind::Speech, Kind::Listening] {
        let id = pid();
        assert!(kind.save(root.path(), &id, "invalid", false).is_err());
        assert!(!root.path().join(&id).exists());
        assert_eq!(revisions::revision(&id), 0);
        assert!(changes(&mut rx, &id).is_empty());
    }
}

#[test]
fn legacy_loads_deserialize_without_applying_save_validation() {
    let root = tempfile::tempdir().unwrap();
    for (kind, value) in [
        (
            Kind::Speech,
            json!({"kind":"speech", "name":"legacy", "settings":{"params":{"temperature":9.0}}}),
        ),
        (Kind::Listening, json!({"kind":"listening", "name":"legacy", "source":"../old.wav"})),
    ] {
        let id = pid();
        let file = root.path().join(&id).join("project.json");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&file, &bytes).unwrap();
        assert_eq!(kind.load(root.path(), &id).unwrap()["name"], "legacy");
        let save = match kind {
            Kind::Speech => crate::speech::save(root.path(), &id, &crate::speech::load(root.path(), &id).unwrap()),
            Kind::Listening => crate::listening::save(root.path(), &id, &crate::listening::load(root.path(), &id).unwrap()),
        };
        assert!(save.is_err());
        assert_eq!(fs::read(&file).unwrap(), bytes);
        assert_eq!(revisions::revision(&id), 0);
    }
}

#[tokio::test]
async fn successful_saves_publish_once_with_actor_revision_and_persisted_content() {
    let root = tempfile::tempdir().unwrap();
    for kind in [Kind::Speech, Kind::Listening] {
        let id = pid();
        let mut rx = events::subscribe();
        let request = RequestContext::new("window:storage-test".into(), Some(0));
        let outcome = request.outcome();
        request
            .run(async {
                kind.save(root.path(), &id, "Сохранено", true).unwrap();
            })
            .await;
        assert_eq!(kind.load(root.path(), &id).unwrap()["name"], "Сохранено");
        assert_eq!(revisions::revision(&id), 1);
        assert_eq!(outcome.saved_revision(), Some(1));
        assert!(!outcome.conflicted());
        assert_eq!(
            changes(&mut rx, &id),
            [json!({"changed":"project", "pid":id, "rev":1, "by":"window:storage-test", "job":false})]
        );
        let dir = root.path().join(&id);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let expected = match kind {
            Kind::Speech => serde_json::to_string_pretty(&crate::speech::load(root.path(), &id).unwrap()).unwrap(),
            Kind::Listening => serde_json::to_string_pretty(&crate::listening::load(root.path(), &id).unwrap()).unwrap(),
        };
        assert_eq!(fs::read_to_string(dir.join("project.json")).unwrap(), expected);
    }
}

#[tokio::test]
async fn revision_conflicts_keep_the_saved_bytes_and_emit_no_second_change() {
    let root = tempfile::tempdir().unwrap();
    for kind in [Kind::Speech, Kind::Listening] {
        let id = pid();
        RequestContext::new("agent".into(), None)
            .run(async {
                kind.save(root.path(), &id, "first", true).unwrap();
            })
            .await;
        let file = root.path().join(&id).join("project.json");
        let before = fs::read(&file).unwrap();
        let mut rx = events::subscribe();
        let request = RequestContext::new("window".into(), Some(0));
        let outcome = request.outcome();
        let error = request
            .run(async { kind.save(root.path(), &id, "overwrite", true) })
            .await
            .unwrap_err();
        assert!(error.starts_with(revisions::PROJECT_CHANGED));
        assert!(outcome.conflicted());
        assert_eq!(outcome.saved_revision(), None);
        assert_eq!(fs::read(file).unwrap(), before);
        assert_eq!(revisions::revision(&id), 1);
        assert!(changes(&mut rx, &id).is_empty());
        assert_eq!(fs::read_dir(root.path().join(&id)).unwrap().count(), 1);
    }
}

#[tokio::test]
async fn failed_writers_keep_the_previous_revision_outcome_and_destination() {
    let root = tempfile::tempdir().unwrap();
    for kind in [Kind::Speech, Kind::Listening] {
        let id = pid();
        let dir = root.path().join(&id);
        let dest = dir.join("project.json");
        fs::create_dir_all(&dir).unwrap();
        match kind {
            Kind::Speech => {
                fs::write(&dest, b"previous project bytes").unwrap();
                fs::create_dir(dir.join("project.json.tmp")).unwrap();
            }
            Kind::Listening => {
                fs::create_dir(&dest).unwrap();
                fs::write(dest.join("keep"), b"destination marker").unwrap();
            }
        }
        let mut rx = events::subscribe();
        let request = RequestContext::new("window".into(), None);
        let outcome = request.outcome();
        assert!(request.run(async { kind.save(root.path(), &id, "fails", true) }).await.is_err());
        assert_eq!(revisions::revision(&id), 0);
        assert_eq!(outcome.saved_revision(), None);
        assert!(!outcome.conflicted());
        assert!(changes(&mut rx, &id).is_empty());
        match kind {
            Kind::Speech => assert_eq!(fs::read(&dest).unwrap(), b"previous project bytes"),
            Kind::Listening => {
                assert_eq!(fs::read(dest.join("keep")).unwrap(), b"destination marker");
                assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "atomic writer cleans up its staging file");
            }
        }
    }
}

#[test]
fn parent_directory_errors_do_not_write_or_advance_a_revision() {
    let root = tempfile::tempdir().unwrap();
    let ws = root.path().join("workspace");
    fs::write(&ws, b"a file blocks workspace creation").unwrap();
    for kind in [Kind::Speech, Kind::Listening] {
        let id = pid();
        let err = kind.save(&ws, &id, "fails", true).unwrap_err();
        assert!(err.starts_with(&format!("{}:", ws.join(&id).display())));
        assert_eq!(revisions::revision(&id), 0);
    }
    assert_eq!(fs::read(ws).unwrap(), b"a file blocks workspace creation");
}

#[test]
fn readers_keep_path_details_for_io_encoding_and_field_type_errors() {
    let root = tempfile::tempdir().unwrap();
    for kind in [Kind::Speech, Kind::Listening] {
        let id = pid();
        let file = root.path().join(&id).join("project.json");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::create_dir(&file).unwrap();
        assert!(kind
            .load(root.path(), &id)
            .unwrap_err()
            .starts_with(&format!("{}:", file.display())));
        fs::remove_dir(&file).unwrap();
        for bytes in [
            vec![0xff],
            format!(
                "{{\"kind\":\"{}\",\"created\":[]}}",
                match kind {
                    Kind::Speech => "speech",
                    Kind::Listening => "listening",
                }
            )
            .into_bytes(),
        ] {
            fs::write(&file, &bytes).unwrap();
            assert!(kind
                .load(root.path(), &id)
                .unwrap_err()
                .starts_with(&format!("{}:", file.display())));
            assert_eq!(fs::read(&file).unwrap(), bytes);
        }
    }
}

#[test]
fn speech_edits_still_serialize_read_modify_write_and_return_the_change_result() {
    let root = tempfile::tempdir().unwrap();
    let id = pid();
    let project = crate::speech::SpeechProject {
        created: 100,
        ..Default::default()
    };
    crate::speech::save(root.path(), &id, &project).unwrap();
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..4 {
            workers.push(scope.spawn(|| {
                barrier.wait();
                (0..10)
                    .map(|_| {
                        crate::speech::edit(root.path(), &id, |p| {
                            let before = p.created;
                            p.created += 1;
                            before
                        })
                        .unwrap()
                    })
                    .collect::<Vec<_>>()
            }));
        }
        let mut old: Vec<_> = workers.into_iter().flat_map(|w| w.join().unwrap()).collect();
        old.sort();
        assert_eq!(old, (100..140).collect::<Vec<_>>());
    });
    assert_eq!(crate::speech::load(root.path(), &id).unwrap().created, 140);
    assert_eq!(revisions::revision(&id), 41);
}

#[test]
fn an_invalid_speech_edit_keeps_the_file_and_returns_a_save_error() {
    let root = tempfile::tempdir().unwrap();
    let id = pid();
    Kind::Speech.save(root.path(), &id, "before", true).unwrap();
    let file = root.path().join(&id).join("project.json");
    let before = fs::read(&file).unwrap();
    let result = crate::speech::edit(root.path(), &id, |p| {
        p.kind = "other".into();
        37
    });
    assert_eq!(result.unwrap_err(), "this is a other project, not a speech one");
    assert_eq!(fs::read(file).unwrap(), before);
    assert_eq!(revisions::revision(&id), 1);
}

#[tokio::test]
async fn saves_in_real_jobs_keep_inherited_authorship_and_job_events() {
    let root = tempfile::tempdir().unwrap();
    let queue = crate::jobs::JobQueue::new();
    for kind in [Kind::Speech, Kind::Listening] {
        let id = pid();
        let owned_id = id.clone();
        let ws = root.path().to_path_buf();
        let mut rx = events::subscribe();
        let (_, done) = RequestContext::new("window:job-storage".into(), Some(999))
            .run(queue.enqueue_awaitable(
                crate::jobs::JobMeta::new(crate::jobs::JobKind::Synth, None),
                Box::new(move |_| {
                    kind.save(&ws, &owned_id, "from job", true)?;
                    Ok(json!({}))
                }),
            ))
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), done)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            changes(&mut rx, &id),
            [json!({"changed":"project", "pid":id, "rev":1, "by":"window:job-storage", "job":true})]
        );
        assert_eq!(kind.load(root.path(), &id).unwrap()["name"], "from job");
    }
}
