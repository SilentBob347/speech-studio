import { BASE, coded, getCoded, postCoded, sendCoded } from "./api";

export type HallucinationFlag = "wordless" | "numeric" | "bracketed" | "credit" | "subtitle_word" | "shouted" | "known_phrase";
export type ListenWord = { text: string; start: number; end: number; cut: boolean };
export type ListenLine = {
  id: string; start: number; end: number; speaker: string; text: string; words: ListenWord[];
  approx: boolean; flag: HallucinationFlag | null; evidence: string | null; hidden: boolean;
};
export type ListenEngine = "" | "parakeet" | "nemotron" | "whisper" | "gguf-parakeet" | "gguf-nemotron" | "gguf-gigaam" | "gguf-cohere" | "gguf-whisper" | "openrouter" | "fal";
export type VadConfig = { threshold: number; hysteresis: number; min_speech_ms: number; min_silence_ms: number; pad_ms: number };
export type ListenSettings = {
  engine: ListenEngine; cloud_model: string; lang: string; speakers: number; separate: boolean; filter: boolean; prompt: string;
  denoise: boolean; denoise_db: number; vad: VadConfig;
};
/** Noise suppression limits in dB (0 removes as much as the model can); 18 keeps speech natural. */
export const DENOISE_LEVELS = [6, 12, 18, 24, 0] as const;
export const VAD_DEFAULTS: VadConfig = { threshold: 0.5, hysteresis: 0.15, min_speech_ms: 250, min_silence_ms: 100, pad_ms: 30 };
export type ListenFix = { heard: string; say: string };
export type ListenMade = { by: string; at: number; lang: string; separated: boolean; hidden: number };
export type ListenProject = {
  kind: "listening"; version: number; name: string; created: number; source: string; origin: string; duration: number;
  settings: ListenSettings; speakers: Record<string, string>; lines: ListenLine[]; fixes: ListenFix[]; reference: string; made: ListenMade | null;
};
export type ListenCard = { pid: string; kind: "listening"; name: string; mtime: number; created: number; duration: number; lines: number; speakers: number; transcribed: boolean };
export type TranscriptFormat = "srt" | "vtt" | "ass" | "txt" | "json";
export const TRANSCRIPT_FORMATS: TranscriptFormat[] = ["srt", "vtt", "ass", "txt", "json"];
export type WordOp = { op: "same" | "inserted" | "deleted" | "substituted"; expected: string | null; heard: string | null };
export type ReferenceCheck = { words: number; wer: number; ops: WordOp[] };

/** One word of the transcript in reading order: which line, which word, and its place in the flat list. */
export type Token = { line: number; word: number; index: number; text: string; start: number; end: number; cut: boolean };

export function tokensOf(lines: ListenLine[]): Token[] {
  const out: Token[] = [];
  lines.forEach((l, li) => {
    if (l.hidden) return;
    l.words.forEach((w, wi) => out.push({ line: li, word: wi, index: out.length, text: w.text, start: w.start, end: w.end, cut: w.cut }));
  });
  return out;
}

/** The player lands a little before the time it is sent to, so a word counts as started this much early. */
const SEEK_SLACK = 0.03;

/** The word sounding at `t`: the last word started by then, while it still sounds or until the next one starts. */
export function activeToken(tokens: Token[], at: number): number {
  const t = at + SEEK_SLACK;
  let lo = 0;
  let hi = tokens.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (tokens[mid].start <= t) { found = mid; lo = mid + 1; } else hi = mid - 1;
  }
  if (found < 0) return -1;
  const next = tokens[found + 1];
  return t < tokens[found].end || (next && t < next.start && t - tokens[found].end < 0.6) ? found : -1;
}

export function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** Who a recording's speaker is like by voice: a named speaker (profile) or a library voice, and whether it is the same person. */
export type SpeakerMatch = { speaker: string; name: string | null; best: { name: string; kind: "profile" | "voice"; score: number; same: boolean } | null };

