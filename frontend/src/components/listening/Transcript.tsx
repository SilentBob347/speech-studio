import { memo, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Eye, EyeOff, Merge, Pencil, Play } from "lucide-react";
import { clock, speakerColor, type ListenLine, type Token } from "../../lib/listen";
import { named } from "../../lib/a11y";

export type Selection = { anchor: number; focus: number } | null;

type RowProps = {
  line: ListenLine; index: number; first: number; speakers: Record<string, string>;
  active: number; selected: [number, number] | null; matches: Set<number>; current: Set<number>;
  onSeek: (t: number) => void; onPlayLine: (l: ListenLine) => void; onSpeaker: (line: number, key: string) => void;
  onText: (line: number, text: string) => void; onHide: (line: number, hidden: boolean) => void; onMerge: (line: number) => void;
  newSpeakerLabel: string; canMerge: boolean;
};

/** One line: speaker, time, the words (sounding, selected, marked for removal, found) and its actions. */
const Row = memo(function Row({ line, index, first, speakers, active, selected, matches, current, onSeek, onPlayLine, onSpeaker, onText, onHide, onMerge, newSpeakerLabel, canMerge }: RowProps) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(line.text);
  const color = speakerColor(line.speaker);
  if (line.hidden) {
    return (
      <li data-mcp-row={`line ${index + 1} hidden: ${line.text.slice(0, 60)}`} className="flex items-center gap-3 px-3 py-1.5 rounded-lg border border-dashed border-[var(--color-border)] text-[12px] text-[var(--color-muted)]">
        <span className="tabnum w-14 shrink-0">{clock(line.start)}</span>
        <span className="flex-1 min-w-0 truncate line-through" title={line.text}>{line.text}</span>
        <span className="shrink-0">{line.flag ? t(`listen.flag_${line.flag}`) : t("listen.hiddenByYou")}</span>
        <button onClick={() => onHide(index, false)} className="shrink-0 inline-flex items-center gap-1 px-2 py-0.5 rounded border border-[var(--color-border)] hover:border-[var(--color-accent)] hover:text-[var(--color-text)]"><Eye size={12} />{t("listen.unhide")}</button>
      </li>
    );
  }
  return (
    <li data-mcp-row={`line ${index + 1} ${clock(line.start)} ${speakers[line.speaker] ?? line.speaker}${line.flag ? ` ${line.flag}` : ""}: ${line.text.slice(0, 60)}`} className="group grid grid-cols-[minmax(0,1fr)_auto] sm:grid-cols-[7.5rem_minmax(0,1fr)_auto] gap-x-3 gap-y-1 px-3 py-2 rounded-lg border-l-2 hover:bg-[var(--color-surface-2)]/60" style={{ borderLeftColor: color }} data-line={index}>
      <div className="min-w-0 flex flex-wrap items-center gap-x-2.5 gap-y-1 sm:block sm:space-y-1">
        <select value={line.speaker} onChange={(e) => onSpeaker(index, e.target.value)} aria-label={t("listen.speaker")}
          className="max-w-[60%] sm:max-w-none sm:w-full bg-transparent text-[12px] font-semibold truncate rounded border border-transparent hover:border-[var(--color-border)] focus:border-[var(--color-accent)] focus:outline-none" style={{ color }}>
          {Object.entries(speakers).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
          <option value="+">{newSpeakerLabel}</option>
        </select>
        <button onClick={() => onSeek(line.start)} className="block text-[11px] tabnum text-[var(--color-muted)] hover:text-[var(--color-accent)]">{clock(line.start)}</button>
        {line.approx && <span className="block text-[10px] text-[var(--color-muted)]" title={t("listen.approxHint")}>{t("listen.approx")}</span>}
        {line.flag && <span className="block text-[10px] text-[var(--color-warn)]" title={t("listen.flagHint")}>{t(`listen.flag_${line.flag}`)}</span>}
      </div>
      {editing ? (
        <textarea autoFocus value={draft} rows={Math.max(2, Math.ceil(draft.length / 90))} onChange={(e) => setDraft(e.target.value)} aria-label={t("listen.lineText")}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); setEditing(false); if (draft.trim() !== line.text) onText(index, draft.trim()); }
            if (e.key === "Escape") { e.stopPropagation(); setEditing(false); setDraft(line.text); }
          }}
          onBlur={() => { setEditing(false); if (draft.trim() && draft.trim() !== line.text) onText(index, draft.trim()); }}
          className="col-span-2 order-last sm:col-span-1 sm:order-none w-full bg-[var(--color-surface-2)] border border-[var(--color-accent)] rounded-md px-2 py-1 text-[14px] leading-relaxed focus:outline-none resize-y" />
      ) : (
        <p className="col-span-2 order-last sm:col-span-1 sm:order-none text-[14px] leading-relaxed select-none" onDoubleClick={() => { setDraft(line.text); setEditing(true); }}>
          {line.words.map((w, wi) => {
            const i = first + wi;
            const sel = selected && i >= selected[0] && i <= selected[1];
            const cls = [
              "rounded-[3px] px-[1px] cursor-text transition-colors",
              i === active ? "bg-[var(--color-accent)] text-[var(--color-on-accent)]" : sel ? "bg-[color-mix(in_oklab,var(--color-accent)_28%,transparent)]" : current.has(i) ? "bg-[color-mix(in_oklab,var(--color-warn)_45%,transparent)]" : matches.has(i) ? "bg-[color-mix(in_oklab,var(--color-warn)_20%,transparent)]" : "",
              w.cut ? "line-through text-[var(--color-muted)] decoration-[var(--color-warn)] decoration-2" : "",
            ].join(" ");
            return <span key={wi}><span data-token={i} className={cls}>{w.text}</span>{" "}</span>;
          })}
        </p>
      )}
      <div className="flex items-start gap-0.5 pointer-fine:opacity-0 pointer-fine:group-hover:opacity-100 focus-within:opacity-100">
        <button onClick={() => onPlayLine(line)} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("listen.playLine"))}><Play size={13} /></button>
        <button onClick={() => { setDraft(line.text); setEditing(true); }} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("listen.editLine"))}><Pencil size={13} /></button>
        {canMerge && <button onClick={() => onMerge(index)} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("listen.mergeNext"))}><Merge size={13} /></button>}
        <button onClick={() => onHide(index, true)} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("listen.hide"))}><EyeOff size={13} /></button>
      </div>
    </li>
  );
});

