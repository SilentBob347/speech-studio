//! DLL validation policy for the installed components and the pinned setup archives.

use std::path::{Path, PathBuf};

use crate::platform::dll_imports::check_binaries;
use crate::setup::{self, Component, Delivery};

/// Каталоги, где процессы студии ищут DLL: PATH приложения (lib.rs::augment_path_for_tools) — ffmpeg, llama и
/// каталог движка Higgs с CUDA-библиотеками. Каталог самого бинаря добавляется к ним при проверке.
fn search_dirs(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join("tools").join("ffmpeg"),
        root.join("tools").join("llama"),
        root.join("models").join("higgs-engine"),
    ]
}

/// Что грузят процессы студии: (компонент, файл). ggml-cuda.dll llama.cpp и BSRoformer, подбиблиотеки cuDNN
/// грузятся LoadLibrary'ем, поэтому проверяются отдельными точками входа.
pub const ENTRY_POINTS: &[(&str, &str)] = &[
    ("llama", "tools/llama/llama-server.exe"),
    ("llama", "tools/llama/ggml-cuda.dll"),
    ("higgs-engine", "models/higgs-engine/audiocpp_engine.dll"),
    ("drama3", "models/drama-engine/drama_engine.dll"),
    ("qwen3-tts-engine", "models/qwen3-tts/qwen.dll"),
    ("qwen3-tts-engine", "models/qwen3-tts/ggml-cuda.dll"),
    ("onnxruntime", "models/runtime/onnxruntime-win-x64-1.28.2/lib/onnxruntime.dll"),
    (
        "onnxruntime-gpu",
        "models/runtime/onnxruntime-win-x64-gpu_cuda13-1.28.2/lib/onnxruntime_providers_cuda.dll",
    ),
    ("cudnn", "models/higgs-engine/cudnn64_9.dll"),
    ("cudnn", "models/higgs-engine/cudnn_graph64_9.dll"),
    ("cudnn", "models/higgs-engine/cudnn_ops64_9.dll"),
    ("cudnn", "models/higgs-engine/cudnn_cnn64_9.dll"),
    ("cudnn", "models/higgs-engine/cudnn_adv64_9.dll"),
    ("cudnn", "models/higgs-engine/cudnn_heuristic64_9.dll"),
    ("cudnn", "models/higgs-engine/cudnn_engines_precompiled64_9.dll"),
    ("bsroformer-engine", "tools/bsroformer/bs_roformer-cli.exe"),
    ("bsroformer-engine", "tools/bsroformer/ggml-cuda.dll"),
    ("bsroformer-engine-cpu", "tools/bsroformer-cpu/bs_roformer-cli.exe"),
    ("ffmpeg", "tools/ffmpeg/ffmpeg.exe"),
    ("whisper-cuda", "tools/whisper/cublas64_11.dll"),
    ("whisper-cuda", "tools/whisper/cudnn_ops_infer64_8.dll"),
    ("whisper-cuda", "tools/whisper/cudnn_cnn_infer64_8.dll"),
];

/// Имена файлов (в нижнем регистре), которые ставит или приносит манифест в каталоги поиска: прямые файлы,
/// маркеры (ключевые файлы архивов) и файлы Bundled-компонентов.
fn manifest_basenames(root: &Path, manifest: &[Component]) -> Vec<(String, PathBuf, &'static str)> {
    let mut out = Vec::new();
    for c in manifest {
        let rels = c
            .files
            .iter()
            .filter(|f| f.extract == setup::Extract::None)
            .map(|f| f.dest_rel)
            .chain(c.markers.iter().map(|m| m.rel));
        for rel in rels {
            let p = root.join(rel);
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                out.push((
                    name.to_ascii_lowercase(),
                    p.parent().map(Path::to_path_buf).unwrap_or_default(),
                    c.id,
                ));
            }
        }
    }
    out
}

#[cfg(test)]
fn check_root(root: &Path, seen: &mut dyn FnMut(&str, &(String, PathBuf))) -> Result<Vec<String>, String> {
    check_components(root, &|_| true, seen)
}

