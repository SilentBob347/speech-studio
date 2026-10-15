import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { MoreHorizontal, type LucideIcon } from "lucide-react";
import { named } from "../lib/a11y";

export type MoreItem = { icon: LucideIcon; label: string; act: () => void; disabled?: boolean; narrowOnly?: boolean; belowLarge?: boolean };

/** The editor's less frequent actions, and on a narrow screen every action the bar has no room for. */
export default function EditorMore({ items, className = "" }: { items: MoreItem[]; className?: string }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open) return;
    const close = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, [open]);
  return (
    <div className={`relative shrink-0 ${className}`}>
      <button onClick={() => setOpen((o) => !o)} aria-haspopup="menu" aria-expanded={open} {...named(t("nav.editorMore"))}
        className="grid place-items-center w-10 h-9 rounded-lg border border-[var(--color-border)] text-[var(--color-text)] hover:border-[var(--color-accent)]"><MoreHorizontal size={16} /></button>
      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div role="menu" aria-label={t("nav.editorMore")} className="absolute right-0 top-full mt-1 z-50 w-64 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-xl p-1.5">
            {items.map((it) => (
              <button key={it.label} role="menuitem" disabled={it.disabled} onClick={() => { setOpen(false); it.act(); }}
                className={`flex w-full items-center gap-3 px-3 py-2.5 rounded-md text-[13px] text-[var(--color-text)] hover:bg-[var(--color-surface-2)] disabled:opacity-40 ${it.narrowOnly ? "md:hidden" : ""} ${it.belowLarge ? "lg:hidden" : ""}`}>
                <it.icon size={15} className="text-[var(--color-muted)]" />{it.label}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
