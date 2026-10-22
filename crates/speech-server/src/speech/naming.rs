//! Pure rules for project asset names and export templates.

pub(crate) fn safe_file(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && !name.contains("..")
}

/// What a file name template of the export may name: `{project}`, `{chapter}`, `{n}` (the file's number), `{date}`.
pub const EXPORT_NAME_KEYS: [&str; 4] = ["project", "chapter", "n", "date"];
/// What a file name template of the per-block export may name besides those: `{speaker}`, `{text}` (its start or
/// note), `{block}` (its id).
pub const CLIP_NAME_KEYS: [&str; 7] = ["project", "chapter", "n", "date", "speaker", "text", "block"];

/// The `{key}`s of a file name template; a key outside `allowed` or a brace left open is refused.
pub fn name_keys<'a>(template: &'a str, allowed: &[&str]) -> Result<Vec<&'a str>, String> {
    let mut keys = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}').ok_or_else(|| format!("{template}: a {{ is not closed"))? + open;
        let key = &rest[open + 1..close];
        if !allowed.contains(&key) {
            return Err(format!(
                "{template}: no {{{key}}}; a name may use {}",
                allowed.iter().map(|k| format!("{{{k}}}")).collect::<Vec<_>>().join(", ")
            ));
        }
        keys.push(key);
        rest = &rest[close + 1..];
    }
    Ok(keys)
}
