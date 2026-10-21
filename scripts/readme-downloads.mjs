// Checks (default) or rewrites (--write) the download tables of README.md and its five
// translations against manifest() of crates/speech-server/src/setup/mod.rs.
// A table sits between <!-- downloads:start --> and <!-- downloads:end -->.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const START = "<!-- downloads:start -->";
const END = "<!-- downloads:end -->";

const FILES = {
  en: "README.md",
  ru: "README.ru.md",
  zh: "README.zh.md",
  es: "README.es.md",
  fr: "README.fr.md",
  pt: "README.pt.md",
};

const TEXT = {
  en: {
    head: ["Component", "Need", "Files (direct links)", "Size", "Put it in"],
    need: { required: "required", recommended: "recommended", optional: "optional" },
    none: "as is, to the path after the arrow",
    zipflat: "unzip the files, without subfolders, into",
    ziptree: "unzip with its folder tree into",
    zippick: "take ffmpeg.exe and ffprobe.exe from the archive into",
    wheeldlls: "take every .dll of the archive into",
  },
  ru: {
    head: ["Компонент", "Нужен", "Файлы (прямые ссылки)", "Размер", "Куда положить"],
    need: { required: "обязателен", recommended: "рекомендуется", optional: "по желанию" },
    none: "как есть, по пути после стрелки",
    zipflat: "распаковать файлы без подпапок в",
    ziptree: "распаковать с деревом папок в",
    zippick: "взять ffmpeg.exe и ffprobe.exe из архива в",
    wheeldlls: "взять все .dll из архива в",
  },
  zh: {
    head: ["组件", "必要性", "文件（直接链接）", "大小", "存放位置"],
    need: { required: "必需", recommended: "推荐", optional: "可选" },
    none: "原样放入，路径见箭头后",
    zipflat: "解压文件（不含子文件夹）到",
    ziptree: "保留文件夹结构解压到",
    zippick: "从压缩包中取出 ffmpeg.exe 和 ffprobe.exe 放入",
    wheeldlls: "取出压缩包中所有 .dll 放入",
  },
  es: {
    head: ["Componente", "Necesidad", "Archivos (enlaces directos)", "Tamaño", "Dónde ponerlo"],
    need: { required: "obligatorio", recommended: "recomendado", optional: "opcional" },
    none: "tal cual, en la ruta tras la flecha",
    zipflat: "descomprimir los archivos, sin subcarpetas, en",
    ziptree: "descomprimir con su árbol de carpetas en",
    zippick: "tomar ffmpeg.exe y ffprobe.exe del archivo y ponerlos en",
    wheeldlls: "tomar todos los .dll del archivo y ponerlos en",
  },
  fr: {
    head: ["Composant", "Besoin", "Fichiers (liens directs)", "Taille", "Où le placer"],
    need: { required: "obligatoire", recommended: "recommandé", optional: "facultatif" },
    none: "tel quel, au chemin indiqué après la flèche",
    zipflat: "décompresser les fichiers, sans sous-dossiers, dans",
    ziptree: "décompresser avec son arborescence dans",
    zippick: "prendre ffmpeg.exe et ffprobe.exe de l'archive et les mettre dans",
    wheeldlls: "prendre tous les .dll de l'archive et les mettre dans",
  },
  pt: {
    head: ["Componente", "Necessidade", "Arquivos (links diretos)", "Tamanho", "Onde colocar"],
    need: { required: "obrigatório", recommended: "recomendado", optional: "opcional" },
    none: "como está, no caminho após a seta",
    zipflat: "descompactar os arquivos, sem subpastas, em",
    ziptree: "descompactar com a árvore de pastas em",
    zippick: "pegar ffmpeg.exe e ffprobe.exe do arquivo e colocar em",
    wheeldlls: "pegar todos os .dll do arquivo e colocar em",
  },
};

const CASTING = {
  en: "Casting models (faces and voice)",
  ru: "Модели кастинга (лица и голос)",
  zh: "选角模型（人脸与声音）",
  es: "Modelos de casting (caras y voz)",
  fr: "Modèles de casting (visages et voix)",
  pt: "Modelos de casting (rostos e voz)",
};

