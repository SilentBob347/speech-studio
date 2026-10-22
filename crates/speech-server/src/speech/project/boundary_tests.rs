use super::*;

#[test]
fn validation_uses_supplied_engine_descriptions_without_the_runtime_catalog() {
    let mut p = from_text("test", "Text.", "Narrator");
    p.chapters[0].blocks[0].engine = Some("test-only-engine".into());
    let local = [EngineRule {
        id: "test-only-engine",
        cloud: false,
    }];
    let mut rules = ValidationRules {
        engines: &local,
        genders: &[],
        ages: &[],
    };
    p.validate(&rules).unwrap();
    let cloud = [EngineRule {
        id: "test-only-engine",
        cloud: true,
    }];
    rules.engines = &cloud;
    assert_eq!(
        p.validate(&rules).unwrap_err(),
        format!(
            "block {}: cloud engine \"test-only-engine\" is set up for the whole project only",
            p.chapters[0].blocks[0].id
        )
    );
    p.settings.engine = "test-only-engine".into();
    p.validate(&rules).unwrap();
    rules.engines = &[];
    assert_eq!(
        p.validate(&rules).unwrap_err(),
        format!(
            "block {}: engine \"test-only-engine\" is not one of the studio's engines",
            p.chapters[0].blocks[0].id
        )
    );
}

#[test]
fn validation_uses_supplied_character_metadata_without_reading_the_voice_library() {
    let mut p = SpeechProject::default();
    p.characters.insert(
        "Alex".into(),
        Character {
            gender: "custom-gender".into(),
            age: "custom-age".into(),
            ..Default::default()
        },
    );
    let rules = ValidationRules {
        engines: &[],
        genders: &["custom-gender"],
        ages: &["custom-age"],
    };
    p.validate(&rules).unwrap();
    let other = ValidationRules { ages: &["adult"], ..rules };
    assert_eq!(
        p.validate(&other).unwrap_err(),
        "character Alex: gender is one of [\"custom-gender\"] and age one of [\"adult\"]"
    );
}

#[test]
fn validation_keeps_error_order_and_pure_file_and_template_rules() {
    let rules = ValidationRules {
        engines: &[],
        genders: &[],
        ages: &[],
    };
    let mut p = SpeechProject {
        kind: "other".into(),
        ..Default::default()
    };
    p.settings.params.temperature = 9.0;
    assert_eq!(p.validate(&rules).unwrap_err(), "this is a other project, not a speech one");
    p.kind = KIND.into();
    assert_eq!(p.validate(&rules).unwrap_err(), "temperature 9 is outside 0.05..2.0");
    p.settings.params = Default::default();
    p.settings.export_name = "{unsupported}".into();
    assert_eq!(
        p.validate(&rules).unwrap_err(),
        "{unsupported}: no {unsupported}; a name may use {project}, {chapter}, {n}, {date}"
    );
    p.settings.export_name.clear();
    p.tracks.push(Track {
        id: "t".into(),
        clips: vec![TrackClip {
            id: "clip".into(),
            file: "../outside.wav".into(),
            duration: 2.0,
            ..Default::default()
        }],
        ..Default::default()
    });
    assert!(p
        .validate(&rules)
        .unwrap_err()
        .starts_with("track : clip \"clip\" (../outside.wav)"));
}
