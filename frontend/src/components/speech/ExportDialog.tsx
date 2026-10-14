import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, Loader2, Package, Sparkles, X } from "lucide-react";
import { EXPORT_FORMATS, speechApi, type AcxReport, type ExportFormat, type ExportProblem, type SpeechProject } from "../../lib/speech";
import { followJob } from "../../lib/followJob";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import FileActions from "./FileActions";
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
  const [subs, setSubs] = useState<string | null>(null);
  const [chapterList, setChapterList] = useState<string | null>(null);
  const [acx, setAcx] = useState<AcxReport | null>(null);
  const [files, setFiles] = useState<{ file: string; bytes: number; mtime: number }[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [withAudio, setWithAudio] = useState(true);
  const [pack, setPack] = useState<{ file: string; bytes: number } | null>(null);
  const [packing, setPacking] = useState(false);
  const runPack = async () => {
    setPacking(true);
    setError(null);
    try { setPack(await speechApi.pack(pid, withAudio)); } catch (e) { setError(errorText(e)); } finally { setPacking(false); }
  };
  useEscapeLayer(() => { if (!busy) onClose(); });
  useEffect(() => { speechApi.exportCheck(pid).then((r) => setProblems(r.problems), (e: unknown) => setError(errorText(e))); }, [pid]);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const { job_id } = await speechApi.export(pid, format);
      const r = (await followJob(job_id, "export")) as { file: string; subtitles?: string; chapters?: string | null; acx?: AcxReport | null };
      setAcx(r.acx ?? null);
      setFile(r.file);
      setSubs(r.subtitles ?? null);
      setChapterList(r.chapters ?? null);
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
              <div className="space-y-1.5">
                <FileActions pid={pid} file={file} primary />
                {chapterList && <FileActions pid={pid} file={chapterList} label={t("speech.saveChapters")} />}
                {subs && <FileActions pid={pid} file={subs} label={t("speech.saveSubs")} />}
              </div>
            ) : (
              <button onClick={run} disabled={busy} className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">
                {busy ? <Loader2 size={14} className="animate-spin" /> : <Download size={14} />}{t("speech.exportRun")}
              </button>
            )}
          </>
        )}
        {acx && (
          <div role="status" className={`rounded-lg border p-3 space-y-1 ${acx.passed ? "border-[var(--color-accent)]/40" : "border-[var(--color-warn)]/40"}`}>
            <p className="text-[12px] font-semibold">{acx.passed ? t("speech.acxPassed") : t("speech.acxFailed")}</p>
            <ul className="text-[11px] space-y-0.5">
              {acx.checks.map((c) => (
                <li key={c.name} className={c.passed ? "text-[var(--color-muted)]" : "text-[var(--color-warn)]"}>
                  {c.passed ? "✓" : "✗"} {t(`speech.acx_${c.name}`, { v: ["rms", "peak", "noise_floor"].includes(c.name) ? c.value.toFixed(1) : c.name === "length" ? (c.value / 60).toFixed(1) : c.value.toFixed(2) })}
                </li>
              ))}
            </ul>
          </div>
        )}
        <div className="pt-3 border-t border-[var(--color-border)] space-y-2">
          <h3 className="text-[12px] font-semibold">{t("speech.packTitle")}</h3>
          <p className="text-[11px] text-[var(--color-muted)]">{t("speech.packHint")}</p>
          <label className="flex items-center gap-2 text-[12px] cursor-pointer">
            <input type="checkbox" checked={withAudio} onChange={(e) => { setWithAudio(e.target.checked); setPack(null); }} className="w-4 h-4 accent-[var(--color-accent)]" />{t("speech.packAudio")}
          </label>
          {pack ? (
            <FileActions pid={pid} file={pack.file} />
          ) : (
            <button onClick={() => void runPack()} disabled={packing} className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-[var(--color-border)] text-[12px] font-medium hover:border-[var(--color-accent)] disabled:opacity-40">
              {packing ? <Loader2 size={13} className="animate-spin" /> : <Package size={13} />}{t("speech.packRun")}
            </button>
          )}
        </div>
        <details onToggle={(e) => { if (e.currentTarget.open) speechApi.exports(pid).then((r) => setFiles(r.files), (er: unknown) => setError(errorText(er))); }} className="pt-3 border-t border-[var(--color-border)]">
          <summary className="cursor-pointer text-[12px] font-semibold">{t("files.title")}</summary>
          <ul className="mt-2 space-y-1.5 max-h-48 overflow-y-auto">
            {files?.length === 0 && <li className="text-[11px] text-[var(--color-muted)]">{t("files.none")}</li>}
            {files?.map((f) => <li key={f.file}><FileActions pid={pid} file={f.file} label={f.file} /></li>)}
          </ul>
        </details>
        {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
      </div>
    </div>
  );
}