const NAMES = {
  higgs: "Higgs Audio v3 Q8_0",
  "higgs-engine": "audiocpp_engine.dll (Higgs engine)",
  gemma: "Gemma-4 12B QAT q4_0 + vision",
  "gemma-q5_0": "Gemma-4 12B Q5_K_M + vision",
  "gemma-q6_k": "Gemma-4 12B Q6_K + vision",
  "gemma-q8_0": "Gemma-4 12B Q8_0 + vision",
  parakeet: "Parakeet-TDT 0.6B v3 Q8_0 (transcribe.cpp)",
  "higgs-q6_k": "Higgs Audio v3 Q6_K",
  "higgs-q4_k_m": "Higgs Audio v3 Q4_K_M",
  "parakeet-fp32": "Parakeet-TDT 0.6B v3 fp32",
  "parakeet-ultra": "Parakeet Ultra 0.6B Q8_0 (Moondream, transcribe.cpp)",
  "whisper-engine": "Whisper-Faster (faster-whisper standalone)",
  "whisper-cuda": "Whisper CUDA (cuBLAS 11, cuDNN 8)",
  "whisper-tiny": "Whisper tiny",
  "whisper-base": "Whisper base",
  "whisper-small": "Whisper small",
  "whisper-medium": "Whisper medium",
  "whisper-large-v3": "Whisper large-v3",
  "whisper-large-v3-turbo": "Whisper large-v3-turbo",
  sortformer: "Nemotron 3 Diarization",
  roformer: "Mel-Band Roformer voc_fv6 Q8_0",
  "roformer-q5": "Mel-Band Roformer voc_fv6 Q5_0",
  "roformer-q4": "Mel-Band Roformer voc_fv6 Q4_0",
  casting: CASTING,
  "bsroformer-engine": "BSRoformer.cpp (CUDA)",
  "bsroformer-engine-cpu": "BSRoformer.cpp (CPU)",
  llama: "llama.cpp server (CUDA)",
  onnxruntime: "ONNX Runtime",
  "onnxruntime-gpu": "ONNX Runtime GPU (CUDA)",
  ffmpeg: "FFmpeg (static build)",
  ytdlp: "yt-dlp + deno",
  "cuda-runtime": "CUDA runtime (cudart, cuBLAS, cuFFT)",
  cudnn: "cuDNN 9",
  drama3: "Fish Audio Drama 3 Q4 + drama_engine (6 GB+ GPU, 32 GB RAM under 24 GB)",
  supertonic3: "Supertonic 3 int8",
  "qwen3-tts-engine": "Qwen3-TTS engine (qwen.dll) and codec",
  "qwen3-tts": "Qwen3-TTS 1.7B Q4_K_M (CustomVoice + Base)",
  "qwen3-tts-q8_0": "Qwen3-TTS 1.7B Q8_0 (CustomVoice + Base)",
  "qwen3-tts-ru-stress": "Qwen3-TTS Russian stress LoRA",
  "qwen3-tts-design": "Qwen3-TTS VoiceDesign Q4_K_M",
  "qwen3-tts-design-q8_0": "Qwen3-TTS VoiceDesign Q8_0",
  ruaccent: "RUAccent (stress, «ё», homographs)",
  "ruaccent-omograph-big": "RUAccent big_poetry homographs int8",
  "transcribe-engine": "transcribe.cpp engine (CUDA)",
  "transcribe-engine-cpu": "transcribe.cpp engine (CPU)",
  "asr-gguf-nemotron": "Nemotron 3.5 ASR 0.6B Q8_0 (NVIDIA)",
  wespeaker: "WeSpeaker ResNet34-LM (voice similarity)",
  "silero-vad": "Silero VAD v6.2",
  langid: "VoxLingua107 language identification Q8_0",
  dpdfnet: "DPDFNet 48 kHz (noise suppression)",
};

function windowsModule(src) {
  return src.slice(src.indexOf("#[cfg(windows)]\nmod platform"), src.indexOf("#[cfg(not(windows))]\nmod platform"));
}

