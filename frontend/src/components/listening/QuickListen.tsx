import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, FileAudio, Loader2, Sparkles, X } from "lucide-react";
import { listenApi, MEDIA, prepare, type Start } from "../../lib/listen";
import { followJob } from "../../lib/followJob";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import FileActions from "../speech/FileActions";

/** A file recognised at once, without opening the editor: who says what, as text to copy or save; the recording stays
 * a Listening recording to open later, as the agent's transcribe_file keeps it. */
export default function QuickListen({ start, onOpen, onClose }: { start: Start; onOpen: (pid: string) => void; onClose: () => void }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [made, setMade] = useState<{ pid: string; file: string; text: string } | null>(null);
  const [copied, setCopied] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEscapeLayer(() => { if (!busy) onClose(); });
  const run = async (f: File) => {
    setBusy(true);
    setError(null);
    setMade(null);
    setCopied(false);
    try {
      const r = await listenApi.upload(f);
      await prepare(r.pid, r.project, start);
      await followJob((await listenApi.transcribe(r.pid)).job_id, "analyze");
      const out = await listenApi.export(r.pid, "txt", true, true);
      setMade({ pid: r.pid, file: out.file, text: out.text });
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const copy = async () => {
    if (!made) return;
    try { await navigator.clipboard.writeText(made.text); setCopied(true); } catch (e) { setError(errorText(e)); }
  };
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade" onClick={() => { if (!busy) onClose(); }}>
      <div role="dialog" aria-modal="true" aria-label={t("listen.quickTitle")} onClick={(e) => e.stopPropagation()} className="w-[min(94vw,620px)] max-h-[90vh] flex flex-col rounded-xl glass-panel anim-pop p-5 gap-3">
        <div className="flex items-center justify-between">
          <h2 className="font-semibold flex items-center gap-2"><Sparkles size={16} className="text-[var(--color-accent)]" />{t("listen.quickTitle")}</h2>
          <button onClick={onClose} disabled={busy} className="text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("a11y.close"))}><X size={16} /></button>
        </div>
        <p className="text-[12px] text-[var(--color-muted)]">{t("listen.quickHint")}</p>
        <div className="flex items-center gap-2">
          <button onClick={() => input.current?.click()} disabled={busy} className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">
            {busy ? <Loader2 size={14} className="animate-spin" /> : <FileAudio size={14} />}{t("listen.quickPick")}
          </button>
          <input ref={input} type="file" accept={MEDIA} hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) void run(f); }} />
          {made && <button onClick={() => onOpen(made.pid)} className="text-[12px] text-[var(--color-muted)] hover:text-[var(--color-text)] underline">{t("listen.quickOpen")}</button>}
        </div>
        {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
        {made && (
          <>
            <pre className="flex-1 min-h-24 max-h-80 overflow-auto rounded-md bg-[var(--color-surface-2)] border border-[var(--color-border)] p-2 text-[12px] whitespace-pre-wrap">{made.text}</pre>
            <div className="flex flex-wrap items-center gap-2">
              <FileActions pid={made.pid} file={made.file} primary />
              <button onClick={() => void copy()} className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)]"><Copy size={13} />{copied ? t("listen.copied") : t("listen.copy")}</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
