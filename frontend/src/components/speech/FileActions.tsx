import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, FolderOpen, Loader2, Save } from "lucide-react";
import { speechApi } from "../../lib/speech";
import { errorText } from "../../lib/voices";

/** A file of the project's exports: save it where the user picks (the system dialog) or show it in the folder. */
export default function FileActions({ pid, file, label, primary = false }: { pid: string; file: string; label?: string; primary?: boolean }) {
  const { t } = useTranslation();
  const [saved, setSaved] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const saveAs = async () => {
    setBusy(true);
    setError(null);
    try { const r = await speechApi.saveExportAs(pid, file); if (r.path) setSaved(r.path); } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const btn = primary
    ? "inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[12px] font-semibold disabled:opacity-40"
    : "inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)] disabled:opacity-40";
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <button onClick={() => void saveAs()} disabled={busy} className={btn} title={file}>
        {busy ? <Loader2 size={13} className="animate-spin" /> : saved ? <Check size={13} /> : <Save size={13} />}{label ?? t("files.saveAs", { file })}
      </button>
      <button onClick={() => speechApi.revealExport(pid, file).catch((e: unknown) => setError(errorText(e)))} className="p-1.5 rounded-lg border border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-text)]" title={t("files.reveal")} aria-label={t("files.reveal")}><FolderOpen size={13} /></button>
      {saved && <span className="text-[11px] text-[var(--color-muted)] truncate max-w-[24ch]" title={saved}>{t("files.savedTo", { path: saved })}</span>}
      {error && <span role="alert" className="text-[11px] text-[var(--color-warn)]">{error}</span>}
    </div>
  );
}
