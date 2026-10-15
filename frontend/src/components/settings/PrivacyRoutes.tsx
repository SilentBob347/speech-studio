import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Cloud, HardDrive, Server } from "lucide-react";
import { api, type Selection } from "../../lib/api";
import { CLOUD_STAGES, consentKey, type CloudStage } from "../../lib/cloudConsent";
import { errorText } from "../../lib/voices";

/** Where each stage's text and audio go: this computer, the person's own server, or a cloud provider; and the
 * stages they allowed to go to the cloud, with a way to be asked again. */
export default function PrivacyRoutes() {
  const { t } = useTranslation();
  const [sel, setSel] = useState<Selection | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => { api.capabilities().then((c) => setSel(c.selection ?? {}), (e: unknown) => setProblem(errorText(e))); }, []);
  const askAgain = (stage: CloudStage) => {
    api.setSelection(consentKey(stage), "0").then(setSel, (e: unknown) => setProblem(errorText(e)));
  };
  const director = sel?.llm_provider === "openrouter" ? "cloud" : sel?.llm_provider === "server" ? "server" : sel?.llm_provider === "agent" ? "agent" : "local";
  const icon = (where: "local" | "server" | "cloud") => where === "cloud" ? <Cloud size={13} className="text-[var(--color-warn)]" /> : where === "server" ? <Server size={13} className="text-[var(--color-accent-2)]" /> : <HardDrive size={13} className="text-[var(--color-accent)]" />;
  type RouteKey = "route_speech" | "route_listening" | "route_local" | `route_director_${typeof director}` | `route_assistant_${"agent" | "openrouter"}`;
  const routeKey = (stage: CloudStage | "local"): RouteKey =>
    stage === "director" ? `route_director_${director}` : stage === "assistant" ? `route_assistant_${sel?.assistant === "agent" ? "agent" : "openrouter"}` : `route_${stage}`;
  const rows: { stage: CloudStage | "local"; where: "local" | "server" | "cloud" | "choice" }[] = [
    { stage: "speech", where: "choice" },
    { stage: "listening", where: "choice" },
    { stage: "director", where: director === "agent" ? "local" : director },
    { stage: "assistant", where: sel?.assistant === "agent" ? "local" : "cloud" },
    { stage: "local", where: "local" },
  ];
  return (
    <section className="space-y-2">
      <h4 className="text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)]">{t("privacy.title")}</h4>
      <p className="text-[12px] text-[var(--color-muted)] leading-relaxed">{t("privacy.hint")}</p>
      <div className="rounded-lg border border-[var(--color-border)] divide-y divide-[var(--color-border)] bg-[var(--color-surface-2)]/40">
        {rows.map(({ stage, where }) => {
          const allowed = stage !== "local" && sel?.[consentKey(stage)] === "1";
          return (
            <div key={stage} className="flex items-start gap-3 px-3 py-2">
              <span className="mt-0.5 shrink-0 inline-flex gap-1">{where === "choice" ? <>{icon("local")}{icon("cloud")}</> : icon(where)}</span>
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium">{t(`privacy.stage_${stage}`)}</div>
                <div className="text-[11px] leading-snug text-[var(--color-muted)]">{t(`privacy.${routeKey(stage)}`)}</div>
              </div>
              {stage !== "local" && (CLOUD_STAGES as readonly string[]).includes(stage) && (where === "choice" || where === "cloud") && (
                allowed
                  ? <button onClick={() => askAgain(stage)} className="shrink-0 text-[11px] underline text-[var(--color-muted)] hover:text-[var(--color-text)]">{t("privacy.askAgain")}</button>
                  : <span className="shrink-0 text-[11px] text-[var(--color-muted)]">{t("privacy.willAsk")}</span>
              )}
            </div>
          );
        })}
      </div>
      {problem && <p role="alert" className="text-[11px] text-[var(--color-warn)] break-words">{problem}</p>}
    </section>
  );
}
