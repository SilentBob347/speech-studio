use std::future::Future;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

#[derive(Clone, Default)]
pub(crate) struct Outcome {
    saved: Arc<AtomicU64>,
    conflict: Arc<AtomicBool>,
}

impl Outcome {
    pub fn saved_revision(&self) -> Option<u64> {
        Some(self.saved.load(Ordering::Relaxed)).filter(|revision| *revision > 0)
    }

    pub fn conflicted(&self) -> bool {
        self.conflict.load(Ordering::Relaxed)
    }
}

#[derive(Clone)]
pub(crate) struct RequestContext {
    actor: String,
    expected: Option<u64>,
    job: bool,
    outcome: Outcome,
}

impl RequestContext {
    pub fn new(actor: String, expected: Option<u64>) -> Self {
        Self {
            actor,
            expected,
            job: false,
            outcome: Outcome::default(),
        }
    }

    pub fn outcome(&self) -> Outcome {
        self.outcome.clone()
    }

    pub async fn run<F: Future>(self, work: F) -> F::Output {
        CURRENT.scope(self, work).await
    }
}

tokio::task_local! { static CURRENT: RequestContext; }

pub(crate) struct SaveOrigin {
    pub actor: String,
    pub expected: Option<u64>,
    pub job: bool,
}

pub(crate) fn save_origin() -> SaveOrigin {
    CURRENT
        .try_with(|context| SaveOrigin {
            actor: context.actor.clone(),
            expected: context.expected,
            job: context.job,
        })
        .unwrap_or_else(|_| SaveOrigin {
            actor: "studio".into(),
            expected: None,
            job: false,
        })
}

pub(crate) fn current_actor() -> String {
    CURRENT
        .try_with(|context| context.actor.clone())
        .unwrap_or_else(|_| "studio".into())
}

pub(crate) fn record_save(revision: u64) {
    let _ = CURRENT.try_with(|context| context.outcome.saved.store(revision, Ordering::Relaxed));
}

pub(crate) fn record_conflict() {
    let _ = CURRENT.try_with(|context| context.outcome.conflict.store(true, Ordering::Relaxed));
}

pub(crate) fn carry<T>(work: impl FnOnce() -> T + Send + 'static) -> impl FnOnce() -> T + Send + 'static {
    let captured = CURRENT.try_with(Clone::clone).ok();
    move || match captured {
        Some(context) => CURRENT.sync_scope(context, work),
        None => work(),
    }
}

pub(crate) fn carry_job<A, R>(work: impl FnOnce(A) -> R + Send + 'static) -> impl FnOnce(A) -> R + Send + 'static {
    // A queued job inherits authorship, never the request's revision precondition or outcome.
    let context = RequestContext {
        job: true,
        ..RequestContext::new(current_actor(), None)
    };
    move |argument| CURRENT.sync_scope(context, || work(argument))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn blocking_work_shares_the_request_outcome_and_revision() {
        let context = RequestContext::new("window:one".into(), Some(19));
        let outcome = context.outcome();
        context
            .run(async {
                tokio::task::spawn_blocking(carry(|| {
                    let origin = save_origin();
                    assert_eq!(origin.actor, "window:one");
                    assert_eq!(origin.expected, Some(19));
                    assert!(!origin.job);
                    record_save(20);
                    record_conflict();
                }))
                .await
                .unwrap();
            })
            .await;
        assert_eq!(outcome.saved_revision(), Some(20));
        assert!(outcome.conflicted());
        assert_eq!(current_actor(), "studio");
    }

    #[tokio::test]
    async fn a_job_captures_its_author_without_request_preconditions_or_outcome() {
        let context = RequestContext::new("agent".into(), Some(7));
        let request_outcome = context.outcome();
        let work = context
            .run(async {
                carry_job(|()| {
                    let origin = save_origin();
                    record_save(8);
                    record_conflict();
                    origin
                })
            })
            .await;
        let origin = RequestContext::new("window:two".into(), Some(99))
            .run(async { tokio::task::spawn_blocking(move || work(())).await.unwrap() })
            .await;
        assert_eq!(origin.actor, "agent");
        assert!(origin.job);
        assert_eq!(origin.expected, None);
        assert_eq!(request_outcome.saved_revision(), None);
        assert!(!request_outcome.conflicted());
        assert_eq!(current_actor(), "studio");
    }

    #[tokio::test]
    async fn concurrent_requests_keep_their_own_context() {
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let run = |actor: &'static str, revision: u64| {
            let barrier = barrier.clone();
            tokio::spawn(RequestContext::new(actor.into(), Some(revision)).run(async move {
                barrier.wait().await;
                tokio::task::spawn_blocking(carry(save_origin)).await.unwrap()
            }))
        };
        let (one, two) = tokio::join!(run("window:one", 1), run("window:two", 2));
        let (one, two) = (one.unwrap(), two.unwrap());
        assert_eq!((one.actor.as_str(), one.expected), ("window:one", Some(1)));
        assert_eq!((two.actor.as_str(), two.expected), ("window:two", Some(2)));
    }

    #[tokio::test]
    async fn unwinding_a_carried_job_restores_the_surrounding_context() {
        let work = RequestContext::new("agent".into(), None)
            .run(async { carry_job(|()| panic!("job failed")) })
            .await;
        RequestContext::new("window:outer".into(), Some(3))
            .run(async {
                let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(())));
                assert!(failed.is_err());
                assert_eq!(current_actor(), "window:outer");
                assert_eq!(save_origin().expected, Some(3));
            })
            .await;
        assert_eq!(current_actor(), "studio");
    }
}
