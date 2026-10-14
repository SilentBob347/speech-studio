//! Qwen3-TTS CustomVoice through qwentts.cpp (ServeurpersoCom, ggml on CUDA) loaded at run time: its public C ABI
//! `qwen.h` (struct ABI 6) — nine named speakers, a style instruction, streaming chunks and cooperative cancel.

use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use libloading::Library;

pub const ENGINE: &str = "qwen3-tts";
/// The folder of the weights and the library inside the models root.
pub const DIR: &str = "qwen3-tts";
pub const CODEC: &str = "qwen-tokenizer-12hz-Q8_0.gguf";
/// Talker quantisations, the first the default (Q4_K_M, as GGUF models go here): the CustomVoice talker speaks the
/// named speakers, the Base talker clones a reference.
pub const QUANTS: &[(&str, &str, &str)] = &[
    ("Q4_K_M", "qwen-talker-1.7b-customvoice-Q4_K_M.gguf", "qwen-talker-1.7b-base-Q4_K_M.gguf"),
    ("Q8_0", "qwen-talker-1.7b-customvoice-Q8_0.gguf", "qwen-talker-1.7b-base-Q8_0.gguf"),
];
#[cfg(windows)]
pub const LIBRARY: &str = "qwen.dll";
#[cfg(not(windows))]
pub const LIBRARY: &str = "libqwen.so";
/// The speakers of the CustomVoice checkpoints with the native language the model card gives them.
pub const SPEAKERS: &[(&str, &str, &str, &str)] = &[
    ("vivian", "Vivian", "female", "zh"),
    ("serena", "Serena", "female", "zh"),
    ("uncle_fu", "Uncle Fu", "male", "zh"),
    ("dylan", "Dylan", "male", "zh"),
    ("eric", "Eric", "male", "zh"),
    ("ryan", "Ryan", "male", "en"),
    ("aiden", "Aiden", "male", "en"),
    ("ono_anna", "Ono Anna", "female", "ja"),
    ("sohee", "Sohee", "female", "ko"),
];
pub const LANGS: &[&str] = &["ru", "en", "zh", "ja", "ko", "de", "fr", "es", "pt", "it"];
pub const SAMPLE_RATE: u32 = 24_000;
const ABI_VERSION: i32 = 6;
const STATUS_CANCELLED: i32 = -5;

#[repr(C)]
struct InitParams {
    abi_version: i32,
    talker_path: *const c_char,
    codec_path: *const c_char,
    use_fa: bool,
    clamp_fp16: bool,
    max_batch: i32,
    codec_chunk_sec: f32,
    device: *const c_char,
}

type ChunkFn = unsafe extern "C" fn(*const f32, i32, *mut c_void) -> bool;
type CancelFn = unsafe extern "C" fn(*mut c_void) -> bool;

#[repr(C)]
struct TtsParams {
    abi_version: i32,
    text: *const c_char,
    lang: *const c_char,
    instruct: *const c_char,
    speaker: *const c_char,
    ref_audio_24k: *const f32,
    ref_n_samples: i32,
    ref_text: *const c_char,
    seed: i64,
    max_new_tokens: i32,
    temperature: f32,
    top_k: i32,
    top_p: f32,
    repetition_penalty: f32,
    subtalker_temperature: f32,
    subtalker_top_k: i32,
    subtalker_top_p: f32,
    dump_dir: *const c_char,
    cancel: Option<CancelFn>,
    cancel_user_data: *mut c_void,
    on_chunk: Option<ChunkFn>,
    on_chunk_user_data: *mut c_void,
    ref_spk_emb: *const f32,
    ref_spk_dim: i32,
    ref_codes: *const i32,
    ref_t: i32,
}

#[repr(C)]
struct Audio {
    samples: *mut f32,
    n_samples: i32,
    sample_rate: i32,
    channels: i32,
}

#[repr(C)]
struct RawContext {
    _private: [u8; 0],
}

#[derive(Clone, Copy)]
struct Api {
    version: unsafe extern "C" fn() -> *const c_char,
    last_error: unsafe extern "C" fn() -> *const c_char,
    init_default: unsafe extern "C" fn(*mut InitParams),
    init: unsafe extern "C" fn(*const InitParams) -> *mut RawContext,
    free: unsafe extern "C" fn(*mut RawContext),
    tts_default: unsafe extern "C" fn(*mut TtsParams),
    synthesize: unsafe extern "C" fn(*mut RawContext, *const TtsParams, *mut Audio) -> i32,
    audio_free: unsafe extern "C" fn(*mut Audio),
    model_type: unsafe extern "C" fn(*const RawContext) -> *const c_char,
}

