use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, OnceLock},
};

use serde_json::json;

use super::{context, events};

pub(crate) const PROJECT_CHANGED: &str = "project_changed";

#[derive(Default)]
struct Saves {
    rev: u64,
    last: String,
    /// The latest revision saved by anyone other than `last`.
    other: u64,
}

impl Saves {
    fn record(&mut self, actor: &str) -> u64 {
        if self.last != actor {
            self.other = self.rev;
            self.last = actor.to_string();
        }
        self.rev += 1;
        self.rev
    }

    fn only_by_since(&self, actor: &str, base: u64) -> bool {
        // A future base never matches; intervening saves must all belong to this actor.
        let others = if self.last == actor { self.other } else { self.rev };
        base <= self.rev && others <= base
    }
}

fn revisions() -> std::sync::MutexGuard<'static, HashMap<String, Saves>> {
    static REVISIONS: OnceLock<Mutex<HashMap<String, Saves>>> = OnceLock::new();
    REVISIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn revision(pid: &str) -> u64 {
    revisions().get(pid).map(|saves| saves.rev).unwrap_or(0)
}

pub(crate) fn save_with_revision(dir: &Path, write: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let pid = dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let context::SaveOrigin { actor, expected, job } = context::save_origin();
    let rev = {
        let mut revisions = revisions();
        let saves = revisions.entry(pid.clone()).or_default();
        if expected.is_some_and(|base| !saves.only_by_since(&actor, base)) {
            context::record_conflict();
            return Err(format!("{PROJECT_CHANGED}: project {pid} is at revision {}", saves.rev));
        }
        write()?;
        saves.record(&actor)
    };
    context::record_save(rev);
    events::publish(json!({ "changed": "project", "pid": pid, "rev": rev, "by": actor, "job": job }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_tell_whether_one_author_alone_saved_after_a_revision() {
        let mut saves = Saves::default();
        assert!(saves.only_by_since("window:a", 0), "a project nobody saved");
        saves.record("window:a");
        saves.record("window:a");
        assert!(saves.only_by_since("window:a", 0) && !saves.only_by_since("agent", 0) && saves.only_by_since("agent", 2));
        saves.record("agent");
        saves.record("window:a");
        assert_eq!(saves.rev, 4);
        assert!(!saves.only_by_since("window:a", 2) && saves.only_by_since("window:a", 3));
        assert!(!saves.only_by_since("agent", 3) && saves.only_by_since("agent", 4));
        assert!(!saves.only_by_since("window:a", 5));
    }

    #[tokio::test]
    async fn failed_writes_do_not_advance_the_revision_or_mark_the_request_saved() {
        let dir = std::path::PathBuf::from(uuid::Uuid::new_v4().simple().to_string());
        let pid = dir.to_str().unwrap();
        let request = context::RequestContext::new("api".into(), None);
        let outcome = request.outcome();
        request
            .run(async {
                assert_eq!(save_with_revision(&dir, || Err("disk full".into())), Err("disk full".into()));
                assert_eq!(revision(pid), 0);
                assert_eq!(outcome.saved_revision(), None);
                assert!(!outcome.conflicted());
                save_with_revision(&dir, || Ok(())).unwrap();
            })
            .await;
        assert_eq!(revision(pid), 1);
        assert_eq!(outcome.saved_revision(), Some(1));
    }

    #[tokio::test]
    async fn a_conflict_never_calls_the_writer_and_marks_the_request_outcome() {
        let dir = std::path::PathBuf::from(uuid::Uuid::new_v4().simple().to_string());
        context::RequestContext::new("agent".into(), None)
            .run(async {
                save_with_revision(&dir, || Ok(())).unwrap();
            })
            .await;
        let request = context::RequestContext::new("window:one".into(), Some(0));
        let outcome = request.outcome();
        let called = std::cell::Cell::new(false);
        let result = request
            .run(async {
                save_with_revision(&dir, || {
                    called.set(true);
                    Ok(())
                })
            })
            .await;
        assert!(result.unwrap_err().starts_with(PROJECT_CHANGED));
        assert!(!called.get());
        assert!(outcome.conflicted());
        assert_eq!(outcome.saved_revision(), None);
        assert_eq!(revision(dir.to_str().unwrap()), 1);
    }
}
