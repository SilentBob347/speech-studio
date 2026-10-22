# Changelog

What changed, newest first. Dates are release dates; the app is versioned by its Windows
build. Every change a user can see is written here in the commit that makes it, and the
release notes on GitHub are taken from the release's section.

## Unreleased

### Changed

- **Whisper is CrisperWhisper 2.0 turbo in the audio.cpp engine**, as in Dub Studio: 99 languages with word times, the language of Auto heard by VoxLingua107, the Listening prompt passed to it. Faster-Whisper, its six models and its CUDA 11 libraries are gone, and this program's copies of them are removed at start (a model folder shared with another program loses only its link).
- **Higgs Audio v3 runs in the audio.cpp server** that CrisperWhisper runs in, as in Dub Studio, instead of its own engine library.
- **Recognition runs on Vulkan** on an AMD or Intel card and on an NVIDIA card CUDA 13 does not run on (Auto), and Vulkan can be chosen by hand in Settings → Models → Recognition; before, such a computer recognised on the processor.

### Fixed

- **Recognition no longer closes the program on processors without AVX-512** (Intel Core before the 11th generation and the 12th–14th, most AMD before Zen 4): transcribe.cpp engine-0.1.3 is built for AVX2, FMA and F16C instead of the build machine's own instructions, and a processor without them gets a message instead of a crash. The take check, a voice's reference text, cloning from a long reference and Listening all went through it.
- **Voices of the pack with their text in a `.lab` file have their reference text** (English_Female, Arabic_Female and six more): the studio read only `.txt`, so Drama 3 refused them for a missing transcript and the studio tried to recognise text it already had.
- **Drama 3 says which voice lacks its reference text and where to add it** (Voice library → Reference text, or Recognise the reference text), and does so before loading the model; a long voice with no piece of whole words up to the reference length is named as such.
- **Cleaning a voice works on any sample rate**: a voice recorded at 48 or 24 kHz went to the separator as it was, and the separator, which takes 44.1 kHz only, failed with exit code 1.
- **The log is kept on disk and survives a crash** (`logs/speech-studio.log` in the data folder, the previous one beside it as `speech-studio.old.log` past 8 MB): every step of a job, every failed job with its whole error, a panic of any thread and, on Windows, an exception that ends the process with its code and the module it came from. *Report a problem* packs both files.
- **Checking the SHA-256 after a download moves the bar** (bytes checked of the bytes to check) instead of leaving it at 100 %; a failure inside the download thread ends the download with its cause instead of leaving it spinning.
- **A synthesis that timed out or hung names its engine.**
- **The director's verbatim check takes full-width digits and letters (３, Ａ) as the ordinary ones.**
- **Drama 3, Qwen3-TTS and the separator check the processor before they load** (AVX2, FMA, F16C, BMI2) and name what is missing instead of closing the program.
- **Cleaning a voice works in a folder with Cyrillic or other non-ASCII letters in its path**, and a separator failure shows its error line.
- **Higgs voices in a data folder with Cyrillic or other non-ASCII letters in its path**: its engine library could not open such a path and said the model folder does not exist.
- **An engine left over from an earlier version is downloaded again**: an engine or program counts as installed only when its SHA-256 is the pinned one, not when a file of its name and size is there (a build with other processor flags can be the same size).
- **The separator's graphics card build that found no card stops with that cause** instead of cleaning on the processor unnoticed, and the local Gemma build that sees no card says so before loading.
- **CrisperWhisper is not offered Vulkan**: Settings → Models → Recognition shows Vulkan only for Parakeet and Nemotron, and CrisperWhisper with Vulkan chosen earlier says it has no Vulkan build instead of running on CUDA or the processor.
- **The diagnostics check the transcribe.cpp builds' libraries too.**
- **Stop ends the engines a job started at once** (separator, llama-server, ffmpeg, yt-dlp): on Windows they were ended through taskkill, which waits as long as WMI hangs, so a stopped job could keep running to its end.
- **An edited reference text is used at once**: the piece cut from a voice was kept by the audio's size and date alone, so a corrected text or new word timings came in only after the audio changed.

