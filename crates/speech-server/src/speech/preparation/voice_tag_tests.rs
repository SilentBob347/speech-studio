use super::*;

#[test]
fn voice_tags_keep_the_legacy_metadata_transcript_and_missing_voice_tokens() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(voice_tag(root.path(), None), "base");
    assert_eq!(voice_tag(root.path(), Some("absent")), "missing:absent");
    let audio = root.path().join("Anna.wav");
    std::fs::write(&audio, b"abc").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&audio)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)))
        .unwrap();
    std::fs::write(root.path().join("Anna.txt"), "Привет").unwrap();
    let expected = format!("Anna|3|1700000000|{}", blake3::hash("Привет".as_bytes()).to_hex());
    assert_eq!(voice_tag(root.path(), Some("Anna")), expected);
    std::fs::write(root.path().join("Anna.txt"), "Новая фраза").unwrap();
    assert_ne!(voice_tag(root.path(), Some("Anna")), expected);
    std::fs::remove_file(root.path().join("Anna.txt")).unwrap();
    assert_eq!(
        voice_tag(root.path(), Some("Anna")),
        format!("Anna|3|1700000000|{}", blake3::hash(b"").to_hex())
    );
}
