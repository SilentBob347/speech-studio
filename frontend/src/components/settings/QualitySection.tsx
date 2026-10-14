import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Timer } from "lucide-react";
import { api } from "../../lib/api";
import SettingSwitch from "./SettingSwitch";
import { SETTINGS_CHANGED, useChanged } from "../../lib/mcpBridge";

// Тумблеры — слоты active.json. defaultOn: слот, которого ещё нет в файле, считается включённым.
const DIAGNOSTICS = [
  { key: "bench", icon: Timer, labelKey: "settings.bench", hintKey: "settings.benchHint", tipKey: "settings.benchHint", defaultOn: false },
] as const;

type Row = (typeof DIAGNOSTICS)[number];

type Selection = Record<string, unknown>;

const isOn = (sel: Selection, row: Row) => (row.defaultOn ? sel[row.key] !== "0" : sel[row.key] === "1");

export default function QualitySection() {
  const { t } = useTranslation();
  const [sel, setSel] = useState<Selection | null>(null);
  const [saving, setSaving] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reread, setReread] = useState(0);
  useChanged(SETTINGS_CHANGED, () => setReread((n) => n + 1));

  useEffect(() => {
    api.capabilities()
      .then((c) => setSel(c.selection ?? {}))
      .catch((e: unknown) => setError(t("prefs.loadFailed", { error: e instanceof Error ? e.message : String(e) })));
  }, [t, reread]);

  const toggle = async (row: Row) => {
    if (!sel || saving) return;
    const next = isOn(sel, row) ? "0" : "1";
    setSaving(row.key);
    setError(null);
    try {
      setSel(await api.setSelection(row.key, next));
    } catch (e) {
      setError(t("prefs.saveFailed", { error: e instanceof Error ? e.message : String(e) }));
    } finally {
      setSaving(null);
    }
  };

  const group = (title: string, rows: readonly Row[]) => (
    <section className="mb-5">
      <h4 className="text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)] mb-2">{title}</h4>
      <div className="rounded-lg border border-[var(--color-border)] divide-y divide-[var(--color-border)] bg-[var(--color-surface-2)]/40">
        {rows.map((row) => (
          <SettingSwitch key={row.key} icon={row.icon} label={t(row.labelKey)} hint={t(row.hintKey)} tip={t(row.tipKey)}
            on={sel ? isOn(sel, row) : row.defaultOn} busy={saving === row.key} disabled={!sel || saving !== null}
            onToggle={() => { void toggle(row); }} />
        ))}
      </div>
    </section>
  );

  return (
    <div className="max-w-2xl">
      {error && <p role="alert" className="mb-3 mono text-[11px] text-[var(--color-warn)] break-words">{error}</p>}
      {group(t("prefs.diagnostics"), DIAGNOSTICS)}
    </div>
  );
}
