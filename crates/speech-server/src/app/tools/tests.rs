use super::*;

#[tokio::test]
async fn chat_batches_keep_the_same_tool_restrictions_and_stop_policy() {
    for keep_going in [false, true] {
        let answer = chat_call(
            "studio_batch",
            &json!({"keep_going":keep_going,"calls":[
                {"name":"assistant_request_answer","arguments":{"request_id":"absent","answer":"x"}},
                {"name":"studio_guide","arguments":{"topic":"tags"}}
            ]}),
        )
        .await
        .unwrap();
        let result: Value = serde_json::from_str(&answer.text).unwrap();
        assert_eq!(result["calls"][0]["error"], "Unknown tool: assistant_request_answer");
        assert_eq!(result["calls"].as_array().unwrap().len(), if keep_going { 2 } else { 1 });
        if keep_going {
            assert!(result["calls"][1]["result"].as_str().unwrap().contains("Expression tags"));
        }
    }
}

// ---------------------------------------------------------------- every route is a tool (as in Dub Studio)

/// The routes of build_router: (METHOD, path pattern).
fn router_routes() -> Vec<(String, String)> {
    let source = include_str!("../router.rs").replace("\r\n", "\n");
    let body = source
        .split("pub fn build_router(")
        .nth(1)
        .expect("build_router")
        .split("\n}\n")
        .next()
        .unwrap();
    let text_source = include_str!("../text_api.rs").replace("\r\n", "\n");
    let text_routes = text_source
        .split("pub(crate) fn router(")
        .nth(1)
        .expect("text router")
        .split("\n}\n")
        .next()
        .unwrap();
    let body = body.replace(".merge(dict_api::router())", text_routes);
    let code: String = body
        .lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    let mut routes = Vec::new();
    for chunk in code.split(".route(").skip(1) {
        let path = chunk
            .trim_start()
            .strip_prefix('"')
            .and_then(|rest| rest.split('"').next())
            .expect("a route's path")
            .to_string();
        let handlers = chunk
            .split(".fallback(")
            .next()
            .unwrap()
            .split(".layer(")
            .next()
            .unwrap()
            .split(".with_state(")
            .next()
            .unwrap();
        for method in ["get", "post", "patch", "put", "delete"] {
            let call = format!("{method}(");
            let called = handlers
                .match_indices(&call)
                .any(|(at, _)| !handlers[..at].chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_'));
            if called {
                routes.push((method.to_uppercase(), path.clone()));
            }
        }
    }
    routes
}

fn route_of(pattern: &str, path: &str) -> bool {
    let path = path.split('?').next().unwrap_or_default();
    let (wanted, given): (Vec<&str>, Vec<&str>) = (pattern.split('/').collect(), path.split('/').collect());
    wanted.len() == given.len()
        && wanted
            .iter()
            .zip(&given)
            .all(|(part, value)| part.starts_with('{') && !value.is_empty() || part == value)
}

/// Routes an agent is not given, and why.
const NOT_TOOLS: &[(&str, &str, &str)] = &[
    (
        "POST",
        "/record/start",
        "the microphone is turned on only by the person, with their button in the window",
    ),
    (
        "POST",
        "/record/test",
        "the microphone is turned on only by the person, with their button in the window",
    ),
    (
        "POST",
        "/listen/record/start",
        "the microphone is turned on only by the person, with their button in the window",
    ),
];

/// Arguments for every property of a tool, and one set more for each other choice of an enum.
fn samples(tool: &Tool, file: &std::path::Path) -> Vec<Value> {
    let schema = (tool.schema)();
    let properties = schema["properties"].as_object().cloned().unwrap_or_default();
    let sample = |name: &str, property: &Value| -> Value {
        if let Some(first) = property["enum"].as_array().and_then(|choices| choices.first()) {
            return first.clone();
        }
        let kind = match &property["type"] {
            Value::Array(kinds) => kinds[0].as_str().unwrap_or("string").to_string(),
            other => other.as_str().unwrap_or("string").to_string(),
        };
        match (kind.as_str(), name) {
            ("integer", _) => json!(1),
            ("number", _) => json!(1.5),
            ("boolean", _) => json!(true),
            ("array", _) if property["items"]["type"] == "integer" => json!([1]),
            ("array", n) if n.ends_with("paths") => json!([file.to_string_lossy()]),
            ("array", _) => json!(["x1"]),
            ("object", _) => json!({}),
            (_, n) if n.ends_with("path") || n == "file" => json!(file.to_string_lossy()),
            _ => json!("x1"),
        }
    };
    let base: serde_json::Map<String, Value> = properties
        .iter()
        .map(|(name, property)| (name.clone(), sample(name, property)))
        .collect();
    let mut all = vec![Value::Object(base.clone())];
    for (name, property) in &properties {
        for choice in property["enum"].as_array().into_iter().flatten().skip(1) {
            let mut other = base.clone();
            other.insert(name.clone(), choice.clone());
            all.push(Value::Object(other));
        }
    }
    all
}

/// The routes a composite tool serves by running the studio's code itself.
const COMPOSITE_ROUTES: &[(&str, &[(&str, &str)])] = &[
    ("composite:atomic:separate_file", &[("POST", "/separate"), ("GET", "/jobs/x1")]),
    ("composite:agent", &[("GET", "/mcp/status")]),
    ("composite:questions", &[("POST", "/mcp/agent-llm/v1/chat/completions")]),
    ("composite:windows", &[("GET", "/mcp/window")]),
    ("composite:window_focus", &[("POST", "/mcp/window/focus")]),
    ("composite:window_drop", &[("POST", "/mcp/window/result")]),
    ("composite:batch", &[("POST", "/mcp")]),
];

/// The routes a tool reaches: (METHOD, path).
fn reached_by(tool: &Tool, file: &std::path::Path) -> Vec<(String, String)> {
    let mut reached = Vec::new();
    for args in samples(tool, file) {
        let Ok(call) = (tool.call)(&args) else { continue };
        if matches!(call.payload, Payload::Window { .. }) {
            continue;
        }
        if call.path.starts_with("composite:") {
            let served = COMPOSITE_ROUTES
                .iter()
                .find(|(kind, _)| *kind == call.path)
                .map(|(_, routes)| *routes)
                .unwrap_or_default();
            reached.extend(served.iter().map(|(method, path)| (method.to_string(), path.to_string())));
            continue;
        }
        reached.push((call.method.to_string(), call.path.clone()));
    }
    reached
}

#[test]
fn every_route_is_a_tool() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("clip.wav");
    std::fs::write(&file, b"x").unwrap();
    let routes = router_routes();
    assert!(routes.len() > 100, "build_router read: {routes:?}");
    let mut covered = vec![false; routes.len()];
    for tool in tools() {
        for (method, path) in reached_by(tool, &file) {
            let found: Vec<usize> = routes
                .iter()
                .enumerate()
                .filter(|(_, (m, p))| *m == method && route_of(p, &path))
                .map(|(at, _)| at)
                .collect();
            assert!(
                !found.is_empty(),
                "{} calls {method} {path}, a route the studio does not have",
                tool.name
            );
            for at in found {
                covered[at] = true;
            }
        }
    }
    let by_tool = covered.clone();
    let mut stale = Vec::new();
    for (method, path) in NOT_TOOLS.iter().map(|(method, path, _)| (method, path)) {
        let at = routes
            .iter()
            .position(|(m, p)| m == method && p == path)
            .unwrap_or_else(|| panic!("{method} {path} of NOT_TOOLS is not a route of build_router"));
        if by_tool[at] {
            stale.push(format!("{method} {path}"));
        }
        covered[at] = true;
    }
    assert!(
        stale.is_empty(),
        "these routes of NOT_TOOLS are reached by a tool: drop them from NOT_TOOLS: {stale:?}"
    );
    let missing: Vec<String> = routes
        .iter()
        .zip(&covered)
        .filter(|(_, done)| !**done)
        .map(|((method, path), _)| format!("{method} {path}"))
        .collect();
    assert!(
        missing.is_empty(),
        "these routes have no tool: give each one, or put it into NOT_TOOLS with why: {missing:#?}"
    );
}

