//! The book player written next to an export: one HTML file with the text and the audio beside it, the line being
//! read and its word lit as it plays, a click on a line plays from it. It needs nothing but a browser.

/// Text for an HTML element's content.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The player for the audio `file` (beside the page) of `timings` (the export's blocks with start, end, speaker, text
/// and the words heard with their times).
pub fn player_html(title: &str, file: &str, timings: &serde_json::Value) -> String {
    let data = serde_json::json!({ "file": file, "blocks": timings }).to_string().replace("</", "<\\/");
    let title = escape(title);
    format!(
        r##"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ color-scheme: light dark; --bg: #f6f7f9; --fg: #161b22; --muted: #5d6673; --lit: #d8e8ff; --word: #9cc5ff; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg: #0d1117; --fg: #e6ebf2; --muted: #8892a3; --lit: #1b2a40; --word: #2f5d99; }} }}
body {{ margin: 0; background: var(--bg); color: var(--fg); font: 18px/1.7 Georgia, "Times New Roman", serif; }}
header {{ position: sticky; top: 0; background: var(--bg); padding: 12px 16px; border-bottom: 1px solid color-mix(in srgb, var(--fg) 12%, transparent); }}
h1 {{ margin: 0 0 8px; font: 600 16px/1.3 system-ui, sans-serif; }}
audio {{ width: 100%; }}
main {{ max-width: 42rem; margin: 0 auto; padding: 16px 16px 40vh; }}
p {{ margin: 0 0 0.6em; padding: 2px 6px; border-radius: 6px; cursor: pointer; }}
p.lit {{ background: var(--lit); }}
.who {{ display: block; font: 12px/1.4 system-ui, sans-serif; color: var(--muted); }}
.w.on {{ background: var(--word); border-radius: 3px; }}
</style>
</head>
<body>
<header><h1>{title}</h1><audio id="audio" controls preload="metadata"></audio></header>
<main id="text"></main>
<script type="application/json" id="data">{data}</script>
<script>
const data = JSON.parse(document.getElementById("data").textContent);
const audio = document.getElementById("audio");
audio.src = encodeURIComponent(data.file);
const main = document.getElementById("text");
const lines = data.blocks.map((b, i) => {{
  const p = document.createElement("p");
  if (b.speaker && (i === 0 || data.blocks[i - 1].speaker !== b.speaker)) {{
    const who = document.createElement("span"); who.className = "who"; who.textContent = b.speaker; p.appendChild(who);
  }}
  const parts = b.text.split(/(\s+)/);
  const words = [];
  for (const part of parts) {{
    if (!part.trim()) {{ p.appendChild(document.createTextNode(part)); continue; }}
    const s = document.createElement("span"); s.className = "w"; s.textContent = part; p.appendChild(s); words.push(s);
  }}
  const heard = b.words || [];
  let times;
  if (heard.length === words.length) times = heard.map((w) => w.start);
  else {{
    const weight = words.map((w) => w.textContent.length + 1);
    const total = weight.reduce((a, c) => a + c, 0) || 1;
    let t = b.start;
    times = weight.map((w) => {{ const at = t; t += (w / total) * (b.end - b.start); return at; }});
  }}
  p.addEventListener("click", () => {{ audio.currentTime = b.start; audio.play(); }});
  main.appendChild(p);
  return {{ p, b, words, times }};
}});
let litLine = null, litWord = null;
function tick() {{
  const t = audio.currentTime;
  const line = lines.find((l) => t >= l.b.start && t < l.b.end) || null;
  if (line !== litLine) {{
    if (litLine) litLine.p.classList.remove("lit");
    if (line) {{ line.p.classList.add("lit"); line.p.scrollIntoView({{ block: "center", behavior: "smooth" }}); }}
    litLine = line;
  }}
  let word = null;
  if (line) for (let i = 0; i < line.words.length && line.times[i] <= t; i++) word = line.words[i];
  if (word !== litWord) {{ if (litWord) litWord.classList.remove("on"); if (word) word.classList.add("on"); litWord = word; }}
  requestAnimationFrame(tick);
}}
requestAnimationFrame(tick);
</script>
</body>
</html>
"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_carries_the_file_and_the_text_and_cannot_be_broken_by_it() {
        let html = player_html("Кафе <1>", "Кафе.mp3", &serde_json::json!([{ "start": 0.0, "end": 1.5, "speaker": "Анна", "text": "Привет </script> мир", "words": [] }]));
        assert!(html.contains("<title>Кафе &lt;1&gt;</title>"));
        assert!(html.contains("\"file\":\"Кафе.mp3\""));
        assert!(!html.contains("Привет </script>"));
        assert_eq!(html.matches("</script>").count(), 2);
    }
}
