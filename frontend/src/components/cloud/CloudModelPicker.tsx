import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, RefreshCw, Star } from "lucide-react";
import { cloudApi, favourites, setFavourite, type CloudModel, type CloudProvider, type CloudTask, type ProviderState } from "../../lib/cloud";
import { openSettings } from "../../lib/settingsNav";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";

/** A model of the fal or Runware catalogue for speech or recognition: search, the provider's price line, favourites first. */
export default function CloudModelPicker({ task, provider, value, onChange }: { task: CloudTask; provider: CloudProvider; value: string; onChange: (id: string) => void }) {
  const { t } = useTranslation();
  const [models, setModels] = useState<CloudModel[] | null>(null);
  const [state, setState] = useState<ProviderState | null>(null);
  const [query, setQuery] = useState("");
  const [stars, setStars] = useState(favourites);
  const [error, setError] = useState<string | null>(null);
  const load = () => cloudApi.catalog(task).then((r) => { setModels(r.models.filter((m) => m.provider === provider)); setState(r.providers[provider]); setError(null); }, (e: unknown) => setError(errorText(e)));
  useEffect(() => { void load(); }, [task, provider]); // eslint-disable-line react-hooks/exhaustive-deps
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (models ?? [])
      .filter((m) => !q || m.name.toLowerCase().includes(q) || m.id.toLowerCase().includes(q))
      .sort((a, b) => Number(stars.has(`${b.provider}:${b.id}`)) - Number(stars.has(`${a.provider}:${a.id}`)) || a.name.localeCompare(b.name));
  }, [models, query, stars]);
  const field = "w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] focus:border-[var(--color-accent)] focus:outline-none";
  return (
    <div className="space-y-1.5">
      {state && !state.key.configured && (
        <p className="text-[11px] text-[var(--color-warn)]">{t("cloud2.noKey", { provider })} <button onClick={() => openSettings("cloud")} className="underline">{t("cloud2.addKey")}</button></p>
      )}
      <div className="flex items-center gap-1.5">
        <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("cloud2.search")} aria-label={t("cloud2.search")} className={field} />
        <button onClick={() => { void cloudApi.refresh().then(() => setTimeout(() => void load(), 4000)); }} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("cloud2.refresh"))}>
          {state?.refreshing ? <Loader2 size={13} className="animate-spin" /> : <RefreshCw size={13} />}
        </button>
      </div>
      {state?.error && <p className="text-[10px] text-[var(--color-muted)]">{t("cloud2.catalogError", { error: state.error })}</p>}
      {error && <p role="alert" className="text-[11px] text-[var(--color-warn)]">{error}</p>}
      <ul role="listbox" aria-label={t("cloud2.models")} className="max-h-52 overflow-y-auto rounded-md border border-[var(--color-border)] divide-y divide-[var(--color-border)]">
        {models === null && <li className="p-2"><Loader2 size={13} className="animate-spin text-[var(--color-muted)]" /></li>}
        {shown.map((m) => {
          const key = `${m.provider}:${m.id}`;
          const on = stars.has(key);
          return (
            <li key={key} role="option" aria-selected={value === m.id} className={`flex items-start gap-2 px-2 py-1.5 cursor-pointer ${value === m.id ? "bg-[color-mix(in_oklab,var(--color-accent)_14%,transparent)]" : "hover:bg-[var(--color-surface-2)]"}`} onClick={() => onChange(m.id)}>
              <button onClick={(e) => { e.stopPropagation(); setStars(new Set(setFavourite(key, !on))); }} className={`mt-0.5 ${on ? "text-[var(--color-warn)]" : "text-[var(--color-muted)]"}`} {...named(on ? t("cloud2.unfavourite") : t("cloud2.favourite"))}><Star size={12} fill={on ? "currentColor" : "none"} /></button>
              <span className="flex-1 min-w-0">
                <span className="block text-[12px] truncate">{m.name}</span>
                <span className="block text-[10px] text-[var(--color-muted)] truncate mono">{m.id}</span>
              </span>
              <span className="shrink-0 text-[10px] text-[var(--color-muted)] max-w-[40%] text-right">{m.price ?? t("cloud2.priceAfter")}</span>
            </li>
          );
        })}
        {models && shown.length === 0 && <li className="p-2 text-[11px] text-[var(--color-muted)]">{t("cloud2.none")}</li>}
      </ul>
    </div>
  );
}
