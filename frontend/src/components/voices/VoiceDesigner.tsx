import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Pause, Play, Save, Wand2, X } from "lucide-react";
import { api } from "../../lib/api";
import { followJob } from "../../lib/followJob";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import { LANG_CHOICES } from "../speech/langs";

type Candidate = { k: number; seconds: number; seed: number };

/** A voice from a description (Qwen3-TTS VoiceDesign): describe it, hear a few candidates of a sample text, keep the one
 * that fits as a library voice, the sample text becoming its reference transcript. */
export default function VoiceDesigner({ onClose, onKept }: { onClose: () => void; onKept: (name: string) => void }) {
  const { t } = useTranslation();
  const [description, setDescription] = useState("");
  const [sample, setSample] = useState("");
  const [lang, setLang] = useState("auto");
  const [count, setCount] = useState(3);
  const [batch, setBatch] = useState<{ id: string; candidates: Candidate[]; description: string; sample: string; lang: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [playing, setPlaying] = useState<number | null>(null);
  const [name, setName] = useState("");
  const player = useRef<HTMLAudioElement | null>(null);
  useEscapeLayer(() => { if (!busy) onClose(); });
  const make = async () => {
    setBusy(true);
    setError(null);
    player.current?.pause();
    try {
      const { job_id } = await api.voiceDesign(description, sample, lang, count);
      const r = (await followJob(job_id, "voice_design")) as { id: string; candidates: Candidate[] };
      setBatch({ ...r, description, sample, lang });
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const play = (k: number) => {
    player.current?.pause();
    if (playing === k || !batch) { setPlaying(null); return; }
    const a = new Audio(api.voiceDesignSampleUrl(batch.id, k));
    player.current = a;
    a.onended = () => setPlaying(null);
    setPlaying(k);
    a.play().catch((e: unknown) => { setPlaying(null); setError(errorText(e)); });
  };
  const keep = async (k: number) => {
    if (!batch) return;
    setBusy(true);
    setError(null);
    try {
      const r = await api.voiceDesignKeep({ id: batch.id, k, name: name.trim(), description: batch.description, sample: batch.sample, lang: batch.lang });
      onKept(r.voice.name);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const field = "w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1.5 text-[13px] focus:border-[var(--color-accent)] focus:outline-none";
  return (
    <div className="fixed inset-0 z-[70] grid place-items-center glass-scrim anim-fade" onClick={() => { if (!busy) onClose(); }}>
      <div role="dialog" aria-modal="true" aria-labelledby="vd-title" className="w-[min(94vw,560px)] max-h-[90vh] overflow-y-auto rounded-xl glass-panel anim-pop p-5 space-y-3" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between">
          <h2 id="vd-title" className="font-semibold flex items-center gap-2"><Wand2 size={16} className="text-[var(--color-accent)]" />{t("voices.designTitle")}</h2>
          <button onClick={onClose} disabled={busy} className="text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("a11y.close"))}><X size={16} /></button>
        </div>
        <p className="text-[12px] text-[var(--color-muted)] leading-relaxed">{t("voices.designHint")}</p>
        <label className="block space-y-1 text-[12px] text-[var(--color-muted)]">{t("voices.designDescription")}
          <textarea value={description} onChange={(e) => setDescription(e.target.value)} rows={3} placeholder={t("voices.designDescriptionPlaceholder")} className={`${field} resize-y`} />
        </label>
        <label className="block space-y-1 text-[12px] text-[var(--color-muted)]">{t("voices.designSample")}
          <textarea value={sample} onChange={(e) => setSample(e.target.value)} rows={2} placeholder={t("voices.designSamplePlaceholder")} className={`${field} resize-y`} />
        </label>
        <div className="flex flex-wrap items-end gap-3">
          <label className="space-y-1 text-[12px] text-[var(--color-muted)]">{t("speech.lang")}
            <select value={lang} onChange={(e) => setLang(e.target.value)} className={`${field} w-28`}>
              {LANG_CHOICES.map((l) => <option key={l} value={l}>{l === "auto" ? t("speech.langAuto") : l.toUpperCase()}</option>)}
            </select>
          </label>
          <label className="space-y-1 text-[12px] text-[var(--color-muted)]">{t("voices.designCount")}
            <select value={count} onChange={(e) => setCount(Number(e.target.value))} className={`${field} w-20`}>
              {[1, 2, 3, 4, 5, 6].map((n) => <option key={n} value={n}>{n}</option>)}
            </select>
          </label>
          <button onClick={() => void make()} disabled={busy || !description.trim() || !sample.trim()} className="ml-auto inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">
            {busy && !batch ? <Loader2 size={14} className="animate-spin" /> : <Wand2 size={14} />}{batch ? t("voices.designAgain") : t("voices.designMake")}
          </button>
        </div>
        {batch && (
          <div className="space-y-2 pt-2 border-t border-[var(--color-border)]">
            <label className="block space-y-1 text-[12px] text-[var(--color-muted)]">{t("voices.designName")}
              <input value={name} onChange={(e) => setName(e.target.value)} placeholder={t("voices.designNamePlaceholder")} className={field} />
            </label>
            <ul className="space-y-1.5">
              {batch.candidates.map((c) => (
                <li key={c.k} className="flex items-center gap-2 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5">
                  <button onClick={() => play(c.k)} className="p-1 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(playing === c.k ? t("speech.stopPlaying") : t("speech.play"))}>
                    {playing === c.k ? <Pause size={14} /> : <Play size={14} />}
                  </button>
                  <span className="text-[13px] flex-1">{t("voices.designCandidate", { n: c.k + 1 })}</span>
                  <span className="text-[11px] tabnum text-[var(--color-muted)]">{c.seconds.toFixed(1)} {t("settings.sec")}</span>
                  <button onClick={() => void keep(c.k)} disabled={busy} className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)] disabled:opacity-40"><Save size={12} />{t("voices.designKeep")}</button>
                </li>
              ))}
            </ul>
          </div>
        )}
        {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
      </div>
    </div>
  );
}
