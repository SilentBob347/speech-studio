//! Google Gemini TTS как движок проекта (`google_tts.rs` Dub): модель и режим — из «Облака», голос — проекта. Фраза
//! кэшируется в папке проекта по модели, голосу, тексту и указанию. Режим Batch (вдвое дешевле) собирает все фразы
//! задачи в одну пачку заранее: её имя пишется в журнал до опроса, и после перезапуска пачка опрашивается, а не
//! отправляется заново; готовые фразы ложатся в кэш, и блоки берут звук оттуда.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

const SR: u32 = 24_000;
const BATCH_POLL: Duration = Duration::from_secs(10);

pub struct Setup {
    pub model: String,
    pub voice: String,
    pub batch: bool,
}

/// Модель (проекта, иначе студии), голос проекта и режим студии; без модели или голоса — ошибка с тем, что задать.
pub fn setup(models_root: &Path, model: &str, voice: &str) -> Result<Setup, String> {
    let sel = crate::models::load_selection(models_root);
    let model = match model.trim() {
        "" => sel.get("google_tts_model").and_then(Value::as_str).unwrap_or("").trim().to_string(),
        m => m.to_string(),
    };
    if model.is_empty() {
        return Err(t!("google-no-model"));
    }
    if voice.trim().is_empty() {
        return Err(t!("google-no-voice"));
    }
    Ok(Setup { model, voice: voice.trim().to_string(), batch: sel.get("google_tts_mode").and_then(Value::as_str) == Some("batch") })
}

fn client() -> Result<crate::google_tts::Client, String> {
    let (key, _) = crate::credentials::google_api_key().ok_or_else(|| t!("cloud-no-key", provider = "Google"))?;
    crate::google_tts::Client::new(key)
}

fn cache(dir: &Path, s: &Setup, text: &str, style: &str) -> PathBuf {
    let tag = blake3::hash(format!("{}|{}|{text}|{style}", s.model, s.voice).as_bytes()).to_hex()[..20].to_string();
    dir.join("cloud").join("google").join(format!("{tag}.wav"))
}

/// Звук ответа Google: PCM L16 24 кГц как есть, другой формат — через ffmpeg.
fn samples_of(mime: &str, bytes: &[u8]) -> Result<Vec<f32>, String> {
    if mime.contains("L16") || mime.contains("pcm") {
        return Ok(bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect());
    }
    let dir = tempfile::Builder::new().prefix("google-").tempdir().map_err(|e| e.to_string())?;
    let (src, wav) = (dir.path().join("in.bin"), dir.path().join("out.wav"));
    std::fs::write(&src, bytes).map_err(|e| e.to_string())?;
    crate::media::extract_audio(&src, &wav, SR, 1)?;
    crate::wavio::read_mono_f32(&wav).map(|(s, _)| s)
}

fn spend(repo_root: &Path, pid: &str, s: &Setup, usage: &Value, batch: bool) -> Option<f64> {
    let cost = crate::google_tts::estimate(&s.model, batch, usage).get("usd").and_then(Value::as_f64);
    crate::cloud::record(repo_root, crate::cloud::Spent {
        at: chrono::Utc::now().timestamp_millis(),
        day: chrono::Local::now().format("%Y-%m-%d").to_string(),
        project: pid.to_string(),
        provider: if batch { "google-batch" } else { "google" }.into(),
        model: s.model.clone(),
        task: "speech".into(),
        estimate: cost,
        cost: None,
        cost_source: if cost.is_some() { "estimate" } else { "unknown" }.into(),
        origin: crate::mcp::current_actor(),
    });
    cost
}

/// Фраза Gemini TTS (24 кГц): из кэша, иначе обычным запросом (в кэш и в журнал расходов) с оценкой цены.
pub fn phrase(repo_root: &Path, models_root: &Path, project_dir: &Path, pid: &str, s: &Setup, text: &str, style: &str) -> Result<(Vec<f32>, Option<f64>), String> {
    let path = cache(project_dir, s, text, style);
    if path.is_file() {
        return crate::wavio::read_mono_f32(&path).map(|(x, _)| (x, None));
    }
    crate::cloud::check_limit(repo_root, models_root, None)?;
    let response = client()?.generate(&s.model, &crate::google_tts::request(text, &s.voice, style)?)?;
    let (mime, bytes) = crate::google_tts::audio(&response)?;
    let samples = samples_of(&mime, &bytes)?;
    let cost = spend(repo_root, pid, s, &response["usageMetadata"], false);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    crate::wavio::write_mono_f32(&path, &samples, SR)?;
    Ok((samples, cost))
}

