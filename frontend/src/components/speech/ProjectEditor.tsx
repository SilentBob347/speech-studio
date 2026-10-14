import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, Download, Loader2, Play, Redo2, Settings2, Sparkles, SpellCheck, Square, Undo2, Users } from "lucide-react";
import { api, ApiError } from "../../lib/api";
import { activeTake, newId, spliceChars, SPEAKER_TINTS, speechApi, stressDoubts, type Block, type BlockState, type SpeechProject, type StressWord } from "../../lib/speech";
import { openSettings } from "../../lib/settingsNav";
import { useSpeechProject } from "../../lib/useSpeechProject";
import { followJob } from "../../lib/followJob";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useStore } from "../../store";
import BlockRow, { type BlockActions } from "./BlockRow";
import VoicePicker from "./VoicePicker";
import ProjectSettings from "./ProjectSettings";
import ExportDialog from "./ExportDialog";

function blockList(p: SpeechProject): Block[] {
  return p.chapters.flatMap((c) => c.blocks);
}

/** Where a block sits: its chapter and index, for edits that move or cut blocks. */
function locate(p: SpeechProject, id: string): [number, number] | null {
  for (let c = 0; c < p.chapters.length; c++) {
    const i = p.chapters[c].blocks.findIndex((b) => b.id === id);
    if (i >= 0) return [c, i];
  }
  return null;
}

