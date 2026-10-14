import { useState } from "react";
import { useStore } from "../../store";
import ProjectsHome from "./ProjectsHome";
import ProjectEditor from "./ProjectEditor";

const OPEN_KEY = "speech-studio:speech-project";

function remembered(): string | null {
  try { return localStorage.getItem(OPEN_KEY); } catch { return null; }
}
function remember(pid: string | null) {
  try { if (pid) localStorage.setItem(OPEN_KEY, pid); else localStorage.removeItem(OPEN_KEY); } catch { /* the open project is only remembered where storage works */ }
}

/** Speech: the open project, or the start screen; "New" in the top bar goes back to the start. */
export default function SpeechPage() {
  const fresh = useStore((s) => s.fresh);
  const [state, setState] = useState<{ pid: string | null; fresh: number }>(() => ({ pid: remembered(), fresh }));
  if (state.fresh !== fresh) {
    remember(null);
    setState({ pid: null, fresh });
  }
  const open = (pid: string | null) => { remember(pid); setState({ pid, fresh }); };
  return state.pid ? <ProjectEditor key={state.pid} pid={state.pid} onBack={() => open(null)} /> : <ProjectsHome onOpen={open} />;
}
