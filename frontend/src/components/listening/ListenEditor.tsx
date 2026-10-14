import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, AudioLines, ChevronDown, ChevronUp, Download, Eye, EyeOff, FileCheck2, Loader2, Pause, Play, Redo2, Scissors, Search, Settings2, Undo2, UserPlus, Users, Wand2, X } from "lucide-react";
import { useListenProject } from "../../lib/useSpeechProject";
import {
  activeToken, clock, cutSeconds, listenApi, mergeNext, newSpeaker, restoreAll, setLineText, speakerColor, splitLine, toggleCut, tokensOf, type ListenLine,
} from "../../lib/listen";
import { findTranscriptWordMatches } from "../../lib/transcriptFuzzy";
import { followJob } from "../../lib/followJob";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useStore } from "../../store";
import { usePublishEditor } from "../../lib/editorBridge";
import { flash } from "../../lib/mcpBridge";
import Transcript, { type Selection } from "./Transcript";
import ListenSettings from "./ListenSettings";
import ListenExport from "./ListenExport";
import ReferenceCheck from "./ReferenceCheck";

const RATES = [0.75, 1, 1.25, 1.5, 2];

/** A recording and its transcript: playback, recognition, speakers, editing, cutting by text, exports. */
export default function ListenEditor({ pid, onBack, onOpen }: { pid: string; onBack: () => void; onOpen: (pid: string) => void }) {
  const { t } = useTranslation();
  const { project, error, saving, edit, undo, redo, canUndo, canRedo } = useListenProject(pid);
  const audio = useRef<HTMLAudioElement>(null);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [rate, setRate] = useState(1);
  const stopAt = useRef<number | null>(null);
  const [selection, setSelection] = useState<Selection>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [dialog, setDialog] = useState<"settings" | "export" | "check" | null>(null);
  const [query, setQuery] = useState("");
  const [searching, setSearching] = useState(false);
  const [match, setMatch] = useState(0);
  const [hiddenSpeakers, setHiddenSpeakers] = useState<Set<string>>(new Set());
  const searchBox = useRef<HTMLInputElement>(null);

  const lines = useMemo(() => project?.lines ?? [], [project]);
  const tokens = useMemo(() => tokensOf(lines), [lines]);
  const active = useMemo(() => activeToken(tokens, time), [tokens, time]);
  const found = useMemo(() => findTranscriptWordMatches(tokens.map((x) => x.text), query), [tokens, query]);
  const cutSecs = useMemo(() => cutSeconds(lines), [lines]);
  usePublishEditor({
    kind: "listening",
    pid,
    state: () => ({
      name: project?.name ?? null,
      duration: project?.duration ?? null,
      lines: lines.map((l) => ({ id: l.id, speaker: project?.speakers[l.speaker] ?? l.speaker, start: l.start, end: l.end, text: l.text.slice(0, 160), hidden: l.hidden })),
      selected_words: selection ? Math.abs(selection.focus - selection.anchor) + 1 : 0,
      time,
      playing,
      working: busy,
    }),
    select: (id) => {
      const index = lines.findIndex((l) => l.id === id);
      if (index < 0) return false;
      const words = tokens.filter((x) => x.line === index);
      if (words.length) setSelection({ anchor: words[0].index, focus: words[words.length - 1].index });
      const row = document.querySelector(`[data-line="${index}"]`);
      row?.scrollIntoView({ block: "center", behavior: "smooth" });
      flash(row);
      return true;
    },
  });

  useEffect(() => {
    if (!playing) return;
    let frame = 0;
    const tick = () => {
      const a = audio.current;
      if (a) {
        setTime(a.currentTime);
        if (stopAt.current != null && a.currentTime >= stopAt.current) { a.pause(); stopAt.current = null; }
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [playing]);

  const seek = useCallback((at: number) => {
    const a = audio.current;
    if (!a) return;
    a.currentTime = Math.max(0, at);
    setTime(a.currentTime);
  }, []);
  const toggle = useCallback(() => {
    const a = audio.current;
    if (!a) return;
    if (a.paused) { stopAt.current = null; void a.play(); } else a.pause();
  }, []);
  const playLine = useCallback((l: ListenLine) => {
    const a = audio.current;
    if (!a) return;
    a.currentTime = l.start;
    stopAt.current = l.end;
    void a.play();
  }, []);

  const lineEdit = useCallback((change: (lines: ListenLine[]) => ListenLine[]) => edit((p) => ({ ...p, lines: change(p.lines) })), [edit]);
  const onSpeaker = useCallback((line: number, key: string) => edit((p) => {
    const speakers = { ...p.speakers };
    let k = key;
    if (key === "+") {
      const [nk, name] = newSpeaker(speakers, (n) => t("listen.speakerN", { n }));
      speakers[nk] = name;
      k = nk;
    }
    return { ...p, speakers, lines: p.lines.map((l, i) => (i === line ? { ...l, speaker: k } : l)) };
  }), [edit, t]);
  const onText = useCallback((line: number, text: string) => lineEdit((ls) => ls.map((l, i) => (i === line ? setLineText(l, text) : l))), [lineEdit]);
  const onHide = useCallback((line: number, hidden: boolean) => lineEdit((ls) => ls.map((l, i) => (i === line ? { ...l, hidden } : l))), [lineEdit]);
  const onMerge = useCallback((line: number) => lineEdit((ls) => mergeNext(ls, line)), [lineEdit]);
  const onToggleCut = useCallback(() => {
    if (!selection) return;
    lineEdit((ls) => toggleCut(ls, tokensOf(ls), selection.anchor, selection.focus));
  }, [selection, lineEdit]);
  const onSplit = useCallback(() => {
    if (!selection) return;
    const tok = tokens[Math.min(selection.anchor, selection.focus)];
    if (!tok) return;
    lineEdit((ls) => splitLine(ls, tok.line, tok.word));
    setSelection(null);
  }, [selection, tokens, lineEdit]);

  const run = async (what: "transcribe" | "cut") => {
    setBusy(what);
    setNotice(null);
    try {
      if (what === "transcribe") {
        const { job_id } = await listenApi.transcribe(pid);
        await followJob(job_id, "analyze");
      } else {
        const { job_id } = await listenApi.cut(pid);
        const r = (await followJob(job_id, "remix")) as { pid: string };
        onOpen(r.pid);
      }
    } catch (e) { setNotice(errorText(e)); } finally { setBusy(null); }
  };
  const toSpeech = async () => {
    setBusy("speech");
    try {
      const r = await listenApi.toSpeech(pid);
      try { localStorage.setItem("speech-studio:speech-project", r.pid); } catch { /* Speech opens on its start screen then */ }
      useStore.getState().setMode("speech");
    } catch (e) { setNotice(errorText(e)); } finally { setBusy(null); }
  };
  const makeVoices = async (speaker?: string) => {
    setBusy("voices");
    setNotice(null);
    try {
      const r = await listenApi.voices(pid, speaker);
      setNotice([r.made.length ? t("listen.voicesMade", { names: r.made.map((m) => m.voice).join(", ") }) : "", r.skipped.length ? t("listen.voicesSkipped", { names: r.skipped.join(", ") }) : ""].filter(Boolean).join(" "));
    } catch (e) { setNotice(errorText(e)); } finally { setBusy(null); }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLElement && (e.target.closest("input, textarea, select, [contenteditable]") != null);
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") { e.preventDefault(); setSearching(true); setTimeout(() => searchBox.current?.focus(), 0); }
      else if (typing) return;
      else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") { e.preventDefault(); if (e.shiftKey) redo(); else undo(); }
      else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "y") { e.preventDefault(); redo(); }
      else if (e.key === " " && !(e.target instanceof HTMLButtonElement)) { e.preventDefault(); toggle(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo, redo, toggle]);

  const goMatch = (dir: 1 | -1) => {
    if (!found.spans.length) return;
    const next = (match + dir + found.spans.length) % found.spans.length;
    setMatch(next);
    const span = found.spans[next];
    setSelection({ anchor: span.start, focus: span.end });
    seek(tokens[span.start].start);
  };

  if (!project) {
    return <main className="flex-1 grid place-items-center">{error ? <p role="alert" className="text-[13px] text-[var(--color-warn)]">{error}</p> : <Loader2 size={18} className="animate-spin text-[var(--color-muted)]" />}</main>;
  }
  const bar = "inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)] disabled:opacity-40";
  const shown = lines.filter((l) => !l.hidden).length;
  const flagged = lines.filter((l) => l.hidden && l.flag).length;
  const selCount = selection ? Math.abs(selection.focus - selection.anchor) + 1 : 0;
  const selCut = selection ? tokens.slice(Math.min(selection.anchor, selection.focus), Math.max(selection.anchor, selection.focus) + 1).every((x) => x.cut) : false;
  const speakerLines = (k: string) => lines.filter((l) => l.speaker === k && !l.hidden).length;

  return (
    <main className="flex-1 min-h-0 flex flex-col">
      <header className="px-5 pt-3 pb-2 border-b border-[var(--color-border)] space-y-2">
        <div className="flex items-center gap-2">
          <button onClick={onBack} className="p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("listen.back"))}><ArrowLeft size={16} /></button>
          <input value={project.name} onChange={(e) => edit((p) => ({ ...p, name: e.target.value }))} aria-label={t("speech.name")}
            className="min-w-0 flex-1 bg-transparent text-[15px] font-semibold rounded px-1 border border-transparent hover:border-[var(--color-border)] focus:border-[var(--color-accent)] focus:outline-none" />
          <span className="text-[11px] text-[var(--color-muted)] tabnum whitespace-nowrap">
            {project.made ? t("listen.status", { lines: shown, speakers: Object.keys(project.speakers).length, by: project.made.by }) : t("listen.notYet")}
            {saving ? ` · ${t("speech.saving")}` : ""}
          </span>
          <button onClick={undo} disabled={!canUndo} className="p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("speech.undo"))}><Undo2 size={15} /></button>
          <button onClick={redo} disabled={!canRedo} className="p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("speech.redo"))}><Redo2 size={15} /></button>
        </div>
        <div className="flex flex-wrap items-center gap-1.5">
          <button onClick={() => setDialog("settings")} className={bar}><Settings2 size={13} />{t("listen.settings")}</button>
          <button onClick={() => void run("transcribe")} disabled={!!busy} className={`${bar} ${project.made ? "" : "bg-[var(--color-accent)] text-[var(--color-on-accent)] border-transparent font-semibold"}`}>
            {busy === "transcribe" ? <Loader2 size={13} className="animate-spin" /> : <Wand2 size={13} />}{project.made ? t("listen.retranscribe") : t("listen.transcribe")}
          </button>
          <button onClick={() => { setSearching((v) => !v); setTimeout(() => searchBox.current?.focus(), 0); }} disabled={!shown} className={bar}><Search size={13} />{t("listen.find")}</button>
          <button onClick={() => setDialog("export")} disabled={!shown} className={bar}><Download size={13} />{t("listen.export")}</button>
          <button onClick={() => setDialog("check")} disabled={!shown} className={bar}><FileCheck2 size={13} />{t("listen.check")}</button>
          <button onClick={() => void toSpeech()} disabled={!shown || !!busy} className={bar}><AudioLines size={13} />{t("listen.toSpeech")}</button>
          <button onClick={() => void makeVoices()} disabled={!shown || !!busy} className={bar}>{busy === "voices" ? <Loader2 size={13} className="animate-spin" /> : <UserPlus size={13} />}{t("listen.voicesAll")}</button>
        </div>
        {searching && (
          <div role="search" className="flex flex-wrap items-center gap-2">
            <input ref={searchBox} value={query} onChange={(e) => { setQuery(e.target.value); setMatch(0); }} onKeyDown={(e) => { if (e.key === "Enter") goMatch(e.shiftKey ? -1 : 1); if (e.key === "Escape") { setSearching(false); setQuery(""); } }}
              placeholder={t("listen.findPlaceholder")} aria-label={t("listen.find")} className="w-64 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] focus:border-[var(--color-accent)] focus:outline-none" />
            <button onClick={() => goMatch(-1)} disabled={!found.spans.length} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("search.prev"))}><ChevronUp size={14} /></button>
            <button onClick={() => goMatch(1)} disabled={!found.spans.length} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("search.next"))}><ChevronDown size={14} /></button>
            <span className="text-[11px] tabnum text-[var(--color-muted)]">{query ? (found.approximate ? t("listen.foundSimilar", { n: found.spans.length }) : t("listen.found", { n: found.spans.length })) : ""}</span>
            <button onClick={() => { setSearching(false); setQuery(""); }} className="ml-auto p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("a11y.close"))}><X size={14} /></button>
          </div>
        )}
      </header>

      <div className="px-5 py-2 border-b border-[var(--color-border)] flex items-center gap-3">
        <button onClick={toggle} className="grid place-items-center w-8 h-8 rounded-full bg-[var(--color-accent)] text-[var(--color-on-accent)]" {...named(playing ? t("listen.pause") : t("listen.play"))}>{playing ? <Pause size={14} /> : <Play size={14} />}</button>
        <span className="mono text-[12px] tabnum text-[var(--color-muted)] w-24">{clock(time)} / {clock(duration || project.duration)}</span>
        <input type="range" min={0} max={duration || project.duration || 1} step={0.05} value={time} onChange={(e) => seek(Number(e.target.value))} aria-label={t("listen.position")} className="flex-1 accent-[var(--color-accent)]" />
        <select value={rate} onChange={(e) => { const r = Number(e.target.value); setRate(r); if (audio.current) audio.current.playbackRate = r; }} aria-label={t("listen.rate")}
          className="bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-1.5 py-1 text-[12px] tabnum">
          {RATES.map((r) => <option key={r} value={r}>{r}×</option>)}
        </select>
        <audio ref={audio} src={listenApi.audioUrl(pid)} preload="metadata"
          onLoadedMetadata={(e) => setDuration(e.currentTarget.duration)} onPlay={() => setPlaying(true)} onPause={() => { setPlaying(false); setTime(audio.current?.currentTime ?? 0); }}
          onEnded={() => setPlaying(false)} onSeeked={(e) => setTime(e.currentTarget.currentTime)} />
      </div>

      {Object.keys(project.speakers).length > 0 && (
        <div className="px-5 py-2 border-b border-[var(--color-border)] flex flex-wrap items-center gap-2">
          <Users size={13} className="text-[var(--color-muted)]" />
          {Object.entries(project.speakers).map(([k, name]) => (
            <span key={k} className={`inline-flex items-center gap-1.5 pl-2 pr-1 py-0.5 rounded-full border text-[12px] ${hiddenSpeakers.has(k) ? "opacity-50 border-dashed border-[var(--color-border)]" : "border-[var(--color-border)]"}`}>
              <span className="w-2 h-2 rounded-full" style={{ background: speakerColor(k) }} />
              <input value={name} onChange={(e) => edit((p) => ({ ...p, speakers: { ...p.speakers, [k]: e.target.value } }))} aria-label={t("listen.speakerName")} size={Math.max(6, name.length)}
                className="bg-transparent focus:outline-none" />
              <span className="text-[10px] tabnum text-[var(--color-muted)]">{speakerLines(k)}</span>
              <button onClick={() => setHiddenSpeakers((s) => { const n = new Set(s); if (n.has(k)) n.delete(k); else n.add(k); return n; })} className="p-0.5 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(hiddenSpeakers.has(k) ? t("listen.showSpeaker") : t("listen.hideSpeaker"))}>{hiddenSpeakers.has(k) ? <EyeOff size={12} /> : <Eye size={12} />}</button>
              <button onClick={() => void makeVoices(k)} disabled={!!busy} className="p-0.5 rounded text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("listen.voiceOf", { name }))}><UserPlus size={12} /></button>
            </span>
          ))}
          {flagged > 0 && <span className="ml-auto text-[11px] text-[var(--color-muted)]" title={t("listen.flagHint")}>{t("listen.flaggedHidden", { n: flagged })}</span>}
        </div>
      )}

      {(notice || error) && <p role="alert" className="px-5 py-1.5 text-[12px] text-[var(--color-warn)] border-b border-[var(--color-border)] break-words">{notice || error}</p>}

      <div className="flex-1 min-h-0 overflow-y-auto px-3 py-3">
        {lines.length === 0 ? (
          <div className="max-w-md mx-auto mt-16 text-center space-y-3">
            <p className="text-[14px]">{t("listen.emptyTitle")}</p>
            <p className="text-[12px] text-[var(--color-muted)]">{t("listen.emptyHint")}</p>
            <button onClick={() => void run("transcribe")} disabled={!!busy} className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">
              {busy === "transcribe" ? <Loader2 size={14} className="animate-spin" /> : <Wand2 size={14} />}{t("listen.transcribe")}
            </button>
          </div>
        ) : (
          <Transcript lines={lines} tokens={tokens} speakers={project.speakers} hiddenSpeakers={hiddenSpeakers} time={time} playing={playing} activeIndex={active}
            selection={selection} onSelection={setSelection} matches={found.spans} currentMatch={match}
            onSeek={seek} onPlayLine={playLine} onSpeaker={onSpeaker} onText={onText} onHide={onHide} onMerge={onMerge}
            onToggleCut={onToggleCut} onSplit={onSplit} newSpeakerLabel={t("listen.newSpeaker")} />
        )}
      </div>

      {(selection || cutSecs > 0) && (
        <footer className="px-5 py-2 border-t border-[var(--color-border)] flex flex-wrap items-center gap-2 bg-[var(--color-surface)]">
          {selection && (
            <>
              <span className="text-[12px] text-[var(--color-muted)]">{t("listen.selected", { n: selCount })}</span>
              <button onClick={onToggleCut} className={bar}><Scissors size={13} />{selCut ? t("listen.uncut") : t("listen.cut")}</button>
              <button onClick={onSplit} className={bar}>{t("listen.split")}</button>
              <button onClick={() => setSelection(null)} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("a11y.clearSelection"))}><X size={13} /></button>
            </>
          )}
          {cutSecs > 0 && (
            <>
              <span className="ml-auto text-[12px] text-[var(--color-muted)] tabnum">{t("listen.marked", { time: cutSecs.toFixed(1) })}</span>
              <button onClick={() => lineEdit(restoreAll)} className={bar}>{t("listen.restoreAll")}</button>
              <button onClick={() => void run("cut")} disabled={!!busy} className={`${bar} bg-[var(--color-accent)] text-[var(--color-on-accent)] border-transparent font-semibold`}>
                {busy === "cut" ? <Loader2 size={13} className="animate-spin" /> : <Scissors size={13} />}{t("listen.applyCut")}
              </button>
            </>
          )}
        </footer>
      )}

      {dialog === "settings" && <ListenSettings project={project} onChange={(p) => edit(() => p)} onClose={() => setDialog(null)} />}
      {dialog === "export" && <ListenExport pid={pid} onClose={() => setDialog(null)} />}
      {dialog === "check" && <ReferenceCheck pid={pid} reference={project.reference} onReference={(reference) => edit((p) => ({ ...p, reference }))} onClose={() => setDialog(null)} />}
    </main>
  );
}
