import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";

export type Step = { id: "text" | "voices" | "voice" | "export"; done: boolean; run: () => void };

/** The beginner's path through a project, from text to file: each step says whether it is done and the first one that
 * is not carries the action to do next. */
export default function StepBar({ steps }: { steps: Step[] }) {
  const { t } = useTranslation();
  const next = steps.find((s) => !s.done);
  return (
    <nav aria-label={t("simple.steps")} className="flex flex-wrap items-center gap-x-1 gap-y-2 px-3 md:px-5 py-2 border-b border-[var(--color-border)]">
      <ol className="flex flex-wrap items-center gap-1">
        {steps.map((s, k) => {
          const current = s === next;
          return (
            <li key={s.id} className="flex items-center gap-1">
              {k > 0 && <span aria-hidden className="w-4 h-px bg-[var(--color-border)]" />}
              <button onClick={s.run} aria-current={current ? "step" : undefined}
                className={`inline-flex items-center gap-1.5 pl-1 pr-2.5 py-1 rounded-full border text-[12px] ${current ? "border-[var(--color-accent)] text-[var(--color-text)]" : s.done ? "border-transparent text-[var(--color-muted)] hover:text-[var(--color-text)]" : "border-transparent text-[var(--color-muted)]/70 hover:text-[var(--color-text)]"}`}>
                <span className={`grid place-items-center w-5 h-5 rounded-full text-[11px] font-semibold tabnum ${s.done ? "bg-[var(--color-accent)]/20 text-[var(--color-accent)]" : current ? "bg-[var(--color-accent)] text-[var(--color-on-accent)]" : "bg-[var(--color-surface-2)]"}`}>
                  {s.done ? <Check size={12} /> : k + 1}
                </span>
                {t(`simple.step_${s.id}`)}
              </button>
            </li>
          );
        })}
      </ol>
      {next && <p className="basis-full md:basis-auto md:ml-3 text-[12px] text-[var(--color-muted)]">{t(`simple.next_${next.id}`)}</p>}
      {!next && <p className="basis-full md:basis-auto md:ml-3 text-[12px] text-[var(--color-muted)]">{t("simple.allDone")}</p>}
    </nav>
  );
}
