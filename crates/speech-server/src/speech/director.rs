//! Режиссёр-LLM (роли `write_podcast` и `cast_audiobook` HiggsAudio-Studio): ответ — JSON по схеме, а не свободный
//! текст. Подкаст: живой диалог на тему для заданных спикеров с их манерой. Разметка книги: текст главы кусками по
//! абзацам делится на рассказчика и персонажей с постоянными именами (известные персонажи передаются в каждый кусок);
//! дословность проверяется сравнением букв куска и разметки, расхождение — повтор с указанием, затем ошибка.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
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
fn ask_json<T: DeserializeOwned>(llm: &ChatClient, system: &str, user: &str, schema: &Value, max_tokens: u32) -> Result<T, String> {
    let messages = [Message::system(system), Message::user_text(user)];
    let sampling = Sampling::new(0.6, 0.9, max_tokens);
    let use_schema = llm.structured_output() != StructuredOutput::Unsupported;
    let done = llm.complete(&messages, &sampling, use_schema.then_some(schema)).map_err(|e| t!("director-failed", error = e.to_string()))?;
    let text = done.text.trim();
    let body = match (text.find('{'), text.rfind('}')) {
        (Some(a), Some(b)) if b > a => &text[a..=b],
        _ => return Err(t!("director-not-json", answer = text.chars().take(200).collect::<String>())),
    };
    serde_json::from_str(body).map_err(|e| t!("director-not-json", answer = format!("{e}: {}", body.chars().take(200).collect::<String>())))
}

fn ask(llm: &ChatClient, system: &str, user: &str, schema: &Value, max_tokens: u32) -> Result<Vec<Line>, String> {
    let parsed: Lines = ask_json(llm, system, user, schema, max_tokens)?;
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

/// Буквы и цифры текста в нижнем регистре — для сверки дословности; полноширинные знаки (１, Ａ) приведены к обычным
/// (NFKC): модель, переписавшая их обычными, слов не меняла.
fn letters(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    text.nfkc().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).map(|c| if c == 'ё' { 'е' } else { c }).collect()
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

/// A character of a book as the registry pass knows it: the name the lines go under, and gender and age for picking
/// a voice (`female`/`male`, `child`/`teen`/`adult`/`senior`, or empty when the text does not tell).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Person {
    pub name: String,
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub age: String,
}

#[derive(Deserialize)]
struct People {
    characters: Vec<Person>,
}

fn people_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "characters": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "gender": { "type": "string", "enum": crate::voices::GENDERS },
                        "age": { "type": "string", "enum": crate::voices::AGES }
                    },
                    "required": ["name", "gender", "age"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["characters"],
        "additionalProperties": false
    })
}

/// The first pass's rules: who speaks in the text, under one name each, with gender and age.
fn registry_rules(narrator: &str, known: &[Person], lang: &str) -> String {
    let listed = if known.is_empty() { "none yet".to_string() } else { serde_json::to_string(known).unwrap_or_default() };
    format!(
        "You keep the list of characters of an audiobook. List every character who speaks a line in this text, not the \
         narrator \"{narrator}\". Characters listed so far: {listed}. Keep their names exactly; a listed character met under \
         another name (a nickname, a surname, a title) keeps the listed name. A new character gets a short name in the \
         language of the text (code \"{lang}\"). gender is female or male, age is child, teen, adult or senior; leave either \
         empty when the text does not tell. Answer with JSON only: the characters who speak in this text."
    )
}

/// Правила разметчика книги; их же читает агент в гайде `audiobook`.
pub fn casting_rules(narrator: &str, known_list: &str, lang: &str) -> String {
    format!(
        "You are the casting director of an audiobook. Split the text into the narrator's parts and the characters' lines. \
         The narrator is called \"{narrator}\". Characters already met: {known_list}; use exactly these names for them, and \
         give a new character a short name in the language of the text (code \"{lang}\"). Copy the text VERBATIM, in order, \
         every word and punctuation mark, only splitting it between speakers; the words that introduce a line (\"he said\") \
         belong to the narrator. In a paragraph opening with a dash the author's words stand between dashes, and the words \
         after the next dash are the character's line again. \"direction\" is a short note on how a line sounds, or empty. Answer with JSON only."
    )
}

