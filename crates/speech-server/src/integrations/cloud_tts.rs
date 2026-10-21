//! OpenRouter speech transport: explicit credentials, model, voice and style; output is 24 kHz mono.

use dub_llm::openrouter::{OpenRouter, SpeechAudio};

pub const SR: u32 = 24_000;

/// Одна фраза облаком → сэмплы 24 кГц моно.
pub fn synth(key: String, text: &str, model: &str, voice: &str, style: &str) -> Result<Vec<f32>, String> {
    let client = OpenRouter::new(Some(key)).map_err(|e| t!("cloud-tts-failed", error = format!("{e:#}")))?;
    let (bytes, mime) = match client
        .speech_with_style(model, text, voice, style)
        .map_err(|e| t!("cloud-tts-failed", error = format!("{e:#}")))?
    {
        SpeechAudio::Wav(wav) => (wav, "audio/wav".to_string()),
        SpeechAudio::Encoded { mime, bytes } => (bytes, mime),
    };
    if bytes.len() < 200 {
        return Err(t!("cloud-tts-too-short", bytes = bytes.len()));
    }
    let dir = tempfile::Builder::new().prefix("cloud-tts-").tempdir().map_err(|e| e.to_string())?;
    let ext = if mime.contains("mpeg") || mime.contains("mp3") {
        "mp3"
    } else if mime.contains("ogg") || mime.contains("opus") {
        "ogg"
    } else if mime.contains("flac") {
        "flac"
    } else {
        "wav"
    };
    let src = dir.path().join(format!("in.{ext}"));
    let wav = dir.path().join("out.wav");
    std::fs::write(&src, &bytes).map_err(|e| e.to_string())?;
    crate::audio::media::extract_audio(&src, &wav, SR, 1)?;
    crate::audio::wavio::read_mono_f32(&wav).map(|(s, _)| s)
}
