use super::*;

#[tokio::test]
async fn language_persistence_keeps_validation_and_write_failure_order() {
    let _language = i18n::test_language("en");
    let root = tempfile::tempdir().unwrap();
    let state = AppState::new(root.path());
    let file = root.path().join(UI_LANG_FILE);
    let result = set_ui_lang(State(state.clone()), Json(json!({"lang":"ru"}))).await.unwrap();
    assert_eq!(result.0, json!({"lang":"ru"}));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "ru");
    let (status, body) = set_ui_lang(State(state.clone()), Json(json!({"lang":"unknown"})))
        .await
        .unwrap_err();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body.0["code"], "bad_lang");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "ru");
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    let (status, body) = set_ui_lang(State(state), Json(json!({"lang":"fr"}))).await.unwrap_err();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body.0["code"], "write_failed");
    assert!(body.0["detail"].as_str().unwrap().contains(UI_LANG_FILE));
    assert_eq!(i18n::language().to_string(), "fr");
    i18n::set_language("en").unwrap();
}
