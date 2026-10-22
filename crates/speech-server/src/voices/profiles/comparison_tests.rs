use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use super::*;

fn paths(root: &Path) -> Paths {
    Paths {
        repo_root: root.to_path_buf(),
        voices_dir: root.join("voices"),
        models_root: root.join("models"),
    }
}

fn profile(name: &str, embedding: Vec<f32>) -> Profile {
    Profile {
        name: name.into(),
        embedding,
        recordings: 1,
        pids: vec!["previous".into()],
    }
}

#[test]
fn old_profiles_without_pids_load_and_keep_the_existing_file_schema() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(FILE), br#"[{"name":"Anna","embedding":[1.0,0.0],"recordings":1}]"#).unwrap();
    assert!(load(root.path()).unwrap()[0].pids.is_empty());
    assert_eq!(
        remember_print(root.path(), "new", "Anna", vec![0.0, 1.0]).unwrap(),
        json!({ "name": "Anna", "recordings": 2 })
    );
    let profiles = load(root.path()).unwrap();
    assert_eq!(profiles[0].pids, ["new"]);
    assert!((profiles[0].embedding[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    let stored: Value = serde_json::from_slice(&fs::read(root.path().join(FILE)).unwrap()).unwrap();
    assert_eq!(
        stored[0].as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>().len(),
        4
    );
    assert_eq!(stored[0]["name"], "Anna");
    assert_eq!(stored[0]["recordings"], 2);
    assert!(stored[0]["embedding"].is_array() && stored[0]["pids"].is_array());
    assert!(!root.path().join("speaker-profiles.json.tmp").exists());
}

#[test]
fn each_recording_is_learnt_once_and_profile_names_keep_their_case() {
    let root = tempfile::tempdir().unwrap();
    remember_print(root.path(), "one", "Anna", vec![1.0, 0.0]).unwrap();
    remember_print(root.path(), "two", "Anna", vec![0.0, 1.0]).unwrap();
    let before = fs::read(root.path().join(FILE)).unwrap();
    assert_eq!(
        remember_print(root.path(), "two", "Anna", vec![-1.0, 0.0]).unwrap(),
        json!({ "name": "Anna", "recordings": 2 })
    );
    assert_eq!(fs::read(root.path().join(FILE)).unwrap(), before);
    remember_print(root.path(), "one", "anna", vec![0.0, 1.0]).unwrap();
    let profiles = load(root.path()).unwrap();
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].pids, ["one", "two"]);
    assert_eq!(profiles[1].name, "anna");
    assert_eq!(
        listing(root.path()).unwrap(),
        json!({ "profiles": [{"name":"Anna","recordings":2},{"name":"anna","recordings":1}] })
    );
}

#[test]
fn matching_uses_prepared_samples_and_the_existing_library_print_cache() {
    let root = tempfile::tempdir().unwrap();
    let p = paths(root.path());
    save(&p.repo_root, &[profile("Profile", vec![1.0, 0.0])]).unwrap();
    fs::create_dir_all(&p.voices_dir).unwrap();
    let source = p.voices_dir.join("Library.wav");
    fs::write(&source, b"cached-source").unwrap();
    fs::File::options()
        .write(true)
        .open(&source)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000)))
        .unwrap();
    let cache = p.voices_dir.join(".work/prints");
    fs::create_dir_all(&cache).unwrap();
    let tag = blake3::hash(b"Library|13|1700000000").to_hex()[..16].to_string();
    let cached = cache.join(format!("{tag}.json"));
    fs::write(&cached, b"[0.0,1.0]").unwrap();
    let before = fs::read(&cached).unwrap();
    let mut calls = 0;
    let speakers = [
        Speaker {
            key: "a",
            name: Some("Alice"),
            audio: &[1.0, 0.0],
        },
        Speaker {
            key: "b",
            name: Some("Bob"),
            audio: &[0.0, 1.0],
        },
        Speaker {
            key: "c",
            name: Some("Chris"),
            audio: &[-1.0, 0.0],
        },
    ];
    let result = matches_with(&p, &speakers, |samples| {
        calls += 1;
        Ok(samples.to_vec())
    })
    .unwrap();
    let threshold = dub_faces::voice_cos_threshold() as f64;
    assert_eq!(
        result,
        json!({ "speakers": [
        {"speaker":"a","name":"Alice","best":{"name":"Profile","kind":"profile","score":1.0,"same":true}},
        {"speaker":"b","name":"Bob","best":{"name":"Library","kind":"voice","score":1.0,"same":true}},
        {"speaker":"c","name":"Chris","best":{"name":"Library","kind":"voice","score":0.0,"same":false}}
    ], "threshold": threshold })
    );
    assert_eq!(calls, 3);
    assert_eq!(fs::read(cached).unwrap(), before);
    assert!(!p.models_root.exists());
}

#[test]
fn ties_keep_the_last_candidate_and_threshold_uses_the_unrounded_score() {
    let root = tempfile::tempdir().unwrap();
    let p = paths(root.path());
    save(root.path(), &[profile("First", vec![1.0, 0.0]), profile("Last", vec![1.0, 0.0])]).unwrap();
    let speakers = [Speaker {
        key: "a",
        name: Some("Alice"),
        audio: &[1.0, 0.0],
    }];
    assert_eq!(
        matches_with(&p, &speakers, |s| Ok(s.to_vec())).unwrap()["speakers"][0]["best"]["name"],
        "Last"
    );
    let score = dub_faces::voice_cos_threshold() - 0.0001;
    save(root.path(), &[profile("Almost", vec![score, (1.0 - score * score).sqrt()])]).unwrap();
    let result = matches_with(&p, &speakers, |s| Ok(s.to_vec())).unwrap();
    assert_eq!(result["speakers"][0]["best"]["same"], false);
    assert_eq!(result["speakers"][0]["best"]["score"], ((score as f64) * 100.0).round() / 100.0);
}

#[test]
fn empty_candidates_and_embedding_errors_keep_the_profile_store_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let p = paths(root.path());
    let speakers = [Speaker {
        key: "a",
        name: None,
        audio: &[1.0, 0.0],
    }];
    let result = matches_with(&p, &speakers, |s| Ok(s.to_vec())).unwrap();
    assert_eq!(result["speakers"], json!([{"speaker":"a","name":null,"best":null}]));
    save(root.path(), &[profile("Anna", vec![1.0, 0.0])]).unwrap();
    let before = fs::read(root.path().join(FILE)).unwrap();
    assert_eq!(
        matches_with(&p, &speakers, |_| Err("embedding failed".into())).unwrap_err(),
        "embedding failed"
    );
    assert_eq!(fs::read(root.path().join(FILE)).unwrap(), before);
    fs::write(root.path().join(FILE), b"broken JSON").unwrap();
    assert!(matches_with(&p, &speakers, |_| panic!("profile errors must precede inference"))
        .unwrap_err()
        .contains(FILE));
}

#[test]
fn profile_save_failure_keeps_the_existing_json_and_reports_the_cause() {
    let root = tempfile::tempdir().unwrap();
    save(root.path(), &[profile("Anna", vec![1.0, 0.0])]).unwrap();
    let before = fs::read(root.path().join(FILE)).unwrap();
    let blocked = root.path().join("speaker-profiles.json.tmp");
    fs::create_dir(&blocked).unwrap();
    let error = remember_print(root.path(), "new", "Anna", vec![0.0, 1.0]).unwrap_err();
    assert!(error.starts_with(&format!("{}:", blocked.display())));
    assert_eq!(fs::read(root.path().join(FILE)).unwrap(), before);
}
