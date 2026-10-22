use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::{engines::models, platform::credentials};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Migrated {
    pub openrouter_key: bool,
    pub proxy_password: bool,
}

pub(crate) fn migrate_legacy_selection(models_root: &Path) -> Result<Migrated> {
    let dir = credentials::secrets_dir().context("no per-user application data directory for credential storage")?;
    migrate_legacy_selection_into(models_root, &dir)
}

fn migrate_legacy_selection_into(models_root: &Path, dir: &Path) -> Result<Migrated> {
    let _held = models::selection_writes();
    let mut selection = models::load_selection(models_root);
    let slots = selection.as_object_mut().expect("load_selection returns object");
    let mut migrated = Migrated::default();
    let mut changed = false;

    // Legacy values leave active.json only after every secret has reached its store.
    if let Some(legacy) = slots.remove("or_key") {
        if let Some(key) = legacy.as_str().map(str::trim).filter(|key| !key.is_empty()) {
            credentials::store_openrouter_api_key_in(dir, Some(key))?;
            migrated.openrouter_key = true;
        }
        changed = true;
    }

    if let Some(url) = slots.get("proxy_url").and_then(Value::as_str).map(str::to_owned) {
        let (bare, password) = models::split_proxy_password(&url);
        if let Some(password) = password {
            credentials::store_proxy_password_in(dir, Some(&password))?;
            migrated.proxy_password = true;
        }
        if bare != url {
            slots.insert("proxy_url".into(), Value::String(bare));
            changed = true;
        }
    }

    if changed {
        models::write_selection(models_root, &selection).context("rewrite active.json without secrets")?;
    }
    Ok(migrated)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const LEGACY: &str = r#"{"tts":"q6_k","or_key":"migration-test-key","proxy_on":"1","proxy_url":"http://alice:s3cr%40t@proxy.lan:3128","custom":{"keep":[1,true]}}"#;

    fn assert_migrated(models_root: &Path, secrets: &Path) {
        let mut expected: Value = serde_json::from_str(LEGACY).unwrap();
        expected.as_object_mut().unwrap().remove("or_key");
        expected["proxy_url"] = "http://alice@proxy.lan:3128".into();
        assert_eq!(models::load_selection(models_root), expected);
        assert_eq!(
            fs::read_to_string(secrets.join("openrouter-api-key")).unwrap(),
            "migration-test-key"
        );
        assert_eq!(credentials::proxy_password_in(secrets).as_deref(), Some("s3cr@t"));
    }

    #[test]
    fn legacy_secrets_leave_active_json_for_the_store() {
        let models = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        fs::write(models.path().join("active.json"), LEGACY).unwrap();

        let migrated = migrate_legacy_selection_into(models.path(), secrets.path()).unwrap();
        assert_eq!(
            migrated,
            Migrated {
                openrouter_key: true,
                proxy_password: true
            }
        );
        assert_migrated(models.path(), secrets.path());
        assert!(!models.path().join("active.json.tmp").exists());

        let on_disk = fs::read(models.path().join("active.json")).unwrap();
        assert_eq!(
            migrate_legacy_selection_into(models.path(), secrets.path()).unwrap(),
            Migrated::default()
        );
        assert_eq!(fs::read(models.path().join("active.json")).unwrap(), on_disk);
    }

    #[test]
    fn a_selection_without_secrets_is_left_untouched() {
        let models = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let original = "{\n  \"proxy_url\": \"socks5://proxy.lan:1080\",\n  \"tts\": \"q8_0\"\n}";
        fs::write(models.path().join("active.json"), original).unwrap();
        assert_eq!(
            migrate_legacy_selection_into(models.path(), secrets.path()).unwrap(),
            Migrated::default()
        );
        assert_eq!(fs::read_to_string(models.path().join("active.json")).unwrap(), original);
        assert!(fs::read_dir(secrets.path()).unwrap().next().is_none());
    }

    #[test]
    fn missing_settings_do_not_create_model_or_secret_directories() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            migrate_legacy_selection_into(&root.path().join("models"), &root.path().join("secrets")).unwrap(),
            Migrated::default()
        );
        assert!(fs::read_dir(root.path()).unwrap().next().is_none());
    }

    #[test]
    fn a_failed_secret_write_leaves_the_original_settings_for_retry() {
        let models = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let secrets = root.path().join("secrets");
        fs::write(&secrets, b"not a directory").unwrap();
        fs::write(models.path().join("active.json"), LEGACY).unwrap();

        let error = migrate_legacy_selection_into(models.path(), &secrets).unwrap_err();
        let detail = format!("{error:#}");
        assert!(detail.contains(&secrets.display().to_string()));
        assert!(!detail.contains("migration-test-key") && !detail.contains("s3cr%40t"));
        assert_eq!(fs::read_to_string(models.path().join("active.json")).unwrap(), LEGACY);
        assert_eq!(fs::read(&secrets).unwrap(), b"not a directory");

        fs::remove_file(&secrets).unwrap();
        migrate_legacy_selection_into(models.path(), &secrets).unwrap();
        assert_migrated(models.path(), &secrets);
    }

    #[test]
    fn failure_after_the_first_secret_keeps_settings_and_cleans_up_before_retry() {
        let models = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let blocked = secrets.path().join("proxy-password");
        fs::create_dir(&blocked).unwrap();
        fs::write(models.path().join("active.json"), LEGACY).unwrap();

        let error = migrate_legacy_selection_into(models.path(), secrets.path()).unwrap_err();
        assert!(format!("{error:#}").contains(&blocked.display().to_string()));
        assert_eq!(fs::read_to_string(models.path().join("active.json")).unwrap(), LEGACY);
        assert_eq!(
            fs::read_to_string(secrets.path().join("openrouter-api-key")).unwrap(),
            "migration-test-key"
        );
        let mut files = fs::read_dir(secrets.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        files.sort();
        assert_eq!(files, ["openrouter-api-key", "proxy-password"]);

        fs::remove_dir(&blocked).unwrap();
        migrate_legacy_selection_into(models.path(), secrets.path()).unwrap();
        assert_migrated(models.path(), secrets.path());
    }

    #[cfg(windows)]
    #[test]
    fn a_locked_selection_keeps_the_original_until_a_retry_can_publish() {
        use std::os::windows::fs::OpenOptionsExt;

        let models = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let path = models.path().join("active.json");
        fs::write(&path, LEGACY).unwrap();
        let lock = fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();

        let error = migrate_legacy_selection_into(models.path(), secrets.path()).unwrap_err();
        assert!(error.to_string().contains("rewrite active.json without secrets"));
        assert_eq!(fs::read_to_string(&path).unwrap(), LEGACY);
        assert_eq!(
            fs::read_to_string(secrets.path().join("openrouter-api-key")).unwrap(),
            "migration-test-key"
        );
        assert_eq!(credentials::proxy_password_in(secrets.path()).as_deref(), Some("s3cr@t"));
        drop(lock);

        migrate_legacy_selection_into(models.path(), secrets.path()).unwrap();
        assert_migrated(models.path(), secrets.path());
    }
}
