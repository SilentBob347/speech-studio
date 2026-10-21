use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use super::jobs::{self, SpeechPaths};
use super::project::{self as sp, SpeechProject};
use crate::engines::stress as stress_engine;
use crate::text::{prep, pron_dict, stress};

#[cfg(test)]
mod compatibility_tests;

pub(super) struct Opts<'a> {
    pub lang: &'a str,
    pub voice: Option<&'a str>,
    pub engine: &'a str,
    pub entries: &'a [pron_dict::Entry],
    pub normalize: bool,
    pub normalize_off: &'a [String],
    pub stress: bool,
    pub models_root: &'a Path,
}

impl<'a> Opts<'a> {
    fn rules(&self) -> prep::Opts<'a> {
        prep::Opts {
            lang: self.lang,
            voice: self.voice,
            engine: self.engine,
            entries: self.entries,
            normalize: self.normalize,
            normalize_off: self.normalize_off,
        }
    }
}

pub(super) fn opts<'a>(
    p: &'a SpeechPaths,
    project: &'a SpeechProject,
    lang: &'a str,
    engine: &'a str,
    voice: Option<&'a str>,
    entries: &'a [pron_dict::Entry],
    stress: bool,
) -> Opts<'a> {
    Opts {
        lang,
        voice,
        engine,
        entries,
        normalize: project.settings.normalize,
        normalize_off: &project.settings.normalize_off,
        stress,
        models_root: &p.models_root,
    }
}

pub(super) fn prepare(text: &str, o: &Opts) -> Result<prep::Prepared, String> {
    prepare_with(text, o, |text| stress_engine::shared(o.models_root)?.annotate(text))
}

fn prepare_with(text: &str, o: &Opts, annotate: impl FnOnce(&str) -> Result<stress::Stressed, String>) -> Result<prep::Prepared, String> {
    let mut draft = prep::prepare(text, &o.rules());
    if draft.lang == "ru" && o.stress {
        let stressed = annotate(&draft.prepared.text)?;
        draft.apply_stress(stressed);
    }
    let manual_stress = draft.manual_stress;
    let mut prepared = draft.finish(super::text::adapter(o.engine));
    prepared.ignored.stress = manual_stress && !crate::engines::get(o.engine).is_some_and(|e| e.reads.stress);
    Ok(prepared)
}

pub(super) fn fingerprint(text: &str, o: &Opts, compiled: &[(pron_dict::Entry, fancy_regex::Regex)]) -> String {
    let model = (prep::block_lang(text, o.lang) == "ru" && o.stress && crate::engines::get(o.engine).is_some_and(|e| e.reads.stress))
        .then(|| stress_engine::omograph_model(o.models_root));
    prep::fingerprint(text, &o.rules(), compiled, model.as_deref())
}