export const listenApi = {
  upload: (file: File) => {
    const form = new FormData();
    form.append("file", file, file.name);
    return fetch(`${BASE}/listen/upload`, { method: "POST", body: form }).then(coded<{ pid: string; project: ListenProject }>);
  },
  /** With `live` the recording is recognised phrase by phrase while it records (recordLive), in `lang`. */
  recordStart: (device?: string, live = false, lang = "auto") => postCoded<{ pid: string }>("/listen/record/start", { device: device ?? "", live, lang }),
  recordLive: () => getCoded<{ running: boolean; lines: { start: number; end: number; text: string }[]; partial: { start: number; text: string } | null; error: string | null }>("/listen/record/live"),
  recordStop: () => postCoded<{ pid: string; project: ListenProject }>("/listen/record/stop", {}),
  get: (pid: string) => getCoded<ListenProject>(`/listen/${pid}`),
  put: (pid: string, project: ListenProject) => sendCoded<ListenProject>("PUT", `/listen/${pid}`, project),
  audioUrl: (pid: string) => `${BASE}/listen/${pid}/audio`,
  aligned: (pid: string, reference: string) => postCoded<{ lines: ListenLine[] }>(`/listen/${pid}/aligned`, { reference }),
  transcribe: (pid: string) => postCoded<{ job_id: string }>(`/listen/${pid}/transcribe`, {}),
  cut: (pid: string) => postCoded<{ job_id: string }>(`/listen/${pid}/cut`, {}),
  /** With `rules` (a broadcaster's, from subtitleRules) subtitles go onto screens by its rules; without, as recognised. */
  export: (pid: string, format: TranscriptFormat, speakers: boolean, times: boolean, rules = "", digits = false) => postCoded<{ file: string; text: string; too_fast: number | null; digits_unknown: number | null }>(`/listen/${pid}/export`, { format, speakers, times, rules: rules || undefined, digits: digits || undefined }),
  subtitleRules: () => getCoded<{ rules: SubtitleRules[] }>("/listen/subtitle-rules"),
  stats: (pid: string) => getCoded<SpeakerStats>(`/listen/${pid}/stats`),
  watchStatus: () => getCoded<WatchStatus>("/listen/watch"),
  watchStart: (config: WatchConfig) => postCoded<WatchStatus>("/listen/watch", config),
  watchStop: () => postCoded<WatchStatus>("/listen/watch/stop", {}),
  check: (pid: string, reference: string) => postCoded<ReferenceCheck>(`/listen/${pid}/check`, { reference }),
  toSpeech: (pid: string) => postCoded<{ pid: string }>(`/listen/${pid}/to-speech`, {}),
  speakersMatch: (pid: string) => postCoded<{ speakers: SpeakerMatch[]; threshold: number }>(`/listen/${pid}/speakers/match`, {}),
  speakerRemember: (pid: string, speaker: string) => postCoded<{ name: string; recordings: number }>(`/listen/${pid}/speakers/remember`, { speaker }),
  voices: (pid: string, speaker?: string) => postCoded<{ made: { speaker: string; voice: string; seconds: number }[]; skipped: string[] }>(`/listen/${pid}/voices`, speaker ? { speaker } : {}),
};

/** Marks the words of tokens `a..b` for removal, or returns them when all of them are marked already. */
export function toggleCut(lines: ListenLine[], tokens: Token[], a: number, b: number): ListenLine[] {
  const picked = tokens.slice(Math.min(a, b), Math.max(a, b) + 1);
  if (!picked.length) return lines;
  const cut = !picked.every((t) => t.cut);
  const next = lines.map((l) => ({ ...l, words: [...l.words] }));
  for (const t of picked) next[t.line].words[t.word] = { ...next[t.line].words[t.word], cut };
  return next;
}

export function restoreAll(lines: ListenLine[]): ListenLine[] {
  return lines.map((l) => (l.words.some((w) => w.cut) ? { ...l, words: l.words.map((w) => ({ ...w, cut: false })) } : l));
}

/** Seconds marked for removal. */
export function cutSeconds(lines: ListenLine[]): number {
  return lines.reduce((s, l) => s + l.words.filter((w) => w.cut).reduce((x, w) => x + Math.max(0, w.end - w.start), 0), 0);
}

const joined = (words: ListenWord[]) => words.map((w) => w.text.trim()).filter(Boolean).join(" ");

function freshId(lines: ListenLine[]): string {
  let n = lines.length;
  while (lines.some((l) => l.id === `l${n}`)) n += 1;
  return `l${n}`;
}

/** The line cut in two before word `at`; both halves keep the speaker and the times of their words. */
export function splitLine(lines: ListenLine[], line: number, at: number): ListenLine[] {
  const l = lines[line];
  if (!l || at <= 0 || at >= l.words.length) return lines;
  const head = l.words.slice(0, at);
  const tail = l.words.slice(at);
  const first = { ...l, words: head, text: joined(head), end: head[head.length - 1].end };
  const second = { ...l, id: freshId(lines), words: tail, text: joined(tail), start: tail[0].start };
  return [...lines.slice(0, line), first, second, ...lines.slice(line + 1)];
}