export default function ProjectEditor({ pid, onBack }: { pid: string; onBack: () => void }) {
  const { t } = useTranslation();
  const { project, error, saving, edit, undo, redo, canUndo, canRedo, flush } = useSpeechProject(pid);
  const [job, setJob] = useState<{ id: string; ids: string[] | null } | null>(null);
  const [jobError, setJobError] = useState<string | null>(null);
  const [playing, setPlaying] = useState<string | null>(null);
  const [queue, setQueue] = useState<string[]>([]);
  const [settings, setSettings] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [notes, setNotes] = useState<Record<string, { text: string; words: StressWord[] }>>({});
  const [stressing, setStressing] = useState<string[] | null>(null);
  const [stressMissing, setStressMissing] = useState(false);
  const audio = useRef<HTMLAudioElement | null>(null);
  const pushActivity = useStore((s) => s.pushActivity);

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === "TEXTAREA" || el.tagName === "INPUT")) return;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") { e.preventDefault(); if (e.shiftKey) redo(); else undo(); }
      else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "y") { e.preventDefault(); redo(); }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [undo, redo]);
  useEffect(() => () => { audio.current?.pause(); }, []);

  const blocks = useMemo(() => (project ? blockList(project) : []), [project]);
  const speakers = useMemo(() => [...new Set(blocks.map((b) => b.speaker).filter(Boolean))], [blocks]);
  const tintOf = (s: string) => SPEAKER_TINTS[Math.max(0, speakers.indexOf(s)) % SPEAKER_TINTS.length];
  const stateOf = (b: Block): BlockState => project?.status?.[b.id]?.state ?? "pending";
  const counts = blocks.reduce<Record<string, number>>((acc, b) => { const s = stateOf(b); acc[s] = (acc[s] ?? 0) + 1; return acc; }, {});
  const dirty = (counts.pending ?? 0) + (counts.stale ?? 0) + (counts.failed ?? 0);

  const stopAudio = useCallback(() => { audio.current?.pause(); setPlaying(null); setQueue([]); }, []);
  const playBlock = useCallback((b: Block, rest: string[]) => {
    const take = activeTake(b);
    if (!take) return;
    audio.current?.pause();
    const a = new Audio(`${speechApi.takeUrl(pid, take.file)}?n=${take.n}`);
    audio.current = a;
    setPlaying(b.id);
    setQueue(rest);
    a.onended = () => setPlaying(null);
    a.play().catch((e: unknown) => { setPlaying(null); setJobError(errorText(e)); });
  }, [pid]);

  const latestProject = useRef(project);
  const latestJob = useRef(job);
  useEffect(() => { latestProject.current = project; latestJob.current = job; }, [project, job]);
  useEffect(() => {
    if (playing || !queue.length) return;
    const advance = () => {
      const p = latestProject.current;
      if (!p) return;
      const [next, ...rest] = queue;
      const b = blockList(p).find((x) => x.id === next);
      const state = b ? p.status?.[b.id]?.state : undefined;
      if (b && state === "ready" && activeTake(b)) playBlock(b, rest);
      else if (!b || state === "skipped") setQueue(rest);
      else if (!latestJob.current) setQueue([]);
    };
    const timer = window.setInterval(advance, 300);
    return () => window.clearInterval(timer);
  }, [playing, queue, playBlock]);

  const runSynth = async (scope: { ids?: string[]; from?: string; force?: boolean }) => {
    setJobError(null);
    try {
      await flush();
      const { job_id } = await speechApi.synth(pid, scope);
      setJob({ id: job_id, ids: scope.ids ?? null });
      await followJob(job_id, "synth");
    } catch (e) { setJobError(errorText(e)); } finally { setJob(null); }
  };
  const stopSynth = async () => {
    if (!job) return;
    try { await api.cancelJob(job.id); } catch (e) { setJobError(errorText(e)); }
  };
  const listenFrom = (id: string) => {
    if (!project) return;
    const order = blockList(project).map((b) => b.id);
    const from = order.indexOf(id);
    const rest = order.slice(from);
    const needs = rest.some((x) => { const b = blockList(project).find((y) => y.id === x); return b && ["pending", "stale", "failed"].includes(stateOf(b)); });
    stopAudio();
    setQueue(rest);
    if (needs && !job) void runSynth({ from: id });
  };

  const stressBlocks = async (ids: string[] | null) => {
    if (!project || stressing) return;
    const todo = blockList(project).filter((b) => b.text.trim() && (ids === null || ids.includes(b.id)));
    setStressing(todo.map((b) => b.id));
    setJobError(null);
    setStressMissing(false);
    const done: Record<string, { from: string; text: string; words: StressWord[] }> = {};
    try {
      for (const b of todo) {
        const r = await speechApi.stress(b.text, project.settings.lang);
        if (r.russian) done[b.id] = { from: b.text, text: r.text, words: stressDoubts(r.words) };
      }
    } catch (e) {
      if (e instanceof ApiError && e.code === "stress-model-missing") setStressMissing(true);
      else setJobError(errorText(e));
    } finally {
      setStressing(null);
    }
    const changed = Object.keys(done);
    if (!changed.length) return;
    edit((p) => {
      for (const b of blockList(p)) {
        const d = done[b.id];
        if (d && b.text === d.from) b.text = d.text;
      }
      return p;
    });
    setNotes((n) => {
      const next = { ...n };
      for (const id of changed) next[id] = { text: done[id].text, words: done[id].words };
      return next;
    });
  };
  const pickVariant = (b: Block, w: StressWord, value: string) => {
    const note = notes[b.id];
    if (!note || note.text !== b.text) return;
    const text = spliceChars(b.text, w.start, w.end, value);
    const shift = Array.from(value).length - (w.end - w.start);
    edit((p) => { const at = locate(p, b.id); if (at) p.chapters[at[0]].blocks[at[1]].text = text; return p; });
    setNotes((n) => ({ ...n, [b.id]: { text, words: note.words.filter((x) => x !== w).map((x) => (x.start > w.start ? { ...x, start: x.start + shift, end: x.end + shift } : x)) } }));
  };

  const actions = (b: Block): BlockActions => ({
    setText: (text) => edit((p) => { const at = locate(p, b.id); if (at) p.chapters[at[0]].blocks[at[1]].text = text; return p; }),
    patch: (change) => edit((p) => { const at = locate(p, b.id); if (at) Object.assign(p.chapters[at[0]].blocks[at[1]], change); return p; }),
    split: (pos) => edit((p) => {
      const at = locate(p, b.id);
      if (!at) return p;
      const src = p.chapters[at[0]].blocks[at[1]];
      const head = src.text.slice(0, pos).trim();
      const tail = src.text.slice(pos).trim();
      if (!head || !tail) return p;
      src.text = head;
      p.chapters[at[0]].blocks.splice(at[1] + 1, 0, { ...src, id: newId(), text: tail, takes: [], active: null, pinned: null, error: null });
      return p;
    }),
    mergeNext: () => edit((p) => {
      const at = locate(p, b.id);
      if (!at) return p;
      const list = p.chapters[at[0]].blocks;
      const next = list[at[1] + 1];
      if (!next) return p;
      list[at[1]].text = `${list[at[1]].text.trim()} ${next.text.trim()}`.trim();
      list.splice(at[1] + 1, 1);
      return p;
    }),
    insertAfter: () => edit((p) => {
      const at = locate(p, b.id);
      if (at) p.chapters[at[0]].blocks.splice(at[1] + 1, 0, { id: newId(), speaker: b.speaker, voice: null, text: "", direction: "", pause_ms: null, hidden: false, takes: [], active: null, pinned: null, error: null });
      return p;
    }),
    move: (dir) => edit((p) => {
      const at = locate(p, b.id);
      if (!at) return p;
      const list = p.chapters[at[0]].blocks;
      const to = at[1] + dir;
      if (to < 0 || to >= list.length) return p;
      [list[at[1]], list[to]] = [list[to], list[at[1]]];
      return p;
    }),
    remove: () => edit((p) => { const at = locate(p, b.id); if (at && blockList(p).length > 1) p.chapters[at[0]].blocks.splice(at[1], 1); return p; }),
    synth: (force) => void runSynth({ ids: [b.id], force }),
    play: () => (playing === b.id ? stopAudio() : playBlock(b, [])),
    playFrom: () => listenFrom(b.id),
    stress: () => void stressBlocks([b.id]),
    pick: (w, value) => pickVariant(b, w, value),
  });

  if (!project) {
    return <main className="flex-1 grid place-items-center">{error ? <p role="alert" className="text-[13px] text-[var(--color-warn)]">{error}</p> : <Loader2 size={20} className="animate-spin text-[var(--color-muted)]" />}</main>;
  }
  const bar = "inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-[var(--color-border)] text-[12px] font-medium hover:border-[var(--color-accent)] disabled:opacity-40 transition-colors";
  return (
    <main className="flex-1 min-h-0 flex flex-col">
      <div className="flex flex-wrap items-center gap-2 px-5 py-2.5 border-b border-[var(--color-border)] bg-[var(--color-surface)]/60">
        <button onClick={onBack} className="p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("speech.allProjects"))}><ArrowLeft size={16} /></button>
        <input value={project.name} onChange={(e) => edit((p) => ({ ...p, name: e.target.value }))} aria-label={t("speech.name")}
          className="min-w-0 flex-1 max-w-sm bg-transparent text-[15px] font-semibold focus:outline-none rounded px-1 focus:bg-[var(--color-surface-2)]" />
        <span className="text-[11px] text-[var(--color-muted)] tabnum">{t("speech.readyOf", { ready: counts.ready ?? 0, total: blocks.length - (counts.skipped ?? 0) })}{saving ? ` · ${t("speech.saving")}` : ""}</span>
        <div className="ml-auto flex flex-wrap items-center gap-1.5">
          <button onClick={undo} disabled={!canUndo} className="p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("speech.undo"))}><Undo2 size={15} /></button>
          <button onClick={redo} disabled={!canRedo} className="p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("speech.redo"))}><Redo2 size={15} /></button>
          <button onClick={() => setSettings(true)} className={bar}><Settings2 size={13} />{t("speech.settings")}</button>
          <button onClick={() => void stressBlocks(null)} disabled={!!stressing || !blocks.length} className={bar} title={t("speech.stressAllHint")}>
            {stressing && stressing.length > 1 ? <Loader2 size={13} className="animate-spin" /> : <SpellCheck size={13} />}{t("speech.stressAll")}
          </button>
          {job ? (
            <button onClick={stopSynth} className={`${bar} border-[var(--color-warn)] text-[var(--color-warn)]`}><Square size={13} />{t("speech.stop")}</button>
          ) : (
            <>
              <button onClick={() => void runSynth({})} disabled={!dirty} className={`${bar} bg-[var(--color-accent)] text-[var(--color-on-accent)] border-transparent`}><Sparkles size={13} />{t("speech.voiceDirty", { n: dirty })}</button>
              <button onClick={() => void runSynth({ force: true })} className={bar}>{t("speech.voiceAll")}</button>
            </>
          )}
          <button onClick={() => listenFrom(blocks[0]?.id)} disabled={!blocks.length} className={bar}><Play size={13} />{t("speech.listenAll")}</button>
          <button onClick={() => setExporting(true)} className={bar}><Download size={13} />{t("speech.export")}</button>
        </div>
      </div>

      <div className="flex-1 min-h-0 flex">
        <aside aria-label={t("speech.cast")} className="hidden lg:block w-64 shrink-0 border-r border-[var(--color-border)] overflow-y-auto p-4 space-y-3">
          <h2 className="text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)] flex items-center gap-1.5"><Users size={12} />{t("speech.cast")}</h2>
          <label className="block text-[11px] text-[var(--color-muted)]">{t("speech.projectVoice")}
            <VoicePicker value={project.settings.voice} onChange={(v) => edit((p) => { p.settings.voice = v; return p; })} label={t("speech.projectVoice")}
              className="mt-1 w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-2 py-1.5 text-[12px]" />
          </label>
          {speakers.map((s) => (
            <label key={s} className="block text-[11px]">
              <span className="font-semibold" style={{ color: tintOf(s) }}>{s}</span>
              <VoicePicker value={project.cast[s] ?? null} onChange={(v) => edit((p) => { if (v) p.cast[s] = v; else delete p.cast[s]; return p; })} inherit={t("speech.voiceAsProject")} label={t("speech.voiceFor", { speaker: s })}
                className="mt-1 w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-2 py-1.5 text-[12px]" />
            </label>
          ))}
        </aside>
        <div className="flex-1 min-w-0 overflow-y-auto px-4 py-4">
          {(error || jobError) && <p role="alert" className="mb-3 px-3 py-2 rounded-lg border border-[var(--color-warn)]/40 text-[12px] text-[var(--color-warn)] break-words">{error ?? jobError}</p>}
          {stressMissing && (
            <div role="alert" className="mb-3 px-3 py-2 rounded-lg border border-[var(--color-warn)]/40 text-[12px] text-[var(--color-warn)] flex flex-wrap items-center gap-2">
              <span className="flex-1 min-w-0">{t("speech.stressMissing")}</span>
              <button onClick={() => { setStressMissing(false); openSettings("models"); }} className="px-2 py-1 rounded-md border border-[var(--color-warn)]/60 hover:bg-[var(--color-warn)]/10">{t("speech.openModels")}</button>
            </div>
          )}
          <datalist id="speech-speakers">{speakers.map((s) => <option key={s} value={s} />)}</datalist>
          {project.chapters.map((c) => (
            <section key={c.id} className="max-w-4xl mx-auto">
              {project.chapters.length > 1 && <h2 className="mt-4 mb-2 text-[13px] font-semibold text-[var(--color-muted)]">{c.title || t("speech.chapter")}</h2>}
              <ol className="space-y-0.5">
                {c.blocks.map((b) => {
                  const index = blocks.indexOf(b);
                  const rendering = !!job && (job.ids === null ? ["pending", "stale", "failed"].includes(stateOf(b)) : job.ids.includes(b.id));
                  const note = notes[b.id];
                  return <BlockRow key={b.id} block={b} index={index} state={stateOf(b)} rendering={rendering} playing={playing === b.id} tint={tintOf(b.speaker)}
                    stressing={!!stressing?.includes(b.id)} doubts={note && note.text === b.text ? note.words : []} a={actions(b)} />;
                })}
              </ol>
            </section>
          ))}
        </div>
      </div>
      {settings && <ProjectSettings project={project} onChange={edit} onClose={() => setSettings(false)} />}
      {exporting && <ExportDialog pid={pid} project={project} onClose={() => setExporting(false)} onVoiceMissing={() => { setExporting(false); void runSynth({}); }} onDone={(file) => pushActivity(t("speech.exported", { file }), "done")} />}
    </main>
  );
}
