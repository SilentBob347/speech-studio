use super::*;
use std::cell::RefCell;

fn selection(root: &Path, value: Value) -> std::path::PathBuf {
    let models = root.join("models");
    std::fs::create_dir_all(&models).unwrap();
    std::fs::write(models.join("active.json"), value.to_string()).unwrap();
    models
}

fn sized(path: &Path, bytes: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::File::create(path).unwrap().set_len(bytes).unwrap();
}

fn install_files(root: &Path, id: &str) {
    let component = manifest().into_iter().find(|c| c.id == id).unwrap();
    assert!(component.files.iter().all(|f| f.extract == setup::Extract::None));
    for f in component.files {
        sized(&root.join(f.dest_rel), f.size);
    }
}

#[test]
fn ready_components_do_not_start_preparation_or_emit_progress() {
    let root = tempfile::tempdir().unwrap();
    install_files(root.path(), "whisper-tiny");
    ensure_components_with(
        root.path(),
        &["unknown", "whisper-tiny"],
        &|_| panic!("already ready"),
        &|_, _, _, _| panic!("already ready"),
    )
    .unwrap();
    assert!(!root.path().join("models/active.json").exists());
}

#[test]
fn preparation_keeps_missing_order_progress_and_error_details() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    install_files(root.path(), "whisper-tiny");
    let first = manifest().into_iter().find(|c| c.id == "whisper-tiny").unwrap().files[0].clone();
    sized(&root.path().join(first.dest_rel), first.size - 1);
    let events = RefCell::new(Vec::new());
    let err = ensure_components_with(
        root.path(),
        &["unknown", "whisper-base", "whisper-tiny", "whisper-base"],
        &|event| events.borrow_mut().push(event),
        &|path, ids, cancel, progress| {
            assert_eq!(path, root.path());
            assert_eq!(ids, ["whisper-base", "whisper-tiny", "whisper-base"]);
            assert!(!cancel());
            assert_eq!(
                events.borrow()[0],
                json!({"stage": "download", "msg": t!("setup-fetching-missing")})
            );
            progress(json!({"phase": "verify"}));
            Err(DlError::new("checksum", "pinned hash mismatch"))
        },
    )
    .unwrap_err();
    assert_eq!(err, "pinned hash mismatch");
    assert_eq!(events.borrow().len(), 2);
    assert!(!root.path().join("models/active.json").exists());
}

#[test]
fn remote_llm_preparation_does_not_install_local_components() {
    let root = tempfile::tempdir().unwrap();
    for provider in ["server", "openrouter", "agent"] {
        let models = selection(root.path(), json!({"llm_provider": provider}));
        ensure_llm_components_with(root.path(), &models, &|_| panic!("remote"), &|_, _, _, _| panic!("remote")).unwrap();
    }
}

#[test]
fn local_llm_preparation_reports_its_components_before_returning_failure() {
    let root = tempfile::tempdir().unwrap();
    let models = selection(root.path(), json!({"llm_provider": "local"}));
    let err = ensure_llm_components_with(root.path(), &models, &|_| {}, &|_, ids, _, _| {
        assert_eq!(ids, ["llama", "gemma"]);
        Err(DlError::new("io", "no space for model"))
    })
    .unwrap_err();
    assert_eq!(err, "no space for model");
}

#[test]
fn each_gpu_stage_requests_its_runtime_independently() {
    let root = tempfile::tempdir().unwrap();
    for gpu_stage in ["sep_backend", "diar_backend", "asr_backend"] {
        let mut sel =
            json!({"local_backend":"cpu", "sep_backend":"cpu", "diar_backend":"cpu", "asr_backend":"cpu", "asr_engine":"whisper"});
        sel[gpu_stage] = json!("gpu");
        let models = selection(root.path(), sel);
        let calls = RefCell::new(Vec::new());
        ensure_job_components_with(root.path(), &models, false, false, &|_| {}, &|_, ids, _, _| {
            calls.borrow_mut().push(ids.to_vec());
            Ok(json!({}))
        })
        .unwrap();
        let mut expected = vec!["cuda-runtime", "onnxruntime-gpu", "cudnn", "bsroformer-engine"];
        if gpu_stage == "asr_backend" {
            expected.push("whisper-cuda");
        }
        assert_eq!(calls.into_inner(), [expected], "{gpu_stage}");
    }
    let models = selection(
        root.path(),
        json!({"local_backend":"gpu", "sep_backend":"cpu", "diar_backend":"cpu", "asr_backend":"cpu"}),
    );
    ensure_job_components_with(root.path(), &models, false, false, &|_| panic!("CPU"), &|_, _, _, _| panic!("CPU")).unwrap();
}

#[test]
fn required_preparation_failure_prevents_optional_diarization_download() {
    let root = tempfile::tempdir().unwrap();
    let models = selection(root.path(), json!({"local_backend":"cpu"}));
    let calls = RefCell::new(Vec::new());
    let err = ensure_job_components_with(root.path(), &models, true, true, &|_| {}, &|_, ids, _, _| {
        calls.borrow_mut().push(ids.to_vec());
        Err(DlError::new("io", "required download failed"))
    })
    .unwrap_err();
    assert_eq!(err, "required download failed");
    assert_eq!(calls.into_inner(), [vec!["wespeaker"]]);
}

