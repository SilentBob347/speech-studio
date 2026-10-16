import { useTranslation } from "react-i18next";
import { Mic, MicOff, Square, X } from "lucide-react";
import { named } from "../../lib/a11y";
import { Tip } from "../Hint";
import TalkVisual from "./TalkVisual";
import { useTalk, useTalkStyle } from "./useTalk";

/** The desktop shell opens the widget under this window label. */
const WINDOW = "widget";

type TauriInternals = { invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> };

function close() {
  const tauri = (window as unknown as { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__;
  if (tauri) void tauri.invoke("plugin:window|close", { label: WINDOW });
  else window.close();
}

/** The radio on its own: the picture, what is being said and the microphone, on no background, in a window the desktop
 * app keeps over every other one. The sound plays in the main window, which sends the widget its loudness. */
export default function TalkWidget() {
  const { t } = useTranslation();
  const [look] = useTalkStyle();
  const talk = useTalk({ fed: true });
  const { state, running, partial, saying, lastPerson, lastAgent, speaking, loudness } = talk;
  const line = saying ?? (state === "hearing" ? partial : lastAgent?.text ?? "");
  const shade = "[text-shadow:0_1px_3px_rgba(0,0,0,0.85),0_0_12px_rgba(0,0,0,0.6)]";

  return (
    <main data-tauri-drag-region className="group relative h-full w-full select-none overflow-hidden text-white" title={t("talk.widgetMove")}>
      <div data-tauri-drag-region className="absolute inset-x-0 top-0 flex flex-col items-center pt-3">
        <TalkVisual look={look} state={state} level={loudness} token={state === "thinking" ? "--color-accent-2" : "--color-accent"} className={look === "wave" ? "pointer-events-none h-56 w-full" : "pointer-events-none h-56 w-56"} />
      </div>
      <div data-tauri-drag-region className="absolute inset-x-3 top-[238px] space-y-1 text-center">
        <p role="status" aria-live="polite" className={`text-[12px] font-medium opacity-80 ${shade}`}>{t(`talk.state_${state}`)}</p>
        <p className={`line-clamp-3 text-[15px] leading-snug ${shade}`}>{line}</p>
        {state !== "hearing" && lastPerson && <p className={`line-clamp-1 text-[11px] opacity-70 ${shade}`}>{lastPerson.text}</p>}
      </div>
      <div className="absolute inset-x-0 bottom-3 flex items-center justify-center gap-2 opacity-0 transition-opacity duration-200 group-hover:opacity-100 focus-within:opacity-100">
        <Tip text={running ? t("talk.micOffHint") : t("talk.micOnHint")}>
          <button onClick={() => void talk.toggleMic("auto")} aria-pressed={running} className={`grid h-11 w-11 place-items-center rounded-full backdrop-blur-md ${running ? "bg-[var(--color-accent)] text-[var(--color-on-accent)]" : "bg-black/45 hover:bg-black/60"}`} {...named(running ? t("talk.micOff") : t("talk.micOn"))}>
            {running ? <Mic size={18} /> : <MicOff size={18} />}
          </button>
        </Tip>
        <Tip text={t("talk.silenceHint")}>
          <button onClick={talk.silence} disabled={!speaking} className="grid h-9 w-9 place-items-center rounded-full bg-black/45 backdrop-blur-md hover:bg-black/60 disabled:opacity-40" {...named(t("talk.silence"))}>
            <Square size={14} />
          </button>
        </Tip>
        <Tip text={t("talk.close")}>
          <button onClick={close} className="grid h-9 w-9 place-items-center rounded-full bg-black/45 backdrop-blur-md hover:bg-black/60" {...named(t("talk.close"))}>
            <X size={15} />
          </button>
        </Tip>
      </div>
    </main>
  );
}