pub(crate) struct PrepareText {
    pub text: String,
    pub pid: Option<String>,
    pub block: Option<String>,
    pub lang: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct TextPreview {
    pub prepared: prep::Prepared,
    pub stress_applied: bool,
    pub stress_missing: bool,
}

pub(crate) fn prepare_text(p: &SpeechPaths, request: PrepareText) -> Result<TextPreview, String> {
    let mut project = match &request.pid {
        Some(pid) => sp::load(&p.workspace, pid)?,
        None => SpeechProject::default(),
    };
    if let Some(lang) = &request.lang {
        project.settings.lang = lang.clone();
    }
    let block = request.block.as_ref().and_then(|id| project.blocks().find(|b| &b.id == id));
    let voice = block.and_then(|b| project.voice_of(b));
    let entries = jobs::dictionary(p, &project);
    let russian = stress::is_russian(&request.text, &project.settings.lang);
    let stress = project.settings.auto_stress && russian && stress_engine::installed(&p.repo_root);
    let lang = block.map_or(project.settings.lang.as_str(), |b| project.lang_of(b));
    let prepared = prepare(
        &request.text,
        &opts(p, &project, lang, &project.settings.engine, voice.as_deref(), &entries, stress),
    )?;
    Ok(TextPreview {
        prepared,
        stress_applied: stress,
        stress_missing: project.settings.auto_stress && russian && !stress,
    })
}

pub(crate) fn dictionary_counts(p: &SpeechPaths, pid: &str) -> Result<BTreeMap<String, usize>, String> {
    let project = sp::load(&p.workspace, pid)?;
    let entries = jobs::dictionary(p, &project);
    let text = project.blocks().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n");
    Ok(pron_dict::counts(&text, &entries).into_iter().collect())
}

#[derive(Serialize)]
pub(crate) struct UnknownWord {
    pub word: String,
    pub count: usize,
    pub why: &'static str,
}

#[derive(Serialize)]
pub(crate) struct UnknownWords {
    pub words: Vec<UnknownWord>,
    pub stress_checked: bool,
}

pub(crate) fn unknown_words(p: &SpeechPaths, pid: &str) -> Result<UnknownWords, String> {
    let project = sp::load(&p.workspace, pid)?;
    let entries = jobs::dictionary(p, &project);
    let stresser = if stress_engine::installed(&p.repo_root) {
        Some(stress_engine::shared(&p.models_root)?)
    } else {
        None
    };
    let mut words: BTreeMap<String, (usize, &'static str)> = BTreeMap::new();
    for b in project.blocks().filter(|b| !b.hidden) {
        let voice = project.voice_of(b);
        let prepared = prepare(
            &b.text,
            &opts(
                p,
                &project,
                project.lang_of(b),
                project.engine_of(b),
                voice.as_deref(),
                &entries,
                false,
            ),
        )?;
        for w in &prepared.unknown {
            words.entry(w.clone()).or_insert((0, "unread")).0 += 1;
        }
        if let Some(s) = &stresser {
            for w in stress::cyrillic_words(&prepared.text) {
                if !s.knows(&w) {
                    words.entry(w).or_insert((0, "no_stress")).0 += 1;
                }
            }
        }
    }
    let mut words: Vec<_> = words
        .into_iter()
        .map(|(word, (count, why))| UnknownWord { word, count, why })
        .collect();
    words.sort_by(|a, b| b.count.cmp(&a.count));
    Ok(UnknownWords {
        words,
        stress_checked: stresser.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use crate::text::pron_dict::Entry;

    use super::*;

    fn paths(root: &Path) -> SpeechPaths {
        SpeechPaths {
            repo_root: root.to_path_buf(),
            workspace: root.join("workspace"),
            models_root: root.join("models"),
            voices_dir: root.join("voices"),
            higgs_dll: root.join("unused.dll"),
            threads: 1,
        }
    }

    fn save_fixture(p: &SpeechPaths, project: &SpeechProject) -> String {
        let pid = sp::new_id();
        let path = sp::path(&p.workspace, &pid);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec(project).unwrap()).unwrap();
        pid
    }

    fn rule(id: &str, find: &str, say: &str) -> Entry {
        Entry {
            id: id.into(),
            find: find.into(),
            say: say.into(),
            ..Default::default()
        }
    }

    #[test]
    fn standalone_preview_reports_missing_stress_and_does_not_create_a_project() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(root.path());
        let preview = prepare_text(
            &p,
            PrepareText {
                text: "Привет".into(),
                pid: None,
                block: None,
                lang: None,
            },
        )
        .unwrap();
        assert_eq!(preview.prepared.text, "Привет");
        assert!(!preview.stress_applied);
        assert!(preview.stress_missing);
        assert!(!p.workspace.exists());
        assert!(!p.models_root.exists());
    }

    #[test]
    fn preview_preserves_dictionary_priority_and_block_scope_without_saving_overrides() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(root.path());
        let mut project = sp::from_text("Scope", "NASA", "Narrator");
        project.settings.lang = "en".into();
        project.settings.auto_stress = false;
        project.settings.normalize = false;
        let block = &mut project.chapters[0].blocks[0];
        block.voice = Some("Anna".into());
        block.lang = "ru".into();
        let block_id = block.id.clone();
        project.dictionary.push(Entry {
            lang: "ru".into(),
            voice: "Anna".into(),
            engine: "higgs".into(),
            ..rule("project", "NASA", "НАСА")
        });
        pron_dict::save_global(&p.repo_root, &[rule("global", "NASA", "global")]).unwrap();
        let pid = save_fixture(&p, &project);
        let before = fs::read(sp::path(&p.workspace, &pid)).unwrap();
        let preview = prepare_text(
            &p,
            PrepareText {
                text: "NASA".into(),
                pid: Some(pid.clone()),
                block: Some(block_id),
                lang: Some("fr".into()),
            },
        )
        .unwrap();
        assert_eq!(preview.prepared.text, "НАСА");
        assert_eq!(preview.prepared.steps[0].rule, "project");
        assert!(!preview.stress_applied && !preview.stress_missing);
        let preview = prepare_text(
            &p,
            PrepareText {
                text: "NASA".into(),
                pid: Some(pid.clone()),
                block: Some("absent".into()),
                lang: None,
            },
        )
        .unwrap();
        assert_eq!(preview.prepared.text, "global");
        assert_eq!(fs::read(sp::path(&p.workspace, &pid)).unwrap(), before);
    }

    #[test]
    fn dictionary_counts_include_hidden_blocks_and_both_dictionary_levels() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(root.path());
        let mut project = sp::from_text("Counts", "cat cat\n\ncat dog", "Narrator");
        assert_eq!(project.blocks().count(), 2);
        project.chapters[0].blocks[1].hidden = true;
        project.dictionary.push(rule("project", "cat", "kitten"));
        pron_dict::save_global(&p.repo_root, &[rule("global", "cat", "pet"), rule("unused", "mouse", "mice")]).unwrap();
        let pid = save_fixture(&p, &project);
        assert_eq!(
            dictionary_counts(&p, &pid).unwrap(),
            BTreeMap::from([("global".into(), 3), ("project".into(), 3), ("unused".into(), 0),])
        );
    }

    #[test]
    fn unknown_words_use_each_block_scope_skip_hidden_blocks_and_keep_stable_ties() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(root.path());
        let mut project = sp::from_text("Unknown", "iPhone iPhone Zebra Apple Scoped\n\nHidden\n\nEnglish", "Narrator");
        project.settings.lang = "ru".into();
        project.chapters[0].blocks[0].engine = Some("qwen3-tts".into());
        project.chapters[0].blocks[0].voice = Some("Anna".into());
        project.chapters[0].blocks[1].hidden = true;
        project.chapters[0].blocks[2].lang = "en".into();
        project.dictionary.push(Entry {
            lang: "ru".into(),
            voice: "Anna".into(),
            engine: "qwen3-tts".into(),
            ..rule("scoped", "Scoped", "слово")
        });
        let pid = save_fixture(&p, &project);
        let before = fs::read(sp::path(&p.workspace, &pid)).unwrap();
        let report = unknown_words(&p, &pid).unwrap();
        assert!(!report.stress_checked);
        let words: Vec<_> = report.words.iter().map(|w| (w.word.as_str(), w.count, w.why)).collect();
        assert_eq!(words, [("iPhone", 2, "unread"), ("Apple", 1, "unread"), ("Zebra", 1, "unread")]);
        assert_eq!(fs::read(sp::path(&p.workspace, &pid)).unwrap(), before);
    }
}
