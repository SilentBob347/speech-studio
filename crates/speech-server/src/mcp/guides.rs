//! Guides an agent reads before writing for the studio: the rules its own director model is prompted with and the tags
//! each engine reads, as resources (`studio://guide/<topic>`), the `studio_guide` tool and the task prompts.

use serde_json::{json, Value};

use super::{composite, object, Tool, SKILL};

pub(super) const GUIDE_URI: &str = "studio://guide/";

pub(super) const TOPICS: &[(&str, &str)] = &[
    ("tags", "the expression tags each engine reads, where they go in a sentence, and whether it reads a block's direction"),
    ("podcast", "a podcast script written the way the studio's director model writes one, and how it becomes a project"),
    ("audiobook", "a book split into the narrator and characters the way the studio's director model splits it, from the file to the export"),
];

/// The tags part of the skill, as it stands there.
fn skill_tags() -> String {
    let start = SKILL.find("- **Expression tags differ per engine**").unwrap_or(0);
    let end = SKILL[start..].find("- **Engines**").map_or(SKILL.len(), |at| start + at);
    SKILL[start..end].trim_end().to_string()
}

fn engine_tags() -> String {
    let mut out = String::new();
    for engine in crate::engines::all() {
        out.push_str(&format!("\n\n{} ({})\n{}", engine.name, engine.id, crate::engines::tag_rules(&engine)));
        out.push_str(match (engine.direction, engine.direction_presets_only) {
            (true, true) => "\nA block's direction reaches the engine only with its own voices.",
            (true, false) => "\nA block's direction reaches the engine.",
            (false, _) => "\nA block's direction is not read.",
        });
    }
    out
}

/// A guide by its topic.
pub(super) fn guide(topic: &str) -> Option<String> {
    Some(match topic {
        "tags" => format!("{}\n\nEach engine, as the studio has it (engine_tags gives one as data):{}", skill_tags(), engine_tags()),
        "podcast" => format!(
            "The rules the studio's director model writes a podcast by (podcast_write runs it):\n\n{}\n\nAs a project: voices_list for the voices; project_create with the script as lines 'Name: text' and split lines makes a block per line with its speaker; project_get, then project_update puts each line's direction into its block and each speaker's voice into the project's cast; project_synth, studio_wait, project_export. Expression tags: the guide tags.",
            crate::director::podcast_rules("<the speakers, each as 'name - manner'>", "<language code>", "<about 140 per minute>")
        ),
        "audiobook" => format!(
            "The rules the studio's director model splits a book by (project_cast_characters runs it on one chapter):\n\n{}\n\nFrom the file: book_preview reads the book and lists its chapters (skip names one to leave out: empty, toc, front matter or cover); book_chapter gives one whole; project_create_from_book makes the project of the chapters chosen with the narrator's voice. Then per chapter project_cast_characters, or split it yourself by the rules above and save the blocks (speaker, text, direction) with project_update. Give every character a voice in the project's cast, project_synth, studio_wait, then project_export with format m4b (chapters) and part_minutes for a long book.",
            crate::director::casting_rules("<the narrator's name>", "<the characters met so far, or none yet>", "<language code>")
        ),
        _ => return None,
    })
}

pub(super) fn resources() -> Vec<Value> {
    TOPICS.iter().map(|(topic, about)| json!({ "uri": format!("{GUIDE_URI}{topic}"), "name": format!("guide: {topic}"), "title": format!("Guide: {topic}"), "description": about, "mimeType": "text/plain" })).collect()
}

pub(super) fn prompts() -> Vec<Value> {
    vec![
        json!({ "name": "write_podcast", "title": "Write a podcast", "description": "Write a podcast script for the studio and voice it.", "arguments": [{ "name": "topic", "description": "what the podcast is about", "required": true }, { "name": "speakers", "description": "who speaks and how, e.g. 'Anna - calm host; Oleg - excited guest'", "required": false }] }),
        json!({ "name": "make_audiobook", "title": "Make an audiobook", "description": "Turn a book file into a voiced audiobook with a narrator and characters.", "arguments": [{ "name": "path", "description": "the book file on this computer", "required": true }, { "name": "voice", "description": "the narrator's voice from voices_list", "required": false }] }),
    ]
}

/// A task prompt with its arguments.
pub(super) fn prompt(name: &str, arguments: &Value) -> Option<String> {
    let given = |key: &str| arguments.get(key).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty());
    match name {
        "write_podcast" => {
            let speakers = given("speakers").map(|speakers| format!(" The speakers: {speakers}.")).unwrap_or_default();
            Some(format!("{}\n\n{}\n\nWrite a podcast about: {}.{speakers}", guide("podcast")?, guide("tags")?, given("topic").unwrap_or_default()))
        }
        "make_audiobook" => {
            let voice = given("voice").map(|voice| format!(" The narrator's voice: {voice}.")).unwrap_or_default();
            Some(format!("{}\n\n{}\n\nMake an audiobook of the book {}.{voice}", guide("audiobook")?, guide("tags")?, given("path").unwrap_or_default()))
        }
        _ => None,
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "studio_guide",
        description: "A guide to read before writing for the studio: tags (the expression tags each engine reads and where they go), podcast (the rules the studio's director writes a podcast by, and how it becomes a project), audiobook (how it splits a book into the narrator and characters, from the file to the export). The same texts are the resources studio://guide/<topic>.",
        schema: || object(json!({ "topic": { "type": "string", "enum": TOPICS.iter().map(|(topic, _)| *topic).collect::<Vec<_>>() } }), &["topic"]),
        call: |_| composite("guide"),
    }]
}
