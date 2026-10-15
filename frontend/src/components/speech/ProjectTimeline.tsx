import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Minus, Plus, X } from "lucide-react";
import { speechApi, type TimelineBlock } from "../../lib/speech";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";

export type OverlapChoice = "cut" | "join" | "keep";

const ZOOMS = [4, 8, 16, 32, 64, 128] as const;
const LANE = 28;

/** The project on one time scale, as the export lays it out: a lane per speaker, takes as bars (an estimate while a block
 * is not voiced), an interruption striped where it covers the previous reply, with the choice to cut that reply off, join
 * the two back to back or keep both. */
export default function ProjectTimeline({ pid, speakers, tintOf, refresh, onChoose, onJump, onClose }: {
  pid: string;
  speakers: string[];
  tintOf: (speaker: string) => string;
  /** Changes when the saved project changes; `undefined` while a save is on its way. */
  refresh: unknown;
  /** `previous` is the block the interruption covers: joining zeroes its pause after. */
  onChoose: (block: string, previous: string, choice: OverlapChoice) => void;
  onJump: (block: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [rows, setRows] = useState<TimelineBlock[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [zoom, setZoom] = useState(2);
  const [open, setOpen] = useState<string | null>(null);
  useEffect(() => {
    if (refresh === undefined) return;
    let live = true;
    speechApi.timeline(pid).then((r) => { if (live) { setRows(r.blocks); setError(null); } }, (e: unknown) => { if (live) setError(errorText(e)); });
    return () => { live = false; };
  }, [pid, refresh]);
  const lanes = useMemo(() => {
    const seen = [...speakers];
    for (const r of rows ?? []) if (!seen.includes(r.speaker)) seen.push(r.speaker);
    return seen.filter((s) => rows?.some((r) => r.speaker === s));
  }, [rows, speakers]);
  const pps = ZOOMS[zoom];
  const total = Math.max(1, ...(rows ?? []).map((r) => r.end));
  const overlaps = (rows ?? []).filter((r) => r.overlap);
  const chosen = rows?.find((r) => r.block === open);
  const clock = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
  const ticks = Math.max(1, [1, 5, 10, 30, 60, 300].find((step) => step * pps >= 60) ?? 600);
  return (
    <section aria-label={t("timeline.title")} className="border-b border-[var(--color-border)] bg-[var(--color-surface)]/60 px-5 py-2 space-y-1.5">
      <div className="flex items-center gap-2 text-[12px]">
        <span className="font-semibold">{t("timeline.title")}</span>
        <span className="text-[11px] text-[var(--color-muted)] tabnum">{clock(total)}{overlaps.length ? ` · ${t("timeline.overlaps", { n: overlaps.length })}` : ""}</span>
        <div className="ml-auto flex items-center gap-1">
          <button onClick={() => setZoom((z) => Math.max(0, z - 1))} disabled={zoom === 0} className="p-1 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("timeline.zoomOut"))}><Minus size={13} /></button>
          <button onClick={() => setZoom((z) => Math.min(ZOOMS.length - 1, z + 1))} disabled={zoom === ZOOMS.length - 1} className="p-1 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("timeline.zoomIn"))}><Plus size={13} /></button>
          <button onClick={onClose} className="p-1 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("a11y.close"))}><X size={13} /></button>
        </div>
      </div>
      {error && <p role="alert" className="text-[12px] text-[var(--color-warn)]">{error}</p>}
      {rows && (
        <div className="flex">
          <div className="shrink-0 w-24 pt-4">
            {lanes.map((s) => <div key={s} style={{ height: LANE, color: tintOf(s) }} className="text-[11px] font-semibold truncate leading-[28px] pr-2" title={s}>{s}</div>)}
          </div>
          <div className="relative flex-1 min-w-0 overflow-x-auto pb-1">
            <div className="relative" style={{ width: total * pps + 40, height: 16 + lanes.length * LANE }}>
              {Array.from({ length: Math.floor(total / ticks) + 1 }, (_, k) => (
                <span key={k} className="absolute top-0 text-[9px] tabnum text-[var(--color-muted)] border-l border-[var(--color-border)] pl-0.5 h-full" style={{ left: k * ticks * pps }}>{clock(k * ticks)}</span>
              ))}
              {rows.map((r) => {
                const lane = lanes.indexOf(r.speaker);
                const top = 16 + lane * LANE + 4;
                return (
                  <div key={r.block}>
                    <button onClick={() => onJump(r.block)} title={`${r.speaker} · ${clock(r.start)}–${clock(r.end)}${r.voiced ? "" : ` · ${t("timeline.estimate")}`}`}
                      className={`absolute rounded-[4px] border ${r.voiced ? "" : "border-dashed"} hover:brightness-125`}
                      style={{ left: r.start * pps, width: Math.max(2, (r.end - r.start) * pps), top, height: LANE - 8, borderColor: tintOf(r.speaker), background: r.voiced ? `color-mix(in oklab, ${tintOf(r.speaker)} 35%, transparent)` : "transparent" }}
                      aria-label={t("timeline.block", { speaker: r.speaker, at: clock(r.start) })} />
                    {r.overlap && (
                      <button onClick={() => setOpen(open === r.block ? null : r.block)} aria-expanded={open === r.block} aria-controls="timeline-overlap"
                          className={`absolute rounded-[4px] border z-10 ${open === r.block ? "border-[var(--color-text)]" : "border-[var(--color-warn)]"}`}
                          style={{ left: r.start * pps, width: Math.max(4, r.overlap.secs * pps), top: top - 2, height: LANE - 4, background: "repeating-linear-gradient(135deg, color-mix(in oklab, var(--color-warn) 45%, transparent) 0 4px, transparent 4px 8px)" }}
                          aria-label={t("timeline.overlap", { secs: r.overlap.secs.toFixed(2) })} title={t("timeline.overlap", { secs: r.overlap.secs.toFixed(2) })} />
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      )}
      {chosen?.overlap && (
        <div id="timeline-overlap" role="group" aria-label={t("timeline.overlap", { secs: chosen.overlap.secs.toFixed(2) })} className="flex flex-wrap items-center gap-1.5 text-[12px]">
          <span className="text-[var(--color-warn)] tabnum">{t("timeline.overlap", { secs: chosen.overlap.secs.toFixed(2) })}</span>
          {(["cut", "join", "keep"] as const).map((c) => {
            const current = c === "cut" ? chosen.cut_previous : c === "keep" ? !chosen.cut_previous : false;
            return (
              <button key={c} onClick={() => { if (chosen.overlap) onChoose(chosen.block, chosen.overlap.with, c); setOpen(null); }} aria-pressed={current} title={t(`timeline.choiceHint_${c}`)}
                className={`px-2.5 py-1 rounded-md border text-[12px] ${current ? "border-[var(--color-accent)] text-[var(--color-accent)]" : "border-[var(--color-border)] hover:border-[var(--color-accent)]"}`}>
                {t(`timeline.choice_${c}`)}
              </button>
            );
          })}
        </div>
      )}
    </section>
  );
}
