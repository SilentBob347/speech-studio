//! Recognition by transcribe.cpp (ggml, CUDA): Parakeet TDT v3, Nemotron 3.5, GigaAM-v3, Cohere Transcribe and Whisper
//! large-v3-turbo from their GGUF files. The engine is one library, `transcribe_engine` (fork timoncool/transcribe.cpp,
//! branch studio): libtranscribe and ggml linked whole into it, so it shares nothing with the TTS engines' ggml in the
//! same process. Loaded once and kept, as the other ggml engines are.

use dub_asr::{segment_words, AsrEngine, AsrError, Segment, SpeakerSegment, Turn, Word, SEG_MAX_DUR, SEG_MAX_GAP};
use libloading::Library;
use std::ffi::{c_char, c_int, CStr, CString};
use std::path::{Path, PathBuf};

pub const DIR: &str = "transcribe-engine";
#[cfg(windows)]
pub const LIBRARY: &str = "transcribe_engine.dll";
#[cfg(not(windows))]
pub const LIBRARY: &str = "libtranscribe_engine.so";

/// A model family the studio offers by transcribe.cpp: its id in the settings, its folder and file, and the languages
/// its card lists (empty: every language Whisper knows).
#[derive(Debug)]
pub struct Family {
    pub id: &'static str,
    pub component: &'static str,
    pub file: &'static str,
    pub languages: &'static [&'static str],
}

const EU25: &[&str] = &["bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt", "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk"];

pub const FAMILIES: &[Family] = &[
    Family { id: "gguf-parakeet", component: "asr-gguf-parakeet", file: "parakeet-tdt-0.6b-v3-Q8_0.gguf", languages: EU25 },
    Family {
        id: "gguf-nemotron",
        component: "asr-gguf-nemotron",
        file: "nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf",
        languages: &["en", "es", "fr", "de", "it", "pt", "nl", "ru", "zh", "ja", "ko", "ar", "hi", "tr", "pl", "uk", "cs", "sv", "da", "fi", "no", "el", "hu", "ro", "bg", "hr", "sk", "sl", "et", "lv", "lt", "vi", "th", "id", "ms"],
    },
    Family { id: "gguf-gigaam", component: "asr-gguf-gigaam", file: "gigaam-v3-e2e-rnnt-Q8_0.gguf", languages: &["ru"] },
    Family { id: "gguf-cohere", component: "asr-gguf-cohere", file: "cohere-transcribe-03-2026-Q8_0.gguf", languages: &["en", "fr", "de", "it", "es", "pt", "el", "nl", "pl", "zh", "ja", "ko", "vi", "ar"] },
    Family { id: "gguf-whisper", component: "asr-gguf-whisper", file: "whisper-large-v3-turbo-Q8_0.gguf", languages: &[] },
];

pub fn family(id: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|f| f.id == id)
}

pub fn engine_dir(models_root: &Path) -> PathBuf {
    models_root.join(DIR)
}

pub fn model_path(models_root: &Path, f: &Family) -> PathBuf {
    models_root.join(f.component).join(f.file)
}

pub fn installed(models_root: &Path, f: &Family) -> bool {
    engine_dir(models_root).join(LIBRARY).is_file() && model_path(models_root, f).is_file()
}

/// Whether the family recognises `lang`; empty or auto: it picks the language itself.
pub fn knows(f: &Family, lang: &str) -> bool {
    lang.is_empty() || lang == "auto" || f.languages.is_empty() || f.languages.contains(&lang)
}

#[repr(C)]
struct RunParams {
    struct_size: u64,
    task: c_int,
    timestamps: c_int,
    pnc: c_int,
    itn: c_int,
    diarize: c_int,
    language: *const c_char,
    target_language: *const c_char,
    keep_special_tags: bool,
    family: *const std::ffi::c_void,
    spec_k_drafts: i32,
    vocabulary: *const *const c_char,
    n_vocabulary: i32,
    prompt: *const c_char,
    prefix: *const c_char,
}

#[repr(C)]
struct RawWord {
    struct_size: u64,
    t0_ms: i64,
    t1_ms: i64,
    seg_index: c_int,
    first_token: c_int,
    n_tokens: c_int,
    text: *const c_char,
}

#[repr(C)]
struct Limits {
    struct_size: u64,
    effective_n_ctx: i32,
    effective_max_audio_ms: i64,
    max_kv_bytes: i64,
}

