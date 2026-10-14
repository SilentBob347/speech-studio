import { BASE, coded, postCoded, sendCoded, getCoded } from "./api";

export type Take = {
  n: number; file: string; duration: number; seed: number; temperature: number; key: string; created: number;
  source: string; synth_secs: number; retried: string[];
  qc: QcCheck | null; text: string; direction: string; fit_stretch: number; fit_lead: number; fit_over: boolean;
};
export type QcCheck = { wer: number; words: number; inserted: number; deleted: number; substituted: number; heard: string; passed: boolean };
export type QcSettings = { asr_check: boolean; strictness: "strict" | "normal" | "lenient"; takes: number };
export type Block = {
  id: string; speaker: string; voice: string | null; text: string; direction: string; pause_ms: number | null;
  hidden: boolean; takes: Take[]; active: number | null; pinned: number | null; error: { key: string; text: string } | null;
  note: string; marker: Marker | ""; start: number | null; end: number | null;
};
export type Marker = "red" | "amber" | "green" | "blue" | "violet";
export const MARKERS: Record<Marker, string> = { red: "#ff6b6b", amber: "#ffc46b", green: "#7ee0c3", blue: "#4cc2ff", violet: "#b8a4ff" };
export type Chapter = { id: string; title: string; blocks: Block[] };
export type SeedMode = "fixed" | "random" | "inc" | "dec";
export type SpeechParams = { temperature: number; top_k: number; top_p: number; seed_mode: SeedMode; seed: number; max_tokens: number };
export type ParamSpec = { key: "temperature" | "top_k" | "top_p" | "max_tokens"; kind: "float" | "int"; min: number; max: number; step: number; default: number };
export type EngineCaps = {
  id: string; name: string; languages: string[]; params: ParamSpec[]; seed_modes: SeedMode[]; tag_format: string;
  clone: string; continuation: boolean; streaming: boolean; deterministic_seed: boolean; max_chars: number;
  reads: { stress: boolean; yo: boolean; phonemes: boolean; accent: boolean };
};
export type Loudness = "off" | "video" | "podcast" | "acx";
export type SpeechSettings = {
  lang: string; engine: string; params: SpeechParams; voice: string | null;
  pause_same_ms: number; pause_switch_ms: number; loudness: Loudness; qc: QcSettings; auto_stress: boolean; normalize: boolean;
  timeline: boolean; max_stretch: number; lead_into_silence: boolean;
};
export type DictKind = "word" | "regex" | "phonemes";
export type DictEntry = {
  id: string; enabled: boolean; kind: DictKind; find: string; say: string; case_sensitive: boolean; whole_word: boolean;
  lang: string; voice: string; engine: string; note: string; source: string;
};
export type PrepLayer = "dictionary" | "normalizer" | "stress" | "engine";
export type PrepStep = { layer: PrepLayer; rule: string; from: string; to: string };
export type Prepared = {
  text: string; engine_text: string; steps: PrepStep[]; unknown: string[];
  ignored: { accent: boolean; phonemes: boolean; stress: boolean; unknown_tags: string[] };
  ignored_phonemes: string[]; broken: [string, string][];
};
export type UnknownWord = { word: string; count: number; why: "unread" | "no_stress" };
export type BlockState = "pending" | "ready" | "stale" | "failed" | "skipped";
export type SpeechProject = {
  kind: "speech"; version: number; name: string; created: number; settings: SpeechSettings;
  cast: Record<string, string>; chapters: Chapter[]; dictionary: DictEntry[];
  status?: Record<string, { state: BlockState; key: string; fit?: "fits" | "tight" | "impossible" }>;
};
export type ProjectCard = { pid: string; kind: string; name: string; mtime: number; created?: number; blocks?: number; ready?: number };
export type ExportFormat = "wav" | "flac" | "mp3" | "ogg" | "m4a";
export const EXPORT_FORMATS: ExportFormat[] = ["wav", "flac", "mp3", "ogg", "m4a"];
export type StressWord = {
  start: number; end: number; word: string; source: "manual" | "dictionary" | "model" | "omograph" | "yo";
  variants?: [string, number][]; doubt: boolean; yo_choice?: string;
};
export type StressResult = { text: string; words: StressWord[]; russian: boolean };
export type ExportProblem = { block: string; state: BlockState | "missing" };

