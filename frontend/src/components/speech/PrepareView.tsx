import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { speechApi, type Prepared, type PrepLayer } from "../../lib/speech";
import { errorText } from "../../lib/voices";

const LAYER_TINT: Record<PrepLayer, string> = {
  ssml: "text-[#8fc8ff] border-[#8fc8ff]/40",
  dictionary: "text-[#b8a4ff] border-[#b8a4ff]/40",
  normalizer: "text-[#7ee0c3] border-[#7ee0c3]/40",
  stress: "text-[#ffc46b] border-[#ffc46b]/40",
  engine: "text-[var(--color-muted)] border-[var(--color-border)]",
};

/** What synthesis will say for a block: the text the engine gets and every change of each layer, live as the text changes. */
export default function PrepareView({ pid, block, text }: { pid: string; block: string; text: string }) {
  const { t } = useTranslation();
  const [result, setResult] = useState<{ prepared: Prepared; stress_missing: boolean } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  useEffect(() => {
    let live = true;
    const timer = window.setTimeout(() => {
      setBusy(true);
      speechApi.prepare(text, pid, block).then(
        (r) => { if (live) { setResult(r); setError(null); } },
        (e: unknown) => { if (live) setError(errorText(e)); },
      ).finally(() => { if (live) setBusy(false); });
    }, 400);
    return () => { live = false; window.clearTimeout(timer); };
  }, [pid, block, text]);

  if (error) return <p role="alert" className="text-[11px] text-[var(--color-warn)]">{error}</p>;
  if (!result) return <Loader2 size={13} className="animate-spin text-[var(--color-muted)]" />;
  const p = result.prepared;
  const notes = [
    p.ignored.stress && t("prep.ignoredStress"),
    p.ignored.accent && t("prep.ignoredAccent"),
    p.ignored.phonemes && t("prep.ignoredPhonemes"),
    p.ignored.unknown_tags.length > 0 && t("prep.ignoredTags", { tags: p.ignored.unknown_tags.join(" ") }),
    p.ignored_phonemes.length > 0 && t("prep.ignoredDictPhonemes", { words: p.ignored_phonemes.join(", ") }),
    result.stress_missing && t("prep.stressMissing"),
  ].filter(Boolean) as string[];
  return (
    <div className={`space-y-1.5 text-[12px] ${busy ? "opacity-70" : ""}`}>
      <p className="mono text-[12px] leading-relaxed text-[var(--color-text)] break-words">{p.engine_text || "—"}</p>
      {p.steps.length > 0 && (
        <ul className="flex flex-wrap gap-1">
          {p.steps.map((s, i) => (
            <li key={i} title={`${t(`prep.layer_${s.layer}`)} · ${s.rule}`} className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded border text-[11px] ${LAYER_TINT[s.layer]}`}>
              <span className="text-[var(--color-muted)] line-through decoration-1">{s.from}</span>→<span>{s.to}</span>
            </li>
          ))}
        </ul>
      )}
      <div className="flex flex-wrap gap-x-3 text-[10px] text-[var(--color-muted)]">
        {(["dictionary", "normalizer", "stress"] as const).map((l) => <span key={l} className={LAYER_TINT[l].split(" ")[0]}>● {t(`prep.layer_${l}`)}</span>)}
      </div>
      {p.unknown.length > 0 && <p className="text-[11px] text-[var(--color-warn)]">{t("prep.unknown", { words: [...new Set(p.unknown)].join(", ") })}</p>}
      {p.broken.length > 0 && <p className="text-[11px] text-[var(--color-warn)]">{t("prep.broken", { n: p.broken.length })}</p>}
      {notes.map((n) => <p key={n} className="text-[11px] text-[var(--color-muted)]">{n}</p>)}
    </div>
  );
}
