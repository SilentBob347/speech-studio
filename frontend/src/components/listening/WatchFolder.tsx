import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, FolderSearch, Loader2, Play, Square } from "lucide-react";
import { api } from "../../lib/api";
import { listenApi, TRANSCRIPT_FORMATS, type Start, type SubtitleRules, type TranscriptFormat, type WatchStatus } from "../../lib/listen";
import { errorText } from "../../lib/voices";

/** A folder of recordings recognised in the background into subtitle or text files next to them, once or watching it
 * for new files; the studio goes on with it after a restart. */
export default function WatchFolder({ start, onOpen }: { start: Start; onOpen: (pid: string) => void }) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<WatchStatus | null>(null);
  const [folder, setFolder] = useState("");
  const [out, setOut] = useState("");
  const [formats, setFormats] = useState<TranscriptFormat[]>(["srt", "txt"]);
  const [keep, setKeep] = useState(false);
  const [rules, setRules] = useState("");
  const [digits, setDigits] = useState(false);
  const [profiles, setProfiles] = useState<SubtitleRules[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const on = !!status?.on;
  const show = (s: WatchStatus) => {
    setStatus(s);
    if (s.config.folder) {
      setFolder(s.config.folder);
      setOut(s.config.out);
      setFormats(s.config.formats);
      setKeep(s.config.keep);
      setRules(s.config.rules);
      setDigits(s.config.digits);
    }
  };
  useEffect(() => {
    listenApi.watchStatus().then(show, (e: unknown) => setError(errorText(e)));
    listenApi.subtitleRules().then((r) => setProfiles(r.rules), (e: unknown) => setError(errorText(e)));
  }, []);
  useEffect(() => {
    if (!on) return;
    const id = window.setInterval(() => { listenApi.watchStatus().then(setStatus, (e: unknown) => setError(errorText(e))); }, 2000);
    return () => window.clearInterval(id);
  }, [on]);
  const pick = async (set: (dir: string) => void) => {
    try { const r = await api.pickFolder(); if (r.dir) set(r.dir); } catch (e) { setError(errorText(e)); }
  };
  const act = async (f: () => Promise<WatchStatus>) => {
    setBusy(true);
    setError(null);
    try { setStatus(await f()); } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const subtitles = formats.some((f) => f === "srt" || f === "vtt" || f === "ass");
  const items = status?.config.folder === folder ? status.items : [];
  const total = items.length;
  return (
    <div className="rounded-xl border border-[var(--color-border)] px-4 py-3 space-y-2.5">
      <div className="flex items-center gap-2">
        <FolderSearch size={15} className="text-[var(--color-accent)]" />
        <span className="text-[13px] font-medium">{t("listen.watchTitle")}</span>
        {on && <span className="ml-auto inline-flex items-center gap-1.5 text-[11px] text-[var(--color-accent)]"><Loader2 size={12} className="animate-spin" />{status?.running && !status.queued && status.config.keep ? t("listen.watchWatching") : t("listen.watchWorking")}</span>}
      </div>
      <p className="text-[12px] text-[var(--color-muted)] leading-relaxed">{t("listen.watchHint")}</p>
      <div className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-x-2 gap-y-1.5 text-[12px]">
        <span className="text-[var(--color-muted)]">{t("listen.watchFolder")}</span>
        <span className={`truncate ${folder ? "mono text-[11px]" : "text-[var(--color-muted)]"}`} title={folder}>{folder || t("listen.watchNoFolder")}</span>
        <button onClick={() => void pick(setFolder)} disabled={on} className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md border border-[var(--color-border)] hover:border-[var(--color-accent)] disabled:opacity-40"><FolderOpen size={12} />{t("listen.watchPick")}</button>
        <span className="text-[var(--color-muted)]">{t("listen.watchOut")}</span>
        <span className={`truncate ${out ? "mono text-[11px]" : ""}`} title={out}>{out || t("listen.watchOutSame")}</span>
        <span className="flex gap-1">
          {out && <button onClick={() => setOut("")} disabled={on} className="px-2 py-1 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-40">{t("listen.watchOutReset")}</button>}
          <button onClick={() => void pick(setOut)} disabled={on} className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md border border-[var(--color-border)] hover:border-[var(--color-accent)] disabled:opacity-40"><FolderOpen size={12} />{t("listen.watchOutPick")}</button>
        </span>
      </div>
      <fieldset disabled={on} className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-[12px] disabled:opacity-60">
        <legend className="sr-only">{t("batch.formats")}</legend>
        <span className="text-[var(--color-muted)]">{t("batch.formats")}</span>
        {TRANSCRIPT_FORMATS.map((f) => (
          <label key={f} className="inline-flex items-center gap-1 cursor-pointer">
            <input type="checkbox" checked={formats.includes(f)} onChange={(e) => setFormats((all) => (e.target.checked ? [...all, f] : all.filter((x) => x !== f)))} className="accent-[var(--color-accent)]" />{f.toUpperCase()}
          </label>
        ))}
        <label className="inline-flex items-center gap-1 cursor-pointer" title={t("listen.digitsHint")}>
          <input type="checkbox" checked={digits} onChange={(e) => setDigits(e.target.checked)} className="accent-[var(--color-accent)]" />{t("listen.digits")}
        </label>
        <label className="inline-flex items-center gap-1 cursor-pointer" title={t("listen.watchKeepHint")}>
          <input type="checkbox" checked={keep} onChange={(e) => setKeep(e.target.checked)} className="accent-[var(--color-accent)]" />{t("listen.watchKeep")}
        </label>
      </fieldset>
      {subtitles && (
        <label className="flex items-center gap-2 text-[12px]">
          <span className="text-[var(--color-muted)] shrink-0">{t("listen.rules")}</span>
          <select value={rules} onChange={(e) => setRules(e.target.value)} disabled={on} className="flex-1 min-w-0 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] disabled:opacity-60">
            <option value="">{t("listen.rulesNone")}</option>
            {profiles.map((p) => <option key={p.name} value={p.name}>{t("listen.rulesOne", { name: p.name, chars: p.max_line_len, lines: p.max_lines, cps: p.max_cps })}</option>)}
          </select>
        </label>
      )}
      <div className="flex items-center gap-2">
        {on ? (
          <button onClick={() => void act(listenApi.watchStop)} disabled={busy} className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[var(--color-warn)] text-[var(--color-on-accent)] text-[12px] font-semibold disabled:opacity-40"><Square size={12} />{t("listen.watchStop")}</button>
        ) : (
          <button onClick={() => void act(() => listenApi.watchStart({ folder, out, formats, keep, engine: start.engine, lang: start.lang, rules: subtitles ? rules : "", digits }))} disabled={busy || !folder || !formats.length}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[12px] font-semibold disabled:opacity-40">
            {busy ? <Loader2 size={12} className="animate-spin" /> : <Play size={12} />}{t(items.some((i) => i.state === "queued" || i.state === "working") ? "listen.watchGoOn" : "listen.watchStart")}
          </button>
        )}
        {total > 0 && (
          <span className="text-[12px] text-[var(--color-muted)] tabnum">
            {t("batch.progress", { done: status?.done ?? 0, total })}{status?.errors ? ` · ${t("listen.watchErrors", { n: status.errors })}` : ""}
          </span>
        )}
      </div>
      {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
      {total > 0 && (
        <ul className="max-h-56 overflow-y-auto space-y-1 text-[12px]" aria-live="polite">
          {items.map((it) => (
            <li key={it.file} className="flex items-center gap-2 rounded-md bg-[var(--color-surface-2)] px-2 py-1">
              {it.state === "working" && <Loader2 size={12} className="animate-spin text-[var(--color-accent)] shrink-0" />}
              <span className="flex-1 min-w-0">
                <span className="block truncate" title={it.file}>{it.file.split(/[\\/]/).pop()}</span>
                {it.error && <span className="block text-[11px] text-[var(--color-warn)] break-words">{it.error}</span>}
              </span>
              <span className={`shrink-0 text-[11px] ${it.state === "error" ? "text-[var(--color-warn)]" : it.state === "done" ? "text-[var(--color-accent)]" : "text-[var(--color-muted)]"}`}>{t(`batch.${it.state}`)}</span>
              {it.pid && it.state === "done" && <button onClick={() => onOpen(it.pid!)} className="shrink-0 text-[11px] underline text-[var(--color-muted)] hover:text-[var(--color-text)]">{t("batch.open")}</button>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
