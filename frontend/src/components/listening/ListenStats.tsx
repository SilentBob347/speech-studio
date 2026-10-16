import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, X } from "lucide-react";
import { clock, listenApi, type SpeakerStats } from "../../lib/listen";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import { Tip } from "../Hint";

/** Who speaks how much in the recording, over the lines shown: talk time and share, lines, words, pace, longest stretch. */
/** `refresh` changes when the saved project changes; `undefined` while a save is on its way. */
export default function ListenStats({ pid, refresh, onClose }: { pid: string; refresh: unknown; onClose: () => void }) {
  const { t } = useTranslation();
  useEscapeLayer(onClose);
  const [stats, setStats] = useState<SpeakerStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (refresh === undefined) return;
    let alive = true;
    listenApi.stats(pid).then((s) => { if (alive) { setStats(s); setError(null); } }, (e: unknown) => { if (alive) setError(errorText(e)); });
    return () => { alive = false; };
  }, [pid, refresh]);
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade" onClick={onClose}>
      <div role="dialog" aria-modal="true" aria-labelledby="ls-title" className="w-[min(94vw,620px)] max-h-[90vh] flex flex-col rounded-xl glass-panel anim-pop p-5 gap-3" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between">
          <h2 id="ls-title" className="font-semibold">{t("listen.statsTitle")}</h2>
          <button onClick={onClose} className="text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("a11y.close"))}><X size={16} /></button>
        </div>
        {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
        {!stats && !error && <Loader2 size={14} className="animate-spin text-[var(--color-muted)]" />}
        {stats && (
          <>
            <p className="text-[12px] text-[var(--color-muted)] tabnum">{t("listen.statsTalk", { talk: clock(stats.talk_seconds), total: clock(stats.duration) })}</p>
            <div className="overflow-auto">
              <table className="w-full text-[12px] tabnum">
                <thead className="text-[var(--color-muted)] text-left">
                  <tr>
                    <th className="font-normal pb-1 pr-3">{t("listen.statsSpeaker")}</th>
                    <th className="font-normal pb-1 pr-3"><Tip text={t("listen.statsShareHint")} className="inline">{t("listen.statsShare")}</Tip></th>
                    <th className="font-normal pb-1 pr-3">{t("listen.statsTime")}</th>
                    <th className="font-normal pb-1 pr-3">{t("listen.statsLines")}</th>
                    <th className="font-normal pb-1 pr-3">{t("listen.statsWords")}</th>
                    <th className="font-normal pb-1 pr-3"><Tip text={t("listen.statsWpmHint")} className="inline">{t("listen.statsWpm")}</Tip></th>
                    <th className="font-normal pb-1"><Tip text={t("listen.statsLongestHint")} className="inline">{t("listen.statsLongest")}</Tip></th>
                  </tr>
                </thead>
                <tbody>
                  {stats.speakers.map((s) => (
                    <tr key={s.speaker} className="border-t border-[var(--color-border)]">
                      <td className="py-1.5 pr-3 font-semibold">{s.name}</td>
                      <td className="py-1.5 pr-3">
                        <span className="inline-flex items-center gap-2">
                          <span className="block h-1.5 w-16 rounded bg-[var(--color-surface-2)] overflow-hidden"><span className="block h-full bg-[var(--color-accent)]" style={{ width: `${s.share}%` }} /></span>
                          {s.share}%
                        </span>
                      </td>
                      <td className="py-1.5 pr-3">{clock(s.seconds)}</td>
                      <td className="py-1.5 pr-3">{s.lines}</td>
                      <td className="py-1.5 pr-3">{s.words}</td>
                      <td className="py-1.5 pr-3">{s.wpm}</td>
                      <td className="py-1.5">{clock(s.longest)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