export const speechApi = {
  list: () => getCoded<{ projects: ProjectCard[] }>("/projects"),
  create: (body: { name?: string; text: string; voice?: string | null; lang?: string }) => postCoded<{ project_id: string; project: SpeechProject }>("/projects", body),
  get: (pid: string) => getCoded<SpeechProject>(`/projects/${pid}`),
  put: (pid: string, project: SpeechProject) => {
    const { status: _status, ...body } = project;
    void _status;
    return sendCoded<SpeechProject>("PUT", `/projects/${pid}`, body);
  },
  remove: (pid: string) => sendCoded<{ deleted: string }>("DELETE", `/projects/${pid}`),
  synth: (pid: string, scope: { ids?: string[]; from?: string; force?: boolean }) => postCoded<{ job_id: string }>(`/projects/${pid}/synth`, scope),
  exportCheck: (pid: string) => getCoded<{ problems: ExportProblem[] }>(`/projects/${pid}/export-check`),
  export: (pid: string, format: ExportFormat, chapter?: string) => postCoded<{ job_id: string }>(`/projects/${pid}/export`, { format, chapter }),
  fromSubtitles: (file: File, name?: string) => {
    const form = new FormData();
    if (name) form.append("name", name);
    form.append("file", file, file.name);
    return fetch(`${BASE}/projects/from-subtitles`, { method: "POST", body: form }).then(coded<{ project_id: string; project: SpeechProject }>);
  },
  pack: (pid: string, audio: boolean) => postCoded<{ file: string; bytes: number }>(`/projects/${pid}/package`, { audio }),
  importPackage: (file: File) => {
    const form = new FormData();
    form.append("file", file, file.name);
    return fetch(`${BASE}/speechproj/import`, { method: "POST", body: form }).then(coded<{ project_id: string; voices_added: string[] }>);
  },
  engines: () => getCoded<{ engines: EngineCaps[] }>("/engines"),
  stress: (text: string, lang: string) => postCoded<StressResult>("/text/stress", { text, lang }),
  prepare: (text: string, pid?: string, block?: string) => postCoded<{ prepared: Prepared; stress_applied: boolean; stress_missing: boolean }>("/text/prepare", { text, pid, block }),
  dictionary: () => getCoded<{ entries: DictEntry[] }>("/dictionary"),
  saveDictionary: (entries: DictEntry[]) => sendCoded<{ entries: DictEntry[] }>("PUT", "/dictionary", { entries }),
  importDictionary: (file: File) => {
    const form = new FormData();
    form.append("file", file, file.name);
    return fetch(`${BASE}/dictionary/import`, { method: "POST", body: form }).then(coded<{ entries: DictEntry[]; source: string }>);
  },
  dictionaryCounts: (pid: string) => getCoded<{ counts: Record<string, number> }>(`/projects/${pid}/dictionary-counts`),
  unknownWords: (pid: string) => getCoded<{ words: UnknownWord[]; stress_checked: boolean }>(`/projects/${pid}/unknown-words`),
  takeUrl: (pid: string, file: string) => `${BASE}/projects/${pid}/takes/${encodeURIComponent(file)}`,
  exportUrl: (pid: string, file: string) => `${BASE}/projects/${pid}/exports/${encodeURIComponent(file)}`,
};

export const activeTake = (b: Block): Take | null => (b.active == null ? null : b.takes.find((t) => t.n === b.active) ?? null);

export const newId = () => crypto.randomUUID().replace(/-/g, "").slice(0, 10);

export const SPEAKER_TINTS = ["#4cc2ff", "#f7a8c8", "#7ee0c3", "#ffc46b", "#b8a4ff", "#ff8b6b", "#a5b8ff", "#9be36e"];

/** Words of a stress pass a person may want to choose: every homograph (the unsure ones first) and е/ё pairs. */
export const stressDoubts = (words: StressWord[]) =>
  words.filter((w) => (w.variants?.length ?? 0) > 1 || !!w.yo_choice).sort((a, b) => Number(b.doubt) - Number(a.doubt) || a.start - b.start);

/** `variant` (lower case, maybe with `+`) in the letter case of `word`. */
export function withCase(word: string, variant: string): string {
  const letters = Array.from(word).filter((c) => c !== "+");
  let i = 0;
  return Array.from(variant).map((c) => {
    if (c === "+") return c;
    const src = letters[i++];
    return src && src !== src.toLowerCase() ? c.toUpperCase() : c;
  }).join("");
}

/** Replace the characters `start..end` of `text` (counted in code points, as the server counts them). */
export function spliceChars(text: string, start: number, end: number, insert: string): string {
  const chars = Array.from(text);
  return [...chars.slice(0, start), ...Array.from(insert), ...chars.slice(end)].join("");
}

export const newEntry = (find = "", say = ""): DictEntry => ({
  id: newId(), enabled: true, kind: "word", find, say, case_sensitive: false, whole_word: true, lang: "", voice: "", engine: "", note: "", source: "manual",
});

/** Seconds as m:ss.s (or h:mm:ss.s) for cue times. */
export function cueTime(t: number): string {
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const sec = (t % 60).toFixed(1).padStart(4, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}
