use std::path::{Path, PathBuf};

pub(crate) fn is_pid(pid: &str) -> bool {
    !pid.is_empty() && pid.len() <= 40 && pid.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Untrusted IDs must pass `is_pid` before composing a path; this function does not validate or create files.
pub(crate) fn project_file(workspace: &Path, pid: &str) -> PathBuf {
    workspace.join(pid).join("project.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_ids_keep_their_original_grammar_and_length_limits() {
        for pid in ["a", "Z9", "0123456789", "LegacyProjectABC123", &"A9".repeat(20)] {
            assert!(is_pid(pid), "{pid}");
        }
        for pid in [
            "",
            &"a".repeat(41),
            ".",
            "..",
            "../outside",
            "..\\outside",
            "/absolute",
            "C:\\outside",
            "C:relative",
            "a/b",
            "a\\b",
            "abc_def",
            "abc-def",
            "a.b",
            "a b",
            "a\n",
            "a\0b",
            "%2e%2e",
            "проект",
            "café",
        ] {
            assert!(!is_pid(pid), "{pid:?}");
        }
    }

    #[test]
    fn project_file_keeps_the_legacy_layout_without_io() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("несуществующая папка");
        for pid in ["a", "LegacyProjectABC123", &"A9".repeat(20)] {
            assert_eq!(project_file(&workspace, pid), workspace.join(pid).join("project.json"));
            assert!(project_file(&workspace, pid).starts_with(&workspace));
        }
        assert_eq!(
            project_file(Path::new("relative workspace"), "Z9"),
            Path::new("relative workspace/Z9/project.json")
        );
        assert!(!workspace.exists());
    }
}
