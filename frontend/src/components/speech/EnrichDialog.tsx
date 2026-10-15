import { useState } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";

export type EnrichChange = { id: string; before: string; after: string };

/** The director's tags for each block, before and after; the ones kept are applied together. */
export default function EnrichDialog({ changes, refused, mode, onApply, onClose }: { changes: EnrichChange[]; refused: number; mode: "tags" | "punctuation"; onApply: (kept: EnrichChange[]) => void; onClose: () => void }) {
  const { t } = useTranslation();
  const [kept, setKept] = useState<Set<string>>(() => new Set(changes.map((c) => c.id)));
  useEscapeLayer(onClose);
  const toggle = (id: string) => setKept((all) => { const next = new Set(all); if (next.has(id)) next.delete(id); else next.add(id); return next; });
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center bg-black/50 p-4" onClick={onClose}>
      <div role="dialog" aria-modal="true" aria-label={mode === "tags" ? t("enrich.title") : t("enrich.punctTitle")} onClick={(e) => e.stopPropagation()}
        className="w-[min(94vw,760px)] max-h-[86vh] flex flex-col rounded-xl glass-panel anim-pop overflow-hidden">
        <div className="flex items-center gap-2 px-4 py-3 border-b border-[var(--color-border)]">
          <h2 className="font-semibold">{mode === "tags" ? t("enrich.title") : t("enrich.punctTitle")}</h2>
          <span className="text-[12px] text-[var(--color-muted)] tabnum">{t(mode === "tags" ? "enrich.count" : "enrich.punct_count", { n: changes.length })}</span>
          <button onClick={onClose} className="ml-auto p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("a11y.close"))}><X size={16} /></button>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto px-4 py-3 space-y-2">
          {changes.length === 0 && <p className="text-[13px] text-[var(--color-muted)]">{t(mode === "tags" ? "enrich.none" : "enrich.punct_none")}</p>}
          {changes.map((c, i) => (
            <label key={c.id} data-mcp-row={`suggestion ${i + 1}: ${c.after.slice(0, 60)}`} className="flex items-start gap-2.5 rounded-lg border border-[var(--color-border)] px-3 py-2 cursor-pointer">
              <input type="checkbox" checked={kept.has(c.id)} onChange={() => toggle(c.id)} aria-label={t("enrich.keep")} className="mt-1 w-4 h-4 accent-[var(--color-accent)]" />
              <span className="min-w-0 space-y-1 text-[13px]">
                <span className="block text-[var(--color-muted)] line-through decoration-[var(--color-muted)]/40">{c.before}</span>
                <span className="block">{c.after}</span>
              </span>
            </label>
          ))}
          {refused > 0 && <p className="text-[12px] text-[var(--color-warn)]">{t("enrich.refused", { n: refused })}</p>}
        </div>
        <div className="flex items-center gap-2 px-4 py-3 border-t border-[var(--color-border)]">
          <p className="flex-1 text-[11px] text-[var(--color-muted)]">{t(mode === "tags" ? "enrich.hint" : "enrich.punct_hint")}</p>
          <button onClick={() => onApply(changes.filter((c) => kept.has(c.id)))} disabled={kept.size === 0}
            className="px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">{t("enrich.apply", { n: kept.size })}</button>
        </div>
      </div>
    </div>
  );
}
