import { useTranslation } from "react-i18next";
import type { WordOp } from "../../lib/speech";

/** The words recognition heard otherwise than the text says, as the check compared them: a missed word struck out, an
 * extra one with «+», a wrong one as «said → heard»; nothing when they agree. */
export default function HeardDiff({ off, className = "" }: { off: WordOp[] | undefined; className?: string }) {
  const { t } = useTranslation();
  const steps = (off ?? []).filter((s) => s.op !== "same");
  if (!steps.length) return null;
  return (
    <p className={`flex flex-wrap items-center gap-1 text-[11px] ${className}`} aria-label={t("speech.heardDiff")}>
      <span className="text-[var(--color-muted)]">{t("speech.heardDiff")}</span>
      {steps.map((s, k) => (
        <span key={k} className="px-1 rounded border border-[var(--color-warn)]/50 text-[var(--color-warn)] whitespace-nowrap">
          {s.op === "deleted" ? <s>{s.expected}</s> : s.op === "inserted" ? `+${s.heard}` : `${s.expected} → ${s.heard}`}
        </span>
      ))}
    </p>
  );
}
