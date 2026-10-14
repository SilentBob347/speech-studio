import { useCallback, useEffect, useRef, useState } from "react";
import i18n from "./i18n";
import { ApiError } from "./api";
import { speechApi, type SpeechProject } from "./speech";
import { listenApi, type ListenProject } from "./listen";
import { PROJECT_CHANGED, isMine, useChanged } from "./mcpBridge";
import { errorText } from "./voices";

const SAVE_DELAY = 500;
const HISTORY = 100;

type Doc<T> = {
  get: (pid: string) => Promise<T>;
  put: (pid: string, project: T) => Promise<T>;
  /** What of the saved answer the window takes while newer local edits wait to be saved. */
  keep: (cur: T, saved: T) => T;
};

const SPEECH: Doc<SpeechProject> = { get: speechApi.get, put: speechApi.put, keep: (cur, saved) => ({ ...cur, status: saved.status }) };
const LISTEN: Doc<ListenProject> = { get: listenApi.get, put: listenApi.put, keep: (cur) => cur };

/**
 * One speech project in the window: read, edited locally, saved after a short pause (a PUT with the revision the
 * window knows), undone and redone, and read again whenever a job, an agent or another window saved it.
 */
export function useSpeechProject(pid: string) {
  return useEditedProject(pid, SPEECH);
}

/** The same for a Listening project. */
export function useListenProject(pid: string) {
  return useEditedProject(pid, LISTEN);
}

function useEditedProject<T>(pid: string, doc: Doc<T>) {
  const [project, setProject] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const past = useRef<T[]>([]);
  const future = useRef<T[]>([]);
  const latest = useRef<T | null>(null);
  const timer = useRef<number | null>(null);
  const [history, setHistory] = useState({ undo: 0, redo: 0 });
  const counted = () => setHistory({ undo: past.current.length, redo: future.current.length });

  const load = useCallback(() => {
    doc.get(pid).then((p) => { latest.current = p; setProject(p); setError(null); }, (e: unknown) => setError(errorText(e)));
  }, [pid, doc]);
  useEffect(() => { past.current = []; future.current = []; load(); }, [load]);
  useEffect(() => () => { past.current = []; future.current = []; }, []);

  const save = useCallback(async () => {
    if (timer.current) { window.clearTimeout(timer.current); timer.current = null; }
    const p = latest.current;
    if (!p) return;
    setSaving(true);
    try {
      const saved = await doc.put(pid, p);
      if (latest.current === p) { latest.current = saved; setProject(saved); }
      else setProject((cur) => (cur ? doc.keep(cur, saved) : cur));
      setError(null);
    } catch (e) {
      if (e instanceof ApiError && e.code === "project_changed") { setError(i18n.t("speech.changedElsewhere")); load(); }
      else setError(errorText(e));
    } finally { setSaving(false); }
  }, [pid, load, doc]);

  const schedule = useCallback(() => {
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => { void save(); }, SAVE_DELAY);
  }, [save]);
  useEffect(() => () => { if (timer.current) { window.clearTimeout(timer.current); void save(); } }, [save]);

  const edit = useCallback((change: (p: T) => T) => {
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

  const step = useCallback((from: React.MutableRefObject<T[]>, to: React.MutableRefObject<T[]>) => {
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
    // the project was changed by an agent or another window: the old snapshots would undo their change too
    if (!isMine(n)) { past.current = []; future.current = []; counted(); }
    load();
  });

  return { project, error, saving, edit, undo, redo, canUndo: history.undo > 0, canRedo: history.redo > 0, reload: load, flush: save };
}
