use super::*;

fn selection(root: &Path, value: serde_json::Value) -> std::path::PathBuf {
    let models = root.join("models");
    std::fs::create_dir_all(&models).unwrap();
    std::fs::write(models.join("active.json"), value.to_string()).unwrap();
    models
}

#[test]
fn asr_preparation_uses_its_stage_backend_instead_of_other_stages() {
    let root = tempfile::tempdir().unwrap();
    let models = selection(
        root.path(),
        json!({"local_backend":"gpu", "asr_backend":"cpu", "sep_backend":"gpu", "diar_backend":"gpu"}),
    );
    assert_eq!(
        missing_parts(root.path(), &models, "parakeet"),
        ["transcribe-engine-cpu", "parakeet"]
    );
    selection(
        root.path(),
        json!({"local_backend":"cpu", "asr_backend":"gpu", "sep_backend":"cpu", "diar_backend":"cpu"}),
    );
    assert_eq!(
        missing_parts(root.path(), &models, "whisper"),
        ["whisper-engine", "whisper-large-v3-turbo", "whisper-cuda"]
    );
}

#[test]
fn an_explicit_missing_recognizer_returns_an_error_without_installing() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    let models = selection(root.path(), json!({"local_backend":"cpu", "asr_engine":"parakeet"}));
    let before = std::fs::read(models.join("active.json")).unwrap();
    assert_eq!(
        choice_of(root.path(), &models, "en", "voice", "whisper").unwrap_err(),
        t!("listen-whisper-missing")
    );
    assert!(!root.path().join("tools").exists());
    assert!(!models.join("whisper").exists());
    assert_eq!(std::fs::read(models.join("active.json")).unwrap(), before);
}

#[test]
fn resolved_whisper_spec_keeps_cpu_int8_when_other_stages_use_gpu() {
    let root = tempfile::tempdir().unwrap();
    let models = selection(
        root.path(),
        json!({
            "local_backend":"gpu", "asr_backend":"cpu", "sep_backend":"gpu", "diar_backend":"gpu",
            "asr_engine":"whisper", "whisper_model":"tiny", "whisper_compute":"int8_float16"
        }),
    );
    let bin = if cfg!(windows) {
        root.path().join("tools/whisper/Faster-Whisper-XXL/faster-whisper-xxl.exe")
    } else {
        root.path().join("tools/whisper/whisper-faster")
    };
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, b"not an executable").unwrap();
    let weights = models.join("whisper/faster-whisper-tiny/model.bin");
    std::fs::create_dir_all(weights.parent().unwrap()).unwrap();
    std::fs::write(weights, b"not model weights").unwrap();
    match choice(root.path(), &models, "en", "voice").unwrap() {
        crate::engines::asr::AsrChoice::Whisper {
            bin: actual,
            model,
            compute,
            device,
            ..
        } => {
            assert_eq!(actual, bin);
            assert_eq!(model, "tiny");
            assert_eq!(compute, "int8");
            assert_eq!(device, "cpu");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn each_request_resolves_current_selection_without_changing_previous_specification() {
    let root = tempfile::tempdir().unwrap();
    let models = selection(root.path(), json!({"local_backend":"cpu", "asr":"int8"}));
    for id in ["transcribe-engine-cpu", "parakeet", "parakeet-ultra"] {
        for marker in crate::setup::manifest().iter().find(|c| c.id == id).unwrap().markers {
            let path = root.path().join(marker.rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"invalid engine or model, only availability is checked").unwrap();
        }
    }
    let first = choice(root.path(), &models, "en", "voice").unwrap();
    selection(root.path(), json!({"local_backend":"cpu", "asr":"ultra-int8"}));
    let second = choice(root.path(), &models, "en", "voice").unwrap();
    assert!(
        matches!(first, crate::engines::asr::AsrChoice::Ggml {model, build: crate::engines::tcpp_asr::Build::Cpu, ..} if model.id == "parakeet-v3")
    );
    assert!(
        matches!(second, crate::engines::asr::AsrChoice::Ggml {model, build: crate::engines::tcpp_asr::Build::Cpu, ..} if model.id == "parakeet-ultra")
    );
    assert_ne!(first.label(), second.label());
}

#[test]
fn an_explicit_engine_language_error_keeps_the_requested_engine() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    let models = selection(root.path(), json!({"local_backend":"cpu"}));
    let err = choice_of(root.path(), &models, "zz", "voice", "nemotron").unwrap_err();
    assert_eq!(
        err,
        t!(
            "listen-gguf-lang",
            engine = crate::engines::tcpp_asr::model("nemotron").unwrap().name,
            lang = "zz".to_string()
        )
    );
}

#[test]
fn missing_parts_name_the_chosen_recogniser() {
    let root = tempfile::tempdir().unwrap();
    let models = root.path().join("models");
    let whisper = super::missing_parts(root.path(), &models, "whisper");
    assert!(whisper.iter().any(|id| id == "whisper-engine"), "{whisper:?}");
    assert!(whisper.iter().any(|id| id == "whisper-large-v3-turbo"), "{whisper:?}");
    let nemotron = super::missing_parts(root.path(), &models, "nemotron");
    assert!(nemotron.iter().any(|id| id == "asr-gguf-nemotron"), "{nemotron:?}");
    assert!(!nemotron.iter().any(|id| id.starts_with("whisper")), "{nemotron:?}");
}
