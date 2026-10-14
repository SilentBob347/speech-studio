import { useTranslation } from "react-i18next";
import { motion } from "motion/react";
import { AudioLines, Check, Ear } from "lucide-react";
import { useStore, type Mode } from "../store";

const EASE = [0.22, 1, 0.36, 1] as const;

// Стартовый экран режима: что он делает. Рабочие экраны Speech и Listening приходят своими этапами.
export default function ModePage({ mode }: { mode: Mode }) {
  const { t } = useTranslation();
  const fresh = useStore((s) => s.fresh);
  const Icon = mode === "speech" ? AudioLines : Ear;
  const points = t(`modes.${mode}Points`, { returnObjects: true }) as unknown as string[];
  return (
    <main key={`${mode}-${fresh}`} className="flex-1 min-h-0 overflow-y-auto grid place-items-center px-6 py-10">
      <motion.div initial={{ opacity: 0, y: 14 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.45, ease: EASE }} className="w-full max-w-xl">
        <div className="flex items-center gap-3">
          <span className="grid place-items-center w-11 h-11 rounded-xl bg-[color-mix(in_oklab,var(--color-accent)_14%,transparent)] text-[var(--color-accent)]"><Icon size={22} /></span>
          <h1 className="text-2xl lg:text-[28px] leading-tight font-extrabold tracking-tight">{t(`modes.${mode}`)}</h1>
        </div>
        <p className="mt-4 text-[14px] leading-relaxed text-[var(--color-muted)]">{t(`modes.${mode}Lead`)}</p>
        <ul className="mt-5 space-y-2">
          {points.map((p, i) => (
            <li key={i} className="flex gap-2.5 text-[13px] leading-relaxed"><Check size={15} className="text-[var(--color-accent)] shrink-0 mt-[3px]" /><span>{p}</span></li>
          ))}
        </ul>
        <p className="mt-6 text-[12px] text-[var(--color-muted)]">{t("modes.comingNext")}</p>
      </motion.div>
    </main>
  );
}
