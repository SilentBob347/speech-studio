//! Режиссёр-LLM (роли `write_podcast` и `cast_audiobook` HiggsAudio-Studio): ответ — JSON по схеме, а не свободный
//! текст. Подкаст: живой диалог на тему для заданных спикеров с их манерой. Разметка книги: текст главы кусками по
//! абзацам делится на рассказчика и персонажей с постоянными именами (известные персонажи передаются в каждый кусок);
//! дословность проверяется сравнением букв куска и разметки, расхождение — повтор с указанием, затем ошибка.

use serde::Deserialize;
use serde_json::{json, Value};

use dub_llm::{ChatClient, Message, Sampling, StructuredOutput};

/// Кусок главы на один вызов: укладывается в контекст Gemma вместе с ответом.
const CHUNK_CHARS: usize = 2_500;

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Line {
    pub speaker: String,
    pub text: String,
    #[serde(default)]
    pub direction: String,
}

#[derive(Deserialize)]
struct Lines {
    lines: Vec<Line>,
}

fn lines_schema(speakers: Option<&[String]>) -> Value {
    let speaker = match speakers {
        Some(names) if !names.is_empty() => json!({ "type": "string", "enum": names }),
        _ => json!({ "type": "string" }),
    };
    json!({
        "type": "object",
        "properties": {
            "lines": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": { "speaker": speaker, "text": { "type": "string" }, "direction": { "type": "string" } },
                    "required": ["speaker", "text", "direction"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["lines"],
        "additionalProperties": false
    })
}

/// Ответ модели в JSON: по схеме, если сервер её держит, иначе первый JSON-объект в тексте.
fn ask(llm: &ChatClient, system: &str, user: &str, schema: &Value, max_tokens: u32) -> Result<Vec<Line>, String> {
    let messages = [Message::system(system), Message::user_text(user)];
    let sampling = Sampling::new(0.6, 0.9, max_tokens);
    let use_schema = llm.structured_output() != StructuredOutput::Unsupported;
    let done = llm.complete(&messages, &sampling, use_schema.then_some(schema)).map_err(|e| t!("director-failed", error = e.to_string()))?;
    let text = done.text.trim();
    let body = match (text.find('{'), text.rfind('}')) {
        (Some(a), Some(b)) if b > a => &text[a..=b],
        _ => return Err(t!("director-not-json", answer = text.chars().take(200).collect::<String>())),
    };
    let parsed: Lines = serde_json::from_str(body).map_err(|e| t!("director-not-json", answer = format!("{e}: {}", body.chars().take(200).collect::<String>())))?;
    Ok(parsed.lines.into_iter().filter(|l| !l.text.trim().is_empty()).collect())
}

/// Спикер подкаста: имя и манера речи.
#[derive(Deserialize, Clone, Debug)]
pub struct Speaker {
    pub name: String,
    #[serde(default)]
    pub manner: String,
}

/// Правила сценариста подкаста; их же читает агент в гайде `podcast`.
pub fn podcast_rules(cast: &str, lang: &str, words: &str) -> String {
    format!(
        "You are a podcast scriptwriter. Write a lively spoken dialogue for these speakers: {cast}. Each speaker keeps their \
         own manner and character. Write in the language with code \"{lang}\" (\"auto\" means the language of the topic). About \
         {words} words in total. Each line is what one speaker says aloud: no stage directions or sound effects inside the \
         text, numbers as they are said. \"direction\" is a short note on how the line sounds (tone, pace), or empty. Answer \
         with JSON only."
    )
}

/// Диалог подкаста на тему: строки спикеров по очереди, у каждой — короткая подсказка интонации.
pub fn write_podcast(llm: &ChatClient, topic: &str, speakers: &[Speaker], lang: &str, minutes: u32) -> Result<Vec<Line>, String> {
    let names: Vec<String> = speakers.iter().map(|s| s.name.clone()).collect();
    let cast = speakers.iter().map(|s| if s.manner.trim().is_empty() { s.name.clone() } else { format!("{} — {}", s.name, s.manner.trim()) }).collect::<Vec<_>>().join("; ");
    let words = minutes.clamp(1, 20) * 140;
    let system = podcast_rules(&cast, lang, &words.to_string());
    let lines = ask(llm, &system, topic, &lines_schema(Some(&names)), (words * 4).clamp(1_024, 8_192))?;
    if lines.is_empty() {
        return Err(t!("director-empty"));
    }
    Ok(lines)
}

/// Буквы и цифры текста в нижнем регистре — для сверки дословности.
fn letters(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).map(|c| if c == 'ё' { 'е' } else { c }).collect()
}

/// Куски главы по границам абзацев (абзац длиннее куска идёт целиком отдельно).
pub fn chunks(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for para in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        if !cur.is_empty() && cur.chars().count() + para.chars().count() > CHUNK_CHARS {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push_str("\n\n");
        }
        cur.push_str(para);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Правила разметчика книги; их же читает агент в гайде `audiobook`.
pub fn casting_rules(narrator: &str, known_list: &str, lang: &str) -> String {
    format!(
        "You are the casting director of an audiobook. Split the text into the narrator's parts and the characters' lines. \
         The narrator is called \"{narrator}\". Characters already met: {known_list}; use exactly these names for them, and \
         give a new character a short name in the language of the text (code \"{lang}\"). Copy the text VERBATIM, in order, \
         every word and punctuation mark, only splitting it between speakers; the words that introduce a line (\"he said\") \
         belong to the narrator. \"direction\" is a short note on how a line sounds, or empty. Answer with JSON only."
    )
}

/// Разметка куска книги: рассказчик и персонажи; `known` — уже встреченные персонажи, их имена не меняются.
fn cast_chunk(llm: &ChatClient, chunk: &str, narrator: &str, known: &[String], lang: &str) -> Result<Vec<Line>, String> {
    let known_list = if known.is_empty() { "none yet".to_string() } else { known.join(", ") };
    let system = casting_rules(narrator, &known_list, lang);
    let schema = lines_schema(None);
    let max_tokens = ((chunk.chars().count() * 2) as u32).clamp(1_024, 8_192);
    let want = letters(chunk);
    let mut lines = ask(llm, &system, chunk, &schema, max_tokens)?;
    if letters(&lines.iter().map(|l| l.text.as_str()).collect::<String>()) != want {
        let again = format!("{chunk}\n\n(The previous split changed or dropped words. Keep every word of the text exactly.)");
        lines = ask(llm, &system, &again, &schema, max_tokens)?;
        if letters(&lines.iter().map(|l| l.text.as_str()).collect::<String>()) != want {
            return Err(t!("director-not-verbatim", start = chunk.chars().take(60).collect::<String>()));
        }
    }
    Ok(lines)
}

/// Разметка главы целиком: куски по абзацам, персонажи копятся по ходу; `progress` — номер куска и их число.
pub fn cast_audiobook(llm: &ChatClient, text: &str, narrator: &str, known: &[String], lang: &str, progress: &dyn Fn(usize, usize)) -> Result<Vec<Line>, String> {
    let parts = chunks(text);
    let mut met: Vec<String> = known.to_vec();
    let mut out = Vec::new();
    for (i, chunk) in parts.iter().enumerate() {
        progress(i + 1, parts.len());
        let lines = cast_chunk(llm, chunk, narrator, &met, lang)?;
        for l in &lines {
            if l.speaker != narrator && !met.contains(&l.speaker) {
                met.push(l.speaker.clone());
            }
        }
        out.extend(lines);
    }
    Ok(out)
}

/// A block's text with expression tags the director put in, and the text it had.
pub struct Enriched {
    pub id: String,
    pub before: String,
    pub after: String,
}

#[derive(Deserialize)]
struct Tagged {
    lines: Vec<TaggedLine>,
}

#[derive(Deserialize)]
struct TaggedLine {
    id: String,
    text: String,
}

static TAG_LIKE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"<\|[^|<>]*\|>|</?[A-Za-z_][\w-]*>|\[[^\[\]\n]{1,40}\]").expect("tag"));

