import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Sparkles, X } from "lucide-react";
import { speechApi } from "../../lib/speech";
import { followJob } from "../../lib/followJob";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import { useEngines } from "../../lib/engines";
import VoicePicker from "./VoicePicker";
import FileActions from "./FileActions";

/** A phrase voiced at once, without making a project of it: the same text, voice and engine again answer with the file
 * already made, as the agent's speak_text does. */
export default function QuickSpeak({ onOpen, onClose }: { onOpen: (pid: string) => void; onClose: () => void }) {
  const { t } = useTranslation();
  const engines = useEngines();
  const [text, setText] = useState("");
  const [voice, setVoice] = useState<string | null>(null);
  const [engine, setEngine] = useState("higgs");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [made, setMade] = useState<{ pid: string; file: string } | null>(null);
  const player = useRef<HTMLAudioElement>(null);
  useEscapeLayer(() => { if (!busy) onClose(); });
  const speak = async () => {
    setBusy(true);
    setError(null);
    setMade(null);
    try {
      const key = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(JSON.stringify({ text, voice, engine })));
      const hex = Array.from(new Uint8Array(key)).slice(0, 8).map((b) => b.toString(16).padStart(2, "0")).join("");
      const { project_id: pid, project } = await speechApi.create({ name: text.trim().slice(0, 40), text, voice, engine, key: `quick-${hex}` });
      const ready = project.chapters.every((c) => c.blocks.every((b) => project.status?.[b.id]?.state === "ready"));
      if (!ready) await followJob((await speechApi.synth(pid, {})).job_id, "synth");
      const r = (await followJob((await speechApi.export(pid, "wav")).job_id, "export")) as { file: string };
      setMade({ pid, file: r.file });
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const field = "w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1.5 text-[13px] focus:border-[var(--color-accent)] focus:outline-none";
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade" onClick={() => { if (!busy) onClose(); }}>
      <div role="dialog" aria-modal="true" aria-label={t("quick.title")} onClick={(e) => e.stopPropagation()} className="w-[min(94vw,560px)] rounded-xl glass-panel anim-pop p-5 space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="font-semibold flex items-center gap-2"><Sparkles size={16} className="text-[var(--color-accent)]" />{t("quick.title")}</h2>
          <button onClick={onClose} disabled={busy} className="text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("a11y.close"))}><X size={16} /></button>
        </div>
        <textarea value={text} onChange={(e) => setText(e.target.value)} rows={4} placeholder={t("quick.placeholder")} aria-label={t("quick.text")} className={`${field} resize-y`} />
        <div className="grid grid-cols-2 gap-2">
          <label className="text-[12px] text-[var(--color-muted)] space-y-1">{t("speech.voice")}
            <VoicePicker engine={engine} value={voice} onChange={setVoice} label={t("speech.voice")} className={field} />
          </label>
          <label className="text-[12px] text-[var(--color-muted)] space-y-1">{t("speech.engine")}
            <select value={engine} onChange={(e) => { setEngine(e.target.value); setVoice(null); }} className={field}>
              {(engines ?? []).filter((e) => !e.unavailable).map((e) => <option key={e.id} value={e.id}>{e.name}</option>)}
            </select>
          </label>
        </div>
        <div className="flex items-center gap-2">
          <button onClick={() => void speak()} disabled={busy || !text.trim()} className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">
            {busy ? <Loader2 size={14} className="animate-spin" /> : <Sparkles size={14} />}{t("quick.speak")}
          </button>
          {made && <button onClick={() => onOpen(made.pid)} className="text-[12px] text-[var(--color-muted)] hover:text-[var(--color-text)] underline">{t("quick.openProject")}</button>}
        </div>
        {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
        {made && (
          <div className="space-y-2">
            <audio ref={player} controls src={speechApi.exportUrl(made.pid, made.file)} className="w-full h-9" aria-label={t("quick.result")} />
            <FileActions pid={made.pid} file={made.file} />
          </div>
        )}
      </div>
    </div>
  );
}
