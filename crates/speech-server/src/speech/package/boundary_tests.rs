use super::*;

const LEGACY: &[u8] = br#"{
  "kind":"speech", "version":1, "name":"Legacy", "created":1700000123,
  "chapters":[{"id":"chapter-old", "blocks":[{
    "id":"block-old", "speaker":"Anna", "text":"Hello", "active":1,
    "takes":[{"n":1,"file":"old.flac","duration":1.25,"key":"legacy-key","created":1700000456}]
  }]}]
}"#;

fn paths(root: &std::path::Path) -> SpeechPaths {
    SpeechPaths {
        repo_root: root.into(),
        workspace: root.join("workspace"),
        voices_dir: root.join("voices"),
        models_root: root.join("models"),
    }
}

fn legacy_package(project: &[u8], extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut files = vec![("project.json", project)];
    files.extend_from_slice(extra);
    let manifest = serde_json::json!({
        "format":"speechproj", "version":1, "app_version":"0.1.0", "created":1700000123,
        "name":"Legacy", "engines":[{"id":"higgs"}], "with_audio":!extra.is_empty(), "voices":[],
        "assets":files.iter().map(|(path, bytes)| serde_json::json!({"path":path,"size":bytes.len(),"sha256":sha(bytes)})).collect::<Vec<_>>()
    });
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    out.start_file("manifest.json", options).unwrap();
    out.write_all(&serde_json::to_vec(&manifest).unwrap()).unwrap();
    for (name, bytes) in files {
        out.start_file(name, options).unwrap();
        out.write_all(bytes).unwrap();
    }
    out.finish().unwrap().into_inner()
}

#[test]
fn old_packages_keep_project_fields_and_defaults_through_the_new_storage() {
    let root = tempfile::tempdir().unwrap();
    let p = paths(root.path());
    let (id, voices) = unpack(&p, &legacy_package(LEGACY, &[])).unwrap();
    assert!(voices.is_empty());
    let project = crate::speech::load(&p.workspace, &id).unwrap();
    assert_eq!(project.created, 1_700_000_123);
    assert_eq!(project.settings.engine, "higgs");
    assert_eq!(project.settings.params.top_k, 50);
    let block = project.blocks().next().unwrap();
    assert_eq!(block.id, "block-old");
    assert_eq!(block.active, Some(1));
    assert_eq!(block.takes[0].key, "legacy-key");
    assert_eq!(block.takes[0].created, 1_700_000_456);
    assert_eq!(block.takes[0].bytes, 0);
    assert!(block.takes[0].engine.is_empty());
    assert_eq!(crate::app::revision(&id), 1);
    assert_eq!(std::fs::read_dir(p.workspace.join(id)).unwrap().count(), 1);
}

#[test]
fn invalid_package_projects_are_refused_before_voice_import_or_audio_publication() {
    let root = tempfile::tempdir().unwrap();
    let p = paths(root.path());
    let mut project: serde_json::Value = serde_json::from_slice(LEGACY).unwrap();
    let block = project["chapters"][0]["blocks"][0].clone();
    project["chapters"][0]["blocks"].as_array_mut().unwrap().push(block);
    let bytes = serde_json::to_vec(&project).unwrap();
    let archive = legacy_package(&bytes, &[("voices.zip", b"invalid voices zip"), ("audio/old.flac", b"audio")]);
    let error = unpack(&p, &archive).unwrap_err();
    assert_eq!(error, "block id \"block-old\" is empty or repeated");
    assert!(!p.voices_dir.exists());
    assert!(!p.workspace.exists());
}

#[test]
fn audio_packages_round_trip_take_bytes_keys_and_active_selection() {
    let root = tempfile::tempdir().unwrap();
    let p = paths(root.path());
    let audio = b"opaque archived take bytes";
    let (id, _) = unpack(&p, &legacy_package(LEGACY, &[("audio/old.flac", audio)])).unwrap();
    let before = crate::speech::load(&p.workspace, &id).unwrap();
    let (name, _) = pack(&p, &id, true).unwrap();
    let bytes = std::fs::read(p.workspace.join(&id).join("exports").join(name)).unwrap();
    let other = tempfile::tempdir().unwrap();
    let dest = paths(other.path());
    let (copied, _) = unpack(&dest, &bytes).unwrap();
    assert_eq!(crate::speech::load(&dest.workspace, &copied).unwrap(), before);
    assert_eq!(std::fs::read(dest.workspace.join(copied).join("takes/old.flac")).unwrap(), audio);
}
