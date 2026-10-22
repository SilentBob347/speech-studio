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
        ["crisperwhisper-turbo", "langid", "transcribe-engine", "cuda-runtime"]
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
fn each_request_resolves_current_selection_without_changing_previous_specification() {
    let root = tempfile::tempdir().unwrap();
    let models = selection(root.path(), json!({"local_backend":"cpu", "asr":"int8"}));
    for id in ["transcribe-engine-cpu", "parakeet", "parakeet-ultra"] {
        // installed means the pinned size: the content is not read here
        for f in crate::setup::manifest().iter().find(|c| c.id == id).unwrap().files {
            let path = root.path().join(f.dest_rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::File::create(path).unwrap().set_len(f.size).unwrap();
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
    assert!(whisper.iter().any(|id| id == "crisperwhisper-turbo"), "{whisper:?}");
    assert!(whisper.iter().any(|id| id == "langid"), "{whisper:?}");
    let nemotron = super::missing_parts(root.path(), &models, "nemotron");
    assert!(nemotron.iter().any(|id| id == "asr-gguf-nemotron"), "{nemotron:?}");
    assert!(!nemotron.iter().any(|id| id.starts_with("crisperwhisper")), "{nemotron:?}");
}
