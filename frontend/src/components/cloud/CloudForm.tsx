import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { cloudApi, editableFields, type CloudField, type CloudForm as Form, type CloudProvider } from "../../lib/cloud";
import { errorText } from "../../lib/voices";

/** The settings of a cloud model built from its schema (voice, speed, style…), the text field left to the blocks. */
export default function CloudForm({ provider, id, values, onChange, runs, chars }: {
  provider: CloudProvider; id: string; values: Record<string, unknown>; onChange: (v: Record<string, unknown>) => void; runs: number; chars: number;
}) {
  const { t, i18n } = useTranslation();
  const [form, setForm] = useState<Form | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [cost, setCost] = useState<{ usd: number | null; exact: boolean | null; note: string | null } | null>(null);
  const [more, setMore] = useState(false);
  useEffect(() => {
    let alive = true;
    cloudApi.form(provider, id).then((f) => { if (alive) { setForm(f); setError(null); } }, (e: unknown) => { if (alive) setError(errorText(e)); });
    return () => { alive = false; };
  }, [provider, id]);
  useEffect(() => {
    let alive = true;
    const timer = window.setTimeout(() => {
      cloudApi.estimate(provider, id, values, runs, chars).then((c) => { if (alive) setCost(c); }, () => { if (alive) setCost(null); });
    }, 400);
    return () => { alive = false; window.clearTimeout(timer); };
  }, [provider, id, values, runs, chars]);
  if (error) return <p role="alert" className="text-[11px] text-[var(--color-warn)] break-words">{error}</p>;
  if (!form) return <Loader2 size={13} className="animate-spin text-[var(--color-muted)]" />;
  const fields = editableFields(form);
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
    <div className="space-y-2">
      {visible.map((f) => (
        <label key={f.key} className="block space-y-0.5 text-[11px] text-[var(--color-muted)]" title={f.description}>
          <span>{f.label}{f.required ? " *" : ""}</span>
          {control(f)}
        </label>
      ))}
      {fields.some((f) => f.advanced) && <button onClick={() => setMore((m) => !m)} className="text-[11px] underline text-[var(--color-muted)]">{more ? t("cloud2.fewer") : t("cloud2.more")}</button>}
      <p className="text-[11px] tabnum">
        {cost?.usd != null
          ? t(cost.exact === false ? "cloud2.costAbout" : "cloud2.cost", { usd: cost.usd.toLocaleString(i18n.language, { maximumFractionDigits: 4 }), runs })
          : t("cloud2.costAfter")}
        {form.price.note ? <span className="block text-[10px] text-[var(--color-muted)]">{form.price.note}</span> : null}
      </p>
    </div>
  );
}
