import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, History, Mic, MicOff, Square } from "lucide-react";
import { api, BASE, type McpStatus } from "../../lib/api";
import { aloudPlayback, type LiveEvent } from "../../lib/livePlayback";
import { talkApi, type HeardLine } from "../../lib/talk";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { LANG_CHOICES } from "../speech/langs";
import Hint, { Tip } from "../Hint";
import Aura from "./Aura";

type Turn = { who: "person" | "agent"; text: string; at: number; firstMs?: number | null };
type State = "off" | "listening" | "hearing" | "thinking" | "speaking";

const CONNECT = "claude mcp add --transport http speech-studio http://127.0.0.1:8797/mcp";

/** A spoken conversation with an agent through the studio: the person's words heard live, the agent's answer said aloud
 * as it writes it, the state of both, and the conversation as text. */
export default function TalkPage() {
  const { t } = useTranslation();
  const [lang, setLang] = useState("auto");
  const [mic, setMic] = useState(false);
  const [heard, setHeard] = useState<HeardLine[]>([]);
  const [partial, setPartial] = useState("");
  const [saying, setSaying] = useState<{ session: string; block: string; text: string } | null>(null);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [agent, setAgent] = useState<McpStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState(true);
  const [speaking, setSpeaking] = useState(false);
  const micLevel = useRef(0);
  const seen = useRef(0);
  const texts = useRef(new Map<string, string>());

  // what the person says: the phrases heard so far and the one going on
  useEffect(() => {
    if (!mic) return;
    const id = window.setInterval(() => {
      talkApi.heard().then((h) => {
        setHeard(h.lines);
        setPartial(h.partial?.text ?? "");
        if (h.error) setError(h.error);
        if (h.lines.length > seen.current) {
          const fresh = h.lines.slice(seen.current).map((l) => ({ who: "person" as const, text: l.text, at: Date.now() }));
          seen.current = h.lines.length;
          setTurns((all) => [...all, ...fresh]);
        }
      }, (e: unknown) => setError(errorText(e)));
    }, 250);
    return () => window.clearInterval(id);
  }, [mic]);

  // what the agent says: each phrase as it starts, its text from its session
  useEffect(() => {
    const source = new EventSource(`${BASE}/speech/live`);
    source.onmessage = (msg: MessageEvent<string>) => {
      const ev = JSON.parse(msg.data) as LiveEvent;
      if (ev.kind === "lagged" || !ev.aloud) return;
      if (ev.kind === "stop") { setSaying(null); return; }
      if (ev.kind !== "start") return;
      const key = `${ev.project}/${ev.block}`;
      const known = texts.current.get(key);
      const show = (text: string) => {
        setSaying({ session: ev.project, block: ev.block, text });
        setTurns((all) => all.some((x) => x.who === "agent" && x.text === text && Date.now() - x.at < 60_000) ? all : [...all, { who: "agent", text, at: Date.now() }]);
      };
      if (known) { show(known); return; }
      talkApi.session(ev.project).then((s) => {
        for (const p of s.phrases) texts.current.set(`${ev.project}/${p.id}`, p.text);
        const text = texts.current.get(key);
        if (text) show(text);
      }, () => undefined);
    };
    return () => source.close();
  }, []);

  // how loud the person is, for the aura; the agent's loudness the aura reads from the player itself
  useEffect(() => {
    micLevel.current = 0;
    if (!mic) return;
    const id = window.setInterval(() => { talkApi.level().then((l) => { micLevel.current = l.level; }, () => undefined); }, 80);
    return () => window.clearInterval(id);
  }, [mic]);

  useEffect(() => {
    const id = window.setInterval(() => setSpeaking(aloudPlayback()?.speaking ?? false), 100);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    const look = () => { api.mcpStatus().then(setAgent, () => setAgent(null)); };
    look();
    const id = window.setInterval(look, 3000);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => () => { void talkApi.listenStop().catch(() => undefined); }, []);

  const toggleMic = async () => {
    setError(null);
    try {
      if (mic) {
        await talkApi.listenStop();
        setMic(false);
        setPartial("");
      } else {
        seen.current = 0;
        setHeard([]);
        await talkApi.listenStart(lang);
        setMic(true);
      }
    } catch (e) {
      setError(errorText(e));
    }
  };

  const lastPerson = turns.filter((x) => x.who === "person").at(-1);
  const lastAgent = turns.filter((x) => x.who === "agent").at(-1);
  const state: State = speaking ? "speaking"
    : partial ? "hearing"
    : mic && lastPerson && (!lastAgent || lastAgent.at < lastPerson.at) && agent?.agent_connected ? "thinking"
    : mic ? "listening" : "off";
  const loudness = () => (speaking ? aloudPlayback()?.level() ?? 0 : micLevel.current);

  return (
    <main className="flex-1 min-h-0 flex" data-mcp-page={t("talk.title")}>
      <section className="flex-1 min-w-0 flex flex-col items-center justify-between px-4 py-6 md:py-10">
        <div className="w-full max-w-3xl flex items-center justify-between gap-3">
          <h1 className="text-[15px] font-semibold">{t("talk.title")} <Hint text={t("talk.hint")} /></h1>
          <span className={`inline-flex items-center gap-1.5 text-[12px] ${agent?.agent_connected ? "text-[var(--color-good)]" : "text-[var(--color-muted)]"}`}>
            <span className="w-2 h-2 rounded-full" style={{ background: agent?.agent_connected ? "var(--color-good)" : "var(--color-muted)" }} aria-hidden />
            {agent?.agent_connected ? t("talk.agentOn") : t("talk.agentOff")}
          </span>
        </div>

        <div className="flex flex-col items-center gap-6 w-full max-w-3xl">
          <Aura state={state} level={loudness} token={state === "thinking" ? "--color-accent-2" : "--color-accent"} className="h-64 w-64 md:h-80 md:w-80" />
          <p role="status" aria-live="polite" className="text-[15px] font-medium">{t(`talk.state_${state}`)}</p>
          <div className="w-full space-y-3 text-center min-h-24">
            <p className="text-[13px] text-[var(--color-muted)]">{partial || lastPerson?.text || (mic ? t("talk.saySomething") : "")}</p>
            <p className="text-[18px] md:text-[20px] leading-snug">{saying?.text ?? lastAgent?.text ?? ""}</p>
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
            <button onClick={() => void toggleMic()} aria-pressed={mic} className={`grid place-items-center w-16 h-16 rounded-full transition-colors ${mic ? "bg-[var(--color-accent)] text-[var(--color-on-accent)]" : "border border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-accent)]"}`} {...named(mic ? t("talk.micOff") : t("talk.micOn"))}>
              {mic ? <Mic size={24} /> : <MicOff size={24} />}
            </button>
          </Tip>
          <Tip text={t("talk.silenceHint")}>
            <button onClick={() => void talkApi.silence().catch((e: unknown) => setError(errorText(e)))} disabled={!speaking} className="grid place-items-center w-11 h-11 rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-warn)] disabled:opacity-40" {...named(t("talk.silence"))}>
              <Square size={16} />
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
