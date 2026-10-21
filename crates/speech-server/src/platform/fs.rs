use std::{fs::File, io::Write, path::Path};

use tempfile::TempPath;

/// The caller owns parent creation and synchronization of read-modify-write operations.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_atomic_with(path, |file| file.write_all(bytes))
}

fn write_atomic_with(path: &Path, write: impl FnOnce(&mut File) -> std::io::Result<()>) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    // A unique sibling prevents collisions and keeps publication on the same filesystem.
    let mut staged = tempfile::Builder::new()
        .prefix(".atomic-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if let Err(error) = write(staged.as_file_mut()).and_then(|()| staged.as_file().sync_all()) {
        return Err(cleanup(staged.into_temp_path(), format!("{}: {error}", path.display())));
    }
    // An open handle to a published file can block a concurrent replacement on Windows.
    match staged.into_temp_path().persist(path) {
        Ok(()) => Ok(()),
        Err(error) => Err(cleanup(error.path, format!("{}: {}", path.display(), error.error))),
    }
}

fn cleanup(staged: TempPath, cause: String) -> String {
    let path = staged.to_path_buf();
    match staged.close() {
        Ok(()) => cause,
        Err(error) => format!("{cause}; {}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs, io,
        sync::{Arc, Barrier},
    };

    use super::*;

    fn files(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut files = fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
        files.sort();
        files
    }

    #[test]
    fn creates_and_replaces_a_file_without_leaving_staging_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("словарь.json");
        write_atomic(&path, b"first\x00\xff").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first\x00\xff");
        write_atomic(&path, b"next").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"next");
        write_atomic(&path, b"").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"");
        assert_eq!(files(dir.path()), vec![path]);
    }

    #[test]
    fn a_partial_write_keeps_the_previous_file_and_removes_the_staging_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.tmp");
        fs::write(&path, b"previous").unwrap();
        let error = write_atomic_with(&path, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected write failure"))
        })
        .unwrap_err();
        assert!(error.contains(&path.display().to_string()) && error.contains("injected write failure"));
        assert_eq!(fs::read(&path).unwrap(), b"previous");
        assert_eq!(files(dir.path()), vec![path]);
    }

    #[test]
    fn a_failed_first_write_does_not_publish_a_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.json");
        let error = write_atomic_with(&path, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected write failure"))
        })
        .unwrap_err();
        assert!(error.contains("injected write failure"));
        assert!(!path.exists());
        assert!(files(dir.path()).is_empty());
    }

    #[test]
    fn publication_failure_preserves_the_destination_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("directory");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("keep.txt"), b"keep").unwrap();
        let error = write_atomic(&path, b"replacement").unwrap_err();
        assert!(error.contains(&path.display().to_string()));
        assert_eq!(fs::read(path.join("keep.txt")).unwrap(), b"keep");
        assert_eq!(files(dir.path()), vec![path]);
    }

    #[test]
    fn a_missing_parent_is_reported_without_creating_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing").join("state.json");
        let error = write_atomic(&path, b"state").unwrap_err();
        assert!(error.contains(&path.display().to_string()));
        assert!(files(dir.path()).is_empty());
    }

    #[test]
    fn simultaneous_saves_with_the_same_stem_keep_each_files_contents() {
        let dir = tempfile::tempdir().unwrap();
        let ready = Arc::new(Barrier::new(3));
        let release = Arc::new(Barrier::new(3));
        std::thread::scope(|scope| {
            let one = |name: &str, contents: &'static [u8]| {
                let path = dir.path().join(name);
                let ready = ready.clone();
                let release = release.clone();
                scope.spawn(move || {
                    write_atomic_with(&path, |file| {
                        file.write_all(contents)?;
                        ready.wait();
                        release.wait();
                        Ok(())
                    })
                    .unwrap();
                    path
                })
            };
            let json = one("voice.json", b"card");
            let text = one("voice.txt", b"transcript");
            ready.wait();
            let unpublished = !dir.path().join("voice.json").exists() && !dir.path().join("voice.txt").exists();
            release.wait();
            assert_eq!(fs::read(json.join().unwrap()).unwrap(), b"card");
            assert_eq!(fs::read(text.join().unwrap()).unwrap(), b"transcript");
            assert!(unpublished);
        });
        assert_eq!(files(dir.path()).len(), 2);
    }

    #[test]
    fn competing_saves_keep_a_complete_result_and_report_rejected_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.json");
        fs::write(&path, b"previous").unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let first = vec![b'a'; 128 * 1024];
        let second = vec![b'b'; 128 * 1024];
        let results = std::thread::scope(|scope| {
            let mut writers = Vec::new();
            for contents in [&first, &second] {
                let barrier = barrier.clone();
                let path = &path;
                writers.push(scope.spawn(move || {
                    write_atomic_with(path, |file| {
                        file.write_all(contents)?;
                        barrier.wait();
                        Ok(())
                    })
                }));
            }
            writers.into_iter().map(|writer| writer.join().unwrap()).collect::<Vec<_>>()
        });
        assert!(results.iter().any(Result::is_ok));
        for error in results.into_iter().filter_map(Result::err) {
            assert!(error.contains(&path.display().to_string()));
        }
        let saved = fs::read(&path).unwrap();
        assert!(saved == first || saved == second);
        assert_eq!(files(dir.path()), vec![path]);
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_lock_preserves_the_old_file_and_reports_the_failure() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locked.json");
        fs::write(&path, b"previous").unwrap();
        let lock = fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        let error = write_atomic(&path, b"replacement").unwrap_err();
        assert!(error.contains(&path.display().to_string()));
        drop(lock);
        assert_eq!(fs::read(&path).unwrap(), b"previous");
        assert_eq!(files(dir.path()), vec![path]);
    }
}