const SPEC = /FileSpec \{ url: (?:"([^"]+)"|(\w+)), dest_rel: "([^"]+)", size: ([\d_]+),(?: sha256: "[0-9a-f]{64}",)? extract: Extract::(\w+) \}/;

// The files of one component: inline FileSpec literals, FileSpec constants of setup/mod.rs (the Windows platform module
// first), constant arrays and the files of a Windows `Parts`.
function filesOf(src, expr, consts) {
  const windows = windowsModule(src);
  const named = (name) => {
    for (const scope of [windows, src]) {
      const one = scope.match(new RegExp(`const ${name}: FileSpec = (FileSpec \\{[^\\n]*\\});`))?.[1];
      if (one) return [one];
      const at = scope.indexOf(`const ${name}: &[FileSpec] = &[`);
      if (at >= 0) return elements(scope.slice(at + `const ${name}: &[FileSpec] = &[`.length, scope.indexOf("\n];", at)));
      const parts = scope.indexOf(`const ${name}: Parts = Parts {`);
      if (parts >= 0) return elements(scope.slice(parts, scope.indexOf("\n    };", parts)).match(/files: &\[([\s\S]*?)\],\n/)?.[1] ?? "");
    }
    throw new Error(`FileSpec ${name} not found in setup/mod.rs`);
  };
  const elements = (list) => {
    const out = [];
    for (const m of list.replace(/^\s*\/\/[^\n]*/gm, "").matchAll(/FileSpec \{[^\n]*?\}|(?:platform::)?([A-Z][A-Z0-9_]+)/g)) {
      out.push(...(m[0].startsWith("FileSpec") ? [m[0]] : named(m[1])));
    }
    return out;
  };
  const parts = expr.match(/^platform::(\w+)\.files$/)?.[1];
  const listed = parts ?? expr.match(/^(?:platform::)?([A-Z][A-Z0-9_]+)$/)?.[1];
  const specs = listed ? named(listed) : elements(expr.replace(/^&\[/, "").replace(/\]$/, ""));
  return specs.map((spec) => {
    const f = spec.match(SPEC);
    if (!f) throw new Error("cannot parse " + spec.slice(0, 120));
    const url = f[1] ?? consts[f[2]];
    if (!url) throw new Error(`unknown URL constant ${f[2]}`);
    const extract = { None: "none", Flat: "zipflat", Pick: "zippick", Tree: "ziptree", Libs: "wheeldlls" }[f[5]];
    if (!extract) throw new Error(`unknown Extract::${f[5]}`);
    return { url, destRel: f[3], size: Number(f[4].replace(/_/g, "")), extract };
  });
}

function readManifest() {
  const src = readFileSync(join(root, "crates/speech-server/src/setup/mod.rs"), "utf8").replace(/\r\n/g, "\n");
  const consts = {};
  for (const m of src.matchAll(/const (\w+): &str =\s*"([^"]+)";/g)) consts[m[1]] = m[2];
  for (const m of windowsModule(src).matchAll(/const (\w+): &str =\s*"([^"]+)";/g)) consts[m[1]] = m[2];
  const at = src.indexOf("pub fn manifest()");
  if (at < 0) throw new Error("manifest() not found in setup/mod.rs");
  const body = src.slice(at, src.indexOf("\n}\n", at));
  return body.split(/\n {8}Component \{/).slice(1).map((p) => {
    const id = p.match(/id: "([^"]+)"/)?.[1];
    const requirement = p.match(/requirement: Requirement::(\w+)/)?.[1]?.toLowerCase();
    const delivery = p.match(/delivery: Delivery::(\w+)/)?.[1];
    if (!id || !requirement || !delivery) throw new Error("cannot parse a manifest component: " + p.slice(0, 120));
    const expr = p.match(/\n\s*files: ([\s\S]*?),\n\s*markers:/)?.[1]?.trim() ?? "&[]";
    const files = filesOf(src, expr, consts);
    return { id, requirement, delivery, size: files.reduce((s, f) => s + f.size, 0), files };
  });
}

function human(bytes) {
  if (bytes >= 1e9) return (bytes / 1e9).toFixed(1) + " GB";
  if (bytes >= 1e6) return Math.round(bytes / 1e6) + " MB";
  return Math.max(1, Math.round(bytes / 1e3)) + " KB";
}

function winPath(rel) {
  return rel.replace(/\//g, "\\");
}

function folderOf(rel) {
  const i = rel.lastIndexOf("/");
  return (i < 0 ? "" : rel.slice(0, i + 1)).replace(/\//g, "\\");
}

function table(manifest, lang) {
  const T = TEXT[lang];
  const rows = [`| ${T.head.join(" | ")} |`, `|${T.head.map(() => "---").join("|")}|`];
  for (const c of manifest) {
    if (c.delivery !== "Download") continue;
    const name = NAMES[c.id];
    if (!name) throw new Error(`component "${c.id}" has no name in scripts/readme-downloads.mjs`);
    const label = typeof name === "string" ? name : name[lang];
    const links = c.files.map((f) => `[${f.url.split("/").pop()}](${f.url})${f.extract === "none" ? ` → \`${winPath(f.destRel)}\`` : ""}`).join("<br>");
    const places = [...new Set(c.files.map((f) => (f.extract === "none" ? T.none : `${T[f.extract]} \`${folderOf(f.destRel)}\``)))];
    rows.push(`| ${label} | ${T.need[c.requirement]} | ${links} | ${human(c.size)} | ${places.join("<br>")} |`);
  }
  return rows.join("\n");
}