## 0.2.0 — 2026-10-20

### Live

- **Live mode: talk with an agent by voice.** An agent connected over MCP speaks each finished phrase at once
  (`say_start`, `say_text`), the person answers into the microphone and is recognised phrase by phrase; speaking over
  the agent stops it. Mute with M or Ctrl+Shift+Space from any window (hold to talk), type to the agent, choose the
  recogniser and the agent's voice, the agent's name beside its lines. Ctrl+3 and the command palette open Live, as
  Ctrl+1 and Ctrl+2 open Speech and Listening.
- A new conversation never starts muted: the window drops the last one's mute at the start, and M, the mute button
  and the widget ask the studio to toggle rather than set what the window last saw.
- **The sound window**: microphone, agent's voice and speaker tabs, level meters with the peak held, hear yourself, a
  test chime, the agent's volume.
- **Five looks** (aura, wave, bars, radial, matrix) and a widget — a frameless bar over every other window.

### Speech

- **Drama 3 from 6 GB cards.** When the model does not fit the card, its experts (18 GB) live in RAM (32 GB needed)
  and the card keeps a cache of the ones in use; the sound is the same. Settings → Models → *VRAM for Drama 3*: Auto
  (the whole model when it fits the free VRAM) or 5 to 24 GB by hand. Measured on an RTX 4090: whole model about 8×
  faster than real time, 12 GB 4.7×, 8 GB 3×, 6 GB 2.2×. Engine drama-rs engine-0.1.3.
- **Whisper keeps int8 weights on the card** (int8_float16 by default; only the int8 computes are offered, a saved float16/float32 choice falls back to the default), and a card that fails says so instead of recognising on the processor in silence.
- **A single line with a colon is voiced whole** («Внимание: поезд прибывает»): only two lines or more like «Anna: …» make a dialog, a single one lost its words before the colon as a speaker's name.
- **A stress model that failed to load no longer stops voicing for the rest of the session.** The failure kept a lock
  closed and every later block failed with "stress lock"; it now says why the load failed, and the next block tries
  again. The load itself failed when ONNX Runtime was downloaded after the studio started (on the first-run screen):
  Windows then found another program's older onnxruntime.dll (1.17 in System32); the studio now always names its own.
- **On an NVIDIA card the GPU ONNX Runtime and cuDNN are required.** Skipped on first run, the studio loaded the CPU
  build of ONNX Runtime, which a running process cannot swap, and Supertonic on the card failed until a restart; first
  run now fetches them with the other required parts.
- **The voice pack comes from Dub Studio when it has it.** *Download the voice pack* links the pack's 100 files from
  another program of timoncool's that already has them, each checked by its hash, and downloads only when none has
  the whole pack; that program's own voices are not touched.
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

- **Recognition on transcribe.cpp** (GGUF Q8_0 on the GPU, or its processor build on a computer without an NVIDIA
  card; Settings → Models → Recognition, switched without a restart): Parakeet-TDT v3 and Parakeet Ultra move off ONNX Runtime,
  and **Nemotron 3.5 ASR** (28 languages with punctuation) joins them; the model stays loaded between jobs instead of
  loading for each one. Whisper stays on faster-whisper for its word times. The engine and the models are the ones
  Dub Studio uses, so either program takes them from the other.
- The ONNX recognition models of 0.1.0 (Parakeet v3 and Ultra, Silero VAD) are removed at start: transcribe.cpp does not read them.
- **Live recognition** from the microphone phrase by phrase, a **watched folder** recognised in the background,
  **quick recognition** of a file without the editor, a batch of files saved in several formats at once.
- Subtitles by a broadcaster's rules (Netflix, BBC and others), who talks how much, a repeated phrase in the player,
  numbers as digits in the export, the recogniser chosen by the recording's language.

### Window and agents

- **First run asks only what to have**: voices, recognition (one Whisper model or none) and extras; the engines, runtimes and the build for this computer's card come with them by themselves, named in one line with their size.
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