#[repr(C)]
struct Capabilities {
    struct_size: u64,
    native_sample_rate: i32,
    n_languages: c_int,
    languages: *const *const c_char,
    rest: [u8; 0],
}

const ABI_RUN_PARAMS: c_int = 2;
const ABI_CAPABILITIES: c_int = 4;
const ABI_WORD: c_int = 7;
const ABI_SESSION_LIMITS: c_int = 11;
const TIMESTAMPS_WORD: c_int = 3;

enum RawModel {}
enum RawSession {}

#[derive(Clone, Copy)]
struct Api {
    status_string: unsafe extern "C" fn(c_int) -> *const c_char,
    abi_size: unsafe extern "C" fn(c_int) -> usize,
    load: unsafe extern "C" fn(*const c_char, *const std::ffi::c_void, *mut *mut RawModel) -> c_int,
    model_free: unsafe extern "C" fn(*mut RawModel),
    session_init: unsafe extern "C" fn(*mut RawModel, *const std::ffi::c_void, *mut *mut RawSession) -> c_int,
    session_free: unsafe extern "C" fn(*mut RawSession),
    run_params_init: unsafe extern "C" fn(*mut RunParams),
    run: unsafe extern "C" fn(*mut RawSession, *const f32, c_int, *const RunParams) -> c_int,
    n_words: unsafe extern "C" fn(*const RawSession) -> c_int,
    word_init: unsafe extern "C" fn(*mut RawWord),
    get_word: unsafe extern "C" fn(*const RawSession, c_int, *mut RawWord) -> c_int,
    full_text: unsafe extern "C" fn(*const RawSession) -> *const c_char,
    limits_init: unsafe extern "C" fn(*mut Limits),
    get_limits: unsafe extern "C" fn(*const RawSession, *mut Limits) -> c_int,
    caps_init: unsafe extern "C" fn(*mut Capabilities),
    get_caps: unsafe extern "C" fn(*const RawModel, *mut Capabilities) -> c_int,
}

/// What the engine's capabilities struct is read into: room for the struct as it grows, its first fields read by
/// `Capabilities`.
type CapsBuffer = [u64; 64];

/// The library, loaded once for the life of the process (ggml's static initialiser refuses a second load).
static LOADED: std::sync::Mutex<Option<(Library, Api)>> = std::sync::Mutex::new(None);

fn library(dir: &Path) -> Result<Api, String> {
    let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, api)) = loaded.as_ref() {
        return Ok(*api);
    }
    let path = dir.join(LIBRARY);
    let lib = unsafe { Library::new(&path) }.map_err(|e| format!("{}: {e}", path.display()))?;
    let api = unsafe {
        macro_rules! sym {
            ($name:literal) => {
                *lib.get(concat!($name, "\0").as_bytes()).map_err(|e| format!("transcribe.cpp {}: {e}", $name))?
            };
        }
        Api {
            status_string: sym!("transcribe_status_string"),
            abi_size: sym!("transcribe_abi_struct_size"),
            load: sym!("transcribe_model_load_file"),
            model_free: sym!("transcribe_model_free"),
            session_init: sym!("transcribe_session_init"),
            session_free: sym!("transcribe_session_free"),
            run_params_init: sym!("transcribe_run_params_init"),
            run: sym!("transcribe_run"),
            n_words: sym!("transcribe_n_words"),
            word_init: sym!("transcribe_word_init"),
            get_word: sym!("transcribe_get_word"),
            full_text: sym!("transcribe_full_text"),
            limits_init: sym!("transcribe_session_limits_init"),
            get_limits: sym!("transcribe_session_get_limits"),
            caps_init: sym!("transcribe_capabilities_init"),
            get_caps: sym!("transcribe_model_get_capabilities"),
        }
    };
    // the structs are laid out here by hand: a library of another layout is refused before any call fills one
    for (which, ours, name) in [(ABI_RUN_PARAMS, std::mem::size_of::<RunParams>(), "run params"), (ABI_WORD, std::mem::size_of::<RawWord>(), "word"), (ABI_SESSION_LIMITS, std::mem::size_of::<Limits>(), "session limits")] {
        let theirs = unsafe { (api.abi_size)(which) };
        if theirs != ours {
            return Err(format!("transcribe.cpp {}: its {name} struct is {theirs} bytes, the studio's {ours}", path.display()));
        }
    }
    let caps = unsafe { (api.abi_size)(ABI_CAPABILITIES) };
    if caps < std::mem::size_of::<Capabilities>() || caps > std::mem::size_of::<CapsBuffer>() {
        return Err(format!("transcribe.cpp {}: its capabilities struct is {caps} bytes, the studio reads {}..{}", path.display(), std::mem::size_of::<Capabilities>(), std::mem::size_of::<CapsBuffer>()));
    }
    *loaded = Some((lib, api));
    Ok(api)
}