/**
 * The transcript as text to read and edit: the sounding word follows playback, a click seeks, dragging or Shift+click
 * selects words, Delete marks the selection for removal (again — returns it), double click edits a line.
 */
export default function Transcript(props: {
  lines: ListenLine[]; tokens: Token[]; speakers: Record<string, string>; hiddenSpeakers: Set<string>; time: number; playing: boolean;
  selection: Selection; onSelection: (s: Selection) => void; matches: { start: number; end: number }[]; currentMatch: number;
  onSeek: (t: number) => void; onPlayLine: (l: ListenLine) => void; onSpeaker: (line: number, key: string) => void;
  onText: (line: number, text: string) => void; onHide: (line: number, hidden: boolean) => void; onMerge: (line: number) => void;
  onToggleCut: () => void; onSplit: () => void; newSpeakerLabel: string; activeIndex: number;
}) {
  const { t } = useTranslation();
  const { lines, tokens, selection, onSelection, matches, currentMatch, activeIndex } = props;
  const box = useRef<HTMLOListElement>(null);
  const dragging = useRef(false);
  const firstOf = useMemo(() => {
    const out: number[] = [];
    let n = 0;
    for (const l of lines) { out.push(n); if (!l.hidden) n += l.words.length; }
    return out;
  }, [lines]);
  const selected: [number, number] | null = selection ? [Math.min(selection.anchor, selection.focus), Math.max(selection.anchor, selection.focus)] : null;
  const matchSet = useMemo(() => new Set(matches.flatMap((m) => Array.from({ length: m.end - m.start + 1 }, (_, k) => m.start + k))), [matches]);
  const currentSet = useMemo(() => {
    const m = matches[currentMatch];
    return new Set(m ? Array.from({ length: m.end - m.start + 1 }, (_, k) => m.start + k) : []);
  }, [matches, currentMatch]);

  useEffect(() => {
    if (!props.playing || activeIndex < 0) return;
    box.current?.querySelector(`[data-token="${activeIndex}"]`)?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, props.playing]);
  useEffect(() => {
    const m = matches[currentMatch];
    if (m) box.current?.querySelector(`[data-token="${m.start}"]`)?.scrollIntoView({ block: "center" });
  }, [matches, currentMatch]);

  const tokenAt = (x: number, y: number): number | null => {
    const el = document.elementFromPoint(x, y)?.closest("[data-token]");
    return el ? Number(el.getAttribute("data-token")) : null;
  };

  return (
    <ol
      ref={box}
      tabIndex={0}
      aria-label={t("listen.transcript")}
      className="space-y-0.5 focus:outline-none"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        const i = tokenAt(e.clientX, e.clientY);
        if (i == null) return;
        dragging.current = true;
        onSelection(e.shiftKey && selection ? { anchor: selection.anchor, focus: i } : { anchor: i, focus: i });
        if (!e.shiftKey) props.onSeek(tokens[i].start);
      }}
      onPointerMove={(e) => {
        if (!dragging.current || !(e.buttons & 1) || !selection) return;
        const i = tokenAt(e.clientX, e.clientY);
        if (i != null && i !== selection.focus) onSelection({ anchor: selection.anchor, focus: i });
      }}
      onPointerUp={() => { dragging.current = false; }}
      onKeyDown={(e) => {
        if ((e.key === "Delete" || e.key === "Backspace") && selection) { e.preventDefault(); props.onToggleCut(); }
        else if (e.key === "Escape" && selection) { e.stopPropagation(); onSelection(null); }
        else if (e.key === "Enter" && selection) { e.preventDefault(); props.onSplit(); }
        else if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && tokens.length) {
          e.preventDefault();
          const from = selection ? selection.focus : Math.max(0, activeIndex);
          const to = Math.min(tokens.length - 1, Math.max(0, from + (e.key === "ArrowRight" ? 1 : -1)));
          onSelection(e.shiftKey && selection ? { anchor: selection.anchor, focus: to } : { anchor: to, focus: to });
          props.onSeek(tokens[to].start);
        }
      }}
    >
      {lines.map((l, i) => (props.hiddenSpeakers.has(l.speaker) ? null : (
        <Row key={l.id} line={l} index={i} first={firstOf[i]} speakers={props.speakers}
          active={activeIndex >= firstOf[i] && activeIndex < firstOf[i] + l.words.length ? activeIndex : -1}
          selected={selected && selected[1] >= firstOf[i] && selected[0] < firstOf[i] + l.words.length ? selected : null}
          matches={matchSet} current={currentSet}
          onSeek={props.onSeek} onPlayLine={props.onPlayLine} onSpeaker={props.onSpeaker} onText={props.onText} onHide={props.onHide} onMerge={props.onMerge}
          newSpeakerLabel={props.newSpeakerLabel} canMerge={i + 1 < lines.length && !lines[i + 1].hidden} />
      )))}
    </ol>
  );
}

