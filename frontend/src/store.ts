import { create } from "zustand";
import { journalApi, type JournalLine } from "./lib/api";

export type Stage = "boot" | "offline" | "setup" | "ready";
export type Mode = "speech" | "listening";
export const MODES: readonly Mode[] = ["speech", "listening"];
// строка журнала «что делает приложение»; agent — действие агента по MCP; verb/noun/target — изменение агента, словами окна
export type Activity = { t: number; text: string; kind: "work" | "done" | "error" | "agent"; verb?: string; noun?: string; target?: string };

const MODE_KEY = "speech-studio:mode";

function savedMode(): Mode {
  try {
    const m = new URLSearchParams(location.search).get("mode") ?? localStorage.getItem(MODE_KEY);
    return m === "listening" ? "listening" : "speech";
  } catch {
    return "speech";
  }
}

type State = {
  stage: Stage;
  mode: Mode;
  fresh: number;                     // растёт на каждое «Новое»: открытый режим начинает с чистого листа
  progress: { stage: string; msg: string; pct: number | null; since: number; eta: number | null };   // since — начало стадии; eta — сколько секунд осталось по её скорости
  activities: Activity[];            // журнал «что делает приложение» — статус-строка в шапке + разворот в лог; хранится сервером
  speechEngine: string | null;       // имя движка открытого проекта Speech: его и показывает подвал
  setSpeechEngine: (engine: string | null) => void;
  setStage: (s: Stage) => void;
  setMode: (m: Mode) => void;
  startFresh: () => void;
  setProgress: (stage: string, msg: string, pct?: number | null) => void;
  pushActivity: (text: string, kind?: Activity["kind"]) => void;
  loadJournal: () => Promise<void>;
  clearJournal: () => Promise<void>;
};

const KEPT = 500;

function withActivity(list: Activity[], text: string, kind: Activity["kind"]): Activity[] | null {
  const clean = text.trim();
  if (!clean) return null;
  const last = list[list.length - 1];
  if (last && last.text === clean && last.kind === kind) return null;
  return [...list, { t: Date.now(), text: clean, kind }].slice(-KEPT);
}

const fromLine = (l: JournalLine): Activity => ({ t: l.at, text: l.text, kind: l.tone, ...(l.source === "agent" ? { verb: l.verb, noun: l.kind, target: l.target } : {}) });

function keep(text: string, kind: Activity["kind"]): void {
  journalApi.write(kind, text.trim()).catch((problem: unknown) => console.error("[ERROR] the journal did not take a line:", problem));
}

export const useStore = create<State>((set) => ({
  stage: "boot",
  mode: savedMode(),
  fresh: 0,
  progress: { stage: "", msg: "", pct: null, since: 0, eta: null },
  activities: [],
  speechEngine: null,
  setSpeechEngine: (speechEngine) => set({ speechEngine }),
  setStage: (stage) => set({ stage }),
  setMode: (mode) => {
    try { localStorage.setItem(MODE_KEY, mode); } catch { /* the mode is only remembered where storage works */ }
    set({ mode });
  },
  startFresh: () => set((s) => ({ fresh: s.fresh + 1 })),
  setProgress: (stage, msg, pct = null) => set((s) => {
    const restarted = stage !== s.progress.stage || (pct ?? 0) < (s.progress.pct ?? 0);
    const now = Date.now();
    const since = restarted ? now : s.progress.since;
    const spent = (now - since) / 1000;
    const eta = pct != null && pct >= 3 && pct < 99 && spent >= 5 ? Math.round((spent * (100 - pct)) / pct) : null;
    const progress = { stage, msg: msg || s.progress.msg, pct: pct ?? null, since, eta };
    const kind: Activity["kind"] = stage === "error" ? "error" : stage === "done" ? "done" : "work";
    const activities = withActivity(s.activities, msg || "", kind);
    if (activities) keep(msg, kind);
    return activities ? { progress, activities } : { progress };
  }),
  pushActivity: (text, kind = "work") => set((s) => {
    const activities = withActivity(s.activities, text || "", kind);
    if (activities) keep(text, kind);
    return activities ? { activities } : {};
  }),
  loadJournal: async () => {
    const lines = await journalApi.read();
    set({ activities: lines.map(fromLine) });
  },
  clearJournal: async () => {
    await journalApi.clear();
    set({ activities: [] });
  },
}));
