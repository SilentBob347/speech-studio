import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, FolderOpen, Loader2, Save } from "lucide-react";
import { speechApi } from "../../lib/speech";
import { errorText } from "../../lib/voices";
import ShareMenu from "../ShareMenu";

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
    ? "inline-flex min-w-0 items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[12px] font-semibold disabled:opacity-40"
    : "inline-flex min-w-0 items-center gap-1.5 px-2.5 py-1.5 rounded-lg border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)] disabled:opacity-40";
  return (
    <div className="flex items-center gap-1.5 min-w-0">
      <button onClick={() => void saveAs()} disabled={busy} className={btn} title={file}>
        {busy ? <Loader2 size={13} className="shrink-0 animate-spin" /> : saved ? <Check size={13} className="shrink-0" /> : <Save size={13} className="shrink-0" />}<span className="truncate">{label ?? t("files.saveAs", { file })}</span>
      </button>
      <button onClick={() => speechApi.revealExport(pid, file).catch((e: unknown) => setError(errorText(e)))} className="shrink-0 p-1.5 rounded-lg border border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-text)]" title={t("files.reveal")} aria-label={t("files.reveal")}><FolderOpen size={13} /></button>
      <ShareMenu title={file} file={() => fetch(speechApi.exportUrl(pid, file)).then((r) => { if (!r.ok) throw new Error(`${r.status} ${r.statusText}`); return r.blob(); }).then((b) => new File([b], file, { type: b.type }))} />
      {saved && <span className="text-[11px] text-[var(--color-muted)] truncate max-w-[24ch]" title={saved}>{t("files.savedTo", { path: saved })}</span>}
      {error && <span role="alert" className="text-[11px] text-[var(--color-warn)]">{error}</span>}
    </div>
  );
}
