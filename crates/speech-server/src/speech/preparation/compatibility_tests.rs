use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use super::*;
use crate::text::adapter::Adapter;
use crate::text::stress::{Stressed, WordNote};

fn baseline() -> Value {
    serde_json::from_str(include_str!("compatibility.json")).unwrap()
}

fn basic_opts(root: &Path) -> Opts<'_> {
    Opts {
        lang: "ru",
        voice: None,
        engine: "higgs",
        entries: &[],
        normalize: true,
        normalize_off: &[],
        stress: false,
        models_root: root,
    }
}

#[test]
fn prepared_and_fingerprints_match_the_pre_boundary_baseline() {
    let root = tempfile::tempdir().unwrap();
    for case in baseline()["cases"].as_array().unwrap() {
        let entries: Vec<pron_dict::Entry> = serde_json::from_value(case.get("entries").cloned().unwrap_or(json!([]))).unwrap();
        let off: Vec<String> = serde_json::from_value(case.get("normalize_off").cloned().unwrap_or(json!([]))).unwrap();
        let o = Opts {
            lang: case.get("lang").and_then(Value::as_str).unwrap_or("auto"),
            voice: case.get("voice").and_then(Value::as_str),
            engine: case.get("engine").and_then(Value::as_str).unwrap_or("higgs"),
            entries: &entries,
            normalize: case.get("normalize").and_then(Value::as_bool).unwrap_or(true),
            normalize_off: &off,
            stress: case.get("stress").and_then(Value::as_bool).unwrap_or(false),
            models_root: root.path(),
        };
        let text = case["text"].as_str().unwrap();
        assert_eq!(
            serde_json::to_value(prepare(text, &o).unwrap()).unwrap(),
            case["prepared"],
            "{}",
            case["name"]
        );
        assert_eq!(
            fingerprint(text, &o, &prep::compile_all(&entries)),
            case["fingerprint"].as_str().unwrap(),
            "{}",
            case["name"]
        );
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn model_choice_and_capabilities_preserve_fingerprints_without_loading_models() {
    for case in baseline()["fingerprints"].as_array().unwrap() {
        let root = tempfile::tempdir().unwrap();
        if case["model"] == "big_poetry" {
            let dir = root.path().join("ruaccent/nn/nn_omograph/big_poetry");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("model.onnx"), b"").unwrap();
            fs::write(dir.join("tokenizer.json"), b"").unwrap();
        }
        let o = Opts {
            engine: case["engine"].as_str().unwrap(),
            stress: case["stress"].as_bool().unwrap(),
            ..basic_opts(root.path())
        };
        assert_eq!(
            fingerprint(case["text"].as_str().unwrap(), &o, &[]),
            case["fingerprint"].as_str().unwrap(),
            "{case}"
        );
    }
}

fn note(word: &str, source: &'static str) -> WordNote {
    WordNote {
        start: 0,
        end: word.chars().count(),
        word: word.into(),
        source,
        variants: Vec::new(),
        doubt: false,
        yo_choice: None,
    }
}

#[test]
fn model_receives_normalized_dictionary_text_and_adaptation_runs_last() {
    let root = tempfile::tempdir().unwrap();
    let entries = [pron_dict::Entry {
        id: "dict".into(),
        find: "ИИ".into(),
        say: "искусственный интеллект".into(),
        ..Default::default()
    }];
    let o = Opts {
        entries: &entries,
        stress: true,
        engine: "qwen3-tts-ru",
        ..basic_opts(root.path())
    };
    let p = prepare_with("<speak>ИИ <break time=\"200ms\"/> 5 книг.</speak>", &o, |text| {
        assert_eq!(text, "искусственный интеллект [pause 200ms] пять книг.");
        Ok(Stressed {
            text: "иск+усственный интеллект [pause 200ms] пять книг, ёлка и з+амок.".into(),
            words: vec![
                note("иск+усственный", "model"),
                note("ёлка", "yo"),
                note("з+амок", "omograph"),
                note("п+ять", "dictionary"),
                note("кн+иг", "manual"),
                note("слово", "model"),
            ],
        })
    })
    .unwrap();
    assert_eq!(p.text, "иск+усственный интеллект [pause 200ms] пять книг, ёлка и з+амок.");
    assert_eq!(p.engine_text, "иску\u{301}сственный интеллект пять книг, ёлка и за\u{301}мок.");
    assert_eq!(
        p.steps.iter().map(|s| s.layer).collect::<Vec<_>>(),
        ["ssml", "dictionary", "normalizer", "stress", "stress", "stress"]
    );
    assert_eq!(
        p.steps
            .iter()
            .filter(|s| s.layer == "stress")
            .map(|s| (s.rule.as_str(), s.from.as_str(), s.to.as_str()))
            .collect::<Vec<_>>(),
        [
            ("model", "искусственный", "иск+усственный"),
            ("yo", "ёлка", "ёлка"),
            ("omograph", "замок", "з+амок")
        ]
    );
}

#[test]
fn skipped_stress_never_invokes_inference() {
    let root = tempfile::tempdir().unwrap();
    for (text, lang, enabled) in [
        ("Привет", "ru", false),
        ("Hello", "en", true),
        ("Hello", "auto", true),
        ("Привет", "en", true),
    ] {
        let o = Opts {
            lang,
            stress: enabled,
            ..basic_opts(root.path())
        };
        let p = prepare_with(text, &o, |_| panic!("inference must stay lazy")).unwrap();
        assert_eq!(p.text, text);
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn inference_errors_reach_the_caller_without_a_partial_result_or_fallback() {
    let root = tempfile::tempdir().unwrap();
    let o = Opts {
        stress: true,
        ..basic_opts(root.path())
    };
    assert_eq!(
        prepare_with("Привет", &o, |_| Err("model failed with cause".into())).unwrap_err(),
        "model failed with cause"
    );
    let expected = stress_engine::shared(root.path()).err().expect("missing model must fail");
    assert_eq!(prepare("Привет", &o).unwrap_err(), expected);
}

#[test]
fn ignored_stress_keeps_the_original_manual_mark_policy() {
    let root = tempfile::tempdir().unwrap();
    let o = Opts {
        stress: true,
        ..basic_opts(root.path())
    };
    let automatic = prepare_with("замок", &o, |_| {
        Ok(Stressed {
            text: "з+амок".into(),
            words: vec![note("з+амок", "model")],
        })
    })
    .unwrap();
    assert_eq!(automatic.engine_text, "замок");
    assert!(!automatic.ignored.stress);
    let manual = prepare_with("з+амок", &o, |_| {
        Ok(Stressed {
            text: "з+амок".into(),
            words: Vec::new(),
        })
    })
    .unwrap();
    assert!(manual.ignored.stress);
    let entries = [pron_dict::Entry {
        id: "manual".into(),
        find: "замок".into(),
        say: "з+амок".into(),
        ..Default::default()
    }];
    let dictionary = prepare(
        "замок",
        &Opts {
            entries: &entries,
            stress: false,
            ..o
        },
    )
    .unwrap();
    assert!(!dictionary.ignored.stress);
}

#[test]
fn common_preparation_accepts_opaque_engine_scope_and_explicit_adapter_data() {
    let o = prep::Opts {
        lang: "ru",
        voice: None,
        engine: "caller-owned-format",
        entries: &[],
        normalize: false,
        normalize_off: &[],
    };
    let draft = prep::prepare("**З+амок** <custom>да [pause 300ms] [[нет]]", &o);
    let p = draft.finish(Adapter::Plain {
        acute: true,
        own: &["custom"],
    });
    assert_eq!(p.engine_text, "За\u{301}мок <custom>да нет");
    assert!(p.ignored.accent && p.ignored.phonemes);
    assert!(p.ignored.unknown_tags.is_empty());
    assert_ne!(
        prep::fingerprint("Привет", &o, &[], Some("caller-owned-stress")),
        prep::fingerprint("Привет", &o, &[], None)
    );
}
