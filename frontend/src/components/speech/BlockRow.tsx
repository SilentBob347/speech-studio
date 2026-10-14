import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown, ArrowUp, ChevronDown, Combine, Eye, EyeOff, Loader2, Pause, Pin, Play, Plus, RefreshCw, Scissors, SlidersHorizontal, SpellCheck, Trash2 } from "lucide-react";
import { activeTake, MARKERS, withCase, type Block, type BlockState, type Marker, type StressWord } from "../../lib/speech";
import { named } from "../../lib/a11y";
import VoicePicker from "./VoicePicker";
import TagPalette from "./TagPalette";
import PrepareView from "./PrepareView";

const STATE_DOT: Record<BlockState | "rendering", string> = {
  pending: "bg-[var(--color-muted)]",
  ready: "bg-[var(--color-accent)]",
  stale: "bg-[var(--color-warn)]",
  failed: "bg-[#ff6b6b]",
  skipped: "bg-transparent border border-[var(--color-border)]",
  rendering: "bg-[var(--color-accent)] animate-pulse",
};

export type BlockActions = {
  setText: (text: string) => void;
  patch: (change: Partial<Block>) => void;
  split: (at: number) => void;
  mergeNext: () => void;
  insertAfter: () => void;
  move: (dir: -1 | 1) => void;
  remove: () => void;
  synth: (force: boolean) => void;
  play: () => void;
  playFrom: () => void;
  stress: () => void;
  pick: (w: StressWord, value: string) => void;
};

function AutoText({ value, onCommit, label, inputRef }: { value: string; onCommit: (v: string) => void; label: string; inputRef: React.RefObject<HTMLTextAreaElement | null> }) {
  const [draft, setDraft] = useState(value);
  const [seen, setSeen] = useState(value);
  if (value !== seen) { setSeen(value); setDraft(value); }
  useLayoutEffect(() => {
    const el = inputRef.current;
    if (el) { el.style.height = "auto"; el.style.height = `${el.scrollHeight}px`; }
  }, [draft, inputRef]);
  useEffect(() => {
    if (draft === value) return;
    const timer = window.setTimeout(() => onCommit(draft), 700);
    return () => window.clearTimeout(timer);
  }, [draft, value, onCommit]);
  return (
    <textarea ref={inputRef} value={draft} rows={1} aria-label={label} spellCheck
      onChange={(e) => setDraft(e.target.value)} onBlur={() => { if (draft !== value) onCommit(draft); }}
      className="w-full resize-none overflow-hidden bg-transparent text-[14px] leading-relaxed focus:outline-none placeholder:text-[var(--color-muted)]" />
  );
}