/// The library, loaded once and kept for the life of the process: ggml's static initialiser refuses a second load
/// at the address it had (its terminate handler would already be the one it installs), so freeing the library and
/// loading it again aborts the process.
static LOADED: std::sync::Mutex<Option<(Library, Api)>> = std::sync::Mutex::new(None);

fn library(dir: &Path) -> Result<Api, String> {
    let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, api)) = loaded.as_ref() {
        return Ok(*api);
    }
    let lib = unsafe { Library::new(dir.join(LIBRARY)) }.map_err(|e| format!("{}: {e}", dir.join(LIBRARY).display()))?;
    let api = unsafe {
        macro_rules! sym {
            ($name:literal) => {
                *lib.get(concat!($name, "\0").as_bytes()).map_err(|e| format!("qwen3-tts {}: {e}", $name))?
            };
        }
        Api {
            version: sym!("qt_version"),
            last_error: sym!("qt_last_error"),
            init_default: sym!("qt_init_default_params"),
            init: sym!("qt_init"),
            free: sym!("qt_free"),
            tts_default: sym!("qt_tts_default_params"),
            synthesize: sym!("qt_synthesize"),
            audio_free: sym!("qt_audio_free"),
            model_type: sym!("qt_model_type"),
        }
    };
    *loaded = Some((lib, api));
    Ok(api)
}

/// A loaded talker and codec on the GPU; freed on drop. One synthesis at a time is the caller's to keep.
pub struct Engine {
    api: Api,
    raw: *mut RawContext,
    stop: AtomicBool,
}

// qt_synthesize is thread safe and the handle is only freed on drop.
unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

/// What one phrase is said with: a named speaker (CustomVoice talker) or a reference to clone (Base talker) — mono
/// 24 kHz audio, with its transcript for the in-context mode or without it for the timbre alone.
pub struct Say<'a> {
    pub text: &'a str,
    pub lang: &'a str,
    pub speaker: Option<&'a str>,
    pub reference: Option<(&'a [f32], Option<&'a str>)>,
    pub instruct: Option<&'a str>,
    pub seed: u64,
    pub temperature: f32,
    pub top_k: i32,
    pub top_p: f32,
    pub max_new_tokens: i32,
}

pub fn dir(models_root: &Path) -> PathBuf {
    models_root.join(DIR)
}

/// Whether the cloning talker reads the U+0301 stress marks: only a Base trained to follow them does (the stock one
/// misreads the mark); none of the shipped talkers is such yet.
pub fn follows_stress(_models_root: &Path) -> bool {
    false
}

pub fn talker(models_root: &Path, quant: &str, clone: bool) -> Option<PathBuf> {
    QUANTS.iter().find(|(q, _, _)| *q == quant).map(|(_, custom, base)| dir(models_root).join(if clone { base } else { custom }))
}

/// The library, the codec and the talker of `quant` for named speakers (`clone` false) or for cloning.
pub fn installed(models_root: &Path, quant: &str, clone: bool) -> bool {
    let d = dir(models_root);
    d.join(LIBRARY).is_file() && d.join(CODEC).is_file() && talker(models_root, quant, clone).is_some_and(|t| t.is_file())
}

fn cstring(s: &str, what: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| format!("qwen3-tts: {what} contains NUL"))
}

/// The upstream language name the ABI reads; `None` lets the model infer it.
fn lang_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "ru" => "russian",
        "en" => "english",
        "zh" => "chinese",
        "ja" => "japanese",
        "ko" => "korean",
        "de" => "german",
        "fr" => "french",
        "es" => "spanish",
        "pt" => "portuguese",
        "it" => "italian",
        _ => return None,
    })
}

unsafe extern "C" fn chunk_trampoline(samples: *const f32, n: i32, user: *mut c_void) -> bool {
    if !samples.is_null() && n > 0 && !user.is_null() {
        let sink = &*(user as *const &(dyn Fn(&[f32]) + Sync));
        sink(std::slice::from_raw_parts(samples, n as usize));
    }
    true
}

unsafe extern "C" fn cancel_trampoline(user: *mut c_void) -> bool {
    !user.is_null() && (*(user as *const AtomicBool)).load(Ordering::Relaxed)
}

