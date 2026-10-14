import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Download, Loader2, Sparkles, X } from "lucide-react";
import { EXPORT_FORMATS, speechApi, type ExportFormat, type ExportProblem, type SpeechProject } from "../../lib/speech";
import { followJob } from "../../lib/followJob";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";

/** Export: what keeps it back (blocks not voiced or out of date), the format and the finished file to save. */
export default function ExportDialog({ pid, project, onClose, onVoiceMissing, onDone }: {
  pid: string;
  project: SpeechProject;
  onClose: () => void;
  onVoiceMissing: () => void;
  onDone: (file: string) => void;
}) {
  const { t } = useTranslation();
  const [problems, setProblems] = useState<ExportProblem[] | null>(null);
  const [format, setFormat] = useState<ExportFormat>(project.settings.loudness === "acx" ? "mp3" : "wav");
  const [busy, setBusy] = useState(false);
  const [file, setFile] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEscapeLayer(() => { if (!busy) onClose(); });
  useEffect(() => { speechApi.exportCheck(pid).then((r) => setProblems(r.problems), (e: unknown) => setError(errorText(e))); }, [pid]);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const { job_id } = await speechApi.export(pid, format);
      const r = (await followJob(job_id, "export")) as { file: string };
      setFile(r.file);
      onDone(r.file);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const counts = (problems ?? []).reduce<Record<string, number>>((a, p) => { a[p.state] = (a[p.state] ?? 0) + 1; return a; }, {});
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade" onClick={() => { if (!busy) onClose(); }}>
      <div role="dialog" aria-modal="true" aria-labelledby="ex-title" className="w-[min(94vw,460px)] rounded-xl glass-panel anim-pop p-5 space-y-4" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between">
          <h2 id="ex-title" className="font-semibold">{t("speech.exportTitle")}</h2>
          <button onClick={onClose} disabled={busy} {...named(t("a11y.close"))} className="text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30"><X size={16} /></button>
        </div>
        {problems === null && !error && <Loader2 size={16} className="animate-spin text-[var(--color-muted)]" />}
        {problems && problems.length > 0 && (
          <div role="alert" className="rounded-lg border border-[var(--color-warn)]/40 bg-[color-mix(in_oklab,var(--color-warn)_8%,transparent)] p-3 space-y-2">
            <p className="text-[13px]">{t("speech.exportBlocked", { n: problems.length })}</p>
            <ul className="text-[12px] text-[var(--color-muted)] space-y-0.5">
              {Object.entries(counts).map(([state, n]) => <li key={state}>{t(`speech.state_${state as ExportProblem["state"]}`)}: {n}</li>)}
            </ul>
            <button onClick={onVoiceMissing} className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[12px] font-semibold"><Sparkles size={13} />{t("speech.voiceTheMissing")}</button>
          </div>
        )}
        {problems && problems.length === 0 && (
          <>
            <div className="flex flex-wrap gap-1.5" role="radiogroup" aria-label={t("speech.format")}>
              {EXPORT_FORMATS.map((f) => (
                <button key={f} role="radio" aria-checked={format === f} onClick={() => setFormat(f)}
                  className={`px-3 py-1.5 rounded-lg border text-[12px] font-semibold uppercase ${format === f ? "border-[var(--color-accent)] text-[var(--color-accent)]" : "border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-text)]"}`}>{f}</button>
              ))}
            </div>
            <p className="text-[12px] text-[var(--color-muted)]">{t("speech.exportLoudness", { preset: t(`speech.loudness_${project.settings.loudness}`) })}</p>
            {file ? (
              <a href={speechApi.exportUrl(pid, file)} download={file} className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold"><Check size={14} />{t("speech.saveFile", { file })}</a>
            ) : (
              <button onClick={run} disabled={busy} className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">
                {busy ? <Loader2 size={14} className="animate-spin" /> : <Download size={14} />}{t("speech.exportRun")}
              </button>
            )}
          </>
        )}
        {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
      </div>
    </div>
  );
}