function blockOf(text) {
  const a = text.indexOf(START);
  const b = text.indexOf(END);
  if (a < 0 || b < a) return null;
  return { a: a + START.length, b };
}

const manifest = readManifest();
const write = process.argv.includes("--write");
const urlsOf = (s) => new Set([...s.matchAll(/\]\((https?:\/\/[^)]+)\)/g)].map((m) => m[1]));
const wanted = new Set(manifest.flatMap((c) => (c.delivery === "Download" ? c.files.map((f) => f.url) : [])));
let failed = false;

for (const [lang, file] of Object.entries(FILES)) {
  const path = join(root, file);
  const raw = readFileSync(path, "utf8");
  const eol = raw.includes("\r\n") ? "\r\n" : "\n";
  const text = raw.replace(/\r\n/g, "\n");
  const blk = blockOf(text);
  if (!blk) {
    console.error(`[ERROR] ${file}: no ${START} ... ${END} block`);
    failed = true;
    continue;
  }
  const fresh = "\n" + table(manifest, lang) + "\n";
  if (write) {
    writeFileSync(path, (text.slice(0, blk.a) + fresh + text.slice(blk.b)).replace(/\n/g, eol));
    console.log(`[OK] ${file}: table written`);
    continue;
  }
  const have = text.slice(blk.a, blk.b);
  for (const c of manifest) {
    if (c.delivery !== "Download") continue;
    for (const f of c.files) {
      if (f.extract === "none" && !have.includes(`${f.url.split("/").pop()}](${f.url}) → \`${winPath(f.destRel)}\``)) {
        console.error(`[ERROR] ${file}: no destination ${winPath(f.destRel)} next to ${f.url}`);
        failed = true;
      }
    }
  }
  const present = urlsOf(have);
  const missing = [...wanted].filter((u) => !present.has(u));
  const extra = [...present].filter((u) => !wanted.has(u));
  if (missing.length || extra.length || have !== fresh) {
    console.error(`[ERROR] ${file}: the table differs from setup/mod.rs manifest()`);
    for (const u of missing) console.error(`  missing in README: ${u}`);
    for (const u of extra) console.error(`  not in manifest: ${u}`);
    if (!missing.length && !extra.length) console.error("  links match, sizes, names or folders differ; run with --write");
    failed = true;
  } else {
    console.log(`[OK] ${file}: ${wanted.size} links match the manifest`);
  }
}
process.exit(failed ? 1 : 0);
