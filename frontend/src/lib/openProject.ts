import { useEffect, useState } from "react";
import { useStore } from "../store";

function remembered(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}
function remember(key: string, pid: string | null) {
  try { if (pid) localStorage.setItem(key, pid); else localStorage.removeItem(key); } catch { /* the open project is only remembered where storage works */ }
}

const KEYS = { speech: "speech-studio:speech-project", listening: "speech-studio:listen-project" } as const;
export const PROJECT_KEY = KEYS;

/** The project open now in a mode, for those outside its page (the Assistant panel). */
export const openProjectId = (mode: keyof typeof KEYS): string | null => remembered(KEYS[mode]);

const OPEN_PROJECT = "speech:open-project";

/** Opens a project in its mode from outside its page (an agent's editor_open): a mounted page opens it now, one that
 * mounts later finds it remembered. */
export function requestOpen(mode: keyof typeof KEYS, pid: string): void {
  remember(KEYS[mode], pid);
  window.dispatchEvent(new CustomEvent<{ key: string; pid: string }>(OPEN_PROJECT, { detail: { key: KEYS[mode], pid } }));
}

/** The project open in a mode, remembered across restarts; "New" in the top bar closes it. */
export function useOpenProject(key: string): [string | null, (pid: string | null) => void] {
  const fresh = useStore((s) => s.fresh);
  const [state, setState] = useState<{ pid: string | null; fresh: number }>(() => ({ pid: remembered(key), fresh }));
  useEffect(() => {
    const opened = (e: Event) => {
      const detail = (e as CustomEvent<{ key: string; pid: string }>).detail;
      if (detail.key === key) setState((s) => ({ pid: detail.pid, fresh: s.fresh }));
    };
    window.addEventListener(OPEN_PROJECT, opened);
    return () => window.removeEventListener(OPEN_PROJECT, opened);
  }, [key]);
  if (state.fresh !== fresh) {
    remember(key, null);
    setState({ pid: null, fresh });
  }
  return [state.pid, (pid) => { remember(key, pid); setState({ pid, fresh }); }];
}