/// The second pass's rules: the speakers are the narrator and the registry's characters, and no one else.
fn closed_casting_rules(narrator: &str, names: &[String]) -> String {
    format!(
        "You are the casting director of an audiobook. Split the text into the narrator's parts and the characters' lines. \
         The speakers are the narrator \"{narrator}\" and these characters, and no one else: {}. Give each line to the \
         character who says it, under exactly that name. Copy the text VERBATIM, in order, every word and punctuation mark, \
         only splitting it between speakers; the words that introduce a line (\"he said\") belong to the narrator. In a \
         paragraph opening with a dash the author's words stand between dashes, and the words after the next dash are the \
         character's line again. \"direction\" is a short note on how a line sounds, or empty. Answer with JSON only.",
        names.join(", ")
    )
}

/// Разметка куска книги: рассказчик и персонажи; `known` — уже встреченные персонажи, их имена не меняются; с
/// `closed` других персонажей нет.
fn cast_chunk(llm: &ChatClient, chunk: &str, narrator: &str, known: &[String], lang: &str, closed: bool) -> Result<Vec<Line>, String> {
    let known_list = if known.is_empty() { "none yet".to_string() } else { known.join(", ") };
    let speakers: Vec<String> = std::iter::once(narrator.to_string()).chain(known.iter().cloned()).collect();
    let (system, schema) = if closed { (closed_casting_rules(narrator, known), lines_schema(Some(&speakers))) } else { (casting_rules(narrator, &known_list, lang), lines_schema(None)) };
    let max_tokens = ((chunk.chars().count() * 2) as u32).clamp(1_024, 8_192);
    let want = letters(chunk);
    let mut lines = ask(llm, &system, chunk, &schema, max_tokens)?;
    if letters(&lines.iter().map(|l| l.text.as_str()).collect::<String>()) != want {
        let again = format!("{chunk}\n\n(The previous split changed or dropped words. Keep every word of the text exactly.)");
        lines = ask(llm, &system, &again, &schema, max_tokens)?;
        if letters(&lines.iter().map(|l| l.text.as_str()).collect::<String>()) != want {
            tracing::warn!(chunk = %chunk, answer = ?lines, "director: the split does not keep the text word for word");
            return Err(t!("director-not-verbatim", start = chunk.chars().take(60).collect::<String>()));
        }
    }
    Ok(lines)
}

/// One more sighting of a character: a new one joins the registry, a known one keeps its name and takes the gender
/// and age it did not have yet.
pub fn register(people: &mut Vec<Person>, p: Person, narrator: &str) {
    let name = p.name.trim();
    if name.is_empty() || name == narrator {
        return;
    }
    let ok = |v: &str, all: &[&str]| if all.contains(&v) { v.to_string() } else { String::new() };
    let (gender, age) = (ok(&p.gender, &crate::voices::GENDERS), ok(&p.age, &crate::voices::AGES));
    match people.iter_mut().find(|x| x.name == name) {
        Some(x) => {
            if x.gender.is_empty() {
                x.gender = gender;
            }
            if x.age.is_empty() {
                x.age = age;
            }
        }
        None => people.push(Person { name: name.to_string(), gender, age }),
    }
}

/// Разметка главы в два прохода: сначала реестр персонажей (кто говорит, под одним именем, с полом и возрастом) по
/// кускам, потом реплики кусков раздаются только им. `known` — персонажи проекта; ответ — реплики и реестр.
/// `progress` — номер шага и их число (по куску на проход).
pub fn cast_audiobook(llm: &ChatClient, text: &str, narrator: &str, known: &[Person], lang: &str, progress: &dyn Fn(usize, usize)) -> Result<(Vec<Line>, Vec<Person>), String> {
    let parts = chunks(text);
    let steps = parts.len() * 2;
    let mut people: Vec<Person> = Vec::new();
    for p in known {
        register(&mut people, p.clone(), narrator);
    }
    for (i, chunk) in parts.iter().enumerate() {
        progress(i + 1, steps);
        let max_tokens = ((chunk.chars().count() / 2) as u32).clamp(512, 2_048);
        let found: People = ask_json(llm, &registry_rules(narrator, &people, lang), chunk, &people_schema(), max_tokens)?;
        for p in found.characters {
            register(&mut people, p, narrator);
        }
    }
    let names: Vec<String> = people.iter().map(|p| p.name.clone()).collect();
    let mut out = Vec::new();
    for (i, chunk) in parts.iter().enumerate() {
        progress(parts.len() + i + 1, steps);
        let lines = cast_chunk(llm, chunk, narrator, &names, lang, !names.is_empty())?;
        for l in &lines {
            register(&mut people, Person { name: l.speaker.clone(), ..Default::default() }, narrator);
        }
        out.extend(lines);
    }
    Ok((out, people))
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
        lang_note = lang_note(lang),
        rules = crate::engines::tag_rules(engine),
    );
    rewrite(llm, &system, blocks, 0.4, progress, &|before, answer| {
        let after = keep_known(answer, before, &known);
        same_words(before, &after).then_some(after)
    })
}