export default function BlockRow({ pid, block, index, state, rendering, playing, tint, stressing, doubts, a }: {
  pid: string;
  block: Block;
  stressing: boolean;
  doubts: StressWord[];
  index: number;
  state: BlockState;
  rendering: boolean;
  playing: boolean;
  tint: string;
  a: BlockActions;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [howRead, setHowRead] = useState(false);
  const text = useRef<HTMLTextAreaElement>(null);
  const take = activeTake(block);
  const insertTag = (tag: string) => {
    const el = text.current;
    const value = el?.value ?? block.text;
    const at = el?.selectionStart ?? value.length;
    const before = value.slice(0, at);
    a.setText(`${before}${before && !before.endsWith(" ") ? " " : ""}${tag} ${value.slice(at).trimStart()}`.trimEnd());
  };
  const shown: BlockState | "rendering" = rendering ? "rendering" : state;
  const icon = "p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-2)] disabled:opacity-30";
  return (
    <li id={`block-${block.id}`} className={`group rounded-xl border transition-colors ${playing ? "border-[var(--color-accent)]" : "border-transparent hover:border-[var(--color-border)]"} ${block.hidden ? "opacity-50" : ""}`}>
      <div className="flex gap-3 px-2 py-2">
        <div className="w-28 shrink-0 pt-0.5 space-y-1">
          <input list="speech-speakers" value={block.speaker} onChange={(e) => a.patch({ speaker: e.target.value })} aria-label={t("speech.speaker")}
            className="w-full bg-transparent text-[12px] font-semibold truncate focus:outline-none rounded px-1 -mx-1 focus:bg-[var(--color-surface-2)]" style={{ color: tint }} />
          <div className="flex items-center gap-1.5 text-[10px] text-[var(--color-muted)]">
            <span className={`w-2 h-2 rounded-full shrink-0 ${STATE_DOT[shown]}`} title={t(`speech.state_${shown}`)} />
            <span>{t(`speech.state_${shown}`)}</span>
            {take && <span className="mono tabnum ml-auto">{take.duration.toFixed(1)}{t("settings.sec")}</span>}
          </div>
          {(block.marker || block.note) && (
            <div className="flex items-center gap-1 text-[10px] text-[var(--color-muted)]" title={block.note || undefined}>
              {block.marker && <span className="w-2 h-2 rounded-sm shrink-0" style={{ background: MARKERS[block.marker] }} />}
              {block.note && <span className="truncate italic">{block.note}</span>}
            </div>
          )}
        </div>
        <div className="flex-1 min-w-0">
          <AutoText value={block.text} onCommit={a.setText} label={t("speech.blockText", { n: index + 1 })} inputRef={text} />
          {doubts.length > 0 && (
            <div className="mt-1 flex flex-wrap items-center gap-1 text-[11px]">
              <span className="text-[var(--color-muted)]">{t("speech.stressCheck")}</span>
              {doubts.map((w) => (
                <span key={`${w.start}-${w.word}`} title={w.doubt ? t("speech.stressDoubt") : undefined}
                  className={`inline-flex items-center gap-0.5 rounded-md border px-1 py-0.5 ${w.doubt || w.yo_choice ? "border-[var(--color-warn)]/60" : "border-[var(--color-border)]"}`}>
                  {w.yo_choice ? (
                    <>
                      <button onClick={() => a.pick(w, w.word)} className="px-1 rounded hover:text-[var(--color-accent)]" title={t("speech.stressKeep")}>{w.word}</button>
                      <span className="text-[var(--color-muted)]">/</span>
                      <button onClick={() => a.pick(w, w.yo_choice!)} className="px-1 rounded hover:text-[var(--color-accent)]">{w.yo_choice}</button>
                    </>
                  ) : (w.variants ?? []).map(([v, p], k) => {
                    const shown = withCase(w.word, v);
                    return (
                      <span key={v} className="inline-flex items-center">
                        {k > 0 && <span className="text-[var(--color-muted)] px-0.5">/</span>}
                        <button onClick={() => a.pick(w, shown)} aria-pressed={shown === w.word}
                          className={`px-1 rounded mono hover:text-[var(--color-accent)] ${shown === w.word ? "text-[var(--color-accent)]" : ""}`}>{shown} <span className="text-[var(--color-muted)] tabnum">{Math.round(p * 100)}%</span></button>
                      </span>
                    );
                  })}
                </span>
              ))}
            </div>
          )}
          {take && take.text && take.text !== block.text && (
            <p className="mt-0.5 flex flex-wrap items-center gap-1.5 text-[11px] text-[var(--color-muted)]">
              <span className="truncate max-w-[60ch]" title={take.text}>{t("speech.takeText", { n: take.n, text: take.text })}</span>
              <button onClick={() => a.patch({ text: take.text, direction: take.direction })} className="px-1.5 rounded border border-[var(--color-border)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]">{t("speech.takeTextRestore")}</button>
            </p>
          )}
          {take?.qc && <p className={`mt-0.5 text-[11px] ${take.qc.passed ? "text-[var(--color-muted)]" : "text-[var(--color-warn)]"}`} title={take.qc.heard}>{t("speech.qcLine", { wer: Math.round(take.qc.wer * 100), ins: take.qc.inserted, del: take.qc.deleted, sub: take.qc.substituted })}</p>}
          {block.error && state === "failed" && <p role="alert" className="mt-1 text-[11px] text-[#ff8f8f] break-words">{block.error.text}</p>}
          {block.direction && !open && <p className="mt-0.5 text-[11px] text-[var(--color-muted)] italic truncate">{t("speech.directionShort", { text: block.direction })}</p>}
        </div>
        <div className="flex items-start gap-0.5 shrink-0 opacity-70 group-hover:opacity-100 focus-within:opacity-100">
          <button onClick={a.play} disabled={!take} className={icon} {...named(playing ? t("speech.stopPlaying") : t("speech.play"))}>{playing ? <Pause size={14} /> : <Play size={14} />}</button>
          <button onClick={() => a.synth(state === "ready")} disabled={rendering || state === "skipped"} className={icon} {...named(state === "ready" ? t("speech.newTake") : t("speech.voiceBlock"))}>
            {rendering ? <Loader2 size={14} className="animate-spin" /> : <RefreshCw size={14} />}
          </button>
          <button onClick={() => setOpen((o) => !o)} aria-expanded={open} className={icon} {...named(t("speech.blockMore"))}>{open ? <ChevronDown size={14} className="rotate-180" /> : <SlidersHorizontal size={14} />}</button>
        </div>
      </div>
      {open && (
        <div className="mx-2 mb-2 ml-[8.25rem] rounded-lg bg-[var(--color-surface-2)]/60 border border-[var(--color-border)] p-2.5 space-y-2">
          <div className="grid gap-2 sm:grid-cols-3">
            <label className="text-[11px] text-[var(--color-muted)]">{t("speech.blockVoice")}
              <VoicePicker value={block.voice} onChange={(v) => a.patch({ voice: v })} inherit={t("speech.voiceAsCast")} label={t("speech.blockVoice")}
                className="mt-1 w-full bg-[var(--color-surface)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px]" />
            </label>
            <label className="text-[11px] text-[var(--color-muted)]">{t("speech.pauseAfter")}
              <input type="number" step={50} min={-2000} max={10000} value={block.pause_ms ?? ""} placeholder={t("speech.pauseAuto")}
                onChange={(e) => a.patch({ pause_ms: e.target.value === "" ? null : Math.max(-2000, Math.min(10000, Number(e.target.value))) })}
                className="mt-1 w-full bg-[var(--color-surface)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] mono" />
            </label>
            <label className="text-[11px] text-[var(--color-muted)]">{t("speech.takes")}
              <div className="mt-1 flex items-center gap-1">
                <select value={block.active ?? ""} onChange={(e) => a.patch({ active: e.target.value ? Number(e.target.value) : null })} disabled={!block.takes.length}
                  className="flex-1 min-w-0 bg-[var(--color-surface)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px]">
                  {!block.takes.length && <option value="">—</option>}
                  {[...block.takes].reverse().map((tk) => <option key={tk.n} value={tk.n}>#{tk.n} · {tk.duration.toFixed(1)}{t("settings.sec")}{tk.qc ? ` · ${t("speech.takeWer", { wer: Math.round(tk.qc.wer * 100) })}` : ""}{tk.n === block.pinned ? ` · ${t("speech.pinned")}` : ""}</option>)}
                </select>
                <button onClick={() => a.patch({ pinned: block.pinned === block.active ? null : block.active })} disabled={block.active == null} aria-pressed={block.pinned != null && block.pinned === block.active}
                  className={`p-1 rounded-md ${block.pinned != null && block.pinned === block.active ? "text-[var(--color-accent)]" : "text-[var(--color-muted)]"} hover:text-[var(--color-text)] disabled:opacity-30`} {...named(t("speech.pinTake"))}><Pin size={13} /></button>
              </div>
            </label>
          </div>
          <div className="flex flex-wrap items-end gap-2">
            <label className="flex-1 min-w-48 text-[11px] text-[var(--color-muted)]">{t("speech.note")}
              <input value={block.note} onChange={(e) => a.patch({ note: e.target.value })} placeholder={t("speech.noteHint")}
                className="mt-1 w-full bg-[var(--color-surface)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px]" />
            </label>
            <div role="radiogroup" aria-label={t("speech.marker")} className="flex items-center gap-1 pb-1">
              <button role="radio" aria-checked={!block.marker} onClick={() => a.patch({ marker: "" })} className={`w-4 h-4 rounded-sm border ${!block.marker ? "border-[var(--color-text)]" : "border-[var(--color-border)]"}`} {...named(t("speech.markerNone"))} />
              {(Object.keys(MARKERS) as Marker[]).map((m) => (
                <button key={m} role="radio" aria-checked={block.marker === m} onClick={() => a.patch({ marker: m })} {...named(t(`speech.marker_${m}`))}
                  className={`w-4 h-4 rounded-sm border-2 ${block.marker === m ? "border-[var(--color-text)]" : "border-transparent"}`} style={{ background: MARKERS[m] }} />
              ))}
            </div>
          </div>
          <label className="block text-[11px] text-[var(--color-muted)]">{t("speech.direction")}
            <input value={block.direction} onChange={(e) => a.patch({ direction: e.target.value })} placeholder={t("speech.directionHint")}
              className="mt-1 w-full bg-[var(--color-surface)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px]" />
          </label>
          <details open={howRead} onToggle={(e) => setHowRead(e.currentTarget.open)} className="rounded-md border border-[var(--color-border)] px-2 py-1.5">
            <summary className="cursor-pointer text-[11px] text-[var(--color-muted)]">{t("speech.howRead")}</summary>
            {howRead && <div className="mt-2"><PrepareView pid={pid} block={block.id} text={block.text} /></div>}
          </details>
          <details className="rounded-md border border-[var(--color-border)] px-2 py-1.5">
            <summary className="cursor-pointer text-[11px] text-[var(--color-muted)]">{t("speech.tagsTitle")}</summary>
            <div className="mt-2"><TagPalette onInsert={insertTag} /></div>
          </details>
          <div className="flex flex-wrap gap-1">
            <button onClick={() => a.split(text.current?.selectionStart ?? block.text.length)} className={icon + " inline-flex items-center gap-1 text-[11px]"}><Scissors size={12} />{t("speech.split")}</button>
            <button onClick={a.mergeNext} className={icon + " inline-flex items-center gap-1 text-[11px]"}><Combine size={12} />{t("speech.mergeNext")}</button>
            <button onClick={a.insertAfter} className={icon + " inline-flex items-center gap-1 text-[11px]"}><Plus size={12} />{t("speech.insertAfter")}</button>
            <button onClick={a.stress} disabled={stressing} className={icon + " inline-flex items-center gap-1 text-[11px]"} title={t("speech.stressAllHint")}>
              {stressing ? <Loader2 size={12} className="animate-spin" /> : <SpellCheck size={12} />}{t("speech.stressBlock")}
            </button>
            <button onClick={() => a.move(-1)} className={icon} {...named(t("speech.moveUp"))}><ArrowUp size={12} /></button>
            <button onClick={() => a.move(1)} className={icon} {...named(t("speech.moveDown"))}><ArrowDown size={12} /></button>
            <button onClick={() => a.patch({ hidden: !block.hidden })} className={icon + " inline-flex items-center gap-1 text-[11px]"}>{block.hidden ? <Eye size={12} /> : <EyeOff size={12} />}{block.hidden ? t("speech.show") : t("speech.hide")}</button>
            <button onClick={a.playFrom} disabled={!take} className={icon + " inline-flex items-center gap-1 text-[11px]"}><Play size={12} />{t("speech.playFromHere")}</button>
            <button onClick={a.remove} className={icon + " ml-auto inline-flex items-center gap-1 text-[11px] hover:text-[var(--color-warn)]"}><Trash2 size={12} />{t("speech.deleteBlock")}</button>
          </div>
        </div>
      )}
    </li>
  );
}
