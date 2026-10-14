import { useTranslation } from "react-i18next";
import { prettyVoice, useVoices } from "../../lib/voices";
import { useEngines } from "../../lib/engines";

/** A voice from the library, or the engine's own base voice (no clone); for an engine with voices of its own, those
 * first — alone when it does not clone, beside the library when it does. `inherit` adds "as the project". */
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
  const caps = useEngines()?.find((e) => e.id === engine);
  const presets = caps?.presets ?? [];
  const clones = !presets.length || caps?.clone !== "none";
  const style = className ?? "bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-2 py-1.5 text-[12px] max-w-full";
  const voices = clones ? [...(listing?.voices ?? [])].sort((a, b) => Number(b.card.favorite) - Number(a.card.favorite) || prettyVoice(a.name).localeCompare(prettyVoice(b.name))) : [];
  const known = presets.some((p) => p.id === value) || (clones && listing?.voices.some((v) => v.name === value));
  const missing = value && (presets.length || listing) && !known;
  const library = voices.map((v) => (
    <option key={v.name} value={v.name}>{v.card.favorite ? "★ " : ""}{prettyVoice(v.name)}{v.card.lang ? ` · ${v.card.lang.toUpperCase()}` : ""}</option>
  ));
  return (
    <select aria-label={label} value={value ?? ""} onChange={(e) => onChange(e.target.value || null)} className={style}>
      {(inherit || !presets.length) && <option value="">{inherit ?? t("speech.baseVoice")}</option>}
      {missing && <option value={value}>{t(presets.length && !clones ? "speech.voiceNotPreset" : "speech.voiceMissing", { name: prettyVoice(value) })}</option>}
      {presets.length > 0 && (
        <optgroup label={t("speech.engineVoices")}>
          {presets.map((p) => (
            <option key={p.id} value={p.id}>{p.name} · {t(p.gender === "female" ? "voice.g_female" : "voice.g_male")}{p.native ? ` · ${t(`speech.native_${p.native}` as "speech.native_en")}` : ""}</option>
          ))}
        </optgroup>
      )}
      {presets.length > 0 && voices.length > 0 ? <optgroup label={t("speech.libraryClone")}>{library}</optgroup> : library}
    </select>
  );
}
