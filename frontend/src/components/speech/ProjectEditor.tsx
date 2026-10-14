import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, BookOpen, Download, Drama, History, ListOrdered, Loader2, Play, Redo2, Search, Settings2, Sparkles, SpellCheck, Square, Undo2, Users, X } from "lucide-react";
import { api, ApiError } from "../../lib/api";
import { activeTake, newId, spliceChars, SPEAKER_TINTS, speechApi, stressDoubts, type Block, type BlockState, type SpeechProject, type QueuedSynth, type StressWord } from "../../lib/speech";
import { openSettings } from "../../lib/settingsNav";
import { useSpeechProject } from "../../lib/useSpeechProject";
import { useLivePlayback } from "../../lib/livePlayback";
import { followJob } from "../../lib/followJob";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useStore } from "../../store";
import { usePublishEditor } from "../../lib/editorBridge";
import ErrorNote from "../ErrorNote";
import RecastDialog from "./RecastDialog";
import { CLOUD_ENGINE_NAMES, askCloud, askDirector } from "../../lib/cloudConsent";
import { flash } from "../../lib/mcpBridge";
import { closestCenter, DndContext, KeyboardSensor, PointerSensor, useSensor, useSensors, type Announcements, type DragEndEvent, type UniqueIdentifier } from "@dnd-kit/core";
import { SortableContext, sortableKeyboardCoordinates, verticalListSortingStrategy } from "@dnd-kit/sortable";
import BlockRow, { type BlockActions } from "./BlockRow";
import VoicePicker from "./VoicePicker";
import ProjectSettings from "./ProjectSettings";
import ExportDialog from "./ExportDialog";
import DictionaryDialog from "./DictionaryDialog";
import SearchReplace from "./SearchReplace";
import HistoryDialog from "./HistoryDialog";

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
  const [dictionary, setDictionary] = useState(false);
  const [searching, setSearching] = useState(false);
  const [history, setHistory] = useState(false);
  const [castOpen, setCastOpen] = useState(false);
  const [casting, setCasting] = useState(false);
  const castCharacters = async () => {
    if (!project) return;
    setCasting(true);
    try {
      if (!(await askDirector())) return;
      flush();
      for (const c of project.chapters) {
        const { job_id } = await speechApi.castChapter(pid, c.id);
        await followJob(job_id, "cast");
      }
    } catch (e) { setJobError(errorText(e)); } finally { setCasting(false); }
  };
  const [selected, setSelected] = useState<string[]>([]);
  const [recast, setRecast] = useState<{ speaker: string; to: string | null } | null>(null);
  const [anchor, setAnchor] = useState<string | null>(null);

  const [synthQueue, setQueueList] = useState<QueuedSynth[]>([]);
  const [queueOpen, setQueueOpen] = useState(false);
  const [notes, setNotes] = useState<Record<string, { text: string; words: StressWord[] }>>({});
  const [stressing, setStressing] = useState<string[] | null>(null);
  const [stressMissing, setStressMissing] = useState(false);
  const audio = useRef<HTMLAudioElement | null>(null);
  const pushActivity = useStore((s) => s.pushActivity);

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") { e.preventDefault(); setSearching(true); return; }
      if (el && (el.tagName === "TEXTAREA" || el.tagName === "INPUT")) return;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") { e.preventDefault(); if (e.shiftKey) redo(); else undo(); }
      else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "y") { e.preventDefault(); redo(); }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [undo, redo]);
  useEffect(() => () => { audio.current?.pause(); }, []);
  useLivePlayback(pid);

  const blocks = useMemo(() => (project ? blockList(project) : []), [project]);
  const speakers = useMemo(() => [...new Set(blocks.map((b) => b.speaker).filter(Boolean))], [blocks]);
  const tintOf = (s: string) => SPEAKER_TINTS[Math.max(0, speakers.indexOf(s)) % SPEAKER_TINTS.length];
  const stateOf = (b: Block): BlockState => project?.status?.[b.id]?.state ?? "pending";
  const counts = blocks.reduce<Record<string, number>>((acc, b) => { const s = stateOf(b); acc[s] = (acc[s] ?? 0) + 1; return acc; }, {});
  const dirty = (counts.pending ?? 0) + (counts.stale ?? 0) + (counts.failed ?? 0);
  // the blocks a character's voice reaches: theirs, without a voice of their own
  const castBlocks = (speaker: string) => blocks.filter((b) => b.speaker === speaker && !b.voice && !b.hidden);
  const setCast = (speaker: string, v: string | null) => edit((p) => { if (v) p.cast[speaker] = v; else delete p.cast[speaker]; return p; });
  const castVoice = (speaker: string, v: string | null) => {
    if (castBlocks(speaker).some((b) => b.takes.length > 0)) setRecast({ speaker, to: v });
    else setCast(speaker, v);
  };
  usePublishEditor({
    kind: "speech",
    pid,
    state: () => ({
      name: project?.name ?? null,
      blocks: blocks.map((b) => ({ id: b.id, speaker: b.speaker, text: b.text.slice(0, 160), state: stateOf(b), hidden: b.hidden })),
      selected,
      playing,
      voicing: !!job,
    }),
    select: (id) => {
      if (!blocks.some((b) => b.id === id)) return false;
      setSelected([id]);
      setAnchor(id);
      const row = document.getElementById(`block-${id}`);
      row?.scrollIntoView({ block: "center", behavior: "smooth" });
      flash(row);
      return true;
    },
  });

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

  useEffect(() => {
    if (!job) return;
    const tick = () => speechApi.queue(pid).then((r) => setQueueList(r.queue), () => undefined);
    const timer = window.setInterval(tick, 2000);
    return () => window.clearInterval(timer);
  }, [job, pid]);
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

  const runSynth = async (scope: { ids?: string[]; from?: string; force?: boolean; seed?: number }) => {
    setJobError(null);
    try {
      const provider = project ? CLOUD_ENGINE_NAMES[project.settings.engine] : undefined;
      if (provider && !(await askCloud("speech", provider))) return;
      await flush();
      const r = await speechApi.synth(pid, scope);
      if (r.queued != null) {
        pushActivity(t("speech.queuedAt", { n: r.queued }), "work");
        setQueueList((await speechApi.queue(pid)).queue);
        return;
      }
      const job_id = r.job_id;
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

  const selectBlock = (id: string, range: boolean) => {
    const order = blocks.map((x) => x.id);
    if (range && anchor) {
      const [a1, a2] = [order.indexOf(anchor), order.indexOf(id)].sort((x, y) => x - y);
      setSelected([...new Set([...selected, ...order.slice(a1, a2 + 1)])]);
    } else {
      setSelected((cur) => (cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]));
      setAnchor(id);
    }
  };
  /** Move blocks (in their order) next to `target`: after it when dragged down, before it when dragged up; across chapters too. */
  const moveTo = useCallback((ids: string[], target: string, after: boolean) => {
    if (ids.includes(target)) return;
    edit((p) => {
      const moving: Block[] = [];
      for (const c of p.chapters) {
        c.blocks = c.blocks.filter((b) => { if (ids.includes(b.id)) { moving.push(b); return false; } return true; });
      }
      moving.sort((x, y) => ids.indexOf(x.id) - ids.indexOf(y.id));
      const at = locate(p, target);
      if (!at) return p;
      p.chapters[at[0]].blocks.splice(at[1] + (after ? 1 : 0), 0, ...moving);
      return p;
    });
  }, [edit]);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    if (!over || active.id === over.id) return;
    const order = blocks.map((x) => x.id);
    const from = order.indexOf(String(active.id));
    const to = order.indexOf(String(over.id));
    const ids = selected.includes(String(active.id)) ? order.filter((id) => selected.includes(id)) : [String(active.id)];
    moveTo(ids, String(over.id), from < to);
  };
  const blockLabel = (id: UniqueIdentifier) => {
    const i = blocks.findIndex((x) => x.id === id);
    return i < 0 ? "" : t("speech.dragBlock", { n: i + 1 });
  };
  const announcements: Announcements = {
    onDragStart: ({ active }) => t("speech.dragPicked", { block: blockLabel(active.id) }),
    onDragOver: ({ active, over }) => (over ? t("speech.dragOver", { block: blockLabel(active.id), target: blockLabel(over.id) }) : undefined),
    onDragEnd: ({ active, over }) => (over ? t("speech.dragDropped", { block: blockLabel(active.id), target: blockLabel(over.id) }) : t("speech.dragCancelled", { block: blockLabel(active.id) })),
    onDragCancel: ({ active }) => t("speech.dragCancelled", { block: blockLabel(active.id) }),
  };


  const actions = (b: Block): BlockActions => ({
    select: (range) => selectBlock(b.id, range),
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
      p.chapters[at[0]].blocks.splice(at[1] + 1, 0, { ...src, id: newId(), text: tail, takes: [], active: null, pinned: null, error: null, note: "", marker: "", start: null, end: null });
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
      if (at) p.chapters[at[0]].blocks.splice(at[1] + 1, 0, { id: newId(), speaker: b.speaker, voice: null, text: "", direction: "", pause_ms: null, hidden: false, takes: [], active: null, pinned: null, error: null, note: "", marker: "", start: null, end: null });
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
          <button onClick={() => setSearching((v) => !v)} aria-pressed={searching} className={bar} title={t("search.title")}><Search size={13} />{t("search.open")}</button>
          <button onClick={() => setHistory(true)} className={bar} title={t("history.title")}><History size={13} />{t("history.open")}</button>
          <button onClick={() => setCastOpen((v) => !v)} aria-expanded={castOpen} className={`${bar} lg:hidden`}><Users size={13} />{t("speech.cast")}</button>
          <button onClick={() => void castCharacters()} disabled={casting} className={bar} title={t("speech.castCharactersHint")}>{casting ? <Loader2 size={13} className="animate-spin" /> : <Drama size={13} />}{t("speech.castCharacters")}</button>
          <button onClick={() => setDictionary(true)} className={bar} title={t("speech.dictionaryHint")}><BookOpen size={13} />{t("speech.dictionary")}</button>
          <button onClick={() => void stressBlocks(null)} disabled={!!stressing || !blocks.length} className={bar} title={t("speech.stressAllHint")}>
            {stressing && stressing.length > 1 ? <Loader2 size={13} className="animate-spin" /> : <SpellCheck size={13} />}{t("speech.stressAll")}
          </button>
          {job && synthQueue.length > 0 && (
            <div className="relative">
              <button onClick={() => setQueueOpen((v) => !v)} aria-expanded={queueOpen} className={bar}><ListOrdered size={13} />{t("speech.queue", { n: synthQueue.length })}</button>
              {queueOpen && (
                <div className="absolute right-0 top-full mt-1 z-30 w-72 rounded-lg glass-panel p-2 space-y-1 text-[12px]">
                  {synthQueue.map((q) => (
                    <div key={q.n} className="flex items-center gap-2">
                      <span className="flex-1 truncate">{q.scope.ids ? t("speech.queueBlocks", { n: q.scope.ids.length }) : q.scope.from ? t("speech.queueFrom") : t("speech.queueAll")}</span>
                      <button onClick={() => speechApi.unqueue(pid, q.n).then(() => speechApi.queue(pid)).then((r) => setQueueList(r.queue), () => undefined)} className="p-0.5 rounded text-[var(--color-muted)] hover:text-[var(--color-warn)]" {...named(t("speech.unqueue"))}><X size={12} /></button>
                    </div>
                  ))}
                  <button onClick={() => speechApi.unqueue(pid).then(() => setQueueList([]), () => undefined)} className="w-full mt-1 px-2 py-1 rounded border border-[var(--color-border)] text-[11px] hover:border-[var(--color-warn)]">{t("speech.queueClear")}</button>
                </div>
              )}
            </div>
          )}
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

      {selected.length > 0 && (
        <div role="toolbar" aria-label={t("speech.selection")} className="flex flex-wrap items-center gap-2 px-5 py-2 border-b border-[var(--color-border)] bg-[color-mix(in_oklab,var(--color-accent)_8%,var(--color-surface))]">
          <span className="text-[12px] font-medium tabnum">{t("speech.selectedCount", { n: selected.length })}</span>
          <button onClick={() => void runSynth({ ids: selected, force: true })} disabled={!!job} className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[12px] font-medium disabled:opacity-40"><Sparkles size={12} />{t("speech.voiceSelected")}</button>
          <button onClick={() => void stressBlocks(selected)} className="px-2.5 py-1 rounded-md border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)]">{t("speech.stressBlock")}</button>
          <button onClick={() => edit((p) => { const all = blockList(p).filter((b) => selected.includes(b.id)); const hide = all.some((b) => !b.hidden); all.forEach((b) => { b.hidden = hide; }); return p; })} className="px-2.5 py-1 rounded-md border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)]">{t("speech.hideShow")}</button>
          <button onClick={() => { edit((p) => { for (const c of p.chapters) c.blocks = c.blocks.filter((b) => !selected.includes(b.id)); if (!blockList(p).length) p.chapters[0].blocks.push({ id: newId(), speaker: t("speech.narrator"), voice: null, text: "", direction: "", pause_ms: null, hidden: false, takes: [], active: null, pinned: null, error: null, note: "", marker: "", start: null, end: null }); return p; }); setSelected([]); }} className="px-2.5 py-1 rounded-md border border-[var(--color-border)] text-[12px] hover:border-[var(--color-warn)] hover:text-[var(--color-warn)]">{t("speech.deleteSelected")}</button>
          <button onClick={() => setSelected(blocks.map((b) => b.id))} className="px-2.5 py-1 rounded-md text-[12px] text-[var(--color-muted)] hover:text-[var(--color-text)]">{t("speech.selectAll")}</button>
          <button onClick={() => setSelected([])} className="ml-auto p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("speech.selectNone"))}><X size={14} /></button>
        </div>
      )}
      {searching && (
        <SearchReplace blocks={blocks} onClose={() => setSearching(false)}
          onJump={(id) => document.getElementById(`block-${id}`)?.scrollIntoView({ block: "center", behavior: "smooth" })}
          onReplace={(changes) => edit((p) => { for (const b of blockList(p)) if (changes[b.id] !== undefined) b.text = changes[b.id]; return p; })} />
      )}
      <div className="relative flex-1 min-h-0 flex">
        <aside aria-label={t("speech.cast")} className={`${castOpen ? "absolute inset-y-0 left-0 z-20 block bg-[var(--color-surface)] shadow-xl" : "hidden"} lg:static lg:block lg:shadow-none w-64 shrink-0 border-r border-[var(--color-border)] overflow-y-auto p-4 space-y-3`}>
          <h2 className="text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)] flex items-center gap-1.5"><Users size={12} />{t("speech.cast")}</h2>
          <label className="block text-[11px] text-[var(--color-muted)]">{t("speech.projectVoice")}
            <VoicePicker value={project.settings.voice} onChange={(v) => edit((p) => { p.settings.voice = v; return p; })} label={t("speech.projectVoice")}
              className="mt-1 w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-2 py-1.5 text-[12px]" />
          </label>
          {speakers.map((s) => (
            <label key={s} className="block text-[11px]">
              <span className="flex items-baseline gap-1.5"><span className="font-semibold" style={{ color: tintOf(s) }}>{s}</span><span className="text-[10px] tabnum text-[var(--color-muted)]">{t("speech.castBlocks", { n: blocks.filter((b) => b.speaker === s && !b.hidden).length })}</span></span>
              <VoicePicker value={project.cast[s] ?? null} onChange={(v) => castVoice(s, v)} inherit={t("speech.voiceAsProject")} label={t("speech.voiceFor", { speaker: s })}
                className="mt-1 w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-2 py-1.5 text-[12px]" />
            </label>
          ))}
        </aside>
        <div className="flex-1 min-w-0 overflow-y-auto px-4 py-4">
          {error && <ErrorNote text={error} />}
          {jobError && <ErrorNote text={jobError} onClose={() => setJobError(null)} />}
          {stressMissing && (
            <div role="alert" className="mb-3 px-3 py-2 rounded-lg border border-[var(--color-warn)]/40 text-[12px] text-[var(--color-warn)] flex flex-wrap items-center gap-2">
              <span className="flex-1 min-w-0">{t("speech.stressMissing")}</span>
              <button onClick={() => { setStressMissing(false); openSettings("models"); }} className="px-2 py-1 rounded-md border border-[var(--color-warn)]/60 hover:bg-[var(--color-warn)]/10">{t("speech.openModels")}</button>
            </div>
          )}
          <datalist id="speech-speakers">{speakers.map((s) => <option key={s} value={s} />)}</datalist>
          <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}
            accessibility={{ announcements, screenReaderInstructions: { draggable: t("speech.dragInstructions") } }}>
          {project.chapters.map((c) => (
            <SortableContext key={c.id} items={c.blocks.map((b) => b.id)} strategy={verticalListSortingStrategy}>
            <section className="max-w-4xl mx-auto">
              {project.chapters.length > 1 && <h2 className="mt-4 mb-2 text-[13px] font-semibold text-[var(--color-muted)]">{c.title || t("speech.chapter")}</h2>}
              <ol className="space-y-0.5">
                {c.blocks.map((b) => {
                  const index = blocks.indexOf(b);
                  const rendering = !!job && (job.ids === null ? ["pending", "stale", "failed"].includes(stateOf(b)) : job.ids.includes(b.id));
                  const note = notes[b.id];
                  return <BlockRow key={b.id} pid={pid} block={b} index={index} state={stateOf(b)} fit={project.status?.[b.id]?.fit} rendering={rendering} playing={playing === b.id} tint={tintOf(b.speaker)}
                    stressing={!!stressing?.includes(b.id)} doubts={note && note.text === b.text ? note.words : []}
                    selected={selected.includes(b.id)} selecting={selected.length > 0} a={actions(b)} />;
                })}
              </ol>
            </section>
            </SortableContext>
          ))}
          </DndContext>
        </div>
      </div>
      {history && <HistoryDialog pid={pid} onClose={() => setHistory(false)} onRepeat={(block, seed) => { setHistory(false); void runSynth({ ids: [block], force: true, seed }); }} />}
      {dictionary && <DictionaryDialog pid={pid} project={project} onChange={edit} onClose={() => setDictionary(false)} />}
      {recast && (
        <RecastDialog speaker={recast.speaker} from={project.cast[recast.speaker] ?? null} to={recast.to} blocks={castBlocks(recast.speaker)}
          onApply={(revoice) => {
            const ids = castBlocks(recast.speaker).map((b) => b.id);
            setCast(recast.speaker, recast.to);
            setRecast(null);
            if (revoice) void runSynth({ ids, force: true });
          }}
          onClose={() => setRecast(null)} />
      )}
      {settings && <ProjectSettings project={project} onChange={edit} onClose={() => setSettings(false)} />}
      {exporting && <ExportDialog pid={pid} project={project} onClose={() => setExporting(false)} onVoiceMissing={() => { setExporting(false); void runSynth({}); }} onDone={(file) => pushActivity(t("speech.exported", { file }), "done")} />}
    </main>
  );
}
