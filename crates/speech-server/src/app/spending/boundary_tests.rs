use super::*;

#[test]
fn legacy_jsonl_keeps_origins_real_cost_and_the_existing_summary_shape() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("cloud");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(
        folder.join("spend.jsonl"),
        b"{\"day\":\"2026-10-14\",\"provider\":\"fal\",\"project\":\"p\",\"estimate\":0.5,\"cost\":0.2,\"origin\":\"api\"}\nbroken row\n",
    )
    .unwrap();
    let rows = spent(root.path());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].origin, "api");
    assert!(rows[0].model.is_empty());
    record(
        root.path(),
        Spent {
            day: today(),
            project: "other".into(),
            provider: "google".into(),
            estimate: Some(0.1),
            origin: "agent".into(),
            ..Default::default()
        },
    );
    let all = spent(root.path());
    assert_eq!(all.len(), 2);
    assert!((summary(&all, None)["total"].as_f64().unwrap() - 0.3).abs() < 1e-9);
    assert_eq!(summary(&all, Some("p"))["total"], json!(0.2));
    assert_eq!(summary(&all, None)["today"], json!(0.1));
    let line = std::fs::read_to_string(spend_path(root.path()))
        .unwrap()
        .lines()
        .last()
        .unwrap()
        .to_string();
    let value: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 10);
    assert_eq!(value["origin"], "agent");
}

#[test]
fn daily_limits_count_agent_and_api_use_only_and_keep_the_inclusive_boundary() {
    let _language = crate::i18n::test_language("ru");
    let root = tempfile::tempdir().unwrap();
    crate::models::set_selection(root.path(), "cloud_daily_limit_usd", "1").unwrap();
    for (origin, day, cost, estimate) in [
        ("api", today(), Some(0.2), Some(0.9)),
        ("agent", today(), None, Some(0.3)),
        ("window:1", today(), Some(10.0), None),
        ("agent", "2000-01-01".into(), Some(10.0), None),
    ] {
        record(
            root.path(),
            Spent {
                origin: origin.into(),
                day,
                cost,
                estimate,
                ..Default::default()
            },
        );
    }
    assert!(check_limit_as("agent", root.path(), root.path(), Some(0.5)).is_ok());
    assert!(check_limit_as("api", root.path(), root.path(), Some(0.51)).is_err());
    assert_eq!(
        check_limit_as("api", root.path(), root.path(), None).unwrap_err(),
        t!("cloud-limit-unpriced", limit = "1.00")
    );
    assert!(check_limit_as("window:1", root.path(), root.path(), None).is_ok());
    crate::models::set_selection(root.path(), "cloud_daily_limit_usd", "0").unwrap();
    assert!(check_limit_as("agent", root.path(), root.path(), None).is_ok());
}
