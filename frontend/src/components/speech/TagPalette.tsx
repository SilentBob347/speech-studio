import { useTranslation } from "react-i18next";
import { useEngines } from "../../lib/engines";
import en from "../../locales/en.json";

type TagKey = keyof typeof en.tags;
/** Where a tag goes: at the start of the cursor's sentence, at the cursor glued to the word after it, or at the cursor. */
export type TagPlace = "sentence" | "glued" | "inline";

const PAUSES = ["300ms", "500ms", "1s", "2s"].map((p) => ({ label: p, tag: `[pause ${p}]` }));
const RATES = ["0.75", "0.9", "1", "1.1", "1.25"].map((r) => ({ label: `×${r}`, tag: `[rate:${r}]` }));
const chip = "px-1.5 py-0.5 rounded border border-[var(--color-border)] text-[11px] text-[var(--color-text)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]";
const rowLabel = "w-20 shrink-0 text-[10px] uppercase tracking-wide text-[var(--color-muted)]";

/** The block engine's expression tags by category: a click puts a tag at the text's cursor, a span tag wraps the
 * selection. Pauses and tempo work with every engine. */
export default function TagPalette({ engine, onInsert, onWrap }: { engine: string; onInsert: (tag: string, place: TagPlace) => void; onWrap: (open: string, close: string) => void }) {
  const { t } = useTranslation();
  const caps = useEngines()?.find((e) => e.id === engine);
  const label = (k: string) => (k in en.tags ? t(`tags.${k as TagKey}`) : k);
  const tag = (category: string, value: string) => (caps?.tag_format ?? "").replace("{category}", category).replace("{value}", value);
  const place = (category: string, value: string): TagPlace =>
    caps?.sentence_tags.includes(category) || caps?.sentence_tags.includes(value) ? "sentence" : caps?.glued_tags.includes(category) ? "glued" : "inline";
  return (
    <div className="space-y-1.5">
      {caps && caps.tags.length === 0 && <p className="text-[11px] text-[var(--color-muted)]">{t(caps.id.startsWith("qwen3-tts") ? "tags.qwen" : "tags.none", { engine: caps.name })}</p>}
      {caps?.tags.map(([category, values]) => (
        <div key={category} className="flex flex-wrap items-center gap-1">
          <span className={rowLabel}>{label(category)}</span>
          {values.map((v) => category === "scope"
            ? <button key={v} type="button" onClick={() => onWrap(`<${v}>`, `</${v}>`)} title={`<${v}>…</${v}>`} className={chip + " mono"}>{label(v)}</button>
            : <button key={v} type="button" onClick={() => onInsert(tag(category, v), place(category, v))} title={tag(category, v)} className={chip}>{label(v)}</button>)}
        </div>
      ))}
      <div className="flex flex-wrap items-center gap-1 pt-1 border-t border-[var(--color-border)]">
        <span className={rowLabel}>{t("tags.pause")}</span>
        {PAUSES.map((p) => <button key={p.tag} type="button" onClick={() => onInsert(p.tag, "inline")} title={p.tag} className={chip + " mono"}>{p.label}</button>)}
      </div>
      <div className="flex flex-wrap items-center gap-1">
        <span className={rowLabel}>{t("tags.rate")}</span>
        {RATES.map((r) => <button key={r.tag} type="button" onClick={() => onInsert(r.tag, "inline")} title={r.tag} className={chip + " mono"}>{r.label}</button>)}
      </div>
      {engine === "drama3" && <p className="text-[10px] text-[var(--color-warn)]">{t("tags.dramaRussian")}</p>}
      <p className="text-[10px] text-[var(--color-muted)]">{engine === "drama3" ? t("tags.legendDrama") : caps?.glued_tags.length ? t("tags.legend") : t("tags.legendPlain")} {t("tags.universal")}</p>
    </div>
  );
}
