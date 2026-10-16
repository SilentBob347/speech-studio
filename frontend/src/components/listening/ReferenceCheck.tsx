import { useState } from "react";
import { useTranslation } from "react-i18next";
import { FileCheck2, Loader2, TextCursorInput, X } from "lucide-react";
import { listenApi, type ListenLine, type ReferenceCheck as Check } from "../../lib/listen";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import { Tip } from "../Hint";

/** The recording against the text it should say: missed, extra and replaced words, and the word error rate; or the text
 * itself laid over the recording, each word at the time it was heard (forced alignment). */
export default function ReferenceCheck({ pid, reference, onReference, onAlign, onClose }: { pid: string; reference: string; onReference: (text: string) => void; onAlign: (lines: ListenLine[]) => void; onClose: () => void }) {
  const { t } = useTranslation();
  useEscapeLayer(onClose);
  const [text, setText] = useState(reference);
  const [result, setResult] = useState<Check | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const compare = async () => {
    setBusy(true);
    setError(null);
    try {
      if (text !== reference) onReference(text);
      setResult(await listenApi.check(pid, text));
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const align = async () => {
    setBusy(true);
    setError(null);
    try {
      if (text !== reference) onReference(text);
      onAlign((await listenApi.aligned(pid, text)).lines);
      onClose();
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const counts = result ? { deleted: 0, inserted: 0, substituted: 0, ...Object.fromEntries(["deleted", "inserted", "substituted"].map((k) => [k, result.ops.filter((o) => o.op === k).length])) } : null;
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade" onClick={onClose}>
      <div role="dialog" aria-modal="true" aria-labelledby="rc-title" className="w-[min(94vw,760px)] max-h-[90vh] flex flex-col rounded-xl glass-panel anim-pop p-5 gap-3" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between">
          <h2 id="rc-title" className="font-semibold flex items-center gap-2"><FileCheck2 size={16} className="text-[var(--color-accent)]" />{t("listen.checkTitle")}</h2>
          <button onClick={onClose} className="text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("a11y.close"))}><X size={16} /></button>
        </div>
        <p className="text-[12px] text-[var(--color-muted)] leading-relaxed">{t("listen.checkHint")}</p>
        <textarea value={text} onChange={(e) => setText(e.target.value)} rows={6} placeholder={t("listen.referencePlaceholder")} aria-label={t("listen.reference")}
          className="w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1.5 text-[13px] leading-relaxed focus:border-[var(--color-accent)] focus:outline-none resize-y" />
        <div className="flex items-center gap-3">
          <button onClick={() => void compare()} disabled={busy || !text.trim()} className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">
            {busy && <Loader2 size={14} className="animate-spin" />}{t("listen.compare")}
          </button>
          <Tip text={t("listen.alignHint")}><button onClick={() => void align()} disabled={busy || !text.trim()} className="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg border border-[var(--color-border)] text-[13px] hover:border-[var(--color-accent)] disabled:opacity-40">
            <TextCursorInput size={14} />{t("listen.align")}
          </button></Tip>
          {result && counts && (
            <span className="text-[12px] tabnum">
              {t("listen.checkSummary", { wer: Math.round(result.wer * 100), words: result.words, missed: counts.deleted, extra: counts.inserted, replaced: counts.substituted })}
            </span>
          )}
        </div>
        {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
        {result && (
          <div className="flex-1 min-h-24 overflow-auto rounded-md bg-[var(--color-surface-2)] border border-[var(--color-border)] p-3 text-[14px] leading-loose">
            {result.ops.map((o, i) => {
              if (o.op === "same") return <span key={i}>{o.expected} </span>;
              if (o.op === "deleted") return <span key={i} className="px-0.5 rounded bg-[color-mix(in_oklab,var(--color-warn)_25%,transparent)] line-through" title={t("listen.opMissed")}>{o.expected}</span>;
              if (o.op === "inserted") return <span key={i} className="px-0.5 rounded underline decoration-wavy decoration-[var(--color-warn)] text-[var(--color-muted)]" title={t("listen.opExtra")}>{o.heard}</span>;
              return <span key={i} className="px-0.5 rounded bg-[color-mix(in_oklab,var(--color-accent)_20%,transparent)]" title={t("listen.opReplaced", { heard: o.heard })}>{o.expected}<span className="text-[var(--color-muted)]"> → {o.heard}</span></span>;
            }).reduce<React.ReactNode[]>((acc, el, i) => (i ? [...acc, " ", el] : [el]), [])}
          </div>
        )}
      </div>
    </div>
  );
}
