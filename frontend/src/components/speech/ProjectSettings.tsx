import { useTranslation } from "react-i18next";
import { Dices, X } from "lucide-react";
import type { Loudness, SpeechProject } from "../../lib/speech";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import { LANG_CHOICES } from "./langs";

const LOUDNESS: Loudness[] = ["video", "podcast", "acx", "off"];
const field = "mt-1 w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1.5 text-[13px] mono";
const label = "block text-[12px] text-[var(--color-muted)]";

/** The project's synthesis: language, sampling, seed, pauses between lines and the loudness of the export. */
export default function ProjectSettings({ project, onChange, onClose }: { project: SpeechProject; onChange: (f: (p: SpeechProject) => SpeechProject) => void; onClose: () => void }) {
  const { t } = useTranslation();
  useEscapeLayer(onClose);
  const s = project.settings;
  const set = (f: (p: SpeechProject) => void) => onChange((p) => { f(p); return p; });
  const num = (v: string, lo: number, hi: number, fallback: number) => { const n = Number(v); return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : fallback; };
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade" onClick={onClose}>
      <div role="dialog" aria-modal="true" aria-labelledby="ps-title" className="w-[min(94vw,560px)] max-h-[88vh] overflow-y-auto rounded-xl glass-panel anim-pop p-5 space-y-4" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between">
          <h2 id="ps-title" className="font-semibold">{t("speech.settingsTitle")}</h2>
          <button onClick={onClose} {...named(t("a11y.close"))} className="text-[var(--color-muted)] hover:text-[var(--color-text)]"><X size={16} /></button>
        </div>
        <label className={label}>{t("speech.lang")}
          <select value={s.lang} onChange={(e) => set((p) => { p.settings.lang = e.target.value; })} className={field}>
            {LANG_CHOICES.map((l) => <option key={l} value={l}>{l === "auto" ? t("speech.langAuto") : l.toUpperCase()}</option>)}
          </select>
        </label>
        <fieldset className="space-y-2">
          <legend className="text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)] mb-1">{t("speech.sampling")}</legend>
          <div className="grid grid-cols-3 gap-2">
            <label className={label} title={t("speech.temperatureHint")}>{t("speech.temperature")}
              <input type="number" step={0.05} min={0.05} max={2} value={s.params.temperature} onChange={(e) => set((p) => { p.settings.params.temperature = num(e.target.value, 0.05, 2, 1); })} className={field} />
            </label>
            <label className={label} title={t("speech.topKHint")}>top-k
              <input type="number" step={1} min={1} max={500} value={s.params.top_k} onChange={(e) => set((p) => { p.settings.params.top_k = Math.round(num(e.target.value, 1, 500, 50)); })} className={field} />
            </label>
            <label className={label} title={t("speech.topPHint")}>top-p
              <input type="number" step={0.01} min={0.05} max={1} value={s.params.top_p} onChange={(e) => set((p) => { p.settings.params.top_p = num(e.target.value, 0.05, 1, 0.95); })} className={field} />
            </label>
          </div>
          <p className="text-[11px] text-[var(--color-muted)]">{t("speech.samplingDefaults")}</p>
          <div className="flex items-end gap-2">
            <label className={label + " flex-1"}>{t("speech.seed")}
              <select value={s.params.seed_mode} onChange={(e) => set((p) => { p.settings.params.seed_mode = e.target.value === "fixed" ? "fixed" : "random"; })} className={field}>
                <option value="random">{t("speech.seedRandom")}</option>
                <option value="fixed">{t("speech.seedFixed")}</option>
              </select>
            </label>
            {s.params.seed_mode === "fixed" && (
              <>
                <label className={label + " w-36"}>{t("speech.seedValue")}
                  <input type="number" min={0} max={2147483647} value={s.params.seed} onChange={(e) => set((p) => { p.settings.params.seed = Math.round(num(e.target.value, 0, 2147483647, 42)); })} className={field} />
                </label>
                <button onClick={() => set((p) => { p.settings.params.seed = Math.floor(Math.random() * 2147483647); })} className="mb-0.5 p-2 rounded-md border border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("speech.seedNew"))}><Dices size={14} /></button>
              </>
            )}
          </div>
        </fieldset>
        <fieldset className="grid grid-cols-2 gap-2">
          <legend className="col-span-2 text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)] mb-1">{t("speech.pauses")}</legend>
          <label className={label}>{t("speech.pauseSame")}
            <input type="number" step={50} min={-2000} max={10000} value={s.pause_same_ms} onChange={(e) => set((p) => { p.settings.pause_same_ms = Math.round(num(e.target.value, -2000, 10000, 300)); })} className={field} />
          </label>
          <label className={label}>{t("speech.pauseSwitch")}
            <input type="number" step={50} min={-2000} max={10000} value={s.pause_switch_ms} onChange={(e) => set((p) => { p.settings.pause_switch_ms = Math.round(num(e.target.value, -2000, 10000, 600)); })} className={field} />
          </label>
          <p className="col-span-2 text-[11px] text-[var(--color-muted)]">{t("speech.pausesHint")}</p>
        </fieldset>
        <fieldset className="space-y-2">
          <legend className="text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)] mb-1">{t("speech.quality")}</legend>
          <label className="flex items-center gap-2 text-[13px] cursor-pointer">
            <input type="checkbox" checked={s.qc.asr_check} onChange={(e) => set((p) => { p.settings.qc.asr_check = e.target.checked; })} className="w-4 h-4 accent-[var(--color-accent)]" />
            {t("speech.asrCheck")}
          </label>
          <p className="text-[11px] text-[var(--color-muted)]">{t("speech.asrCheckHint")}</p>
          <div className="grid grid-cols-2 gap-2">
            <label className={label}>{t("speech.strictness")}
              <select value={s.qc.strictness} onChange={(e) => set((p) => { p.settings.qc.strictness = e.target.value as "strict" | "normal" | "lenient"; })} className={field}>
                {(["strict", "normal", "lenient"] as const).map((k) => <option key={k} value={k}>{t(`speech.strictness_${k}`)}</option>)}
              </select>
            </label>
            <label className={label}>{t("speech.takesPerBlock")}
              <input type="number" min={1} max={20} value={s.qc.takes} onChange={(e) => set((p) => { p.settings.qc.takes = Math.round(num(e.target.value, 1, 20, 1)); })} className={field} />
            </label>
          </div>
          <p className="text-[11px] text-[var(--color-muted)]">{t("speech.takesHint")}</p>
        </fieldset>
        <label className={label}>{t("speech.loudness")}
          <select value={s.loudness} onChange={(e) => set((p) => { p.settings.loudness = e.target.value as Loudness; })} className={field}>
            {LOUDNESS.map((l) => <option key={l} value={l}>{t(`speech.loudness_${l}`)}</option>)}
          </select>
        </label>
      </div>
    </div>
  );
}
