import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { KeyRound, Loader2, Trash2 } from "lucide-react";
import { api } from "../../lib/api";
import { cloudApi, type CloudProvider, type KeyState, type Spend } from "../../lib/cloud";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";

const PROVIDERS: { id: CloudProvider; name: string }[] = [{ id: "fal", name: "fal.ai" }, { id: "runware", name: "Runware" }];

/** Keys of fal and Runware (checked by a free balance call), what the cloud has cost, and the agents' daily limit. */
export default function CloudProviders() {
  const { t, i18n } = useTranslation();
  const [keys, setKeys] = useState<Record<CloudProvider, KeyState> | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [spend, setSpend] = useState<Spend | null>(null);
  const [limit, setLimit] = useState("");
  const money = (usd: number) => usd.toLocaleString(i18n.language, { style: "currency", currency: "USD", maximumFractionDigits: 4 });
  const load = () => {
    cloudApi.keys().then(setKeys, () => setKeys(null));
    cloudApi.spend().then((s) => { setSpend(s); setLimit(s.daily_limit_usd ? String(s.daily_limit_usd) : ""); }, () => setSpend(null));
  };
  useEffect(load, []);
  const save = async (p: CloudProvider) => {
    setBusy(p);
    try {
      const r = await cloudApi.setKey(p, drafts[p] ?? "");
      setNotes((n) => ({ ...n, [p]: r.balance != null ? t("cloud2.keyOkBalance", { balance: money(r.balance) }) : t("cloud2.keyOk") }));
      setDrafts((d) => ({ ...d, [p]: "" }));
      load();
    } catch (e) { setNotes((n) => ({ ...n, [p]: errorText(e) })); } finally { setBusy(null); }
  };
  const remove = async (p: CloudProvider) => {
    try { await cloudApi.deleteKey(p); setNotes((n) => ({ ...n, [p]: "" })); load(); } catch (e) { setNotes((n) => ({ ...n, [p]: errorText(e) })); }
  };
  const box = "bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] focus:border-[var(--color-accent)] focus:outline-none";
  return (
    <section className="space-y-4 mt-6">
      <div className="space-y-1">
        <h3 className="text-[13px] font-semibold">{t("cloud2.providersTitle")}</h3>
        <p className="text-[11px] text-[var(--color-muted)] leading-relaxed">{t("cloud2.providersHint")}</p>
      </div>
      {PROVIDERS.map(({ id, name }) => {
        const k = keys?.[id];
        return (
          <div key={id} className="space-y-1.5 rounded-lg border border-[var(--color-border)] p-3">
            <div className="flex items-center gap-2 text-[13px]">
              <KeyRound size={13} className="text-[var(--color-accent)]" />
              <span className="font-medium">{name}</span>
              <span className="text-[11px] text-[var(--color-muted)]">{k?.configured ? (k.source === "environment" ? t("cloud2.keyFromEnv", { name: k.environment_variable }) : t("cloud2.keySaved")) : t("cloud2.keyNone")}</span>
              {k?.configured && k.source === "local_store" && <button onClick={() => void remove(id)} className="ml-auto p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-warn)]" {...named(t("cloud2.keyRemove", { name }))}><Trash2 size={13} /></button>}
            </div>
            {k?.source !== "environment" && (
              <div className="flex items-center gap-2">
                <input type="password" autoComplete="off" value={drafts[id] ?? ""} onChange={(e) => setDrafts((d) => ({ ...d, [id]: e.target.value }))} placeholder={t("cloud2.keyPlaceholder")} aria-label={t("cloud2.keyFor", { name })} className={`${box} flex-1 min-w-0`} />
                <button onClick={() => void save(id)} disabled={!drafts[id]?.trim() || busy === id} className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[12px] font-semibold disabled:opacity-40">{busy === id && <Loader2 size={12} className="animate-spin" />}{t("cloud2.keyCheck")}</button>
              </div>
            )}
            {notes[id] && <p className="text-[11px] text-[var(--color-muted)] break-words">{notes[id]}</p>}
          </div>
        );
      })}
      <div className="space-y-2 rounded-lg border border-[var(--color-border)] p-3">
        <h3 className="text-[13px] font-semibold">{t("cloud2.spendTitle")}</h3>
        {spend ? (
          <>
            <p className="text-[12px] tabnum">{t("cloud2.spendTotals", { total: money(spend.total), today: money(spend.today) })}{spend.unpriced ? ` · ${t("cloud2.spendUnpriced", { n: spend.unpriced })}` : ""}</p>
            {spend.providers.length > 0 && (
              <ul className="text-[11px] text-[var(--color-muted)] tabnum space-y-0.5">
                {spend.providers.map((g) => <li key={g.key}>{g.key}: {money(g.usd)} · {t("cloud2.runs", { n: g.runs })}</li>)}
              </ul>
            )}
          </>
        ) : <Loader2 size={13} className="animate-spin text-[var(--color-muted)]" />}
        <label className="flex flex-wrap items-center gap-2 text-[12px]">
          {t("cloud2.limit")}
          <input type="number" min={0} max={10000} step="0.5" value={limit} onChange={(e) => setLimit(e.target.value)} onBlur={() => { void api.setSelection("cloud_daily_limit_usd", limit.trim() === "" ? "0" : limit.trim()).then(load); }} className={`${box} w-24`} />
          <span className="text-[11px] text-[var(--color-muted)]">{t("cloud2.limitHint")}</span>
        </label>
      </div>
    </section>
  );
}
