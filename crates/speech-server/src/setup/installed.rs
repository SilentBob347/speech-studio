//! Read-only component availability; no active selection or engine construction.

use super::*;

/// Archive records must name the pinned hash and contain files of the recorded sizes.
pub(super) fn archive_installed(repo_root: &Path, f: &FileSpec) -> bool {
    read_record(repo_root, f)
        .is_some_and(|r| r.sha256 == f.sha256 && !r.files.is_empty() && r.files.iter().all(|x| file_ok(&repo_root.join(&x.path), x.size)))
}

pub(super) fn file_installed(repo_root: &Path, f: &FileSpec) -> bool {
    match f.extract {
        Extract::None => file_ok(&repo_root.join(f.dest_rel), f.size),
        _ => archive_installed(repo_root, f),
    }
}

/// Статус компонента на диске.
pub fn component_status(repo_root: &Path, c: &Component) -> ComponentStatus {
    let system = if FOUND_IN_SYSTEM_DIR.contains(&c.id) { system_dir() } else { None };
    status_with_system_dir(repo_root, c, system.as_deref())
}

/// `system` — системный каталог, где засчитываются маркеры компонентов FOUND_IN_SYSTEM_DIR.
pub(super) fn status_with_system_dir(repo_root: &Path, c: &Component, system: Option<&Path>) -> ComponentStatus {
    let mut missing: Vec<String> = Vec::new();
    let mut bytes_on_disk = 0u64;
    let mut space_needed = 0u64;
    for f in c.files {
        if file_installed(repo_root, f) {
            bytes_on_disk += f.size;
        } else {
            missing.push(f.dest_rel.to_string());
            bytes_on_disk += resumed_bytes(&part_path(repo_root, f), f.size);
            space_needed += file_space_needed(repo_root, f);
        }
    }
    let mut from_system = false;
    for m in c.markers {
        if marker_ok(repo_root, m) || missing.iter().any(|x| x == m.rel) {
            continue;
        }
        if system.is_some_and(|dir| in_system_dir(dir, m)) {
            from_system = true;
        } else {
            missing.push(m.rel.to_string());
        }
    }
    if c.delivery == Delivery::Bundled {
        bytes_on_disk = c.markers.iter().map(|m| file_len(&repo_root.join(m.rel))).sum();
    }
    let (installed, detail) = match c.delivery {
        Delivery::External => {
            let gpu = crate::hw::gpu_report();
            (gpu.cuda13_ok, gpu.summary())
        }
        _ if c.id == "ffmpeg" && !missing.is_empty() && ffmpeg_on_path() => {
            // ffmpeg из PATH годится пайплайну (Command::new("ffmpeg") его найдёт) — закачку не навязываем.
            missing.clear();
            space_needed = 0;
            (true, Some("PATH".to_string()))
        }
        _ => (missing.is_empty(), from_system.then(|| "system".to_string())),
    };
    ComponentStatus {
        id: c.id.to_string(),
        name: c.name.to_string(),
        purpose: c.purpose.to_string(),
        requirement: c.requirement,
        delivery: c.delivery,
        size: c.size,
        installed,
        bytes_on_disk,
        space_needed,
        missing,
        detail,
        external_url: c.external_url.map(|s| s.to_string()),
        vram: vram_estimate(c.id),
        fits_vram: None,
        taken_from: lock(taken()).get(c.id).cloned(),
    }
}
