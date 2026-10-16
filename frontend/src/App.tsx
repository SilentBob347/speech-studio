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
import CloudConsentHost from "./components/CloudConsentHost";
import TopBar from "./components/shell/TopBar";
import Hotkeys from "./components/shell/Hotkeys";
import FirstRun from "./components/setup/FirstRun";
import ListeningPage from "./components/listening/ListeningPage";
import SpeechPage from "./components/speech/SpeechPage";
import TalkPage from "./components/talk/TalkPage";
import { useAloudPlayback } from "./lib/livePlayback";

const modelName = (p?: string) => (p ? (p.split(/[\\/]/).pop() || p).replace(/\.(gguf|onnx|bin|pt|safetensors)$/i, "") : "");

export default function App() {
  const { t, i18n } = useTranslation();
  const stage = useStore((s) => s.stage);
  const mode = useStore((s) => s.mode);
  const setStage = useStore((s) => s.setStage);
  // Анонимная статистика включена по умолчанию: на первом запуске решает галочка «Первого запуска», а установка,
  // обновившаяся мимо него, остаётся при умолчании, пока галочку не снимут в Настройках.
  const hub = useHubState(i18n.language);
  useAloudPlayback();
  useEffect(() => {
    const tel = hub?.telemetry;
    if (stage === "setup" || !tel || tel.acknowledged || tel.disabledByEnv) return;
    setTelemetry(tel.enabled, true).catch((e: Error) => console.warn("[hub] the statistics default was not saved:", e.message)).finally(hubChanged);
  }, [stage, hub?.telemetry.acknowledged]);   // eslint-disable-line react-hooks/exhaustive-deps
  const [cap, setCap] = useState<{ device: string; tts: string; rest: string[] } | null>(null);
  const speechEngine = useStore((s) => s.speechEngine);
  const [capOffline, setCapOffline] = useState(false);
  useEffect(() => {
    if (stage !== "ready") return;
    api.capabilities().then((c) => {
      const sel = (c as { selection?: Record<string, string> }).selection ?? {};
      const asr = sel.asr_engine === "whisper" ? `whisper ${sel.whisper_model || "auto"}` : sel.asr_engine === "nemotron" ? "nemotron-3.5 int8" : `${c.asr_model}${sel.asr ? ` ${sel.asr}` : ""}`;
      const rest = [`ASR ${asr}`];
      if (c.models?.llm) rest.push(`LLM ${modelName(c.models.llm)}`);
      setCap({ device: c.device, tts: c.models?.tts ? `${c.models.tts}${c.tts_quant ? ` ${c.tts_quant}` : ""}` : "", rest });
      setCapOffline(false);
    }).catch(() => setCapOffline(true));
  }, [stage]);
  const tts = mode === "speech" && speechEngine ? speechEngine : cap?.tts;
  const capText = cap ? [cap.device, ...(tts ? [`TTS ${tts}`] : []), ...cap.rest].join(" · ") : "";
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
      {stage === "ready" && <div className="flex-1 min-h-0 flex flex-col lg:[[data-assistant]_&]:pr-[420px]">{mode === "speech" ? <SpeechPage /> : mode === "talk" ? <TalkPage /> : <ListeningPage />}</div>}
      <footer className="mono h-6 px-4 flex items-center gap-2 text-[10px] text-[var(--color-muted)] border-t border-[var(--color-border)] bg-[var(--color-surface)] overflow-hidden">
        <span className="w-1.5 h-1.5 rounded-full bg-[var(--color-accent)] shrink-0" /><span className="truncate" title={capOffline ? t("status.backendOffline") : capText}>{capOffline ? t("status.backendOffline") : capText}</span>
      </footer>
      <BridgeHost />
      <CloudConsentHost />
      <Hotkeys />
      {stage === "ready" && <ResourceMonitor />}
    </div>
  );
}