/// Punctuation put where the lines are read aloud: commas, dashes and full stops where speech pauses, a question
/// mark where it rises, a run-on sentence cut in two. Words and tags stay as they are; a block whose words or tags the
/// model changed keeps its text.
pub fn punctuate(llm: &ChatClient, blocks: &[(String, String, String)], lang: &str, progress: &dyn Fn(usize, usize)) -> Result<(Vec<Enriched>, usize), String> {
    let system = format!(
        "You are an editor preparing lines to be read aloud by a speech engine{lang_note}. Fix the punctuation so each line reads naturally: commas, dashes and full stops where speech pauses, a question mark where it asks, a run-on sentence cut into two with a full stop. Do not change, add, remove or reorder any word, keep its letters and case, and keep every tag such as <|...|>, [...] or <...> exactly where it stands. Change nothing in a line that already reads well. Answer JSON {{\"lines\": [{{\"id\": ..., \"text\": ...}}]}} with every line given, in the same order.",
        lang_note = lang_note(lang),
    );
    let tags = |t: &str| TAG_LIKE.find_iter(t).map(|m| m.as_str().to_string()).collect::<Vec<_>>();
    // «ё» is said otherwise than «е» (всё, все): an answer that adds or drops one is refused
    let yo = |t: &str| TAG_LIKE.replace_all(t, " ").chars().filter(|c| matches!(c, 'ё' | 'Ё')).count();
    rewrite(llm, &system, blocks, 0.2, progress, &|before, after| (same_words(before, after) && tags(before) == tags(after) && yo(before) == yo(after)).then(|| after.to_string()))
}

fn lang_note(lang: &str) -> String {
    if lang.is_empty() || lang == "auto" { String::new() } else { format!(" (language: {lang})") }
}

/// Lines sent to the model in pieces under `CHUNK_CHARS`; each answer `accept` passes replaces its line, the others
/// count as refused. The project is not touched: the answer is what each changed line was and would be.
fn rewrite(llm: &ChatClient, system: &str, blocks: &[(String, String, String)], temperature: f32, progress: &dyn Fn(usize, usize), accept: &dyn Fn(&str, &str) -> Option<String>) -> Result<(Vec<Enriched>, usize), String> {
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
        let messages = [Message::system(system), Message::user_text(&user)];
        let sampling = Sampling::new(temperature, 0.9, ((chars * 2) as u32).clamp(1_024, 8_192));
        let use_schema = llm.structured_output() != StructuredOutput::Unsupported;
        let done = llm.complete(&messages, &sampling, use_schema.then_some(&schema)).map_err(|e| t!("director-failed", error = e.to_string()))?;
        let text = done.text.trim();
        let body = match (text.find('{'), text.rfind('}')) {
            (Some(a), Some(b)) if b > a => &text[a..=b],
            _ => return Err(t!("director-not-json", answer = text.chars().take(200).collect::<String>())),
        };
        let lines: Tagged = serde_json::from_str(body).map_err(|e| t!("director-not-json", answer = format!("{e}: {}", body.chars().take(200).collect::<String>())))?;
        for line in lines.lines {
            let Some((id, before)) = piece.iter().find(|b| b.0 == line.id).map(|b| (&b.0, &b.2)) else { continue };
            match accept(before, &line.text) {
                None => refused += 1,
                Some(after) if after.trim() != before.trim() => out.push(Enriched { id: id.clone(), before: before.clone(), after }),
                Some(_) => {}
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
        assert_eq!(letters("他买了３本书，ＯＫ。"), letters("他买了3本书, ok."), "full-width digits and letters are the same text");
    }

    #[test]
    fn the_podcast_schema_limits_speakers_to_the_cast() {
        let s = lines_schema(Some(&["Анна".to_string(), "Борис".to_string()]));
        assert_eq!(s["properties"]["lines"]["items"]["properties"]["speaker"]["enum"], json!(["Анна", "Борис"]));
    }
}