#[test]
fn every_journaled_tool_exists_and_changes_something() {
    let names: Vec<&str> = tools().iter().map(|tool| tool.name).collect();
    for (tool, verb, kind) in JOURNAL {
        assert!(names.contains(tool), "{tool} is journaled but is not a tool");
        assert_eq!(annotations(tool)["readOnlyHint"], false, "{tool} is journaled but reads only");
        assert!(!verb.is_empty() && !kind.is_empty());
    }
}

#[test]
fn the_window_has_words_for_every_journaled_change() {
    let en: Value = serde_json::from_str(include_str!("../../../../../frontend/src/locales/en.json")).unwrap();
    for (tool, verb, kind) in JOURNAL {
        assert!(en["journal"]["verb"][verb].is_string(), "{tool}: no words for the verb {verb}");
        assert!(en["journal"]["kind"][kind].is_string(), "{tool}: no words for the kind {kind}");
    }
}

#[test]
fn every_tool_is_in_the_skill() {
    let missing: Vec<&str> = tools()
        .iter()
        .map(|tool| tool.name)
        .filter(|name| !SKILL.contains(&format!("`{name}`")))
        .collect();
    assert!(missing.is_empty(), "docs/mcp-skill.md does not name {missing:?}");
}

#[test]
fn every_guide_reads_and_every_prompt_takes_its_arguments() {
    for (topic, _) in guides::TOPICS {
        let (mime, text) = resource(&format!("{}{topic}", guides::GUIDE_URI)).unwrap_or_else(|| panic!("no guide {topic}"));
        assert_eq!(mime, "text/plain");
        assert!(text.len() > 300, "{topic}: {text}");
    }
    let tags = guides::guide("tags").unwrap();
    assert!(
        tags.starts_with("- **Expression tags differ per engine**"),
        "the skill's tags part leads the guide"
    );
    for engine in crate::engines::all() {
        assert!(
            tags.contains(&format!("({})", engine.id)),
            "{} is missing from the tags guide",
            engine.id
        );
    }
    assert!(guides::guide("podcast").unwrap().contains("podcast scriptwriter"));
    assert!(guides::guide("audiobook").unwrap().contains("VERBATIM"));
    for prompt in guides::prompts() {
        let name = prompt["name"].as_str().unwrap();
        let text =
            guides::prompt(name, &json!({ "topic": "кофе по утрам", "path": "C:/books/war.epub" })).unwrap_or_else(|| panic!("{name}"));
        assert!(
            text.contains("кофе по утрам") || text.contains("C:/books/war.epub"),
            "{name} drops its argument"
        );
    }
    assert!(resource("studio://guide/nothing").is_none());
}

#[test]
fn a_journal_line_names_what_the_agent_named() {
    assert_eq!(journal_target(&json!({ "pid": "abc", "name": " Глава 1 " })), "Глава 1");
    assert_eq!(journal_target(&json!({ "pid": "abc" })), "abc");
    assert_eq!(journal_target(&json!({})), "");
}
