import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { cloudApi, editableFields, type CloudForm as Form, type CloudProvider } from "../../lib/cloud";
import FormFields from "./FormFields";
import { errorText } from "../../lib/voices";

/** The settings of a cloud model built from its schema (voice, speed, style…), the text field left to the blocks. */
export default function CloudForm({ provider, id, values, onChange, runs, chars }: {
  provider: CloudProvider; id: string; values: Record<string, unknown>; onChange: (v: Record<string, unknown>) => void; runs: number; chars: number;
}) {
  const { t, i18n } = useTranslation();
  const [form, setForm] = useState<Form | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [cost, setCost] = useState<{ usd: number | null; exact: boolean | null; note: string | null } | null>(null);
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
  return (
    <div className="space-y-2">
      <FormFields fields={fields} values={values} onChange={onChange} />
      <p className="text-[11px] tabnum">
        {cost?.usd != null
          ? t(cost.exact === false ? "cloud2.costAbout" : "cloud2.cost", { usd: cost.usd.toLocaleString(i18n.language, { maximumFractionDigits: 4 }), runs })
          : t("cloud2.costAfter")}
        {form.price.note ? <span className="block text-[10px] text-[var(--color-muted)]">{form.price.note}</span> : null}
      </p>
    </div>
  );
}