fn text(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// A model loaded on the GPU and its session; freed on drop.
pub struct TcppAsr {
    api: Api,
    model: *mut RawModel,
    session: *mut RawSession,
    languages: Vec<String>,
    /// The longest audio one run takes, in seconds; 0: unbounded (the family cuts long audio itself).
    max_secs: f64,
}

// One run at a time on the session is the caller's to keep (an AsrEngine is used through &mut).
unsafe impl Send for TcppAsr {}

impl Drop for TcppAsr {
    fn drop(&mut self) {
        unsafe {
            (self.api.session_free)(self.session);
            (self.api.model_free)(self.model);
        }
    }
}

impl TcppAsr {
    pub fn load(models_root: &Path, f: &Family) -> Result<TcppAsr, String> {
        let api = library(&engine_dir(models_root))?;
        let path = model_path(models_root, f);
        let c_path = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let status = |s: c_int| text(unsafe { (api.status_string)(s) });
        let mut model: *mut RawModel = std::ptr::null_mut();
        let s = unsafe { (api.load)(c_path.as_ptr(), std::ptr::null(), &mut model) };
        if s != 0 || model.is_null() {
            return Err(format!("transcribe.cpp {}: {}", path.display(), status(s)));
        }
        let mut session: *mut RawSession = std::ptr::null_mut();
        let s = unsafe { (api.session_init)(model, std::ptr::null(), &mut session) };
        if s != 0 || session.is_null() {
            unsafe { (api.model_free)(model) };
            return Err(format!("transcribe.cpp {}: {}", path.display(), status(s)));
        }
        let mut caps = std::mem::MaybeUninit::<CapsBuffer>::zeroed();
        let caps_ptr = caps.as_mut_ptr() as *mut Capabilities;
        let mut languages = Vec::new();
        unsafe {
            (api.caps_init)(caps_ptr);
            if (api.get_caps)(model, caps_ptr) == 0 {
                let c = &*caps_ptr;
                let listed = if c.languages.is_null() { 0 } else { c.n_languages.max(0) as usize };
                for i in 0..listed {
                    languages.push(text(*c.languages.add(i)));
                }
            }
        }
        let mut limits = Limits { struct_size: 0, effective_n_ctx: 0, effective_max_audio_ms: 0, max_kv_bytes: 0 };
        let max_secs = unsafe {
            (api.limits_init)(&mut limits);
            if (api.get_limits)(session, &mut limits) == 0 { limits.effective_max_audio_ms.max(0) as f64 / 1000.0 } else { 0.0 }
        };
        Ok(TcppAsr { api, model, session, languages, max_secs })
    }

    /// The model's own name of `lang`: "ru" is "ru-RU" for a model listing languages with their region.
    fn language(&self, lang: &str) -> Result<Option<CString>, AsrError> {
        if lang.is_empty() || lang == "auto" || self.languages.is_empty() {
            return Ok(None);
        }
        let found = self.languages.iter().find(|l| l.eq_ignore_ascii_case(lang)).or_else(|| self.languages.iter().find(|l| l.to_ascii_lowercase().starts_with(&format!("{}-", lang.to_ascii_lowercase()))));
        match found {
            Some(l) => Ok(Some(CString::new(l.as_str()).map_err(|e| AsrError::Parakeet(e.to_string()))?)),
            None => Err(AsrError::Parakeet(format!("transcribe.cpp: the model does not recognise «{lang}» (it knows {})", self.languages.join(", ")))),
        }
    }

    /// The words of 16 kHz mono audio with their times from its start, in seconds.
    fn words(&mut self, pcm: &[f32], lang: &str) -> Result<Vec<Word>, AsrError> {
        let language = self.language(lang)?;
        let window = if self.max_secs > 0.0 { (self.max_secs * 16_000.0) as usize } else { pcm.len().max(1) };
        let mut out = Vec::new();
        for (n, piece) in pcm.chunks(window.max(16_000)).enumerate() {
            let offset = (n * window.max(16_000)) as f64 / 16_000.0;
            let mut p = std::mem::MaybeUninit::<RunParams>::zeroed();
            let params = unsafe {
                (self.api.run_params_init)(p.as_mut_ptr());
                let mut params = p.assume_init();
                params.timestamps = TIMESTAMPS_WORD;
                params.language = language.as_ref().map_or(std::ptr::null(), |l| l.as_ptr());
                params
            };
            let s = unsafe { (self.api.run)(self.session, piece.as_ptr(), piece.len() as c_int, &params) };
            if s != 0 {
                return Err(AsrError::Parakeet(format!("transcribe.cpp: {}", text(unsafe { (self.api.status_string)(s) }))));
            }
            let n_words = unsafe { (self.api.n_words)(self.session) };
            if n_words <= 0 {
                // a family without word times gives its text as one stretch over the piece
                let whole = text(unsafe { (self.api.full_text)(self.session) });
                if !whole.trim().is_empty() {
                    out.push(Word { word: whole.trim().to_string(), start: offset, end: offset + piece.len() as f64 / 16_000.0 });
                }
                continue;
            }
            for i in 0..n_words {
                let mut w = std::mem::MaybeUninit::<RawWord>::zeroed();
                unsafe {
                    (self.api.word_init)(w.as_mut_ptr());
                    if (self.api.get_word)(self.session, i, w.as_mut_ptr()) != 0 {
                        continue;
                    }
                    let w = w.assume_init();
                    let word = text(w.text);
                    if word.trim().is_empty() {
                        continue;
                    }
                    out.push(Word { word: word.trim().to_string(), start: offset + w.t0_ms as f64 / 1000.0, end: offset + w.t1_ms as f64 / 1000.0 });
                }
            }
        }
        Ok(out)
    }
}

fn read_16k(wav: &Path) -> Result<Vec<f32>, AsrError> {
    let (samples, sr) = crate::wavio::read_mono_f32(wav).map_err(|e| AsrError::WavRead(wav.display().to_string(), e))?;
    Ok(if sr == 16_000 { samples } else { crate::listen_live::to_16k(&samples, sr) })
}

impl AsrEngine for TcppAsr {
    fn transcribe(&mut self, wav: &Path, lang: &str) -> Result<Vec<Segment>, AsrError> {
        let pcm = read_16k(wav)?;
        let words = self.words(&pcm, lang)?;
        Ok(segment_words(&words, SEG_MAX_GAP, SEG_MAX_DUR))
    }

    fn transcribe_turns(&mut self, wav: &Path, turns: &[Turn], lang: &str) -> Result<Vec<SpeakerSegment>, AsrError> {
        let pcm = read_16k(wav)?;
        let mut out = Vec::new();
        for t in turns {
            let a = (t.start * 16_000.0) as usize;
            let b = ((t.end * 16_000.0) as usize).min(pcm.len());
            if b <= a || b - a < 3_200 {
                continue;
            }
            let words = self.words(&pcm[a..b], lang)?;
            for s in segment_words(&words, SEG_MAX_GAP, SEG_MAX_DUR) {
                out.push(SpeakerSegment { start: t.start + s.start, end: t.start + s.end, text: s.text, speaker: t.speaker });
            }
        }
        Ok(out)
    }
}

/// A family's engine loaded on first use: the studio builds its recogniser before it knows whether it will run.
pub struct LazyTcpp {
    models_root: PathBuf,
    family: &'static Family,
    loaded: Option<TcppAsr>,
}

impl LazyTcpp {
    pub fn new(models_root: &Path, family: &'static Family) -> LazyTcpp {
        LazyTcpp { models_root: models_root.to_path_buf(), family, loaded: None }
    }

    fn engine(&mut self) -> Result<&mut TcppAsr, AsrError> {
        if self.loaded.is_none() {
            self.loaded = Some(TcppAsr::load(&self.models_root, self.family).map_err(AsrError::Parakeet)?);
        }
        Ok(self.loaded.as_mut().expect("loaded above"))
    }
}

impl AsrEngine for LazyTcpp {
    fn transcribe(&mut self, wav: &Path, lang: &str) -> Result<Vec<Segment>, AsrError> {
        self.engine()?.transcribe(wav, lang)
    }

    fn transcribe_turns(&mut self, wav: &Path, turns: &[Turn], lang: &str) -> Result<Vec<SpeakerSegment>, AsrError> {
        self.engine()?.transcribe_turns(wav, turns, lang)
    }
}
