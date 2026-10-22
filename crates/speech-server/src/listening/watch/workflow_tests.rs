use std::fs;
use std::sync::{mpsc, Arc};
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::jobs::{self, JobFn, JobKind, JobMeta};

const DEADLINE: Duration = Duration::from_secs(12);

async fn isolated() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let guard = LOCK.lock().await;
    assert!(!running().load(Ordering::SeqCst));
    *state().lock().unwrap() = Saved::default();
    guard
}

fn fixture() -> (tempfile::TempDir, Dependencies, WatchConfig) {
    let root = tempfile::tempdir().unwrap();
    let paths = ListenPaths {
        repo_root: root.path().into(),
        workspace: root.path().join("workspace"),
        models_root: root.path().join("models"),
        sortformer_onnx: root.path().join("models/custom-diar.onnx"),
    };
    fs::create_dir(&paths.workspace).unwrap();
    let folder = root.path().join("input");
    fs::create_dir(&folder).unwrap();
    let config = WatchConfig {
        folder: folder.to_string_lossy().into_owned(),
        out: root.path().join("output").to_string_lossy().into_owned(),
        formats: vec!["txt".into(), "srt".into()],
        engine: "whisper".into(),
        lang: "ru".into(),
        ..Default::default()
    };
    (
        root,
        Dependencies {
            paths,
            jobs: jobs::JobQueue::new(),
        },
        config,
    )
}

fn audio(config: &WatchConfig, name: &str) -> PathBuf {
    let path = Path::new(&config.folder).join(name);
    crate::audio::wavio::write_mono_f32(&path, &[0.25; 4_000], 16_000).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000)))
        .unwrap();
    path
}

fn transcribe(paths: &ListenPaths, pid: &str) -> Result<Value, String> {
    let mut project = crate::listening::load(&paths.workspace, pid)?;
    project.speakers.insert("a".into(), "Alice".into());
    project.lines = vec![lp::Line {
        id: "line".into(),
        speaker: "a".into(),
        text: "Hello.".into(),
        start: 0.0,
        end: 0.25,
        ..Default::default()
    }];
    crate::listening::save(&paths.workspace, pid, &project)?;
    Ok(json!({"pid":pid}))
}

fn prepared(deps: &Dependencies, path: &Path) -> String {
    let (pid, _) = crate::listening::project_of_file(&deps.paths.workspace, path).unwrap();
    transcribe(&deps.paths, &pid).unwrap();
    pid
}