#[test]
fn optional_diarization_failure_stays_visible_and_does_not_abort_the_job() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    let models = selection(root.path(), json!({"local_backend":"cpu"}));
    let events = RefCell::new(Vec::new());
    ensure_job_components_with(
        root.path(),
        &models,
        false,
        true,
        &|ev| events.borrow_mut().push(ev),
        &|_, ids, _, _| {
            assert_eq!(ids, ["sortformer"]);
            Err(DlError::new("io", "diarization fixture error"))
        },
    )
    .unwrap();
    let events = events.into_inner();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[1],
        json!({"stage":"download", "msg":t!("setup-diarization-missing", error="diarization fixture error".to_string())})
    );
}

#[tokio::test]
async fn preparation_download_observes_cancellation_of_the_current_job() {
    let root = tempfile::tempdir().unwrap();
    let owned = root.path().to_path_buf();
    let (started, received) = tokio::sync::oneshot::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let queue = jobs::JobQueue::new();
    let (id, done) = queue
        .enqueue_awaitable(
            jobs::JobMeta::new(jobs::JobKind::Synth, None),
            Box::new(move |_| {
                let started = RefCell::new(Some(started));
                ensure_components_with(&owned, &["wespeaker"], &|_| {}, &|_, _, cancel, _| {
                    assert!(!cancel());
                    started.borrow_mut().take().unwrap().send(()).unwrap();
                    gate.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
                    assert!(cancel());
                    Err(DlError::new(setup::CANCELLED, jobs::CANCELLED))
                })
                .map(|_| json!(null))
            }),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), received)
        .await
        .unwrap()
        .unwrap();
    queue.cancel(&id).await.unwrap();
    release.send(()).unwrap();
    assert_eq!(done.await.unwrap().unwrap_err(), jobs::CANCELLED);
}

#[test]
fn completed_installation_activates_only_ready_variants_in_component_order() {
    let root = tempfile::tempdir().unwrap();
    let models = selection(
        root.path(),
        json!({"whisper_model":"large-v3", "asr_engine":"parakeet", "sep_backend":"cpu"}),
    );
    install_files(root.path(), "whisper-tiny");
    let all = manifest();
    let selected: Vec<_> = ["whisper-tiny", "whisper-base"]
        .iter()
        .map(|id| all.iter().find(|c| c.id == *id).unwrap())
        .collect();
    let result = finish(root.path(), &selected);
    let active = models::load_selection(&models);
    assert_eq!(active["asr_engine"], "whisper");
    assert_eq!(active["whisper_model"], "tiny");
    assert_eq!(active["sep_backend"], "cpu");
    assert_eq!(
        result["components"][0],
        json!({"id":"whisper-tiny", "installed":true, "missing":[]})
    );
    assert_eq!(result["components"][1]["installed"], false);
    assert!(!result["components"][1]["missing"].as_array().unwrap().is_empty());
    assert_eq!(result["ready"], setup_status(root.path()).ready);
}

#[test]
fn removal_clears_only_the_matching_variant_and_keeps_the_engine_and_other_stages() {
    let root = tempfile::tempdir().unwrap();
    let models = selection(
        root.path(),
        json!({"asr_engine":"whisper", "whisper_model":"medium", "sep_backend":"gpu"}),
    );
    let report = remove_components(root.path(), &["whisper-medium".into()]).unwrap();
    assert!(report.removed.is_empty());
    assert!(report.errors.is_empty());
    assert_eq!(
        models::load_selection(&models),
        json!({"asr_engine":"whisper", "sep_backend":"gpu"})
    );
    selection(root.path(), json!({"asr_engine":"whisper", "whisper_model":"large-v3"}));
    remove_components(root.path(), &["whisper-medium".into()]).unwrap();
    assert_eq!(models::load_selection(&models)["whisper_model"], "large-v3");
}

#[test]
fn removal_reports_selection_write_failure_after_removing_files() {
    let root = tempfile::tempdir().unwrap();
    let models = selection(root.path(), json!({"whisper_model":"large-v3"}));
    std::fs::create_dir(models.join("active.json.tmp")).unwrap();
    let c = manifest().into_iter().find(|c| c.id == "whisper-large-v3").unwrap();
    let path = root.path().join(c.files[0].dest_rel);
    sized(&path, 7);
    let report = remove_components(root.path(), &[c.id.into()]).unwrap();
    assert!(!path.exists());
    assert_eq!(report.freed_bytes, 7);
    assert_eq!(report.removed, [c.id]);
    assert_eq!(report.errors.len(), 1);
    assert!(report.errors[0].starts_with("active.json (whisper_model): "));
    assert_eq!(models::load_selection(&models)["whisper_model"], "large-v3");
}

