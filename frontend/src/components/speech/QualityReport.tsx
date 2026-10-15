import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckCircle2, Loader2, ScanSearch, X } from "lucide-react";
import { speechApi, type Block, type QualityLight, type QualityReport as Report } from "../../lib/speech";
import { followJob } from "../../lib/followJob";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import HeardDiff from "./HeardDiff";

export const LIGHT_COLOR: Record<QualityLight, string> = { green: "var(--color-good)", amber: "var(--color-warn)", red: "var(--color-bad)", none: "var(--color-muted)" };

/** The project's light in the editor's bar: the share of checked takes that passed; a click opens the report. */
export function QualityButton({ pid, version, onOpen }: { pid: string; version: unknown; onOpen: () => void }) {
  const { t } = useTranslation();
  const [r, setR] = useState<Report | null>(null);
  useEffect(() => {
    const id = window.setTimeout(() => { speechApi.quality(pid).then(setR, () => setR(null)); }, 700);
    return () => window.clearTimeout(id);
  }, [pid, version]);
  if (!r || r.ready === 0) return null;
  const label = r.checked ? t("quality.passedOf", { passed: r.passed, checked: r.checked }) : t("quality.notChecked");
  return (
    <button onClick={onOpen} aria-label={label} className="hidden sm:inline-flex items-center gap-1.5 px-2 py-1 rounded-md text-[11px] text-[var(--color-muted)] hover:text-[var(--color-text)]" title={t("quality.lightHint", { green: Math.round(r.green * 100), amber: Math.round(r.amber * 100) })}>
      <span className="w-2 h-2 rounded-full shrink-0" style={{ background: LIGHT_COLOR[r.light] }} aria-hidden />{label}
    </button>
  );
}

/** How the voicing sounds by the check of its takes: the light, what is not checked yet (checked on a click), and the
 * failed takes, the worst first, with what the text says and what was heard; a click goes to the block. */
export default function QualityReport({ pid, blocks, onJump, onChanged, onClose }: { pid: string; blocks: Block[]; onJump: (id: string) => void; onChanged: () => void; onClose: () => void }) {
  const { t } = useTranslation();
  const [r, setR] = useState<Report | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  useEscapeLayer(() => { if (!checking) onClose(); });
  const load = () => speechApi.quality(pid).then(setR, (e: unknown) => setError(errorText(e)));
  useEffect(() => { void load(); }, [pid]);
  const check = async () => {
    setChecking(true);
    setError(null);
    try {
      await followJob((await speechApi.check(pid)).job_id, "synth");
      onChanged();
      await load();
    } catch (e) { setError(errorText(e)); } finally { setChecking(false); }
  };
  const number = (id: string) => blocks.findIndex((b) => b.id === id) + 1;
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade p-4" onClick={() => { if (!checking) onClose(); }}>
      <div role="dialog" aria-modal="true" aria-labelledby="quality-title" onClick={(e) => e.stopPropagation()}
        className="w-[min(94vw,720px)] max-h-[86vh] flex flex-col rounded-xl glass-panel anim-pop overflow-hidden">
        <div className="flex items-center gap-2 px-4 py-3 border-b border-[var(--color-border)]">
          <h2 id="quality-title" className="font-semibold">{t("quality.title")}</h2>
          <button onClick={onClose} disabled={checking} className="ml-auto p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("a11y.close"))}><X size={16} /></button>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto px-4 py-3 space-y-3">
          {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
          {!r && !error && <Loader2 size={16} className="animate-spin text-[var(--color-muted)]" />}
          {r && (
            <>
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[13px]">
                <span className="w-3 h-3 rounded-full shrink-0" style={{ background: LIGHT_COLOR[r.light] }} aria-hidden />
                <span className="font-medium">{r.ready === 0 ? t("quality.nothingVoiced") : r.checked ? t("quality.passedOf", { passed: r.passed, checked: r.checked }) : t("quality.notChecked")}</span>
                {r.ready > 0 && r.ready < r.total && <span className="text-[12px] text-[var(--color-muted)]">{t("quality.notVoiced", { n: r.total - r.ready })}</span>}
              </div>
              <p className="text-[11px] text-[var(--color-muted)]">{t("quality.lightHint", { green: Math.round(r.green * 100), amber: Math.round(r.amber * 100) })}</p>
              {r.unchecked.length > 0 && (
                <div className="flex flex-wrap items-center gap-2 rounded-lg border border-[var(--color-border)] px-3 py-2 text-[12px]">
                  <span className="flex-1 min-w-48">{t("quality.uncheckedNote", { n: r.unchecked.length })}</span>
                  <button onClick={() => void check()} disabled={checking} className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] font-semibold disabled:opacity-40">
                    {checking ? <Loader2 size={13} className="animate-spin" /> : <ScanSearch size={13} />}{t("quality.check", { n: r.unchecked.length })}
                  </button>
                </div>
              )}
              {r.checked > 0 && r.worst.length === 0 && <p className="flex items-center gap-2 text-[13px]"><CheckCircle2 size={15} style={{ color: "var(--color-good)" }} />{t("quality.allPassed")}</p>}
              {r.worst.length > 0 && <h3 className="text-[12px] font-semibold">{t("quality.worst", { n: r.worst.length })}{r.worst.length > 1 && <span className="font-normal text-[var(--color-muted)]"> · {t("quality.worstFirst")}</span>}</h3>}
              <ul className="space-y-2">
                {r.worst.map((w) => (
                  <li key={w.block}>
                    <button onClick={() => { onJump(w.block); onClose(); }} data-mcp-row={`block ${number(w.block)} ${w.speaker}`}
                      className="w-full text-left rounded-lg border border-[var(--color-border)] hover:border-[var(--color-accent)] px-3 py-2 space-y-1">
                      <span className="block text-[11px] text-[var(--color-muted)] truncate">{[t("lint.block", { n: number(w.block), speaker: w.speaker }), w.chapter, t("quality.take", { n: w.take })].filter(Boolean).join(" · ")}</span>
                      <span className="block text-[12px] line-clamp-2">{w.text}</span>
                      {w.text_checked && <HeardDiff off={w.off} />}
                      <span className="flex flex-wrap gap-1 text-[11px]">
                        {w.text_checked && !w.passed && <span className="px-1.5 rounded bg-[var(--color-surface-2)] text-[var(--color-warn)]">{t("quality.wer", { wer: Math.round(w.wer * 100) })}</span>}
                        {w.similar === false && <span className="px-1.5 rounded bg-[var(--color-surface-2)] text-[var(--color-warn)]">{t("quality.otherVoice")}</span>}
                        {w.issues.map((i) => <span key={i} className="px-1.5 rounded bg-[var(--color-surface-2)] text-[var(--color-warn)]">{t(`speech.qcIssue_${i}`)}</span>)}
                        {!w.text_checked && <span className="px-1.5 rounded bg-[var(--color-surface-2)] text-[var(--color-muted)]">{t("quality.wordsUnchecked")}</span>}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
