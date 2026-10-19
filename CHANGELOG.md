# Changelog

What changed, newest first. Dates are release dates; the app is versioned by its Windows
build. Every change a user can see is written here in the commit that makes it, and the
release notes on GitHub are taken from the release's section.

## Unreleased

### Live

- **Live mode: talk with an agent by voice.** An agent connected over MCP speaks each finished phrase at once
  (`say_start`, `say_text`), the person answers into the microphone and is recognised phrase by phrase; speaking over
  the agent stops it. Mute with M or Ctrl+Shift+Space from any window (hold to talk), type to the agent, choose the
  recogniser and the agent's voice, the agent's name beside its lines. Ctrl+3 and the command palette open Live, as
  Ctrl+1 and Ctrl+2 open Speech and Listening.
- **The sound window**: microphone, agent's voice and speaker tabs, level meters with the peak held, hear yourself, a
  test chime, the agent's volume.
- **Five looks** (aura, wave, bars, radial, matrix) and a widget — a frameless bar over every other window.

### Speech

- **Drama 3 from 6 GB cards.** When the model does not fit the card, its experts (18 GB) live in RAM (32 GB needed)
  and the card keeps a cache of the ones in use; the sound is the same. Settings → Models → *VRAM for Drama 3*: Auto
  (the whole model when it fits the free VRAM) or 5 to 24 GB by hand. Measured on an RTX 4090: whole model about 8×
  faster than real time, 12 GB 4.7×, 8 GB 3×, 6 GB 2.2×. Engine drama-rs engine-0.1.3.
- **Voice-pack voices clone in Drama 3.** A reference longer than the engine's limit and without word timings got cut
  without its text, and Drama 3 refused it; the timings are now recognized once, before voicing, and saved beside the
  voice. The typed transcript is kept.
- **Cloud voices play while they voice.** OpenRouter, Gemini, fal and Runware blocks reach the window phrase by phrase
  as each one arrives, as the local engines do, and their takes keep the time to the first sound.
- **Gemini TTS** in parallel standard requests or as a restart-safe Batch at half price, with an auto mode; Game Dub's
  checks of Google takes (whistle, repeats, padding) redo a bad take, each kind switched in the project.
- **Editing**: a director's note on a take voices the next one, *Edit the line* from the player, take edges on the
  waveform, A/B of takes, blind comparison of two engines, mending a failed take where it went wrong, approved blocks
  kept by voice-all, notes on words, words heard differently shown under the block.
- **Text**: an SSML subset, prosody cues coloured over the words, phones, e-mail, sites and codes read right, groups of
  normalisation rules left as written, a passage in another language with its own voice, the director's punctuation
  and expression tags with words kept, a check of the script before voicing.
- **Books and plays**: direct speech apart from the author's words, a character registry with voices picked by gender,
  age and language, one voice for the whole project, footnotes and addresses dropped at import, the book's names into
  the dictionary, a project rebuilt from its edited book keeping what was voiced, a book player page beside the export.
- **Montage**: a track editor on the project's time scale (move with snapping, trim, fades, gain), tracks of music and
  sounds under the voices from files, folders or a song made in YuE2, MiniMax Music3 or ACE-Step Studio; the timeline
  at the bottom, above the player.
- **Export**: subtitles in SRT, VTT, ASS and LRC, sample rate, bit depth and bitrate, voice effects (radio, phone,
  loudspeaker), even piece levels, a file per chapter levelled as one book, tracks per speaker in a ZIP, a CSV of the
  lines, file name templates, a SHA-256 manifest and a recipe of how each file was made.
- **Voices**: record a voice from the picker, trim a reference by its waveform, a character's variants, a voice from
  attributes, a voice's license, rotating pieces of a long reference, what each engine needs from a reference.
- **Quality**: a quality light and report for the project, a corpus of hard Russian phrases as an engine check, a
  runaway limit per language's speech rate, numbers compared as digits in six languages.

### Listening

- **Nemotron 3.5 ASR** (35 languages with punctuation) and **Cohere Transcribe** (14 languages) as local recognisers
  beside Parakeet and Whisper, both GGUF Q8_0 on the transcribe.cpp engine; each model is offered once.
- **Live recognition** from the microphone phrase by phrase, a **watched folder** recognised in the background,
  **quick recognition** of a file without the editor, a batch of files saved in several formats at once.
- Subtitles by a broadcaster's rules (Netflix, BBC and others), who talks how much, a repeated phrase in the player,
  numbers as digits in the export, the recogniser chosen by the recording's language.

### Window and agents

- The hardware preset names a card's memory as its box does: an RTX 4090 is 24 GB, not 26 (memory is counted in
  2³⁰-byte gigabytes, as the resource monitor already did); so is the diagnostics report.
- The first-run screen's component ticks and Download buttons carry the component's name, so an agent and a screen reader
  know which one they are.
- Every route and window action is an MCP tool; the agent can be the studio's language model, as in YuE2 and MiniMax.
- A command palette (Ctrl+K), own shortcuts, a simple mode with steps, inputs as tabs, built files on the home screen,
  undo of a project deletion, hints on every control in six languages, button texts that say what happens, a phone
  layout, lists in the theme's colours.
- Models another program of timoncool's has are taken from it before downloading; the hub counts model downloads.
- A project page in six languages with examples of every engine.

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
