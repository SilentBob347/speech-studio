import { create } from "zustand";

export type Stage = "boot" | "offline" | "setup" | "ready";
export type Mode = "speech" | "listening";
export const MODES: readonly Mode[] = ["speech", "listening"];
export type Activity = { t: number; text: string; kind: "work" | "done" | "error" | "agent" };   // строка журнала «что делает приложение»; agent — действие агента по MCP

const MODE_KEY = "speech-studio:mode";

function savedMode(): Mode {
  try {
    const m = localStorage.getItem(MODE_KEY);
    return m === "listening" ? "listening" : "speech";
  } catch {
    return "speech";
  }
}

type State = {
  stage: Stage;
  mode: Mode;
  fresh: number;                     // растёт на каждое «Новое»: открытый режим начинает с чистого листа
  progress: { stage: string; msg: string; pct: number | null };
  activities: Activity[];            // журнал «что делает приложение» — статус-строка в шапке + разворот в лог
  setStage: (s: Stage) => void;
  setMode: (m: Mode) => void;
  startFresh: () => void;
  setProgress: (stage: string, msg: string, pct?: number | null) => void;
  pushActivity: (text: string, kind?: Activity["kind"]) => void;
};

function withActivity(list: Activity[], text: string, kind: Activity["kind"]): Activity[] | null {
  const clean = text.trim();
  if (!clean) return null;
  const last = list[list.length - 1];
  if (last && last.text === clean && last.kind === kind) return null;
  return [...list, { t: Date.now(), text: clean, kind }].slice(-200);
}

export const useStore = create<State>((set) => ({
  stage: "boot",
  mode: savedMode(),
  fresh: 0,
  progress: { stage: "", msg: "", pct: null },
  activities: [],
  setStage: (stage) => set({ stage }),
  setMode: (mode) => {
    try { localStorage.setItem(MODE_KEY, mode); } catch { /* the mode is only remembered where storage works */ }
    set({ mode });
  },
  startFresh: () => set((s) => ({ fresh: s.fresh + 1 })),
  setProgress: (stage, msg, pct = null) => set((s) => {
    const progress = { stage, msg: msg || s.progress.msg, pct: pct ?? null };
    const kind: Activity["kind"] = stage === "error" ? "error" : stage === "done" ? "done" : "work";
    const activities = withActivity(s.activities, msg || "", kind);
    return activities ? { progress, activities } : { progress };
  }),
  pushActivity: (text, kind = "work") => set((s) => {
    const activities = withActivity(s.activities, text || "", kind);
    return activities ? { activities } : {};
  }),
}));