/// The text with every tag the engine does not read (and that was not there before) taken out.
fn keep_known(after: &str, before: &str, known: &[String]) -> String {
    TAG_LIKE.replace_all(after, |c: &regex::Captures| if known.iter().any(|k| k == &c[0]) || before.contains(&c[0]) { c[0].to_string() } else { String::new() }).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether two texts say the same words, tags aside.
fn same_words(a: &str, b: &str) -> bool {
    letters(&TAG_LIKE.replace_all(a, " ")) == letters(&TAG_LIKE.replace_all(b, " "))
}

/// Expression tags put into blocks by their meaning, only those `engine` reads; a block whose words the model
/// changed keeps its text. `blocks` are (id, speaker, text); the answer lists the blocks that got tags and how many
/// answers were turned down for changed words.
pub fn enrich(llm: &ChatClient, engine: &crate::engines::Engine, blocks: &[(String, String, String)], lang: &str, progress: &dyn Fn(usize, usize)) -> Result<(Vec<Enriched>, usize), String> {
    if engine.tags.is_empty() {
        return Err(t!("director-no-tags", engine = engine.name.to_string()));
    }
    let known = crate::engines::tag_texts(engine);
    let system = format!(
        "You are a voice director preparing lines for the {name} speech engine{lang_note}. Add expression tags where the meaning of a line asks for them: an emotion, a manner of speaking, a sound like a laugh or a sigh. Use only these tags, written exactly as listed:\n{rules}\n\nDo not change, add, remove or reorder any word or punctuation mark: only insert tags. Keep tags already in a line. Use few tags: most lines need none or one. Answer JSON {{\"lines\": [{{\"id\": ..., \"text\": ...}}]}} with every line given, in the same order.",
        name = engine.name,
        lang_note = if lang.is_empty() || lang == "auto" { String::new() } else { format!(" (language: {lang})") },
        rules = crate::engines::tag_rules(engine),
    );
    let schema = json!({ "type": "object", "properties": { "lines": { "type": "array", "items": { "type": "object", "properties": { "id": { "type": "string" }, "text": { "type": "string" } }, "required": ["id", "text"] } } }, "required": ["lines"] });
    let mut pieces: Vec<Vec<&(String, String, String)>> = vec![Vec::new()];
    let mut size = 0;
    for b in blocks {
        if size > 0 && size + b.2.chars().count() > CHUNK_CHARS {
            pieces.push(Vec::new());
            size = 0;
        }
        size += b.2.chars().count();
        pieces.last_mut().expect("a piece").push(b);
    }
    let (mut out, mut refused) = (Vec::new(), 0);
    for (k, piece) in pieces.iter().enumerate().filter(|(_, p)| !p.is_empty()) {
        progress(k, pieces.len());
        let user = serde_json::to_string(&piece.iter().map(|(id, speaker, text)| json!({ "id": id, "speaker": speaker, "text": text })).collect::<Vec<_>>()).map_err(|e| e.to_string())?;
        let chars: usize = piece.iter().map(|b| b.2.chars().count()).sum();
        let messages = [Message::system(&system), Message::user_text(&user)];
        let sampling = Sampling::new(0.4, 0.9, ((chars * 2) as u32).clamp(1_024, 8_192));
        let use_schema = llm.structured_output() != StructuredOutput::Unsupported;
        let done = llm.complete(&messages, &sampling, use_schema.then_some(&schema)).map_err(|e| t!("director-failed", error = e.to_string()))?;
        let text = done.text.trim();
        let body = match (text.find('{'), text.rfind('}')) {
            (Some(a), Some(b)) if b > a => &text[a..=b],
            _ => return Err(t!("director-not-json", answer = text.chars().take(200).collect::<String>())),
        };
        let tagged: Tagged = serde_json::from_str(body).map_err(|e| t!("director-not-json", answer = format!("{e}: {}", body.chars().take(200).collect::<String>())))?;
        for line in tagged.lines {
            let Some((id, _, before)) = piece.iter().find(|b| b.0 == line.id).map(|b| (&b.0, &b.1, &b.2)) else { continue };
            let after = keep_known(&line.text, before, &known);
            if !same_words(before, &after) {
                refused += 1;
            } else if after.trim() != before.trim() {
                out.push(Enriched { id: id.clone(), before: before.clone(), after });
            }
        }
    }
    Ok((out, refused))
}

#[cfg(test)]
mod tests {

    #[test]
    fn enrich_keeps_only_the_engine_s_tags_and_never_other_words() {
        let known = vec!["<|emotion:joy|>".to_string(), "<|sfx:laughter|>".to_string()];
        assert_eq!(keep_known("<|emotion:joy|>Привет <|style:sad|>мир", "Привет мир", &known), "<|emotion:joy|>Привет мир");
        assert!(same_words("Привет, мир!", "<|emotion:joy|>Привет, мир!"));
        assert!(!same_words("Привет, мир!", "Привет, дорогой мир!"));
        assert_eq!(keep_known("[pause] Да", "[pause] Да", &known), "[pause] Да");
    }
    use super::*;

    #[test]
    fn chapters_split_at_paragraphs_and_verbatim_ignores_case_punctuation_and_yo() {
        let para = "Слово ".repeat(300);
        let text = format!("{para}\n\n{para}\n\nКороткий абзац.");
        let parts = chunks(&text);
        assert_eq!(parts.len(), 2, "two long paragraphs do not share a chunk, the short one joins the second");
        assert_eq!(letters("Ёлка, сказал он."), letters("елка — сказал он"));
        assert_ne!(letters("Ёлка, сказал он."), letters("Ёлка, он."));
    }

    #[test]
    fn the_podcast_schema_limits_speakers_to_the_cast() {
        let s = lines_schema(Some(&["Анна".to_string(), "Борис".to_string()]));
        assert_eq!(s["properties"]["lines"]["items"]["properties"]["speaker"]["enum"], json!(["Анна", "Борис"]));
    }
}