async fn wait_for(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(DEADLINE, async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn finished() {
    wait_for(|| !running().load(Ordering::SeqCst)).await;
}

fn never(_: ListenPaths, _: String) -> JobFn {
    panic!("an existing transcript must not be recognised again")
}

#[tokio::test]
async fn existing_transcripts_keep_their_settings_and_export_order_and_report_output_errors() {
    let (_root, deps, mut config) = fixture();
    let path = audio(&config, "record.wav");
    let pid = prepared(&deps, &path);
    let project_file = deps.paths.workspace.join(&pid).join("project.json");
    let before = fs::read(&project_file).unwrap();
    let revision = crate::app::revision(&pid);
    let (found, outputs) = one(&deps, &config, &path, &never).await.unwrap();
    assert_eq!(found, pid);
    assert_eq!(
        outputs
            .iter()
            .map(|p| Path::new(p).extension().unwrap().to_str().unwrap())
            .collect::<Vec<_>>(),
        ["txt", "srt"]
    );
    for output in &outputs {
        assert!(fs::read_to_string(output).unwrap().contains("Hello."));
    }
    assert_eq!(fs::read(&project_file).unwrap(), before);
    assert_eq!(crate::app::revision(&pid), revision);
    assert!(deps.jobs.list(Some(&pid)).await.is_empty());
    config.out = path.to_string_lossy().into_owned();
    let error = one(&deps, &config, &path, &never).await.unwrap_err();
    assert!(error.starts_with(&format!("{}:", path.display())));
    assert_eq!(fs::read(project_file).unwrap(), before);
}

#[tokio::test]
async fn empty_recordings_get_the_requested_settings_and_use_the_supplied_queue_before_export() {
    let (_root, deps, mut config) = fixture();
    config.lang.clear();
    config.out.clear();
    let path = audio(&config, "record.wav");
    let make_job = |paths: ListenPaths, pid: String| -> JobFn {
        let expected = deps.paths.clone();
        Box::new(move |progress| {
            assert_eq!(paths.repo_root, expected.repo_root);
            assert_eq!(paths.models_root, expected.models_root);
            assert_eq!(paths.sortformer_onnx, expected.sortformer_onnx);
            let project = crate::listening::load(&paths.workspace, &pid)?;
            assert_eq!(project.settings.engine, "whisper");
            assert_eq!(project.settings.lang, "auto");
            progress(json!({"stage":"test"}));
            transcribe(&paths, &pid)
        })
    };
    let (pid, outputs) = one(&deps, &config, &path, &make_job).await.unwrap();
    assert_eq!(outputs.len(), 2);
    assert!(outputs.iter().all(|p| Path::new(p).parent() == Some(Path::new(&config.folder))));
    let records = deps.jobs.list(Some(&pid)).await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["kind"], "analyze");
    assert_eq!(records[0]["status"], "done");
    assert!(!deps.paths.models_root.exists());
}

#[tokio::test]
async fn recognition_errors_reach_the_caller_without_exporting_files() {
    let (_root, deps, config) = fixture();
    let path = audio(&config, "record.wav");
    let fail = |_: ListenPaths, _: String| -> JobFn { Box::new(|_| Err("recognition failed".into())) };
    assert_eq!(one(&deps, &config, &path, &fail).await.unwrap_err(), "recognition failed");
    assert!(!Path::new(&config.out).exists());
    let records = deps.jobs.list(None).await;
    assert_eq!(records[0]["status"], "error");
}

#[tokio::test]
async fn queue_conflicts_keep_the_existing_error_and_do_not_run_another_recogniser() {
    let _language = crate::i18n::test_language("ru");
    let (_root, deps, config) = fixture();
    let path = audio(&config, "record.wav");
    let (pid, _) = crate::listening::project_of_file(&deps.paths.workspace, &path).unwrap();
    let (release, gate) = mpsc::channel();
    let (job_id, done) = deps
        .jobs
        .enqueue_awaitable(
            JobMeta::new(JobKind::Analyze, Some(&pid)),
            Box::new(move |_| {
                gate.recv_timeout(DEADLINE).map_err(|e| e.to_string())?;
                Ok(json!(null))
            }),
        )
        .await
        .unwrap();
    let make_job = |_: ListenPaths, _: String| -> JobFn { Box::new(|_| panic!("conflicting job must not run")) };
    let error = one(&deps, &config, &path, &make_job).await.unwrap_err();
    release.send(()).unwrap();
    tokio::time::timeout(DEADLINE, done).await.unwrap().unwrap().unwrap();
    assert_eq!(error, t!("watch-busy", kind = "analyze"));
    assert_eq!(deps.jobs.list(Some(&pid)).await.len(), 1);
    assert_eq!(deps.jobs.snapshot(&job_id).await.unwrap()["status"], "done");
    assert!(!Path::new(&config.out).exists());
}

#[tokio::test]
async fn cancelling_a_recognition_returns_the_queue_error_and_does_not_export() {
    let (_root, deps, config) = fixture();
    let path = audio(&config, "record.wav");
    let queue = deps.jobs.clone();
    let (sent, received) = tokio::sync::oneshot::channel();
    let sent = Mutex::new(Some(sent));
    let output = config.out.clone();
    let task = tokio::spawn(async move {
        one(&deps, &config, &path, &|_: ListenPaths, pid: String| -> JobFn {
            let sent = sent.lock().unwrap().take().unwrap();
            Box::new(move |_| {
                sent.send(pid).unwrap();
                let deadline = std::time::Instant::now() + DEADLINE;
                while std::time::Instant::now() < deadline {
                    jobs::check_cancelled()?;
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err("cancellation timed out".into())
            })
        })
        .await
    });
    let pid = tokio::time::timeout(DEADLINE, received).await.unwrap().unwrap();
    let (job_id, _) = queue.active_for(&pid).await.unwrap();
    queue.cancel(&job_id).await.unwrap();
    assert_eq!(
        tokio::time::timeout(DEADLINE, task).await.unwrap().unwrap().unwrap_err(),
        jobs::CANCELLED
    );
    assert_eq!(queue.snapshot(&job_id).await.unwrap()["status"], "cancelled");
    assert!(!Path::new(&output).exists());
}

#[tokio::test]
async fn passes_filter_and_sort_files_skip_done_keys_and_replace_changed_files() {
    let _guard = isolated().await;
    let (_root, deps, config) = fixture();
    let b = audio(&config, "b.wav");
    let a = audio(&config, "A.wav");
    fs::write(Path::new(&config.folder).join("notes.txt"), b"skip").unwrap();
    fs::create_dir(Path::new(&config.folder).join("directory.wav")).unwrap();
    let a_pid = prepared(&deps, &a);
    prepared(&deps, &b);
    start(deps.clone(), config.clone()).unwrap();
    finished().await;
    let first = status();
    assert_eq!(first["done"], 2);
    assert_eq!(first["on"], false);
    assert_eq!(first["items"][0]["file"], "A.wav");
    assert_eq!(first["items"][1]["file"], "b.wav");
    fs::remove_file(Path::new(&config.out).join("A.txt")).unwrap();
    start(deps.clone(), config.clone()).unwrap();
    finished().await;
    assert!(!Path::new(&config.out).join("A.txt").exists());
    assert_eq!(status()["items"], first["items"]);
    fs::File::options()
        .write(true)
        .open(&a)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_001)))
        .unwrap();
    let new_pid = prepared(&deps, &a);
    assert_ne!(new_pid, a_pid);
    start(deps.clone(), config).unwrap();
    finished().await;
    let items = status()["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 2);
    assert_eq!(items.iter().find(|i| i["file"] == "A.wav").unwrap()["pid"], new_pid);
    assert_eq!(status()["errors"], 0);
    assert!(deps.jobs.list(None).await.is_empty());
}

