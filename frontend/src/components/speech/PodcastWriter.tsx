import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Mic2, Plus, Trash2, X } from "lucide-react";
import { speechApi } from "../../lib/speech";
import { followJob } from "../../lib/followJob";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import { LANG_CHOICES } from "./langs";

const MAX_SPEAKERS = 6;

/** The director model writes a podcast on a topic for named speakers with their manner; the script opens as a project. */
export default function PodcastWriter({ onClose, onCreated }: { onClose: () => void; onCreated: (pid: string) => void }) {
  const { t } = useTranslation();
  const [topic, setTopic] = useState("");
  const [speakers, setSpeakers] = useState([{ name: t("podcast.host"), manner: t("podcast.hostManner") }, { name: t("podcast.guest"), manner: t("podcast.guestManner") }]);
  const [minutes, setMinutes] = useState(3);
  const [lang, setLang] = useState("auto");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEscapeLayer(() => { if (!busy) onClose(); });
  const ready = topic.trim() && speakers.length >= 2 && speakers.every((s) => s.name.trim());
  const write = async () => {
    setBusy(true);
    setError(null);
    try {
      const { job_id } = await speechApi.podcast({ topic: topic.trim(), speakers, minutes, lang });
      const r = (await followJob(job_id, "write")) as { project_id: string };
      onCreated(r.project_id);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const box = "bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1.5 text-[13px] focus:border-[var(--color-accent)] focus:outline-none";
  const field = `w-full ${box}`;
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade" onClick={() => { if (!busy) onClose(); }}>
      <div role="dialog" aria-modal="true" aria-labelledby="pw-title" className="w-[min(94vw,600px)] max-h-[90vh] overflow-y-auto overflow-x-hidden rounded-xl glass-panel anim-pop p-5 space-y-3" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between">
          <h2 id="pw-title" className="font-semibold flex items-center gap-2"><Mic2 size={16} className="text-[var(--color-accent)]" />{t("podcast.title")}</h2>
          <button onClick={onClose} disabled={busy} className="text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("a11y.close"))}><X size={16} /></button>
        </div>
        <p className="text-[12px] text-[var(--color-muted)] leading-relaxed">{t("podcast.hint")}</p>
        <textarea value={topic} onChange={(e) => setTopic(e.target.value)} rows={4} placeholder={t("podcast.topicPlaceholder")} aria-label={t("podcast.topic")} className={`${field} resize-y leading-relaxed`} />
        <fieldset className="space-y-2">
          <legend className="text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)] mb-1">{t("podcast.speakers")}</legend>
          {speakers.map((s, i) => (
            <div key={i} className="flex items-center gap-2">
              <input value={s.name} onChange={(e) => setSpeakers((x) => x.map((y, j) => (j === i ? { ...y, name: e.target.value } : y)))} aria-label={t("podcast.speakerName")} maxLength={30} className={`${box} w-36 shrink-0`} />
              <input value={s.manner} onChange={(e) => setSpeakers((x) => x.map((y, j) => (j === i ? { ...y, manner: e.target.value } : y)))} placeholder={t("podcast.mannerPlaceholder")} aria-label={t("podcast.manner")} className={`${box} flex-1 min-w-0`} />
              <button onClick={() => setSpeakers((x) => x.filter((_, j) => j !== i))} disabled={speakers.length <= 2} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-warn)] disabled:opacity-30" {...named(t("podcast.removeSpeaker"))}><Trash2 size={13} /></button>
            </div>
          ))}
          <button onClick={() => setSpeakers((x) => [...x, { name: t("podcast.speakerN", { n: x.length + 1 }), manner: "" }])} disabled={speakers.length >= MAX_SPEAKERS} className="inline-flex items-center gap-1 px-2 py-1 rounded-md border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)] disabled:opacity-30"><Plus size={12} />{t("podcast.addSpeaker")}</button>
        </fieldset>
        <div className="flex flex-wrap items-end gap-3">
          <label className="text-[12px] text-[var(--color-muted)] space-y-1">{t("podcast.minutes")}
            <input type="number" min={1} max={20} value={minutes} onChange={(e) => setMinutes(Math.min(20, Math.max(1, Math.round(Number(e.target.value) || 1))))} className={`${field} w-20 block`} />
          </label>
          <label className="text-[12px] text-[var(--color-muted)] space-y-1">{t("speech.lang")}
            <select value={lang} onChange={(e) => setLang(e.target.value)} className={`${field} block`}>
              {LANG_CHOICES.map((l) => <option key={l} value={l}>{l === "auto" ? t("speech.langAuto") : l.toUpperCase()}</option>)}
            </select>
          </label>
          <button onClick={() => void write()} disabled={busy || !ready} className="ml-auto inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">
            {busy && <Loader2 size={14} className="animate-spin" />}{t("podcast.write")}
          </button>
        </div>
        <p className="text-[11px] text-[var(--color-muted)]">{t("podcast.modelNote")}</p>
        {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
      </div>
    </div>
  );
}