/// Пачка Batch на фразы, которых нет в кэше: создаётся (или находится по журналу) и ждётся; ответы — в кэш.
pub fn batch(repo_root: &Path, models_root: &Path, project_dir: &Path, pid: &str, s: &Setup, phrases: &[(String, String)], progress: &dyn Fn(Value)) -> Result<(), String> {
    let missing: Vec<&(String, String)> = phrases.iter().filter(|(t, st)| !cache(project_dir, s, t, st).is_file()).collect();
    let journal = project_dir.join("cloud").join("google-batch.json");
    if missing.is_empty() && !journal.is_file() {
        return Ok(());
    }
    let c = client()?;
    let name = match std::fs::read_to_string(&journal).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|j| j["name"].as_str().map(str::to_string)) {
        Some(name) => name,
        None => {
            crate::cloud::check_limit(repo_root, models_root, None)?;
            let rows: Vec<Value> = missing
                .iter()
                .map(|(t, st)| -> Result<Value, String> {
                    let key = cache(project_dir, s, t, st).file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                    Ok(json!({ "metadata": { "key": key }, "request": crate::google_tts::request(t, &s.voice, st)? }))
                })
                .collect::<Result<_, _>>()?;
            progress(json!({ "type": "progress", "stage": "speech_synth", "msg": t!("google-batch-sending", n = rows.len() as i64) }));
            let file = c.upload_requests(&Value::Array(rows))?;
            let name = c.batch_create(&s.model, &format!("speech-studio-{pid}"), &file).map_err(|(_, e)| e)?;
            if let Some(dir) = journal.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            crate::voice_library::write_atomic(&journal, json!({ "name": name, "model": s.model, "voice": s.voice }).to_string().as_bytes())?;
            name
        }
    };
    let ctl = crate::jobs::current();
    let op = loop {
        if ctl.as_ref().is_some_and(|c| c.is_cancelled()) {
            return Err(crate::jobs::CANCELLED.to_string());
        }
        let op = c.batch_get(&name)?;
        if op["done"] == true {
            break op;
        }
        let state = op.pointer("/metadata/state").and_then(Value::as_str).unwrap_or("BATCH_STATE_PENDING").to_string();
        progress(json!({ "type": "progress", "stage": "speech_synth", "msg": t!("google-batch-waiting", state = state) }));
        std::thread::sleep(BATCH_POLL);
    };
    let _ = std::fs::remove_file(&journal);
    if let Some(error) = op.get("error").filter(|e| !e.is_null()) {
        return Err(t!("google-batch-failed", error = error.to_string()));
    }
    let rows = op.pointer("/response/inlinedResponses/inlinedResponses").and_then(Value::as_array).cloned().unwrap_or_default();
    let dir = project_dir.join("cloud").join("google");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut failed = Vec::new();
    for row in rows {
        let key = row.pointer("/metadata/key").and_then(Value::as_str).unwrap_or_default();
        let response = row.get("response").cloned().unwrap_or(Value::Null);
        match crate::google_tts::audio(&response) {
            Ok((mime, bytes)) if !key.is_empty() => {
                let samples = samples_of(&mime, &bytes)?;
                crate::wavio::write_mono_f32(&dir.join(format!("{key}.wav")), &samples, SR)?;
                spend(repo_root, pid, s, &response["usageMetadata"], true);
            }
            Ok(_) => failed.push("no metadata.key".to_string()),
            Err(e) => failed.push(row.get("error").filter(|e| !e.is_null()).map(Value::to_string).unwrap_or(e)),
        }
    }
    if let Some(first) = failed.first() {
        return Err(t!("google-batch-missing", n = failed.len() as i64, error = first.clone()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_needs_a_model_and_a_voice_and_prefers_the_project_model() {
        let root = tempfile::tempdir().unwrap();
        assert!(setup(root.path(), "", "Kore").is_err());
        assert!(setup(root.path(), "gemini-3.8-flash-lite-tts", " ").is_err());
        let s = setup(root.path(), "gemini-3.8-flash-lite-tts", " Kore ").unwrap();
        assert_eq!((s.model.as_str(), s.voice.as_str(), s.batch), ("gemini-3.8-flash-lite-tts", "Kore", false));
    }

    #[test]
    fn cache_key_changes_with_voice_text_and_style() {
        let s = |voice: &str| Setup { model: "m".into(), voice: voice.into(), batch: false };
        let dir = Path::new("p");
        let a = cache(dir, &s("Kore"), "Привет", "");
        assert_eq!(a, cache(dir, &s("Kore"), "Привет", ""));
        assert_ne!(a, cache(dir, &s("Puck"), "Привет", ""));
        assert_ne!(a, cache(dir, &s("Kore"), "Привет!", ""));
        assert_ne!(a, cache(dir, &s("Kore"), "Привет", "шёпотом"));
    }

    #[test]
    fn pcm_from_google_reads_as_samples() {
        let bytes: Vec<u8> = [0i16, 16384, -32768].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(samples_of("audio/L16;codec=pcm;rate=24000", &bytes).unwrap(), vec![0.0, 0.5, -1.0]);
    }
}
