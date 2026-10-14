import { useTranslation } from "react-i18next";
import { prettyVoice, useVoices } from "../../lib/voices";
import { useEngines } from "../../lib/engines";

/** A voice from the library, or the engine's own base voice (no clone); for an engine that speaks only in its own
 * voices, one of those. `inherit` adds "as the project". */
export default function VoicePicker({ value, onChange, inherit, label, className, engine }: {
  value: string | null;
  onChange: (v: string | null) => void;
  inherit?: string;
  label: string;
  className?: string;
  engine?: string;
}) {
  const { t } = useTranslation();
  const { listing } = useVoices();
  const presets = useEngines()?.find((e) => e.id === engine)?.presets ?? [];
  const style = className ?? "bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-2 py-1.5 text-[12px] max-w-full";
  if (presets.length) {
    const foreign = value && !presets.some((p) => p.id === value);
    return (
      <select aria-label={label} value={value ?? ""} onChange={(e) => onChange(e.target.value || null)} className={style}>
        {inherit && <option value="">{inherit}</option>}
        {foreign && <option value={value}>{t("speech.voiceNotPreset", { name: prettyVoice(value) })}</option>}
        {presets.map((p) => (
          <option key={p.id} value={p.id}>{p.name} · {t(p.gender === "female" ? "voice.g_female" : "voice.g_male")}{p.native ? ` · ${t(`speech.native_${p.native}` as "speech.native_en")}` : ""}</option>
        ))}
      </select>
    );
  }
  const voices = [...(listing?.voices ?? [])].sort((a, b) => Number(b.card.favorite) - Number(a.card.favorite) || prettyVoice(a.name).localeCompare(prettyVoice(b.name)));
  const missing = value && listing && !listing.voices.some((v) => v.name === value);
  return (
    <select aria-label={label} value={value ?? ""} onChange={(e) => onChange(e.target.value || null)} className={style}>
      <option value="">{inherit ?? t("speech.baseVoice")}</option>
      {missing && <option value={value}>{t("speech.voiceMissing", { name: value })}</option>}
      {voices.map((v) => (
        <option key={v.name} value={v.name}>{v.card.favorite ? "★ " : ""}{prettyVoice(v.name)}{v.card.lang ? ` · ${v.card.lang.toUpperCase()}` : ""}</option>
      ))}
    </select>
  );
}
