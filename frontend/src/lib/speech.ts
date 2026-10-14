import { BASE, postCoded, sendCoded, getCoded } from "./api";

export type Take = {
  n: number; file: string; duration: number; seed: number; temperature: number; key: string; created: number;
  source: string; synth_secs: number; retried: string[];
};
export type Block = {
  id: string; speaker: string; voice: string | null; text: string; direction: string; pause_ms: number | null;
  hidden: boolean; takes: Take[]; active: number | null; pinned: number | null; error: { key: string; text: string } | null;
};
export type Chapter = { id: string; title: string; blocks: Block[] };
export type SpeechParams = { temperature: number; top_k: number; top_p: number; seed_mode: "fixed" | "random"; seed: number };
export type Loudness = "off" | "video" | "podcast" | "acx";
export type SpeechSettings = {
  lang: string; engine: string; params: SpeechParams; voice: string | null;
  pause_same_ms: number; pause_switch_ms: number; loudness: Loudness;
};
export type BlockState = "pending" | "ready" | "stale" | "failed" | "skipped";
export type SpeechProject = {
  kind: "speech"; version: number; name: string; created: number; settings: SpeechSettings;
  cast: Record<string, string>; chapters: Chapter[];
  status?: Record<string, { state: BlockState; key: string }>;
};
export type ProjectCard = { pid: string; kind: string; name: string; mtime: number; created?: number; blocks?: number; ready?: number };
export type ExportFormat = "wav" | "flac" | "mp3" | "ogg" | "m4a";
export const EXPORT_FORMATS: ExportFormat[] = ["wav", "flac", "mp3", "ogg", "m4a"];
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
  takeUrl: (pid: string, file: string) => `${BASE}/projects/${pid}/takes/${encodeURIComponent(file)}`,
  exportUrl: (pid: string, file: string) => `${BASE}/projects/${pid}/exports/${encodeURIComponent(file)}`,
};

export const activeTake = (b: Block): Take | null => (b.active == null ? null : b.takes.find((t) => t.n === b.active) ?? null);

export const newId = () => crypto.randomUUID().replace(/-/g, "").slice(0, 10);

export const SPEAKER_TINTS = ["#4cc2ff", "#f7a8c8", "#7ee0c3", "#ffc46b", "#b8a4ff", "#ff8b6b", "#a5b8ff", "#9be36e"];
