import { useCallback, useEffect, useRef, useState } from "react";
import i18n from "./i18n";
import { ApiError } from "./api";
import { speechApi, type SpeechProject } from "./speech";
import { PROJECT_CHANGED, isMine, useChanged } from "./mcpBridge";
import { errorText } from "./voices";

const SAVE_DELAY = 500;
const HISTORY = 100;

/**
 * One speech project in the window: read, edited locally, saved after a short pause (a PUT with the revision the
 * window knows), undone and redone, and read again whenever a job, an agent or another window saved it.
 */
export function useSpeechProject(pid: string) {
  const [project, setProject] = useState<SpeechProject | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const past = useRef<SpeechProject[]>([]);
  const future = useRef<SpeechProject[]>([]);
  const latest = useRef<SpeechProject | null>(null);
  const timer = useRef<number | null>(null);
  const [history, setHistory] = useState({ undo: 0, redo: 0 });
  const counted = () => setHistory({ undo: past.current.length, redo: future.current.length });

  const load = useCallback(() => {
    speechApi.get(pid).then((p) => { latest.current = p; setProject(p); setError(null); }, (e: unknown) => setError(errorText(e)));
  }, [pid]);
  useEffect(() => { past.current = []; future.current = []; load(); }, [load]);
  useEffect(() => () => { past.current = []; future.current = []; }, []);

  const save = useCallback(async () => {
    if (timer.current) { window.clearTimeout(timer.current); timer.current = null; }
    const p = latest.current;
    if (!p) return;
    setSaving(true);
    try {
      const saved = await speechApi.put(pid, p);
      if (latest.current === p) { latest.current = saved; setProject(saved); }
      else setProject((cur) => (cur ? { ...cur, status: saved.status } : cur));
      setError(null);
    } catch (e) {
      if (e instanceof ApiError && e.code === "project_changed") { setError(i18n.t("speech.changedElsewhere")); load(); }
      else setError(errorText(e));
    } finally { setSaving(false); }
  }, [pid, load]);

  const schedule = useCallback(() => {
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => { void save(); }, SAVE_DELAY);
  }, [save]);
  useEffect(() => () => { if (timer.current) { window.clearTimeout(timer.current); void save(); } }, [save]);

  const edit = useCallback((change: (p: SpeechProject) => SpeechProject) => {
    const cur = latest.current;
    if (!cur) return;
    const next = change(structuredClone(cur));
    past.current = [...past.current, cur].slice(-HISTORY);
    future.current = [];
    latest.current = next;
    setProject(next);
    counted();
    schedule();
  }, [schedule]);

  const step = useCallback((from: React.MutableRefObject<SpeechProject[]>, to: React.MutableRefObject<SpeechProject[]>) => {
    const cur = latest.current;
    const target = from.current[from.current.length - 1];
    if (!cur || !target) return;
    from.current = from.current.slice(0, -1);
    to.current = [...to.current, cur].slice(-HISTORY);
    latest.current = target;
    setProject(target);
    counted();
    void save();
  }, [save]);
  const undo = useCallback(() => step(past, future), [step]);
  const redo = useCallback(() => step(future, past), [step]);

  useChanged(PROJECT_CHANGED, (n) => {
    if (n.pid !== pid) return;
    if (isMine(n) && !n.job) return;
    if (timer.current) return;
    load();
  });

  return { project, error, saving, edit, undo, redo, canUndo: history.undo > 0, canRedo: history.redo > 0, reload: load, flush: save };
}
