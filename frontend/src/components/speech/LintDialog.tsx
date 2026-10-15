import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, CheckCircle2, Loader2, X, XCircle } from "lucide-react";
import { speechApi, type Block, type LintIssue } from "../../lib/speech";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";

type Detail = Record<string, unknown>;

/** The script checked before voicing: each block's problems, an error first, and a click goes to the block. */
export default function LintDialog({ pid, blocks, onJump, onClose }: { pid: string; blocks: Block[]; onJump: (id: string) => void; onClose: () => void }) {
  const { t } = useTranslation();
  const [issues, setIssues] = useState<LintIssue[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEscapeLayer(onClose);
  useEffect(() => { speechApi.lint(pid).then((r) => setIssues(r.issues), (e: unknown) => setError(errorText(e))); }, [pid]);
  const say = (i: LintIssue): string => {
    const d = (i.detail ?? {}) as Detail;
    const list = (v: unknown) => (Array.isArray(v) ? v.map((x) => (typeof x === "object" && x ? String((x as Detail).rule ?? "") : String(x))).join(", ") : String(v ?? ""));
    switch (i.kind) {
      case "engine_unavailable": return t("lint.engine_unavailable", { engine: d.engine, why: d.why });
      case "language": return t("lint.language", { engine: d.engine, lang: d.lang });
      case "no_voice": return t("lint.no_voice", { engine: d.engine });
      case "voice_missing": return t("lint.voice_missing", { voice: d.voice });
      case "ref_text": return t("lint.ref_text", { engine: d.engine, voice: d.voice });
      case "direction_ignored": return t("lint.direction_ignored", { engine: d.engine });
      case "tags_ignored": return t("lint.tags_ignored", { engine: d.engine, tags: list(d.tags) });
      case "marks_ignored": return t("lint.marks_ignored", { engine: d.engine });
      case "unread": return t("lint.unread", { words: list(i.detail) });
      case "dictionary": return t("lint.dictionary", { rules: list(i.detail) });
      case "prepare": return t("lint.prepare", { detail: String(i.detail) });
      case "engine": return t("lint.engine", { engine: String(i.detail) });
      default: return t("lint.empty");
    }
  };
  const errors = issues?.filter((i) => i.level === "error").length ?? 0;
  const warns = (issues?.length ?? 0) - errors;
  const order = blocks.filter((b) => issues?.some((i) => i.block === b.id));
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center bg-black/50 p-4" onClick={onClose}>
      <div role="dialog" aria-modal="true" aria-label={t("lint.title")} onClick={(e) => e.stopPropagation()}
        className="w-[min(94vw,720px)] max-h-[86vh] flex flex-col rounded-xl glass-panel anim-pop overflow-hidden">
        <div className="flex items-center gap-2 px-4 py-3 border-b border-[var(--color-border)]">
          <h2 className="font-semibold">{t("lint.title")}</h2>
          {issues && <span className="text-[12px] text-[var(--color-muted)] tabnum">{t("lint.count", { errors, warns })}</span>}
          <button onClick={onClose} className="ml-auto p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("a11y.close"))}><X size={16} /></button>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto px-4 py-3 space-y-2">
          {error && <p role="alert" className="text-[12px] text-[var(--color-warn)]">{error}</p>}
          {!issues && !error && <Loader2 size={16} className="animate-spin text-[var(--color-muted)]" />}
          {issues?.length === 0 && <p className="flex items-center gap-2 text-[13px]"><CheckCircle2 size={15} className="text-[var(--color-accent)]" />{t("lint.clean")}</p>}
          {order.map((b) => (
            <button key={b.id} onClick={() => { onJump(b.id); onClose(); }} data-mcp-row={`block ${blocks.indexOf(b) + 1} ${b.speaker}`}
              className="w-full text-left rounded-lg border border-[var(--color-border)] hover:border-[var(--color-accent)] px-3 py-2 space-y-1">
              <span className="block text-[11px] text-[var(--color-muted)] truncate">{t("lint.block", { n: blocks.indexOf(b) + 1, speaker: b.speaker })} · {b.text.slice(0, 80)}</span>
              {issues!.filter((i) => i.block === b.id).sort((x, y) => (x.level === y.level ? 0 : x.level === "error" ? -1 : 1)).map((i, k) => (
                <span key={k} className={`flex items-start gap-1.5 text-[12px] ${i.level === "error" ? "text-[var(--color-warn)]" : "text-[var(--color-text)]"}`}>
                  {i.level === "error" ? <XCircle size={13} className="mt-0.5 shrink-0" /> : <AlertTriangle size={13} className="mt-0.5 shrink-0 text-[var(--color-muted)]" />}{say(i)}
                </span>
              ))}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
