import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Mic, X } from "lucide-react";
import { getCoded, postCoded, sendCoded } from "../../lib/api";
import { named } from "../../lib/a11y";
import { errorText } from "../../lib/voices";
import { Tip } from "../Hint";

type MicState = { on: false } | { on: true; what: "live" | "listening" | "voice" | "test"; seconds: number; muted: boolean; asr?: string | null };

const OFF: Record<"live" | "listening" | "voice" | "test", () => Promise<unknown>> = {
  live: () => sendCoded("DELETE", "/speech/listen"),
  listening: () => postCoded("/listen/record/stop", {}),
  voice: () => postCoded("/record/stop", { transcribe: true }),
  test: () => postCoded("/record/test/stop", {}),
};

const clock = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

/** While the microphone records - Live, a Listening recording, a voice, a microphone test - the top bar says so, for
 * how long, and turns it off from any screen. */
export default function MicIndicator() {
  const { t } = useTranslation();
  const [mic, setMic] = useState<MicState>({ on: false });
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const read = () => getCoded<MicState>("/mic/state").then((m) => { if (alive) setMic(m); }, (e: unknown) => { if (alive) setProblem(errorText(e)); });
    void read();
    const id = window.setInterval(() => void read(), 1000);
    return () => { alive = false; window.clearInterval(id); };
  }, []);

  if (!mic.on) return null;
  const text = mic.muted ? t("mic.paused") : t("mic.on");
  return (
    <span role="status" className={`inline-flex shrink-0 items-center gap-1.5 rounded-full border pl-2 pr-1 py-0.5 text-[12px] font-medium ${mic.muted ? "border-[var(--color-warn)] text-[var(--color-warn)]" : "border-[var(--color-bad)] text-[var(--color-bad)]"}`}>
      <Mic size={12} className={mic.muted ? "" : "animate-pulse"} aria-hidden />
      <span>{text}</span>
      <span className="text-[var(--color-muted)]" title={mic.asr ?? undefined}>{t(`mic.what_${mic.what}`)}{mic.asr ? ` · ${mic.asr}` : ""} {clock(mic.seconds)}</span>
      <Tip text={problem ?? t("mic.offHint")}>
        <button onClick={() => { setProblem(null); OFF[mic.what]().catch((e: unknown) => setProblem(errorText(e))); }} className="grid h-5 w-5 place-items-center rounded-full hover:bg-[var(--color-surface-2)]" {...named(t("mic.off"))}>
          <X size={12} />
        </button>
      </Tip>
    </span>
  );
}
