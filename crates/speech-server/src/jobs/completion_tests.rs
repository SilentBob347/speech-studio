use super::*;
use std::sync::{atomic::AtomicUsize, mpsc};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

const DEADLINE: Duration = Duration::from_secs(5);

fn observed_queue() -> (JobQueue, UnboundedReceiver<JobCompletion>) {
    let (sent, received) = unbounded_channel();
    (
        JobQueue::with_completion_observer(move |completion| {
            sent.send(completion.clone()).unwrap();
        }),
        received,
    )
}

async fn completion(received: &mut UnboundedReceiver<JobCompletion>) -> JobCompletion {
    tokio::time::timeout(DEADLINE, received.recv()).await.unwrap().unwrap()
}

async fn result(received: oneshot::Receiver<Result<Value, String>>) -> Result<Value, String> {
    tokio::time::timeout(DEADLINE, received).await.unwrap().unwrap()
}

fn gated(started: oneshot::Sender<()>, release: mpsc::Receiver<()>, cooperate: bool) -> JobFn {
    Box::new(move |_| {
        started.send(()).unwrap();
        loop {
            match release.recv_timeout(Duration::from_millis(10)) {
                Ok(()) => return Ok(json!({"finished":true})),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err("test gate dropped".into()),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if cooperate {
                        check_cancelled()?;
                    }
                }
            }
        }
    })
}

#[tokio::test]
async fn completions_preserve_results_errors_and_job_identity_once() {
    let (queue, mut received) = observed_queue();
    for (kind, pid, answer) in [
        (JobKind::Synth, Some("project-1"), Ok(json!({"file":"ready.wav"}))),
        (JobKind::Export, None, Err("disk full".to_string())),
        (JobKind::Frame, Some("preview"), Ok(json!({"frame":true}))),
    ] {
        let expected = answer.clone();
        let (id, pending) = queue
            .enqueue_awaitable(JobMeta::new(kind, pid), Box::new(move |_| answer))
            .await
            .unwrap();
        assert_eq!(result(pending).await, expected);
        let observed = completion(&mut received).await;
        let status = if expected.is_ok() { JobStatus::Done } else { JobStatus::Error };
        assert_eq!(
            observed,
            JobCompletion {
                job_id: id.clone(),
                kind,
                status,
                error: expected.err(),
                pid: pid.map(str::to_string)
            }
        );
        let snapshot = queue.snapshot(&id).await.unwrap();
        assert_eq!(snapshot["status"], status.as_str());
        assert!(received.try_recv().is_err());
    }
    let (id, pending) = queue
        .enqueue_awaitable(JobMeta::new(JobKind::Write, None), Box::new(|_| panic!("job body failed")))
        .await
        .unwrap();
    assert!(result(pending).await.unwrap_err().contains("job body failed"));
    let observed = completion(&mut received).await;
    assert_eq!(observed.job_id, id);
    assert_eq!(observed.status, JobStatus::Error);
    assert!(observed.error.unwrap().contains("job panicked:"));
    assert!(received.try_recv().is_err());
}

#[tokio::test]
async fn queued_cancellation_is_not_reported_and_running_cancellation_is_reported_once() {
    let (queue, mut received) = observed_queue();
    let (started, running) = oneshot::channel();
    let (_release, gate) = mpsc::channel();
    let (id, pending) = queue
        .enqueue_awaitable(JobMeta::new(JobKind::Synth, Some("running")), gated(started, gate, true))
        .await
        .unwrap();
    tokio::time::timeout(DEADLINE, running).await.unwrap().unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    let ran_job = ran.clone();
    let (queued, cancelled_result) = queue
        .enqueue_awaitable(
            JobMeta::new(JobKind::Synth, Some("queued")),
            Box::new(move |_| {
                ran_job.store(true, Ordering::SeqCst);
                Ok(json!(null))
            }),
        )
        .await
        .unwrap();
    assert_eq!(queue.cancel(&queued).await.unwrap()["status"], "cancelled");
    assert_eq!(result(cancelled_result).await.unwrap_err(), CANCELLED);
    assert!(received.try_recv().is_err());
    assert_eq!(queue.cancel(&id).await.unwrap()["status"], "cancelling");
    assert_eq!(result(pending).await.unwrap_err(), CANCELLED);
    let observed = completion(&mut received).await;
    assert_eq!(observed.job_id, id);
    assert_eq!(observed.status, JobStatus::Cancelled);
    assert_eq!(observed.error, None);
    let (_, barrier) = queue
        .enqueue_awaitable(JobMeta::new(JobKind::Write, None), Box::new(|_| Ok(json!(null))))
        .await
        .unwrap();
    result(barrier).await.unwrap();
    assert_eq!(completion(&mut received).await.kind, JobKind::Write);
    assert!(received.try_recv().is_err());
    assert!(!ran.load(Ordering::SeqCst));
    assert_eq!(queue.cancel(&id).await, Err(CancelError::Finished));
}