impl Engine {
    /// Loads the library (its CUDA libraries are found on PATH) and the talker of `quant`: Base when `clone`, else
    /// CustomVoice.
    pub fn load(models_root: &Path, quant: &str, clone: bool) -> Result<Engine, String> {
        let d = dir(models_root);
        // its ggml libraries sit beside it under names other engines' libraries could share: this folder comes first
        let path = std::env::var("PATH").unwrap_or_default();
        if !std::env::split_paths(&path).any(|p| p == d) {
            let joined = std::env::join_paths(std::iter::once(d.clone()).chain(std::env::split_paths(&path))).map_err(|e| e.to_string())?;
            std::env::set_var("PATH", joined);
        }
        let api = library(&d)?;
        let talker = talker(models_root, quant, clone).ok_or_else(|| format!("qwen3-tts: no quantisation {quant}"))?;
        let (t, c) = (cstring(&talker.to_string_lossy(), "talker path")?, cstring(&d.join(CODEC).to_string_lossy(), "codec path")?);
        let mut p: InitParams = unsafe { std::mem::zeroed() };
        unsafe { (api.init_default)(&mut p) };
        p.abi_version = ABI_VERSION;
        p.talker_path = t.as_ptr();
        p.codec_path = c.as_ptr();
        let raw = unsafe { (api.init)(&p) };
        if raw.is_null() {
            return Err(format!("qwen3-tts: {}", unsafe { last_error(&api) }));
        }
        let engine = Engine { api, raw, stop: AtomicBool::new(false) };
        let kind = unsafe { CStr::from_ptr((engine.api.model_type)(engine.raw)) }.to_string_lossy().into_owned();
        let expected = if clone { "base" } else { "custom_voice" };
        if kind != expected {
            return Err(format!("qwen3-tts: {} is a {kind} checkpoint, not {expected}", talker.display()));
        }
        Ok(engine)
    }

    pub fn version(&self) -> String {
        unsafe { CStr::from_ptr((self.api.version)()) }.to_string_lossy().into_owned()
    }

