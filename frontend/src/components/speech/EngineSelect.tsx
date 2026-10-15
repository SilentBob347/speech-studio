import { useTranslation } from "react-i18next";
import { useEngines } from "../../lib/engines";
import { CLOUD_ENGINE_NAMES } from "../../lib/cloudConsent";
import type { EngineCaps } from "../../lib/speech";

/** The engine of one speaker or block over the project's: local engines and the project's own (a cloud engine is set up
 * for the whole project). An engine that is not ready is greyed with the reason; one that cannot speak the voice in use
 * says which of its own voices it takes instead, and choosing it hands that voice over with the engine. */
export default function EngineSelect({ value, onChange, inherit, inheritEngine, projectEngine, voice, label, className }: {
  value: string | null;
  /** `voice` is set when the voice in use does not suit the engine chosen: its first own voice, or null to drop an
   * own voice the inherited engine cannot speak. */
  onChange: (engine: string | null, voice?: string | null) => void;
  inherit: string;
  /** The engine `inherit` stands for. */
  inheritEngine: string;
  projectEngine: string;
  /** The voice the block or speaker would speak with. */
  voice: string | null;
  label: string;
  className?: string;
}) {
  const { t } = useTranslation();
  const engines = useEngines() ?? [];
  const isPreset = voice ? engines.some((e) => e.presets.some((p) => p.id === voice)) : false;
  const swap = (e: EngineCaps): string | undefined => {
    const library = !!voice && !isPreset;
    const fits = e.presets.some((p) => p.id === voice) || (library && e.clone !== "none") || (!voice && !e.needs_voice);
    return fits ? undefined : e.presets[0]?.id;
  };
  /** An engine that speaks only library voices while the voice in use is another engine's own one. */
  const needsLibrary = (e: EngineCaps) => e.needs_voice && !e.presets.length && isPreset;
  const choices = engines.filter((e) => !(e.id in CLOUD_ENGINE_NAMES) || e.id === projectEngine);
  return (
    <select value={value ?? ""} onChange={(ev) => {
      const e = engines.find((x) => x.id === ev.target.value);
      if (e) { onChange(e.id, swap(e)); return; }
      const inherited = engines.find((x) => x.id === inheritEngine);
      onChange(null, isPreset && inherited && !inherited.presets.some((p) => p.id === voice) ? null : undefined);
    }} aria-label={label} className={className}>
      <option value="">{inherit}</option>
      {choices.map((e) => {
        const instead = e.id === value ? undefined : swap(e);
        const blocked = e.unavailable ?? (e.id !== value && needsLibrary(e) ? t("speech.engineNeedsLibrary") : null);
        const note = blocked ?? (instead ? t("speech.engineTakesVoice", { voice: e.presets.find((p) => p.id === instead)?.name ?? instead }) : null);
        return <option key={e.id} value={e.id} disabled={!!blocked} title={note ?? undefined}>{note ? `${e.name} — ${note}` : e.name}</option>;
      })}
    </select>
  );
}
