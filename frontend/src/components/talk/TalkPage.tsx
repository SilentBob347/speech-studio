import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AppWindow, Copy, History, Mic, MicOff, Square } from "lucide-react";
import { named } from "../../lib/a11y";
import { LANG_CHOICES } from "../speech/langs";
import Hint, { Tip } from "../Hint";
import TalkVisual from "./TalkVisual";
import { useTalk, useTalkStyle } from "./useTalk";
import { TALK_STYLES } from "./visual";

const CONNECT = "claude mcp add --transport http speech-studio http://127.0.0.1:8797/mcp";

/** The talk widget: a small window with no frame and no background, over every other window in the desktop app. */
export function openTalkWidget() {
  window.open(`${window.location.origin}/?widget`, "speech-talk-widget", "popup,width=340,height=460");
}

/** A spoken conversation with an agent through the studio: the person's words heard live, the agent's answer said aloud
 * as it writes it, the state of both, and the conversation as text. */
export default function TalkPage() {
  const { t } = useTranslation();
  const [lang, setLang] = useState("auto");
  const [history, setHistory] = useState(true);
  const [look, setLook] = useTalkStyle();
  const talk = useTalk({ fed: false });
  const { state, running: mic, heard, partial, saying, turns, lastPerson, lastAgent, agent, error, speaking, loudness } = talk;

  return (
    <main className="flex-1 min-h-0 flex" data-mcp-page={t("talk.title")}>
      <section className="flex-1 min-w-0 flex flex-col items-center justify-between px-4 py-6 md:py-10">
        <div className="w-full max-w-3xl flex items-center justify-between gap-3">
          <h1 className="text-[15px] font-semibold">{t("talk.title")} <Hint text={t("talk.hint")} /></h1>
          <div role="radiogroup" aria-label={t("talk.style")} className="flex items-center gap-0.5 rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] p-0.5 text-[12px]">
            {TALK_STYLES.map((s) => (
              <button key={s} role="radio" aria-checked={look === s} onClick={() => setLook(s)} className={`rounded-full px-3 py-1 transition-colors ${look === s ? "bg-[var(--color-accent)] text-[var(--color-on-accent)]" : "text-[var(--color-muted)] hover:text-[var(--color-text)]"}`}>
                {t(`talk.style_${s}`)}
              </button>
            ))}
          </div>
          <span className={`inline-flex items-center gap-1.5 text-[12px] ${agent?.agent_connected ? "text-[var(--color-good)]" : "text-[var(--color-muted)]"}`}>
            <span className="w-2 h-2 rounded-full" style={{ background: agent?.agent_connected ? "var(--color-good)" : "var(--color-muted)" }} aria-hidden />
            {agent?.agent_connected ? t("talk.agentOn") : t("talk.agentOff")}
          </span>
        </div>

        <div className="flex flex-col items-center gap-6 w-full max-w-3xl">
          <TalkVisual look={look} state={state} level={loudness} token={state === "thinking" ? "--color-accent-2" : "--color-accent"} className={look === "wave" ? "h-56 w-full max-w-2xl" : "h-64 w-64 md:h-80 md:w-80"} />
          <p role="status" aria-live="polite" className="text-[15px] font-medium">{t(`talk.state_${state}`)}</p>
          <div className="w-full space-y-3 text-center min-h-24">
            <p className="text-[13px] text-[var(--color-muted)]">{partial || lastPerson?.text || (mic ? t("talk.saySomething") : "")}</p>
            <p className="text-[18px] md:text-[20px] leading-snug">{saying ?? lastAgent?.text ?? ""}</p>
          </div>
          {!agent?.agent_connected && (
            <div className="w-full max-w-xl rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 space-y-2 text-[13px]">
              <p>{t("talk.connect")}</p>
              <div className="flex items-center gap-2">
                <code className="flex-1 min-w-0 truncate mono text-[12px] bg-[var(--color-surface-2)] rounded-md px-2 py-1.5">{CONNECT}</code>
                <Tip text={t("talk.copy")}><button onClick={() => void navigator.clipboard.writeText(CONNECT)} className="p-2 rounded-md border border-[var(--color-border)] hover:border-[var(--color-accent)]" {...named(t("talk.copy"))}><Copy size={14} /></button></Tip>
              </div>
              <p className="text-[var(--color-muted)]">{t("talk.ask")}</p>
            </div>
          )}
          {error && <p role="alert" className="text-[13px] text-[var(--color-warn)] text-center">{error}</p>}
        </div>

        <div className="flex items-center gap-3">
          <label className="text-[12px] text-[var(--color-muted)] flex items-center gap-1.5">{t("talk.lang")}
            <select value={lang} disabled={mic} onChange={(e) => setLang(e.target.value)} className="bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px]">
              {LANG_CHOICES.map((l) => <option key={l} value={l}>{l === "auto" ? t("talk.langAuto") : l.toUpperCase()}</option>)}
            </select>
          </label>
          <Tip text={mic ? t("talk.micOffHint") : t("talk.micOnHint")}>
            <button onClick={() => void talk.toggleMic(lang)} aria-pressed={mic} className={`grid place-items-center w-16 h-16 rounded-full transition-colors ${mic ? "bg-[var(--color-accent)] text-[var(--color-on-accent)]" : "border border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-accent)]"}`} {...named(mic ? t("talk.micOff") : t("talk.micOn"))}>
              {mic ? <Mic size={24} /> : <MicOff size={24} />}
            </button>
          </Tip>
          <Tip text={t("talk.silenceHint")}>
            <button onClick={talk.silence} disabled={!speaking} className="grid place-items-center w-11 h-11 rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-warn)] disabled:opacity-40" {...named(t("talk.silence"))}>
              <Square size={16} />
            </button>
          </Tip>
          <Tip text={t("talk.widgetHint")}>
            <button onClick={openTalkWidget} className="grid place-items-center w-11 h-11 rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-accent)]" {...named(t("talk.widget"))}>
              <AppWindow size={16} />
            </button>
          </Tip>
          <Tip text={t("talk.historyHint")}>
            <button onClick={() => setHistory((v) => !v)} aria-pressed={history} className="grid place-items-center w-11 h-11 rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-accent)]" {...named(t("talk.history"))}>
              <History size={16} />
            </button>
          </Tip>
        </div>
      </section>

      {history && (
        <aside aria-label={t("talk.history")} className="hidden lg:flex w-80 shrink-0 flex-col border-l border-[var(--color-border)] bg-[var(--color-surface)]/60">
          <h2 className="px-4 py-3 text-[12px] font-semibold text-[var(--color-muted)] border-b border-[var(--color-border)]">{t("talk.history")}</h2>
          <ol className="flex-1 overflow-y-auto p-3 space-y-2 text-[13px]">
            {turns.length === 0 && <li className="text-[var(--color-muted)]">{t("talk.empty")}</li>}
            {turns.map((x, i) => (
              <li key={i} className={`rounded-lg px-3 py-2 ${x.who === "person" ? "bg-[var(--color-surface-2)]" : "border border-[var(--color-border)]"}`}>
                <span className="block text-[11px] text-[var(--color-muted)]">{x.who === "person" ? t("talk.you") : t("talk.agent")}</span>
                {x.text}
              </li>
            ))}
          </ol>
          <p className="px-4 py-2 text-[11px] text-[var(--color-muted)] border-t border-[var(--color-border)]">{t("talk.heardCount", { n: heard.length })}</p>
        </aside>
      )}
    </main>
  );
}
