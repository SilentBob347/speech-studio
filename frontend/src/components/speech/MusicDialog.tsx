import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ExternalLink, Loader2, Music, RefreshCw, X } from "lucide-react";
import { speechApi, type MusicStudio, type Track } from "../../lib/speech";
import { followJob } from "../../lib/followJob";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import FormFields from "../cloud/FormFields";

/** Music for the book or play from a music studio of this computer (YuE2, MiniMax Music3, ACE-Step): the studio's own
 * song form, the track it goes on and where; the song stays in the studio's library, its audio comes onto the track. */
export default function MusicDialog({ pid, tracks, flush, onClose, onDone }: {
  pid: string;
  tracks: { id: string; name: Track["name"] }[];
  flush: () => Promise<unknown>;
  onClose: () => void;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const [studios, setStudios] = useState<MusicStudio[] | null>(null);
  const [studio, setStudio] = useState<string>("");
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [track, setTrack] = useState<string>("");
  const [at, setAt] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [round, setRound] = useState(0);
  useEscapeLayer(() => { if (!busy) onClose(); });
  useEffect(() => {
    let alive = true;
    speechApi.musicStudios().then((r) => {
      if (!alive) return;
      setStudios(r.studios);
      setStudio((cur) => (r.studios.some((s) => s.id === cur && s.running) ? cur : r.studios.find((s) => s.running)?.id ?? ""));
    }, (e: unknown) => { if (alive) setError(errorText(e)); });
    return () => { alive = false; };
  }, [round]);
  const chosen = studios?.find((s) => s.id === studio);
  const make = async () => {
    if (!chosen) return;
    setBusy(true);
    setError(null);
    try {
      await flush();
      const { job_id } = await speechApi.trackMusic(pid, { studio: chosen.id, values, track: track || null, at: at === "" ? null : Number(at) });
      await followJob(job_id, "music");
      onDone();
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const box = "w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] focus:border-[var(--color-accent)] focus:outline-none";
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" role="dialog" aria-modal="true" aria-label={t("tracks.musicTitle")}>
      <div className="w-[440px] max-w-[calc(100vw-32px)] max-h-[calc(100vh-48px)] overflow-y-auto rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 space-y-3">
        <div className="flex items-center gap-2">
          <Music size={15} className="text-[var(--color-accent)]" />
          <h2 className="text-[14px] font-semibold">{t("tracks.musicTitle")}</h2>
          <button onClick={() => setRound((n) => n + 1)} disabled={busy} className="ml-auto p-1 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("tracks.refresh"))}><RefreshCw size={13} /></button>
          <button onClick={onClose} disabled={busy} className="p-1 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("a11y.close"))}><X size={14} /></button>
        </div>
        <p className="text-[11px] text-[var(--color-muted)]">{t("tracks.musicIntro")}</p>
        {!studios && !error && <Loader2 size={14} className="animate-spin text-[var(--color-muted)]" />}
        {studios && (
          <div className="flex flex-wrap gap-1.5" role="radiogroup" aria-label={t("tracks.studio")}>
            {studios.map((s) => s.running ? (
              <button key={s.id} role="radio" aria-checked={studio === s.id} disabled={busy} onClick={() => { setStudio(s.id); setValues({}); }}
                className={`px-2.5 py-1 rounded-md border text-[12px] disabled:opacity-40 ${studio === s.id ? "border-[var(--color-accent)] text-[var(--color-accent)]" : "border-[var(--color-border)] hover:border-[var(--color-accent)]"}`}>
                {s.name}
              </button>
            ) : (
              <a key={s.id} href={s.page} target="_blank" rel="noreferrer" title={t("tracks.downloadHint", { name: s.name })}
                className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md border border-[var(--color-border)] text-[12px] text-[var(--color-text)] hover:border-[var(--color-accent)]">
                <ExternalLink size={12} className="text-[var(--color-accent)]" />{s.name}<span className="text-[var(--color-muted)]">· {t("tracks.download")}</span>
              </a>
            ))}
          </div>
        )}
        {studios && !studios.some((s) => s.running) && <p className="text-[11px] text-[var(--color-muted)]">{t("tracks.noneRunning")}</p>}
        {chosen && (
          <div className="space-y-2">
            <FormFields fields={chosen.fields} values={values} onChange={setValues} />
            <label className="block space-y-0.5 text-[11px] text-[var(--color-muted)]">
              <span>{t("tracks.where")}</span>
              <select value={track} onChange={(e) => setTrack(e.target.value)} className={box}>
                <option value="">{t("tracks.newTrack")}</option>
                {tracks.map((tr) => <option key={tr.id} value={tr.id}>{tr.name}</option>)}
              </select>
            </label>
            <label className="block space-y-0.5 text-[11px] text-[var(--color-muted)]">
              <span>{t("tracks.at")}</span>
              <input type="number" min={0} step={0.1} value={at} placeholder={t("tracks.atEnd")} onChange={(e) => setAt(e.target.value)} className={box} />
            </label>
          </div>
        )}
        {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
        <div className="flex justify-end">
          <button onClick={make} disabled={!chosen || busy} className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">
            {busy ? <Loader2 size={14} className="animate-spin" /> : <Music size={14} />}{t("tracks.make")}
          </button>
        </div>
      </div>
    </div>
  );
}
