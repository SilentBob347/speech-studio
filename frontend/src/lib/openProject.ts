import { useState } from "react";
import { useStore } from "../store";

function remembered(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}
function remember(key: string, pid: string | null) {
  try { if (pid) localStorage.setItem(key, pid); else localStorage.removeItem(key); } catch { /* the open project is only remembered where storage works */ }
}

/** The project open in a mode, remembered across restarts; "New" in the top bar closes it. */
export function useOpenProject(key: string): [string | null, (pid: string | null) => void] {
  const fresh = useStore((s) => s.fresh);
  const [state, setState] = useState<{ pid: string | null; fresh: number }>(() => ({ pid: remembered(key), fresh }));
  if (state.fresh !== fresh) {
    remember(key, null);
    setState({ pid: null, fresh });
  }
  return [state.pid, (pid) => { remember(key, pid); setState({ pid, fresh }); }];
}
