import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { CloudField } from "../../lib/cloud";

/** The fields of a model's form built from its schema — a cloud model's settings, a music studio's song — each by its
 * type; the advanced ones behind «more». */
export default function FormFields({ fields, values, onChange }: { fields: CloudField[]; values: Record<string, unknown>; onChange: (v: Record<string, unknown>) => void }) {
  const { t } = useTranslation();
  const [more, setMore] = useState(false);
  const set = (key: string, v: unknown) => {
    const next = { ...values };
    if (v === "" || v === undefined) delete next[key]; else next[key] = v;
    onChange(next);
  };
  const box = "w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] focus:border-[var(--color-accent)] focus:outline-none";
  const control = (f: CloudField) => {
    const v = values[f.key] ?? f.default ?? "";
    switch (f.type) {
      case "boolean":
        return <input type="checkbox" checked={Boolean(values[f.key] ?? f.default)} onChange={(e) => set(f.key, e.target.checked)} className="w-4 h-4 accent-[var(--color-accent)]" />;
      case "select":
        return (
          <select value={JSON.stringify(v)} onChange={(e) => set(f.key, JSON.parse(e.target.value))} className={box}>
            {!f.required && <option value={JSON.stringify("")}>{t("cloud2.byDefault")}</option>}
            {(f.options ?? []).map((o) => <option key={JSON.stringify(o.value)} value={JSON.stringify(o.value)}>{o.label}</option>)}
          </select>
        );
      case "integer":
      case "number":
        return <input type="number" value={v === "" ? "" : Number(v)} min={f.min ?? undefined} max={f.max ?? undefined} step={f.type === "integer" ? 1 : "any"} onChange={(e) => set(f.key, e.target.value === "" ? "" : Number(e.target.value))} className={box} />;
      case "textarea":
        return <textarea value={String(v)} rows={2} onChange={(e) => set(f.key, e.target.value)} className={box} />;
      default:
        return <input value={String(v)} onChange={(e) => set(f.key, e.target.value)} className={box} />;
    }
  };
  const visible = fields.filter((f) => more || !f.advanced);
  return (
    <>
      {visible.map((f) => (
        <label key={f.key} className="block space-y-0.5 text-[11px] text-[var(--color-muted)]" title={f.description}>
          <span>{f.label}{f.required ? " *" : ""}</span>
          {control(f)}
        </label>
      ))}
      {fields.some((f) => f.advanced) && <button onClick={() => setMore((m) => !m)} className="text-[11px] underline text-[var(--color-muted)]">{more ? t("cloud2.fewer") : t("cloud2.more")}</button>}
    </>
  );
}
