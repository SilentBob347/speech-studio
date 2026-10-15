import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Check, FolderDown, Loader2, Square, X } from "lucide-react";
import { named } from "../lib/a11y";

export type BatchItem = {
  name: string;
  status: "queued" | "working" | "done" | "error";
  /** What the file is going through now, as the job says it. */
  detail?: string;
  pct: number;
  pid: string | null;
  /** The finished files in the project's exports, for "Save all". */
  files: string[];
  error?: string;
};

/** A batch of files gone through one after another: each file's state, and its results saved together. */
export default function BatchList({ title, items, running, options, onOpen, onSaveAll, onClose }: {
  title: string;
  items: BatchItem[];
  running: boolean;
  /** The settings of "Save all" (formats), shown beside it. */
  options?: ReactNode;
  onOpen: (pid: string) => void;
  /** Saves every finished file into one folder the user picks; null when the pick was cancelled. */
  onSaveAll: () => Promise<{ folder: string; copied: number } | null>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<{ folder: string; copied: number } | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const done = items.filter((i) => i.status === "done").length;
  const finished = items.filter((i) => i.status === "done" || i.status === "error").length;
  const save = async () => {
    setSaving(true);
    setFailed(null);
    try { setSaved(await onSaveAll()); } catch (e) { setFailed(e instanceof Error ? e.message : String(e)); } finally { setSaving(false); }
  };
  const icon = (s: BatchItem["status"]) =>
    s === "done" ? <Check size={15} className="text-[var(--color-accent)]" /> :
    s === "error" ? <X size={15} className="text-[var(--color-warn)]" /> :
    s === "queued" ? <Square size={13} className="text-[var(--color-muted)]" /> :
    <Loader2 size={15} className="animate-spin text-[var(--color-accent)]" />;
  return (
    <section aria-label={title} className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2 border-b border-[var(--color-border)]">
        <FolderDown size={15} className="text-[var(--color-accent)]" />
        <h2 className="text-[13px] font-semibold">{title}</h2>
        <span className="text-[11px] tabnum text-[var(--color-muted)]">{t("batch.progress", { done: finished, total: items.length })}</span>
        <button onClick={onClose} disabled={running} className="ml-auto p-1 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("a11y.close"))}><X size={14} /></button>
      </div>
      <ul className="max-h-72 overflow-y-auto divide-y divide-[var(--color-border)]">
        {items.map((it, i) => (
          <li key={i} data-mcp-row={`file ${i + 1} ${it.status}: ${it.name}`} className="flex items-center gap-3 px-3 py-2">
            <span className="shrink-0">{icon(it.status)}</span>
            <div className="min-w-0 flex-1">
              <div className="text-[13px] truncate" title={it.name}>{it.name}</div>
              {it.status === "working" && (
                <>
                  <div className="mt-1 h-1 rounded bg-[var(--color-surface-2)] overflow-hidden">
                    <div className="h-full bg-[var(--color-accent)] transition-[width]" style={{ width: `${Math.min(100, Math.round(it.pct))}%` }} />
                  </div>
                  {it.detail && <div className="mt-0.5 text-[11px] text-[var(--color-muted)] truncate">{it.detail}</div>}
                </>
              )}
              {it.status === "error" && <div role="alert" className="text-[11px] text-[var(--color-warn)] break-words">{it.error}</div>}
            </div>
            <span className="text-[11px] text-[var(--color-muted)] shrink-0">{t(`batch.${it.status}`)}</span>
            {it.status === "done" && it.pid && <button onClick={() => it.pid && onOpen(it.pid)} className="shrink-0 text-[12px] text-[var(--color-accent)] hover:underline">{t("batch.open")}</button>}
          </li>
        ))}
      </ul>
      <div className="flex flex-wrap items-center gap-2 px-3 py-2 border-t border-[var(--color-border)]">
        {options}
        <button onClick={() => void save()} disabled={running || saving || done === 0}
          className="ml-auto inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[12px] font-semibold disabled:opacity-40">
          {saving ? <Loader2 size={13} className="animate-spin" /> : <FolderDown size={13} />}{t("batch.saveAll")}
        </button>
      </div>
      {saved && <p role="status" className="px-3 pb-2 text-[12px] text-[var(--color-accent)] break-words">{t("batch.saved", { n: saved.copied, folder: saved.folder })}</p>}
      {failed && <p role="alert" className="px-3 pb-2 text-[12px] text-[var(--color-warn)] break-words">{failed}</p>}
    </section>
  );
}