/** The line joined with the next one (the speaker of the first stays). */
export function mergeNext(lines: ListenLine[], line: number): ListenLine[] {
  const a = lines[line];
  const b = lines[line + 1];
  if (!a || !b) return lines;
  const words = [...a.words, ...b.words];
  const merged = { ...a, words, text: joined(words) || `${a.text} ${b.text}`.trim(), end: Math.max(a.end, b.end), approx: a.approx || b.approx, flag: null, evidence: null };
  return [...lines.slice(0, line), merged, ...lines.slice(line + 2)];
}

/** New text of a line: with the same number of words their times stay, otherwise the words spread over the line by length. */
export function setLineText(line: ListenLine, text: string): ListenLine {
  const parts = text.split(/\s+/).filter(Boolean);
  if (parts.length === line.words.length) return { ...line, text: parts.join(" "), words: line.words.map((w, i) => ({ ...w, text: parts[i] })) };
  const weight = (w: string) => Math.max(1, [...w].filter((c) => /[\p{L}\p{N}]/u.test(c)).length);
  const total = parts.reduce((s, w) => s + weight(w), 0);
  let at = line.start;
  const words = parts.map((w) => {
    const len = total ? ((line.end - line.start) * weight(w)) / total : 0;
    const word = { text: w, start: at, end: at + len, cut: false };
    at += len;
    return word;
  });
  return { ...line, text: parts.join(" "), words, approx: true };
}

/** A key for a new speaker and its default name. */
export function newSpeaker(speakers: Record<string, string>, name: (n: number) => string): [string, string] {
  let n = 0;
  while (String(n) in speakers) n += 1;
  return [String(n), name(n + 1)];
}

// readable on both themes: each hue is mixed with the theme's text colour
export const SPEAKER_COLORS = ["color-mix(in oklab, #4cc2ff 60%, var(--color-text))", "color-mix(in oklab, #ffc46b 60%, var(--color-text))", "color-mix(in oklab, #7ee0c3 60%, var(--color-text))", "color-mix(in oklab, #ff8fb1 60%, var(--color-text))", "color-mix(in oklab, #b8a4ff 60%, var(--color-text))", "color-mix(in oklab, #ff6b6b 60%, var(--color-text))", "color-mix(in oklab, #9be15d 60%, var(--color-text))", "color-mix(in oklab, #f0a6ff 60%, var(--color-text))"];
export const speakerColor = (key: string) => SPEAKER_COLORS[(Number.parseInt(key, 10) || 0) % SPEAKER_COLORS.length];

/** A broadcaster's subtitle rules: characters per line, lines per screen, fastest reading, shortest and longest showing. */
export type SubtitleRules = { name: string; max_line_len: number; max_lines: number; max_cps: number; min_display_ms: number; max_display_ms: number };
/** A folder recognised in the background: every recording in it written as `formats` into `out` (empty — the folder
 * itself), `keep` watching it for new files; the work survives a restart. */
export type WatchConfig = { folder: string; out: string; formats: TranscriptFormat[]; keep: boolean; engine: string; lang: string; rules: string; digits: boolean };
export type WatchItem = { file: string; pid: string | null; state: "queued" | "working" | "done" | "error"; error: string | null; outputs: string[] };
export type WatchStatus = { config: WatchConfig; on: boolean; running: boolean; items: WatchItem[]; done: number; errors: number; queued: number };
/** Who speaks how much: per speaker talk seconds, share of all talk (%), lines, words, words a minute, longest stretch. */
export type SpeakerStats = { talk_seconds: number; duration: number; speakers: { speaker: string; name: string; seconds: number; share: number; lines: number; words: number; wpm: number; longest: number }[] };

/** What Listening opens: sound and video files. */
export const MEDIA = "audio/*,video/*,.mkv,.avi,.wma,.opus,.flac,.m4a";
/** The recogniser and language a new recording starts with. */
export type Start = { engine: ListenEngine; lang: string };

/** A new recording takes the start's recogniser and language, unless it was made already. */
export async function prepare(pid: string, project: ListenProject, start: Start): Promise<void> {
  if (project.made || (project.settings.engine === start.engine && project.settings.lang === start.lang)) return;
  await listenApi.put(pid, { ...project, settings: { ...project.settings, engine: start.engine, lang: start.lang } });
}