#[test]
fn status_is_read_only_even_with_invalid_engine_and_model_files() {
    let root = tempfile::tempdir().unwrap();
    let models = selection(
        root.path(),
        json!({"local_backend":"cpu", "asr_engine":"whisper", "whisper_model":"tiny"}),
    );
    let before = std::fs::read(models.join("active.json")).unwrap();
    install_files(root.path(), "whisper-tiny");
    let engine = manifest().into_iter().find(|c| c.id == "transcribe-engine-cpu").unwrap();
    sized(&root.path().join(engine.markers[0].rel), 1);
    let st = setup_status(root.path());
    assert!(st.components.iter().find(|c| c.id == "whisper-tiny").unwrap().installed);
    assert!(!st.ready);
    assert_eq!(std::fs::read(models.join("active.json")).unwrap(), before);
    assert!(!models.join("active.json.tmp").exists());
    assert!(!models.join(".setup-download.json").exists());
}

fn model(id: &str, gb: f64, requirement: Requirement) -> ComponentStatus {
    ComponentStatus {
        id: id.into(),
        name: id.into(),
        purpose: String::new(),
        requirement,
        delivery: Delivery::Download,
        size: 0,
        installed: false,
        bytes_on_disk: 0,
        space_needed: 0,
        missing: vec![],
        detail: None,
        external_url: None,
        vram: (gb * 1024.0 * 1024.0 * 1024.0) as u64,
        fits_vram: None,
        taken_from: None,
    }
}

fn models() -> Vec<ComponentStatus> {
    vec![
        model("higgs", 5.6, Requirement::Required),
        model("higgs-q6_k", 5.1, Requirement::Optional),
        model("higgs-q4_k_m", 4.2, Requirement::Optional),
        model("gemma", 8.5, Requirement::Required),
        model("gemma-q8_0", 14.0, Requirement::Optional),
        model("higgs-engine", 0.0, Requirement::Required),
    ]
}

fn required(comps: &[ComponentStatus]) -> Vec<&str> {
    comps
        .iter()
        .filter(|c| c.requirement == Requirement::Required)
        .map(|c| c.id.as_str())
        .collect()
}

#[test]
fn a_downloaded_variant_stands_for_its_model_on_the_first_run() {
    let mut comps = vec![
        model("parakeet", 1.1, Requirement::Required),
        model("parakeet-ultra", 0.7, Requirement::Optional),
        model("gemma", 8.5, Requirement::Required),
        model("higgs-engine", 0.0, Requirement::Required),
    ];
    comps[1].installed = true;
    installed_variant_covers_group(&mut comps);
    assert_eq!(
        required(&comps),
        ["gemma", "higgs-engine"],
        "ultra was picked, the int8 default is no longer owed"
    );

    let mut comps = models();
    installed_variant_covers_group(&mut comps);
    assert_eq!(
        required(&comps),
        ["higgs", "gemma", "higgs-engine"],
        "nothing downloaded keeps every default"
    );
}

#[test]
fn a_small_card_gets_the_largest_quant_that_fits_and_no_model_it_cannot_hold() {
    let mut comps = models();
    fit_to_vram(&mut comps, 6 * 1024 * 1024 * 1024);
    assert_eq!(required(&comps), ["higgs-q6_k", "higgs-engine"]);
    assert_eq!(comps[0].fits_vram, Some(false));
    assert_eq!(comps[1].fits_vram, Some(true));
    assert_eq!(comps[3].fits_vram, Some(false));
    assert_eq!(comps[5].fits_vram, None);

    let mut comps = models();
    fit_to_vram(&mut comps, 24 * 1024 * 1024 * 1024);
    assert_eq!(required(&comps), ["higgs", "gemma", "higgs-engine"]);
    assert!(comps.iter().filter(|c| c.vram > 0).all(|c| c.fits_vram == Some(true)));

    let mut comps = models();
    comps[0].installed = true;
    fit_to_vram(&mut comps, 6 * 1024 * 1024 * 1024);
    assert_eq!(required(&comps), ["higgs", "higgs-engine"]);

    let mut comps = models();
    fit_to_vram(&mut comps, 0);
    assert_eq!(required(&comps), ["higgs", "gemma", "higgs-engine"]);
    assert!(comps.iter().all(|c| c.fits_vram.is_none()));
}

#[test]
fn drama_downloads_on_a_6_gb_card_and_not_on_a_4_gb_one() {
    let drama = || {
        let mut comps = vec![model("drama3", 0.0, Requirement::Optional)];
        comps[0].vram = component_status(
            tempfile::tempdir().unwrap().path(),
            &manifest().into_iter().find(|c| c.id == "drama3").unwrap(),
        )
        .vram;
        comps
    };
    let mut comps = drama();
    fit_to_vram(&mut comps, 6 * 1024 * 1024 * 1024);
    assert_eq!(comps[0].fits_vram, Some(true), "its experts live in RAM, the card keeps a cache");
    let mut comps = drama();
    fit_to_vram(&mut comps, 4 * 1024 * 1024 * 1024);
    assert_eq!(comps[0].fits_vram, Some(false));
}
