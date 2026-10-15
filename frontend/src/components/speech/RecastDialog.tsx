import { useTranslation } from "react-i18next";
import { Users, X } from "lucide-react";
import type { Block } from "../../lib/speech";
import { useEngines, voiceName } from "../../lib/engines";
import { useEscapeLayer } from "../../lib/escape";
import { named } from "../../lib/a11y";

/** A character's voice changed across the project (Pandrator Recast): the blocks it touches, before anything changes;
 * the old takes stay in each block's history and undo takes the change back. */
export default function RecastDialog({ speaker, from, to, blocks, engine, onApply, onClose }: {
  speaker: string; from: string | null; to: string | null; blocks: Block[]; engine: string;
  onApply: (revoice: boolean) => void; onClose: () => void;
}) {
  const { t } = useTranslation();
  useEscapeLayer(onClose);
  const voiced = blocks.filter((b) => b.takes.length > 0).length;
  const engines = useEngines();
  const name = (v: string | null) => (v ? voiceName(v, engines, engine) : t("speech.voiceAsProject"));
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade" onClick={onClose}>
      <div role="dialog" aria-modal="true" aria-labelledby="recast-title" className="w-[min(94vw,520px)] max-h-[86vh] flex flex-col rounded-xl glass-panel anim-pop p-5" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center gap-2">
          <Users size={15} className="text-[var(--color-accent)]" />
          <h2 id="recast-title" className="font-semibold">{t("recast.title", { speaker })}</h2>
          <button onClick={onClose} {...named(t("a11y.close"))} className="ml-auto text-[var(--color-muted)] hover:text-[var(--color-text)]"><X size={16} /></button>
        </div>
        <p className="mt-2 text-[13px]">{name(from)} → <span className="font-semibold">{name(to)}</span></p>
        <p className="mt-1 text-[12px] text-[var(--color-muted)]">{t("recast.touches", { n: blocks.length, voiced })}</p>
        <ol className="mt-2 flex-1 min-h-0 overflow-y-auto space-y-1 text-[12px]">
          {blocks.map((b) => (
            <li key={b.id} className="flex gap-2 rounded-md bg-[var(--color-surface-2)] px-2 py-1">
              <span className={`shrink-0 w-1.5 h-1.5 mt-1.5 rounded-full ${b.takes.length ? "bg-[var(--color-accent)]" : "bg-[var(--color-muted)]"}`} />
              <span className="truncate">{b.text}</span>
            </li>
          ))}
        </ol>
        <p className="mt-2 text-[11px] text-[var(--color-muted)] leading-relaxed">{t("recast.keep")}</p>
        <div className="mt-4 flex flex-wrap justify-end gap-2">
          <button onClick={onClose} className="px-3.5 py-1.5 rounded-lg border border-[var(--color-border)] text-[13px]">{t("common.cancel")}</button>
          <button onClick={() => onApply(false)} className="px-3.5 py-1.5 rounded-lg border border-[var(--color-border)] text-[13px] hover:border-[var(--color-accent)]">{t("recast.apply")}</button>
          {voiced > 0 && <button onClick={() => onApply(true)} className="px-3.5 py-1.5 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold">{t("recast.applyRevoice", { n: blocks.length })}</button>}
        </div>
      </div>
    </div>
  );
}