#[tokio::test]
async fn stopping_finishes_the_current_file_and_keeps_the_rest_queued() {
    let _guard = isolated().await;
    let (_root, deps, config) = fixture();
    audio(&config, "A.wav");
    audio(&config, "b.wav");
    *state().lock().unwrap() = Saved {
        config,
        on: true,
        ..Default::default()
    };
    running().store(true, Ordering::SeqCst);
    let repo_root = deps.paths.repo_root.clone();
    let queue = deps.jobs.clone();
    let (release, gate) = mpsc::channel();
    let gate = Arc::new(Mutex::new(Some(gate)));
    let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        work_with(deps, move |paths, pid| -> JobFn {
            let gate = gate.lock().unwrap().take().expect("only the current file runs");
            let sent = sent.clone();
            Box::new(move |_| {
                sent.send(pid.clone()).unwrap();
                gate.recv_timeout(DEADLINE).map_err(|e| e.to_string())?;
                transcribe(&paths, &pid)
            })
        })
        .await;
    });
    let pid = tokio::time::timeout(DEADLINE, received.recv()).await.unwrap().unwrap();
    let stopped = stop(&repo_root).unwrap();
    assert_eq!(stopped["on"], false);
    assert_eq!(stopped["running"], true);
    assert_eq!(queue.active_for(&pid).await.unwrap().1, JobKind::Analyze);
    release.send(()).unwrap();
    tokio::time::timeout(DEADLINE, task).await.unwrap().unwrap();
    let result = status();
    assert_eq!(result["done"], 1);
    assert_eq!(result["queued"], 1);
    assert_eq!(result["items"][1]["state"], "queued");
    assert_eq!(result["running"], false);
    let saved: Saved = serde_json::from_slice(&fs::read(file(&repo_root)).unwrap()).unwrap();
    assert!(!saved.on);
    assert_eq!(saved.items[0].pid.as_deref(), Some(pid.as_str()));
}

#[tokio::test]
async fn resume_reuses_interrupted_projects_and_an_off_snapshot_does_not_start_work() {
    let _guard = isolated().await;
    let (_root, deps, config) = fixture();
    let path = audio(&config, "A.wav");
    let pid = prepared(&deps, &path);
    let revision = crate::app::revision(&pid);
    let saved = Saved {
        config: config.clone(),
        on: true,
        items: vec![Item {
            file: "A.wav".into(),
            key: crate::listening::file_key(&path, "listen").unwrap(),
            pid: Some(pid.clone()),
            state: "working".into(),
            ..Default::default()
        }],
    };
    fs::write(file(&deps.paths.repo_root), serde_json::to_vec(&saved).unwrap()).unwrap();
    resume(deps.clone());
    finished().await;
    assert_eq!(status()["done"], 1);
    assert_eq!(status()["items"][0]["pid"], pid);
    assert_eq!(crate::app::revision(&pid), revision);
    assert!(deps.jobs.list(None).await.is_empty());
    fs::remove_file(Path::new(&config.out).join("A.txt")).unwrap();
    let off = Saved { on: false, ..saved };
    fs::write(file(&deps.paths.repo_root), serde_json::to_vec(&off).unwrap()).unwrap();
    resume(deps.clone());
    assert_eq!(status()["running"], false);
    assert_eq!(status()["on"], false);
    assert_eq!(status()["items"][0]["state"], "working");
    assert!(!Path::new(&config.out).join("A.txt").exists());
}

#[tokio::test]
async fn keep_picks_up_new_files_and_restart_retries_an_import_error() {
    let _guard = isolated().await;
    let (_root, deps, mut config) = fixture();
    let broken = Path::new(&config.folder).join("broken.mkv");
    fs::write(&broken, b"invalid media").unwrap();
    fs::File::options()
        .write(true)
        .open(&broken)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(10)))
        .unwrap();
    start(deps.clone(), config.clone()).unwrap();
    finished().await;
    assert_eq!(status()["errors"], 1);
    assert_eq!(status()["items"][0]["state"], "error");
    assert!(status()["items"][0]["error"].as_str().unwrap().contains("ffmpeg"));
    let pid = crate::platform::ids::new_id();
    let mut project = lp::ListenProject::new("Recovered", "source.wav", "broken.mkv");
    project.key = crate::listening::file_key(&broken, "listen").unwrap();
    crate::listening::save(&deps.paths.workspace, &pid, &project).unwrap();
    transcribe(&deps.paths, &pid).unwrap();
    config.keep = true;
    start(deps.clone(), config.clone()).unwrap();
    wait_for(|| status()["done"] == 1).await;
    assert_eq!(status()["errors"], 0);
    let later = audio(&config, "later.wav");
    let later_pid = prepared(&deps, &later);
    wait_for(|| status()["done"] == 2).await;
    assert!(status()["items"].as_array().unwrap().iter().any(|i| i["pid"] == later_pid));
    stop(&deps.paths.repo_root).unwrap();
    finished().await;
    assert_eq!(status()["on"], false);
}
