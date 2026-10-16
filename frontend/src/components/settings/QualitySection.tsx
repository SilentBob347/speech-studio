import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { MemoryStick, Timer } from "lucide-react";
import { api } from "../../lib/api";
import SettingSwitch from "./SettingSwitch";
import DiagnosticsPanel from "./DiagnosticsPanel";
import { SETTINGS_CHANGED, useChanged } from "../../lib/mcpBridge";
import Hint from "../Hint";

// Тумблеры — слоты active.json. defaultOn: слот, которого ещё нет в файле, считается включённым.
const DIAGNOSTICS = [
  { key: "bench", icon: Timer, labelKey: "settings.bench", hintKey: "settings.benchHint", tipKey: "settings.benchHint", defaultOn: false },
] as const;

type Row = (typeof DIAGNOSTICS)[number];
const IDLE_MINUTES = ["0", "5", "15", "30", "60"] as const;

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
      <section className="mb-5">
        <h4 className="text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)] mb-2">{t("prefs.vram")}</h4>
        <div className="flex items-center gap-3 px-3 py-2.5 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)]/40">
          <MemoryStick size={15} className="shrink-0 text-[var(--color-accent-2)]" />
          <label htmlFor="tts-idle" className="min-w-0 flex-1">
            <span className="block text-[13px] font-medium">{t("prefs.idleUnload")} <Hint text={t("prefs.idleUnloadHint")} /></span>
          </label>
          <select id="tts-idle" value={String(sel?.tts_idle_unload_min ?? "15")} disabled={!sel || saving !== null}
            onChange={(e) => { const v = e.target.value; setSaving("tts_idle_unload_min"); api.setSelection("tts_idle_unload_min", v).then(setSel, (er: unknown) => setError(t("prefs.saveFailed", { error: er instanceof Error ? er.message : String(er) }))).finally(() => setSaving(null)); }}
            className="shrink-0 bg-[var(--color-surface)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] focus:border-[var(--color-accent)] focus:outline-none">
            {IDLE_MINUTES.map((m) => <option key={m} value={m}>{m === "0" ? t("prefs.idleNever") : t("prefs.idleMinutes", { n: Number(m) })}</option>)}
          </select>
        </div>
      </section>
      {group(t("prefs.diagnostics"), DIAGNOSTICS)}
      <DiagnosticsPanel />
    </div>
  );
}