    /// Stops the running synthesis within a frame (~83 ms).
    pub fn cancel(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Speaks one phrase; `on_chunk` (when given) gets the audio as it is decoded and the answer holds all of it.
    pub fn speak(&self, say: &Say, on_chunk: Option<&(dyn Fn(&[f32]) + Sync)>) -> Result<Vec<f32>, String> {
        if let Some(s) = say.speaker.filter(|s| !SPEAKERS.iter().any(|k| k.0 == *s)) {
            return Err(format!("qwen3-tts: no speaker {s}"));
        }
        let text = cstring(say.text, "text")?;
        let speaker = say.speaker.map(|s| cstring(s, "speaker")).transpose()?;
        let ref_text = say.reference.and_then(|(_, t)| t).filter(|t| !t.trim().is_empty()).map(|t| cstring(t, "reference text")).transpose()?;
        let lang = lang_name(say.lang).map(|l| cstring(l, "lang")).transpose()?;
        let instruct = say.instruct.filter(|i| !i.trim().is_empty()).map(|i| cstring(i, "instruct")).transpose()?;
        let mut p: TtsParams = unsafe { std::mem::zeroed() };
        unsafe { (self.api.tts_default)(&mut p) };
        p.abi_version = ABI_VERSION;
        p.text = text.as_ptr();
        p.speaker = speaker.as_ref().map_or(std::ptr::null(), |s| s.as_ptr());
        if let Some((audio, _)) = say.reference {
            p.ref_audio_24k = audio.as_ptr();
            p.ref_n_samples = audio.len() as i32;
            p.ref_text = ref_text.as_ref().map_or(std::ptr::null(), |t| t.as_ptr());
        }
        p.lang = lang.as_ref().map_or(std::ptr::null(), |l| l.as_ptr());
        p.instruct = instruct.as_ref().map_or(std::ptr::null(), |i| i.as_ptr());
        p.seed = (say.seed & i64::MAX as u64) as i64;
        p.temperature = say.temperature;
        p.top_k = say.top_k;
        p.top_p = say.top_p;
        if say.max_new_tokens > 0 {
            p.max_new_tokens = say.max_new_tokens;
        }
        self.stop.store(false, Ordering::Relaxed);
        p.cancel = Some(cancel_trampoline);
        p.cancel_user_data = &self.stop as *const AtomicBool as *mut c_void;
        // the library streams to its callback and leaves `out` empty, so the whole take is gathered here
        let gathered = std::sync::Mutex::new(Vec::<f32>::new());
        let sink = |a: &[f32]| {
            gathered.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(a);
            if let Some(f) = on_chunk {
                f(a);
            }
        };
        let sink_ref: &(dyn Fn(&[f32]) + Sync) = &sink;
        p.on_chunk = Some(chunk_trampoline);
        p.on_chunk_user_data = &sink_ref as *const &(dyn Fn(&[f32]) + Sync) as *mut c_void;
        let mut out = Audio { samples: std::ptr::null_mut(), n_samples: 0, sample_rate: 0, channels: 0 };
        let status = unsafe { (self.api.synthesize)(self.raw, &p, &mut out) };
        unsafe { (self.api.audio_free)(&mut out) };
        self.stop.store(false, Ordering::Relaxed);
        match status {
            0 => Ok(gathered.into_inner().unwrap_or_else(|e| e.into_inner())),
            STATUS_CANCELLED => Err(crate::jobs::CANCELLED.to_string()),
            _ => Err(format!("qwen3-tts: {}", unsafe { last_error(&self.api) })),
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe { (self.api.free)(self.raw) };
    }
}

unsafe fn last_error(api: &Api) -> String {
    let p = (api.last_error)();
    if p.is_null() { "unknown error".into() } else { CStr::from_ptr(p).to_string_lossy().into_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_map_to_the_names_the_library_reads() {
        assert_eq!(lang_name("ru"), Some("russian"));
        assert_eq!(lang_name("auto"), None);
        assert!(LANGS.iter().all(|l| lang_name(l).is_some()));
    }

    /// The real library and weights: `QWEN_ROOT` is a models root holding `qwen3-tts/` (qwen.dll, the codec and the
    /// talker); the CUDA libraries are on PATH.
    #[test]
    #[ignore]
    fn speaks_russian_on_the_gpu_and_streams() {
        let root = PathBuf::from(std::env::var("QWEN_ROOT").expect("QWEN_ROOT"));
        let quant = std::env::var("QWEN_QUANT").unwrap_or_else(|_| "Q4_K_M".into());
        let e = Engine::load(&root, &quant, false).unwrap();
        eprintln!("qwentts {}", e.version());
        let say = Say { text: "Сегодня в восемь вечера мы встретимся у старого моста.", lang: "ru", speaker: Some("ryan"), reference: None, instruct: None, seed: 42, temperature: 0.7, top_k: 50, top_p: 0.9, max_new_tokens: 0 };
        let chunks = std::sync::atomic::AtomicUsize::new(0);
        let first = std::sync::Mutex::new(None);
        let started = std::time::Instant::now();
        let wav = e
            .speak(&say, Some(&|_: &[f32]| {
                chunks.fetch_add(1, Ordering::Relaxed);
                first.lock().unwrap().get_or_insert(started.elapsed());
            }))
            .unwrap();
        let secs = wav.len() as f64 / SAMPLE_RATE as f64;
        eprintln!("[OK] {secs:.2} s of audio in {:.3} s, first sound {:?}, {} chunks", started.elapsed().as_secs_f64(), first.lock().unwrap(), chunks.load(Ordering::Relaxed));
        assert!(secs > 1.5 && secs < 10.0, "{secs}");
        if let Ok(out) = std::env::var("QWEN_OUT") {
            crate::wavio::write_mono_f32(Path::new(&out), &wav, SAMPLE_RATE).unwrap();
        }
    }

    /// Cloning on the Base talker, after the CustomVoice one was loaded and freed in the same process: `QWEN_REF` is a
    /// mono 24 kHz WAV, `QWEN_REF_TEXT` its transcript.
    #[test]
    #[ignore]
    fn clones_a_reference_after_the_named_voices_were_freed() {
        let root = PathBuf::from(std::env::var("QWEN_ROOT").expect("QWEN_ROOT"));
        drop(Engine::load(&root, "Q4_K_M", false).unwrap());
        let e = Engine::load(&root, "Q4_K_M", true).unwrap();
        let (reference, sr) = crate::wavio::read_mono_f32(Path::new(&std::env::var("QWEN_REF").expect("QWEN_REF"))).unwrap();
        assert_eq!(sr, SAMPLE_RATE);
        let text = std::env::var("QWEN_REF_TEXT").ok();
        let say = Say { text: "Только после завтрака.", lang: "ru", speaker: None, reference: Some((&reference, text.as_deref())), instruct: None, seed: 7, temperature: 0.7, top_k: 50, top_p: 0.9, max_new_tokens: 0 };
        let wav = e.speak(&say, None).unwrap();
        eprintln!("[OK] clone {:.2} s", wav.len() as f64 / SAMPLE_RATE as f64);
        if let Ok(out) = std::env::var("QWEN_OUT") {
            crate::wavio::write_mono_f32(Path::new(&out), &wav, SAMPLE_RATE).unwrap();
        }
    }
}