#[tokio::test]
async fn late_cancellation_keeps_the_successful_result() {
    let (queue, mut received) = observed_queue();
    let (started, running) = oneshot::channel();
    let (release, gate) = mpsc::channel();
    let (id, pending) = queue
        .enqueue_awaitable(JobMeta::new(JobKind::Synth, None), gated(started, gate, false))
        .await
        .unwrap();
    tokio::time::timeout(DEADLINE, running).await.unwrap().unwrap();
    queue.cancel(&id).await.unwrap();
    release.send(()).unwrap();
    assert_eq!(result(pending).await.unwrap(), json!({"finished":true}));
    let observed = completion(&mut received).await;
    assert_eq!(observed.status, JobStatus::Done);
    assert_eq!(observed.error, None);
    assert_eq!(queue.snapshot(&id).await.unwrap()["status"], "done");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn observer_runs_after_terminal_storage_and_sse_without_holding_the_queue_lock() {
    let (observed, mut received) = unbounded_channel();
    let (release_observer, observer_gate) = mpsc::channel();
    let observer_gate = std::sync::Mutex::new(observer_gate);
    let queue = JobQueue::with_completion_observer(move |completion| {
        observed.send(completion.clone()).unwrap();
        if completion.pid.as_deref() == Some("persistent") {
            observer_gate.lock().unwrap().recv_timeout(DEADLINE).unwrap();
        }
    });
    let root = tempfile::tempdir().unwrap();
    let (started, running) = oneshot::channel();
    let (release_job, job_gate) = mpsc::channel();
    let (id, mut pending) = queue
        .enqueue_awaitable(
            JobMeta::persistent(JobKind::Synth, "persistent", root.path().into(), json!({})),
            gated(started, job_gate, false),
        )
        .await
        .unwrap();
    tokio::time::timeout(DEADLINE, running).await.unwrap().unwrap();
    let mut subscription = queue.subscribe(&id).await.unwrap();
    release_job.send(()).unwrap();
    assert_eq!(completion(&mut received).await.status, JobStatus::Done);
    let snapshot = tokio::time::timeout(Duration::from_secs(1), queue.snapshot(&id))
        .await
        .expect("observer holds the queue lock")
        .unwrap();
    assert_eq!(snapshot["status"], "done");
    assert_eq!(job_store::read(root.path()).unwrap().unwrap().state, job_store::STATE_DONE);
    assert_eq!(
        subscription.rx.try_recv().unwrap(),
        json!({"type":"done","result":{"finished":true}})
    );
    assert_eq!(pending.try_recv(), Err(oneshot::error::TryRecvError::Empty));
    release_observer.send(()).unwrap();
    assert_eq!(result(pending).await.unwrap(), json!({"finished":true}));
}

#[tokio::test]
async fn an_observer_panic_cannot_lose_the_result_or_stop_the_worker() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_observer = calls.clone();
    let queue = JobQueue::with_completion_observer(move |_| {
        if calls_observer.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("observer failed");
        }
    });
    for expected in [Ok(json!({"ok":true})), Err("operation failed".to_string())] {
        let answer = expected.clone();
        let (id, pending) = queue
            .enqueue_awaitable(JobMeta::new(JobKind::Write, None), Box::new(move |_| answer))
            .await
            .unwrap();
        assert_eq!(result(pending).await, expected);
        assert_eq!(
            queue.snapshot(&id).await.unwrap()["status"],
            if expected.is_ok() { "done" } else { "error" }
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn abandoned_frames_are_removed_and_removed_jobs_do_not_report_completion() {
    let (queue, mut received) = observed_queue();
    let (started, running) = oneshot::channel();
    let (release, gate) = mpsc::channel();
    let (frame, pending) = queue
        .enqueue_awaitable(JobMeta::new(JobKind::Frame, None), gated(started, gate, false))
        .await
        .unwrap();
    tokio::time::timeout(DEADLINE, running).await.unwrap().unwrap();
    drop(pending);
    release.send(()).unwrap();
    assert_eq!(completion(&mut received).await.job_id, frame);
    let (started, running) = oneshot::channel();
    let (release, gate) = mpsc::channel();
    let removed = queue
        .enqueue(JobMeta::new(JobKind::Write, None), gated(started, gate, false))
        .await
        .unwrap();
    tokio::time::timeout(DEADLINE, running).await.unwrap().unwrap();
    assert!(queue.snapshot(&frame).await.is_none());
    queue.remove(&removed).await;
    release.send(()).unwrap();
    let (_, barrier) = queue
        .enqueue_awaitable(JobMeta::new(JobKind::Write, None), Box::new(|_| Ok(json!(null))))
        .await
        .unwrap();
    result(barrier).await.unwrap();
    assert_ne!(completion(&mut received).await.job_id, removed);
    assert!(received.try_recv().is_err());
}
