import { useTranslation } from "react-i18next";
import { Check, Minus } from "lucide-react";
import { CLOUD_ENGINES, useEngines } from "../../lib/engines";

const yes = (on: boolean, label: string) => on
  ? <Check size={13} className="text-[var(--color-accent)]" aria-label={label} />
  : <Minus size={13} className="text-[var(--color-muted)]" aria-label={label} />;

/** What each voicing engine can do, read from the engines themselves, to choose one for a project. */
export default function EngineTable() {
  const { t } = useTranslation();
  const engines = useEngines();
  if (!engines) return <p className="text-[12px] text-[var(--color-muted)]">{t("engineTable.loading")}</p>;
  const head = "px-2 py-1.5 text-left align-bottom font-medium leading-tight text-[var(--color-muted)]";
  const cell = "px-2 py-1.5 align-top";
  return (
    <div className="overflow-x-auto rounded-lg border border-[var(--color-border)]">
      <table className="w-full text-[12px]">
        <thead className="bg-[var(--color-surface-2)]">
          <tr>
            <th className={head}>{t("engineTable.engine")}</th>
            <th className={head}>{t("engineTable.where")}</th>
            <th className={head}>{t("engineTable.langs")}</th>
            <th className={head}>{t("engineTable.clone")}</th>
            <th className={head}>{t("engineTable.voices")}</th>
            <th className={head}>{t("engineTable.tags")}</th>
            <th className={head}>{t("engineTable.stress")}</th>
            <th className={head}>{t("engineTable.direction")}</th>
          </tr>
        </thead>
        <tbody>
          {engines.map((e) => {
            const cloud = CLOUD_ENGINES.includes(e.id);
            const tags = e.tags.reduce((n, [, values]) => n + values.length, 0);
            return (
              <tr key={e.id} className="border-t border-[var(--color-border)]">
                <td className={cell}>
                  <span className="font-medium whitespace-nowrap">{e.name}</span>
                  {e.unavailable && <span className="block text-[11px] text-[var(--color-warn)]">{e.unavailable}</span>}
                </td>
                <td className={`${cell} whitespace-nowrap`}>{cloud ? t("engineTable.cloud") : t("engineTable.local")}</td>
                <td className={`${cell} tabnum whitespace-nowrap`}>{e.languages.length ? (e.languages.length > 3 ? String(e.languages.length) : e.languages.join(", ")) : cloud ? t("engineTable.modelLangs") : t("engineTable.anyLang")}</td>
                <td className={cell}>{yes(e.clone !== "none", t("engineTable.clone"))}</td>
                <td className={`${cell} tabnum`}>{e.presets.length || yes(false, t("engineTable.voices"))}</td>
                <td className={`${cell} tabnum`}>{tags || yes(false, t("engineTable.tags"))}</td>
                <td className={cell}>{yes(e.reads.stress, t("engineTable.stress"))}</td>
                <td className={cell}>{yes(e.direction, t("engineTable.direction"))}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
