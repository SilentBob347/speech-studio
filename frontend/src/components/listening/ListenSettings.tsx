import { useTranslation } from "react-i18next";
import { Plus, Trash2, X } from "lucide-react";
import type { ListenEngine, ListenProject } from "../../lib/listen";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import OpenRouterModelSelect from "../OpenRouterModelSelect";
import CloudModelPicker from "../cloud/CloudModelPicker";
import { LANG_CHOICES } from "../speech/langs";

const ENGINES: ListenEngine[] = ["", "parakeet", "whisper", "openrouter", "fal"];
const SPEAKERS = [0, 1, 2, 3, 4, 5, 6, 7, 8];

/** How the recording is recognised: engine, language, speakers, voice separation, hallucination filter, corrections. */
export default function ListenSettings({ project, onChange, onClose }: { project: ListenProject; onChange: (p: ListenProject) => void; onClose: () => void }) {
  const { t } = useTranslation();
  useEscapeLayer(onClose);
  const s = project.settings;
  const set = (patch: Partial<ListenProject["settings"]>) => onChange({ ...project, settings: { ...s, ...patch } });
  const fixes = project.fixes;
  const setFixes = (next: ListenProject["fixes"]) => onChange({ ...project, fixes: next });
  const field = "w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1.5 text-[13px] focus:border-[var(--color-accent)] focus:outline-none";
  const label = "block space-y-1 text-[12px] text-[var(--color-muted)]";
  const check = "flex items-start gap-2 text-[13px] cursor-pointer";
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade" onClick={onClose}>
      <div role="dialog" aria-modal="true" aria-labelledby="ls-title" className="w-[min(94vw,560px)] max-h-[90vh] overflow-y-auto rounded-xl glass-panel anim-pop p-5 space-y-4" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between">
          <h2 id="ls-title" className="font-semibold">{t("listen.settingsTitle")}</h2>
          <button onClick={onClose} className="text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("a11y.close"))}><X size={16} /></button>
        </div>
        <label className={label}>{t("listen.engine")}
          <select value={s.engine} onChange={(e) => set({ engine: e.target.value as ListenEngine })} className={field}>
            {ENGINES.map((e) => <option key={e} value={e}>{t(`listen.engine_${e || "default"}`)}</option>)}
          </select>
          <span className="block text-[11px] leading-relaxed">{t(`listen.engineHint_${s.engine || "default"}`)}</span>
        </label>
        {s.engine === "fal" && <CloudModelPicker task="transcribe" provider="fal" value={s.cloud_model} onChange={(id) => set({ cloud_model: id })} />}
        {s.engine === "openrouter" && (
          <OpenRouterModelSelect kind="asr" value={s.cloud_model} onChange={(id) => set({ cloud_model: id })} placeholder={t("listen.cloudModelDefault")} />
        )}
        <div className="grid grid-cols-2 gap-3">
          <label className={label}>{t("speech.lang")}
            <select value={s.lang} onChange={(e) => set({ lang: e.target.value })} className={field}>
              {LANG_CHOICES.map((l) => <option key={l} value={l}>{l === "auto" ? t("speech.langAuto") : l.toUpperCase()}</option>)}
            </select>
          </label>
          <label className={label}>{t("listen.speakers")}
            <select value={s.speakers} onChange={(e) => set({ speakers: Number(e.target.value) })} className={field}>
              {SPEAKERS.map((n) => <option key={n} value={n}>{n === 0 ? t("listen.speakersAuto") : n === 1 ? t("listen.speakersOne") : n}</option>)}
            </select>
          </label>
        </div>
        <label className={check}>
          <input type="checkbox" checked={s.separate} onChange={(e) => set({ separate: e.target.checked })} className="mt-0.5 w-4 h-4 accent-[var(--color-accent)]" />
          <span>{t("listen.separate")}<span className="block text-[11px] text-[var(--color-muted)]">{t("listen.separateHint")}</span></span>
        </label>
        <label className={check}>
          <input type="checkbox" checked={s.filter} onChange={(e) => set({ filter: e.target.checked })} className="mt-0.5 w-4 h-4 accent-[var(--color-accent)]" />
          <span>{t("listen.filter")}<span className="block text-[11px] text-[var(--color-muted)]">{t("listen.filterHint")}</span></span>
        </label>
        <fieldset className="space-y-2">
          <legend className="text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)] mb-1">{t("listen.fixes")}</legend>
          <p className="text-[11px] text-[var(--color-muted)] leading-relaxed">{t("listen.fixesHint")}</p>
          {fixes.map((f, i) => (
            <div key={i} className="flex items-center gap-2">
              <input value={f.heard} onChange={(e) => setFixes(fixes.map((x, j) => (j === i ? { ...x, heard: e.target.value } : x)))} placeholder={t("listen.fixHeard")} aria-label={t("listen.fixHeard")} className={field} />
              <span className="text-[var(--color-muted)]">→</span>
              <input value={f.say} onChange={(e) => setFixes(fixes.map((x, j) => (j === i ? { ...x, say: e.target.value } : x)))} placeholder={t("listen.fixSay")} aria-label={t("listen.fixSay")} className={field} />
              <button onClick={() => setFixes(fixes.filter((_, j) => j !== i))} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-warn)]" {...named(t("listen.fixRemove"))}><Trash2 size={13} /></button>
            </div>
          ))}
          <button onClick={() => setFixes([...fixes, { heard: "", say: "" }])} className="inline-flex items-center gap-1 px-2 py-1 rounded-md border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)]"><Plus size={12} />{t("listen.fixAdd")}</button>
        </fieldset>
        <p className="text-[11px] text-[var(--color-muted)]">{t("listen.settingsApply")}</p>
      </div>
    </div>
  );
}
