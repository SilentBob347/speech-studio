import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { api } from "./lib/api";
import { useStore } from "./store";
import { hubChanged, setTelemetry, useHubState } from "./lib/studioHub";
import { HubBars } from "./components/HubBars";
import ServerOffline from "./components/ServerOffline";
import ResourceMonitor from "./components/ResourceMonitor";
import BridgeHost from "./components/BridgeHost";
import TopBar from "./components/shell/TopBar";
import Hotkeys from "./components/shell/Hotkeys";
import FirstRun from "./components/setup/FirstRun";
import ListeningPage from "./components/listening/ListeningPage";
import SpeechPage from "./components/speech/SpeechPage";

const modelName = (p?: string) => (p ? (p.split(/[\\/]/).pop() || p).replace(/\.(gguf|onnx|bin|pt|safetensors)$/i, "") : "");

export default function App() {
  const { t, i18n } = useTranslation();
  const stage = useStore((s) => s.stage);
  const mode = useStore((s) => s.mode);
  const setStage = useStore((s) => s.setStage);
  // Анонимная статистика включена по умолчанию: на первом запуске решает галочка «Первого запуска», а установка,
  // обновившаяся мимо него, остаётся при умолчании, пока галочку не снимут в Настройках.
  const hub = useHubState(i18n.language);
  useEffect(() => {
    const tel = hub?.telemetry;
    if (stage === "setup" || !tel || tel.acknowledged || tel.disabledByEnv) return;
    setTelemetry(tel.enabled, true).catch((e: Error) => console.warn("[hub] the statistics default was not saved:", e.message)).finally(hubChanged);
  }, [stage, hub?.telemetry.acknowledged]);   // eslint-disable-line react-hooks/exhaustive-deps
  const [cap, setCap] = useState("");
  const [capOffline, setCapOffline] = useState(false);
  useEffect(() => {
    if (stage !== "ready") return;
    api.capabilities().then((c) => {
      const sel = (c as { selection?: Record<string, string> }).selection ?? {};
      const asr = sel.asr_engine === "whisper" ? `whisper ${sel.whisper_model || "auto"}` : `${c.asr_model}${sel.asr ? ` ${sel.asr}` : ""}`;
      const parts = [c.device];
      if (c.models?.tts) parts.push(`TTS ${c.models.tts}${c.tts_quant ? ` ${c.tts_quant}` : ""}`);
      parts.push(`ASR ${asr}`);
      if (c.models?.llm) parts.push(`LLM ${modelName(c.models.llm)}`);
      setCap(parts.join(" · "));
      setCapOffline(false);
    }).catch(() => setCapOffline(true));
  }, [stage]);
  // Сетевая ошибка (fetch отклонён TypeError) = сервиса нет на порту -> экран «сервер не отвечает»; HTTP-ошибка
  // значит, что сервис ответил, и ведёт к режимам.
  useEffect(() => {
    api.setupStatus()
      .then((s) => setStage(s.ready ? "ready" : "setup"))
      .catch((e: unknown) => setStage(e instanceof TypeError ? "offline" : "ready"));
  }, [setStage]);
  return (
    <div className="h-full flex flex-col">
      {hub && stage !== "setup" && <HubBars items={hub.items} />}
      <TopBar />
      {stage === "boot" && <div className="flex-1 grid place-items-center"><Loader2 size={22} className="animate-spin text-[var(--color-muted)]" /></div>}
      {stage === "offline" && <ServerOffline />}
      {stage === "setup" && <FirstRun />}
      {stage === "ready" && (mode === "speech" ? <SpeechPage /> : <ListeningPage />)}
      <footer className="mono h-6 px-4 flex items-center gap-2 text-[10px] text-[var(--color-muted)] border-t border-[var(--color-border)] bg-[var(--color-surface)] overflow-hidden">
        <span className="w-1.5 h-1.5 rounded-full bg-[var(--color-accent)] shrink-0" /><span className="truncate" title={capOffline ? t("status.backendOffline") : cap}>{capOffline ? t("status.backendOffline") : cap}</span>
      </footer>
      <BridgeHost />
      <Hotkeys />
      {stage === "ready" && <ResourceMonitor />}
    </div>
  );
}
