import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getCoded } from "../../lib/api";
import { errorText } from "../../lib/voices";
import en from "../../locales/en.json";

type TagKey = keyof typeof en.tags;

type TagSet = { engine: string; format: string; categories: { category: string; values: string[] }[] };
let cached: TagSet | null = null;
const PAUSES = ["300ms", "500ms", "1s", "2s"].map((p) => ({ label: p, tag: `[pause ${p}]` }));
const RATES = ["0.75", "0.9", "1", "1.1", "1.25"].map((r) => ({ label: `×${r}`, tag: `[rate:${r}]` }));
const chip = "px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[11px] text-[var(--color-text)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] mono";

/** The engine's expression tags by category; a click inserts the tag at the text's cursor. */
export default function TagPalette({ onInsert }: { onInsert: (tag: string) => void }) {
  const { t } = useTranslation();
  const [set, setSet] = useState<TagSet | null>(cached);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (cached) return;
    getCoded<TagSet>("/engine/tags").then((s) => { cached = s; setSet(s); }, (e: unknown) => setError(errorText(e)));
  }, []);
  const label = (k: string) => (k in en.tags ? t(`tags.${k as TagKey}`) : k);
  if (error) return <p role="alert" className="text-[11px] text-[var(--color-warn)]">{error}</p>;
  if (!set) return null;
  return (
    <div className="space-y-1.5">
      {set.categories.map((c) => (
        <div key={c.category} className="flex flex-wrap items-center gap-1">
          <span className="w-20 shrink-0 text-[10px] uppercase tracking-wide text-[var(--color-muted)]">{label(c.category)}</span>
          {c.values.map((v) => (
            <button key={v} type="button" onClick={() => onInsert(`<|${c.category}:${v}|>`)} title={`<|${c.category}:${v}|>`}
              className="px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[11px] text-[var(--color-text)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]">
              {label(v)}
            </button>
          ))}
        </div>
      ))}
      <div className="flex flex-wrap items-center gap-1 pt-1 border-t border-[var(--color-border)]">
        <span className="w-20 shrink-0 text-[10px] uppercase tracking-wide text-[var(--color-muted)]">{t("tags.pause")}</span>
        {PAUSES.map((p) => <button key={p.tag} type="button" onClick={() => onInsert(p.tag)} title={p.tag} className={chip}>{p.label}</button>)}
      </div>
      <div className="flex flex-wrap items-center gap-1">
        <span className="w-20 shrink-0 text-[10px] uppercase tracking-wide text-[var(--color-muted)]">{t("tags.rate")}</span>
        {RATES.map((r) => <button key={r.tag} type="button" onClick={() => onInsert(r.tag)} title={r.tag} className={chip}>{r.label}</button>)}
      </div>
      <p className="text-[10px] text-[var(--color-muted)]">{t("tags.legend")} {t("tags.universal")}</p>
    </div>
  );
}
