import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown, ArrowUp, ChevronDown, Combine, Eye, EyeOff, Loader2, Pause, Pin, Play, Plus, RefreshCw, Scissors, SlidersHorizontal, Trash2 } from "lucide-react";
import { activeTake, type Block, type BlockState } from "../../lib/speech";
import { named } from "../../lib/a11y";
import VoicePicker from "./VoicePicker";

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

export default function BlockRow({ block, index, state, rendering, playing, tint, a }: {
  block: Block;
  index: number;
  state: BlockState;
  rendering: boolean;
  playing: boolean;
  tint: string;
  a: BlockActions;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const text = useRef<HTMLTextAreaElement>(null);
  const take = activeTake(block);
  const shown: BlockState | "rendering" = rendering ? "rendering" : state;
  const icon = "p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-2)] disabled:opacity-30";
  return (
    <li className={`group rounded-xl border transition-colors ${playing ? "border-[var(--color-accent)]" : "border-transparent hover:border-[var(--color-border)]"} ${block.hidden ? "opacity-50" : ""}`}>
      <div className="flex gap-3 px-2 py-2">
        <div className="w-28 shrink-0 pt-0.5 space-y-1">
          <input list="speech-speakers" value={block.speaker} onChange={(e) => a.patch({ speaker: e.target.value })} aria-label={t("speech.speaker")}
            className="w-full bg-transparent text-[12px] font-semibold truncate focus:outline-none rounded px-1 -mx-1 focus:bg-[var(--color-surface-2)]" style={{ color: tint }} />
          <div className="flex items-center gap-1.5 text-[10px] text-[var(--color-muted)]">
            <span className={`w-2 h-2 rounded-full shrink-0 ${STATE_DOT[shown]}`} title={t(`speech.state_${shown}`)} />
            <span>{t(`speech.state_${shown}`)}</span>
            {take && <span className="mono tabnum ml-auto">{take.duration.toFixed(1)}{t("settings.sec")}</span>}
          </div>
        </div>
        <div className="flex-1 min-w-0">
          <AutoText value={block.text} onCommit={a.setText} label={t("speech.blockText", { n: index + 1 })} inputRef={text} />
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
                  {[...block.takes].reverse().map((tk) => <option key={tk.n} value={tk.n}>#{tk.n} · {tk.duration.toFixed(1)}{t("settings.sec")}{tk.n === block.pinned ? ` · ${t("speech.pinned")}` : ""}</option>)}
                </select>
                <button onClick={() => a.patch({ pinned: block.pinned === block.active ? null : block.active })} disabled={block.active == null} aria-pressed={block.pinned != null && block.pinned === block.active}
                  className={`p-1 rounded-md ${block.pinned != null && block.pinned === block.active ? "text-[var(--color-accent)]" : "text-[var(--color-muted)]"} hover:text-[var(--color-text)] disabled:opacity-30`} {...named(t("speech.pinTake"))}><Pin size={13} /></button>
              </div>
            </label>
          </div>
          <label className="block text-[11px] text-[var(--color-muted)]">{t("speech.direction")}
            <input value={block.direction} onChange={(e) => a.patch({ direction: e.target.value })} placeholder={t("speech.directionHint")}
              className="mt-1 w-full bg-[var(--color-surface)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px]" />
          </label>
          <div className="flex flex-wrap gap-1">
            <button onClick={() => a.split(text.current?.selectionStart ?? block.text.length)} className={icon + " inline-flex items-center gap-1 text-[11px]"}><Scissors size={12} />{t("speech.split")}</button>
            <button onClick={a.mergeNext} className={icon + " inline-flex items-center gap-1 text-[11px]"}><Combine size={12} />{t("speech.mergeNext")}</button>
            <button onClick={a.insertAfter} className={icon + " inline-flex items-center gap-1 text-[11px]"}><Plus size={12} />{t("speech.insertAfter")}</button>
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
