import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../../lib/api";
import { SETTINGS_CHANGED, settingsChangedHere, useChanged } from "../../lib/mcpBridge";

// Пресет под железо: автоопределение GPU/VRAM -> рекомендованные кванты, но юзер применяет любой сам.
// Облачный пресет включает OpenRouter на все стадии — работает даже на слабом ПК без своей GPU.
export default function PresetsSection({ onApplied }: { onApplied?: () => void }) {
  const { t } = useTranslation();
  const [data, setData] = useState<Awaited<ReturnType<typeof api.hwPresets>> | null>(null);
  const [busy, setBusy] = useState(false);
  // «custom» writes nothing, so the settings still hold the preset they were on
  const [custom, setCustom] = useState(false);
  const load = () => { api.hwPresets().then(setData).catch(() => {}); };
  useEffect(() => { load(); }, []);
  useChanged(SETTINGS_CHANGED, load);
  const apply = async (id: string) => {
    setBusy(true);
    setCustom(id === "custom");
    try { await api.applyPreset(id); settingsChangedHere(); onApplied?.(); } catch { /* ignore */ }
    setBusy(false);
  };
  const hw = data?.hardware;
  const cur = custom ? "custom" : data?.current ?? "";
  const curP = data?.presets.find((p) => p.id === cur);
  return (
    <div className="mb-4 pb-3 border-b border-[var(--color-border)]">
      <div className="text-[11px] uppercase tracking-[0.16em] text-[var(--color-muted)] mb-1.5">{t("hwPreset.title")}</div>
      <div className="px-2.5 py-2 rounded-lg bg-[var(--color-surface-2)] border border-[var(--color-border)]">
        <div className="flex items-center gap-2.5">
          <span className="w-1.5 h-1.5 rounded-full shrink-0 bg-[var(--color-accent)]" />
          <div className="min-w-0 flex-1">
            <div className="text-[12px] font-medium truncate">{hw ? (hw.hasGpu ? t("hwPreset.gpu", { name: hw.gpuName, gb: hw.totalVramGb.toFixed(0) }) : t("hwPreset.noGpu")) : "…"}</div>
            <div className="mono text-[10px] text-[var(--color-muted)] break-words">{curP?.subtitle ?? t("hwPreset.ram", { gb: hw?.totalRamGb.toFixed(0) ?? "?" })}</div>
          </div>
          <select value={cur} onChange={(e) => apply(e.target.value)} disabled={busy}
            className="shrink-0 bg-[var(--color-surface)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[11px] mono focus:border-[var(--color-accent)] focus:outline-none disabled:opacity-50">
            {(data?.presets ?? []).map((p) => <option key={p.id} value={p.id}>{p.title}{p.id === hw?.recommended ? " ★" : ""}</option>)}
          </select>
        </div>
      </div>
    </div>
  );
}