pub(crate) fn check_components(
    root: &Path,
    include: &dyn Fn(&str) -> bool,
    seen: &mut dyn FnMut(&str, &(String, PathBuf)),
) -> Result<Vec<String>, String> {
    let manifest = setup::manifest();
    let bundled: Vec<(String, PathBuf)> = manifest_basenames(root, &manifest)
        .into_iter()
        .filter(|(_, _, id)| manifest.iter().any(|c| c.id == *id && c.delivery == Delivery::Bundled))
        .map(|(name, dir, _)| (name, dir))
        .collect();
    let entries: Vec<_> = ENTRY_POINTS.iter().copied().filter(|(component, _)| include(component)).collect();
    check_binaries(root, &entries, &search_dirs(root), &bundled, seen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_checks_only_the_requested_components() {
        let root = tempfile::tempdir().unwrap();
        assert!(check_components(root.path(), &|_| false, &mut |_, _| {}).unwrap().is_empty());
        let missing = check_components(root.path(), &|id| id == "ffmpeg", &mut |_, _| {}).unwrap();
        assert_eq!(missing, ["ffmpeg: no tools/ffmpeg/ffmpeg.exe after installation"]);
    }

    #[test]
    fn archive_names_do_not_count_as_installed_libraries() {
        let root = Path::new("R");
        let manifest = setup::manifest();
        let known = manifest_basenames(root, &manifest);
        let vc = manifest.iter().find(|c| c.id == "vcruntime").unwrap();
        for marker in vc.markers {
            let path = root.join(marker.rel);
            let name = path.file_name().unwrap().to_str().unwrap().to_ascii_lowercase();
            assert!(known.contains(&(name, path.parent().unwrap().to_path_buf(), vc.id)));
        }
        for component in &manifest {
            for archive in component.files.iter().filter(|file| file.extract != setup::Extract::None) {
                let path = root.join(archive.dest_rel);
                let name = path.file_name().unwrap().to_str().unwrap().to_ascii_lowercase();
                assert!(!known.contains(&(name, path.parent().unwrap().to_path_buf(), component.id)));
            }
        }
    }

    /// Библиотеки, которые точка входа берёт из ЧУЖОГО компонента: (кто грузит, DLL, кто её ставит, грузится ли
    /// она LoadLibrary'ем во время работы). Таблицу импорта сняли с настоящих архивов
    /// (scripts/check-dll-imports.ps1, dumpbin /DEPENDENTS); cuDNN и cuFFT CUDA-провайдер onnxruntime, а cuBLAS и
    /// cuDNN 8 — CTranslate2 внутри whisper-faster.exe грузят сами, в таблице импорта их нет.
    const CROSS_COMPONENT: &[(&str, &str, &str, bool)] = &[
        ("llama", "cublas64_13.dll", "cuda-runtime", false),
        ("llama", "cublasLt64_13.dll", "cuda-runtime", false),
        ("higgs-engine", "cublas64_13.dll", "cuda-runtime", false),
        ("bsroformer-engine", "cublas64_13.dll", "cuda-runtime", false),
        ("bsroformer-engine", "cublasLt64_13.dll", "cuda-runtime", false),
        ("onnxruntime-gpu", "cublas64_13.dll", "cuda-runtime", false),
        ("onnxruntime-gpu", "cublasLt64_13.dll", "cuda-runtime", false),
        ("onnxruntime-gpu", "cufft64_12.dll", "cuda-runtime", true),
        ("onnxruntime-gpu", "cudnn64_9.dll", "cudnn", true),
    ];

    /// Юнит-тест по манифесту: каждая точка входа — скачиваемый компонент в своём каталоге, а каждая DLL, которую
    /// точка входа берёт из чужого компонента, этим компонентом и ставится (маркер) туда, где ищет загрузчик.
    #[test]
    fn cross_component_imports_are_installed_where_the_loader_looks() {
        let root = Path::new("R");
        let manifest = setup::manifest();
        for (component, rel) in ENTRY_POINTS {
            let c = manifest
                .iter()
                .find(|c| c.id == *component)
                .unwrap_or_else(|| panic!("нет компонента {component}"));
            assert_eq!(c.delivery, Delivery::Download, "{component}");
            assert!(
                c.files
                    .iter()
                    .any(|f| Path::new(rel).starts_with(Path::new(f.dest_rel).parent().unwrap())),
                "{rel} не в каталоге компонента {component}"
            );
        }
        let dirs = search_dirs(root);
        let known = manifest_basenames(root, &manifest);
        for (importer, dll, provider, _) in CROSS_COMPONENT {
            assert!(ENTRY_POINTS.iter().any(|(c, _)| c == importer), "{importer} не проверяется");
            let lowered = dll.to_ascii_lowercase();
            let hit = known.iter().find(|(n, _, id)| *n == lowered && id == provider);
            let Some((_, dir, _)) = hit else {
                panic!("{dll} ({importer}) не значится в маркерах {provider}")
            };
            assert!(
                dirs.contains(dir),
                "{dll} от {provider} ставится в {}, где загрузчик не ищет",
                dir.display()
            );
        }
    }

    /// Проверка настоящих архивов: штатная закачка закреплённых zip/whl в пустой корень SPEECH_DLL_STAGING и обход
    /// импортов. Тяжёлая (~3 ГБ), запускается скриптом scripts/check-dll-imports.ps1.
    #[test]
    #[ignore]
    fn downloaded_runtime_can_be_loaded() {
        let root = PathBuf::from(std::env::var("SPEECH_DLL_STAGING").expect("SPEECH_DLL_STAGING = пустой корень для закачки"));
        let mut ids: Vec<String> = ENTRY_POINTS.iter().map(|(c, _)| c.to_string()).collect();
        ids.extend(CROSS_COMPONENT.iter().map(|(_, _, p, _)| p.to_string()));
        ids.sort();
        ids.dedup();
        let progress = |ev: serde_json::Value| {
            if let Some(phase) = ev.get("phase").and_then(|v| v.as_str()) {
                if phase != "download" {
                    eprintln!("[{phase}] {}", ev.get("file").and_then(|v| v.as_str()).unwrap_or(""));
                }
            }
        };
        setup::download_components(&root, &ids, &|| false, &progress).unwrap_or_else(|e| panic!("закачка: {} ({})", e.detail, e.code));
        let problems = check_root(&root, &mut |entry, (name, path)| {
            if Path::new(entry).parent() != path.strip_prefix(&root).ok().and_then(Path::parent) {
                eprintln!("{entry} -> {name} ({})", path.strip_prefix(&root).unwrap_or(path).display());
            }
        })
        .expect("чтение таблиц импорта");
        let mut problems = problems;
        for (importer, dll, provider, runtime) in CROSS_COMPONENT {
            if *runtime && !search_dirs(&root).iter().any(|d| d.join(dll).is_file()) {
                problems.push(format!("{importer} грузит {dll}, а {provider} его не положил в каталог поиска"));
            }
        }
        assert!(problems.is_empty(), "не загрузится: {problems:#?}");
    }
}
