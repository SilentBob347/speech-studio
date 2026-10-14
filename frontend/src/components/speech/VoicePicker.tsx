import { useTranslation } from "react-i18next";
import { prettyVoice, useVoices } from "../../lib/voices";

/** A voice from the library, or the engine's own base voice (no clone). `inherit` adds "as the project". */
export default function VoicePicker({ value, onChange, inherit, label, className }: {
  value: string | null;
  onChange: (v: string | null) => void;
  inherit?: string;
  label: string;
  className?: string;
}) {
  const { t } = useTranslation();
  const { listing } = useVoices();
  const voices = [...(listing?.voices ?? [])].sort((a, b) => Number(b.card.favorite) - Number(a.card.favorite) || prettyVoice(a.name).localeCompare(prettyVoice(b.name)));
  const missing = value && listing && !listing.voices.some((v) => v.name === value);
  return (
    <select aria-label={label} value={value ?? ""} onChange={(e) => onChange(e.target.value || null)}
      className={className ?? "bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-2 py-1.5 text-[12px] max-w-full"}>
      <option value="">{inherit ?? t("speech.baseVoice")}</option>
      {missing && <option value={value}>{t("speech.voiceMissing", { name: value })}</option>}
      {voices.map((v) => (
        <option key={v.name} value={v.name}>{v.card.favorite ? "★ " : ""}{prettyVoice(v.name)}{v.card.lang ? ` · ${v.card.lang.toUpperCase()}` : ""}</option>
      ))}
    </select>
  );
}
