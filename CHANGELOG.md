# Changelog

What changed, newest first. Dates are release dates; the app is versioned by its Windows
build. Every change a user can see is written here in the commit that makes it, and the
release notes on GitHub are taken from the release's section.

## 0.1.0 — 2026-10-15

The first release.

### Speech

- **Five local voice engines**: Higgs Audio v3 (cloning, emotions, sounds), Fish Audio Drama 3 (24 GB cards), Supertonic 3 (ten voices, CPU or GPU), Qwen3-TTS 1.7B (nine voices, cloning, an instruction per block) and Qwen3-TTS with Russian stress (a LoRA applied at run time); cloud voices of OpenRouter, Google Gemini (with half-price Batch), fal.ai and Runware behind a confirmation.
- **Projects**: paragraphs become blocks and `Anna:` lines a dialog; takes, stale tracking, undo, drag to reorder, regex search and replace, notes and markers; an engine and a voice per character and per block.
- **Text for Russian**: number normalisation with cases, stress, «ё» and homographs by RUAccent (the big_poetry model in int8 as an option), a pronunciation dictionary with Balabolka import and a check by ear, SpeechKit-compatible markup.
- **Quality**: every take recognised back and compared word by word, defect checks (clipping, silence, cut-off, end click, repeated phrase, tail, music or noise instead of speech), voice similarity, several takes with the best one picked.
- **Podcasts and books**: the director writes a podcast for N speakers and casts a book chapter by character; EPUB, FB2, PDF, DOCX, HTML, Markdown and TXT import; M4B with chapters, parts for long books, ACX check.
- **Subtitles as a project**: SRT, VTT and ASS cues voiced into their time with the tempo raised ahead, a retry, stretching and the silence before the line.
- **Timeline** of the export with overlaps of interruptions to cut, join or keep.
- **Export**: WAV, FLAC, MP3, OGG, M4A, M4B, subtitles and synced text, per-block files, loudness presets with a true-peak limiter, room tone and breaths, and a check before export (missing, stale, failed, changed file, too long).
- **Voice by description** with Qwen3-TTS VoiceDesign: candidates to hear, one kept in the library.
- **Streaming**: Higgs and Drama 3 play while they voice, with the time to the first sound.

### Listening

- **Recognition** by Parakeet-TDT or Whisper (with a hint), up to eight speakers, long recordings cut at pauses by Silero VAD v6, noise removed by DPDFNet, music by Roformer; several files at once.
- **The transcript**: speakers, word times, karaoke player, search, cut by text, hallucination filter, corrections, export to SRT, VTT, ASS, TXT and JSON.
- **Check a voicing** against its text, **lay a known text** over the recording, **voices from speakers**.

### Voices

- **Library**: a voice pack, 743 Russian voices from the cloud catalogue, microphone recording, files and ZIPs, photo and card, reference transcript recognised once; cleaning by Roformer or DPDFNet with the original kept; the best piece of a reference.

### Everywhere

- **First run and the model manager** with resumable downloads checked against SHA-256, import from a folder, and models taken from the author's other studios.
- **Agents**: MCP with every function and the window, an assistant panel, an OpenAI-compatible audio API.
- **Diagnostics**, a bug-report package without keys, six languages, light and dark theme, a question before closing while tasks run, auto-update, anonymous statistics that can be turned off.
