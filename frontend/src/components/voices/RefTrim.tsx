import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Pause, Play, Scissors } from "lucide-react";
import { named } from "../../lib/a11y";

const BARS = 160;

/** A recording's waveform with a start and an end to keep: listen to the piece, then keep it. With `limit` the length
 * a reference should have is checked; `initial` is the piece kept before, `onReset` gives the whole recording back. */
export default function RefTrim({ url, limit, initial, busy, onApply, onReset }: { url: string; limit?: number; initial?: [number, number] | null; busy: boolean; onApply: (start: number, end: number) => void; onReset?: () => void }) {
  const { t } = useTranslation();
  const [peaks, setPeaks] = useState<number[] | null>(null);
  const [duration, setDuration] = useState(0);
  const [range, setRange] = useState<[number, number]>([0, 0]);
  const [error, setError] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);
  const player = useRef<HTMLAudioElement | null>(null);
  const [from, to] = initial ?? [null, null];
  useEffect(() => {
    let alive = true;
    const ctx = new AudioContext();
    fetch(url)
      .then((r) => { if (!r.ok) throw new Error(`${r.status} ${r.statusText}`); return r.arrayBuffer(); })
      .then((bytes) => ctx.decodeAudioData(bytes))
      .then((audio) => {
        if (!alive) return;
        const data = audio.getChannelData(0);
        const step = Math.max(1, Math.floor(data.length / BARS));
        const out: number[] = [];
        for (let i = 0; i < BARS; i++) {
          let top = 0;
          for (let k = i * step; k < Math.min(data.length, (i + 1) * step); k++) top = Math.max(top, Math.abs(data[k]));
          out.push(top);
        }
        const loudest = Math.max(...out, 1e-6);
        setPeaks(out.map((p) => p / loudest));
        setDuration(audio.duration);
        setRange(from != null && to != null ? [from, to] : [0, Math.min(audio.duration, limit ?? audio.duration)]);
      })
      .catch((e: unknown) => { if (alive) setError(e instanceof Error ? e.message : String(e)); })
      .finally(() => { void ctx.close(); });
    return () => { alive = false; player.current?.pause(); };
  }, [url, limit, from, to]);
  const [start, end] = range;
  const length = end - start;
  const listen = () => {
    player.current?.pause();
    if (playing) { setPlaying(false); return; }
    const a = new Audio(url);
    player.current = a;
    a.currentTime = start;
    a.ontimeupdate = () => { if (a.currentTime >= end) { a.pause(); setPlaying(false); } };
    a.onended = () => setPlaying(false);
    setPlaying(true);
    a.play().catch((e: unknown) => { setPlaying(false); setError(e instanceof Error ? e.message : String(e)); });
  };
  const bars = useMemo(() => peaks?.map((p, i) => {
    const at = (i / BARS) * duration;
    const inside = at >= start && at <= end;
    return <rect key={i} x={i * 3} y={20 - p * 18} width={2} height={Math.max(1, p * 36)} className={inside ? "fill-[var(--color-accent)]" : "fill-[var(--color-muted)] opacity-40"} />;
  }), [peaks, duration, start, end]);
  if (error) return <p role="alert" className="text-[11px] text-[var(--color-warn)]">{error}</p>;
  if (!peaks) return <Loader2 size={14} className="animate-spin text-[var(--color-muted)]" />;
  const field = "w-full accent-[var(--color-accent)]";
  return (
    <div className="space-y-1.5">
      <svg viewBox={`0 0 ${BARS * 3} 40`} preserveAspectRatio="none" className="w-full h-12 rounded-md bg-[var(--color-surface-2)]" aria-hidden>{bars}</svg>
      <label className="block text-[11px] text-[var(--color-muted)]">{t("voices.trimStart", { s: start.toFixed(1) })}
        <input type="range" min={0} max={duration} step={0.1} value={start} onChange={(e) => setRange([Math.min(Number(e.target.value), end - 0.5), end])} className={field} />
      </label>
      <label className="block text-[11px] text-[var(--color-muted)]">{t("voices.trimEnd", { s: end.toFixed(1) })}
        <input type="range" min={0} max={duration} step={0.1} value={end} onChange={(e) => setRange([start, Math.max(Number(e.target.value), start + 0.5)])} className={field} />
      </label>
      <div className="flex flex-wrap items-center gap-2">
        <button onClick={listen} className="p-1.5 rounded-md border border-[var(--color-border)] text-[var(--color-text)] hover:border-[var(--color-accent)]" {...named(playing ? t("voices.trimStop") : t("voices.trimListen"))}>{playing ? <Pause size={13} /> : <Play size={13} />}</button>
        {limit == null ? <span className="text-[11px] tabnum text-[var(--color-muted)]">{t("voices.trimLength", { s: length.toFixed(1) })}</span> : (
          <span className={`text-[11px] tabnum ${length > limit || length < 3 ? "text-[var(--color-warn)]" : "text-[var(--color-muted)]"}`}>
            {t("voices.trimLength", { s: length.toFixed(1) })}{length > limit ? ` · ${t("voices.trimTooLong", { s: limit })}` : length < 3 ? ` · ${t("voices.trimTooShort")}` : ""}
          </span>
        )}
        {onReset && initial && <button onClick={() => { player.current?.pause(); onReset(); }} disabled={busy} className="ml-auto px-2.5 py-1 rounded-md border border-[var(--color-border)] text-[11px] hover:border-[var(--color-accent)] disabled:opacity-40">{t("voices.trimReset")}</button>}
        <button onClick={() => { player.current?.pause(); onApply(start, end); }} disabled={busy || (start === 0 && end >= duration) || (initial != null && start === initial[0] && end === initial[1])}
          className={`${onReset && initial ? "" : "ml-auto "}inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[11px] font-semibold disabled:opacity-40`}>
          {busy ? <Loader2 size={12} className="animate-spin" /> : <Scissors size={12} />}{t("voices.trimApply")}
        </button>
      </div>
    </div>
  );
}
