import { useCallback, useEffect, useRef, useState } from "react";
import { api, BASE, type McpStatus } from "../../lib/api";
import { aloudPlayback, type LiveEvent } from "../../lib/livePlayback";
import { receiveTalkFeed } from "../../lib/talkFeed";
import { talkApi, type HeardLine } from "../../lib/talk";
import { errorText } from "../../lib/voices";
import { TALK_STYLES, type TalkState, type TalkStyle } from "./visual";

export type Turn = { who: "person" | "agent"; text: string; at: number };

const STYLE_KEY = "speech.talk.style";

function savedStyle(): TalkStyle {
  try {
    const v = localStorage.getItem(STYLE_KEY);
    return TALK_STYLES.includes(v as TalkStyle) ? (v as TalkStyle) : "orb";
  } catch {
    return "orb";
  }
}

/** The look of the talk screen, the same in the window and in the widget: a change in one shows in the other. */
export function useTalkStyle(): [TalkStyle, (s: TalkStyle) => void] {
  const [style, setStyle] = useState(savedStyle);
  useEffect(() => {
    const follow = (e: StorageEvent) => { if (e.key === STYLE_KEY) setStyle(savedStyle()); };
    window.addEventListener("storage", follow);
    return () => window.removeEventListener("storage", follow);
  }, []);
  const choose = useCallback((s: TalkStyle) => {
    setStyle(s);
    try { localStorage.setItem(STYLE_KEY, s); } catch { /* the choice lives for this window only */ }
  }, []);
  return [style, choose];
}

/** A spoken conversation with an agent through the studio: what the person says heard live, what the agent says as it
 * starts each phrase, and the state of both. `fed`: the sound plays in another window, which sends its loudness. */
export function useTalk({ fed }: { fed: boolean }) {
  const [running, setRunning] = useState(false);
  const [heard, setHeard] = useState<HeardLine[]>([]);
  const [partial, setPartial] = useState("");
  const [saying, setSaying] = useState<string | null>(null);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [agent, setAgent] = useState<McpStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [speaking, setSpeaking] = useState(false);
  const micLevel = useRef(0);
  const fedLevel = useRef(0);
  const seen = useRef(-1);
  const texts = useRef(new Map<string, string>());

  // what the person says; the listening itself is the studio's, so every window shows the same
  useEffect(() => {
    const id = window.setInterval(() => {
      talkApi.heard().then((h) => {
        setRunning(h.running);
        setHeard(h.lines);
        setPartial(h.partial?.text ?? "");
        if (h.error) setError(h.error);
        if (seen.current < 0 || h.lines.length < seen.current) seen.current = h.lines.length;
        if (h.lines.length > seen.current) {
          const fresh = h.lines.slice(seen.current).map((l) => ({ who: "person" as const, text: l.text, at: Date.now() }));
          seen.current = h.lines.length;
          setTurns((all) => [...all, ...fresh]);
        }
      }, (e: unknown) => setError(errorText(e)));
    }, 250);
    return () => window.clearInterval(id);
  }, []);

  // what the agent says: each phrase as it starts, its text from its session
  useEffect(() => {
    const source = new EventSource(`${BASE}/speech/live`);
    source.onmessage = (msg: MessageEvent<string>) => {
      const ev = JSON.parse(msg.data) as LiveEvent;
      if (ev.kind === "lagged" || !ev.aloud) return;
      if (ev.kind === "stop") { setSaying(null); return; }
      if (ev.kind !== "start") return;
      const key = `${ev.project}/${ev.block}`;
      const show = (text: string) => {
        setSaying(text);
        setTurns((all) => all.some((x) => x.who === "agent" && x.text === text && Date.now() - x.at < 60_000) ? all : [...all, { who: "agent", text, at: Date.now() }]);
      };
      const known = texts.current.get(key);
      if (known) { show(known); return; }
      talkApi.session(ev.project).then((s) => {
        for (const p of s.phrases) texts.current.set(`${ev.project}/${p.id}`, p.text);
        const text = texts.current.get(key);
        if (text) show(text);
      }, (e: unknown) => setError(errorText(e)));
    };
    return () => source.close();
  }, []);

  useEffect(() => {
    if (!running) { micLevel.current = 0; return; }
    const id = window.setInterval(() => { talkApi.level().then((l) => { micLevel.current = l.level; }, () => undefined); }, 80);
    return () => window.clearInterval(id);
  }, [running]);

  useEffect(() => {
    if (fed) return receiveTalkFeed((talking, level) => { fedLevel.current = level; setSpeaking(talking); });
    const id = window.setInterval(() => setSpeaking(aloudPlayback()?.speaking ?? false), 100);
    return () => window.clearInterval(id);
  }, [fed]);

  useEffect(() => {
    const look = () => { api.mcpStatus().then(setAgent, () => setAgent(null)); };
    look();
    const id = window.setInterval(look, 3000);
    return () => window.clearInterval(id);
  }, []);

  const toggleMic = useCallback(async (lang: string) => {
    setError(null);
    try {
      if (running) {
        await talkApi.listenStop();
        setRunning(false);
        setPartial("");
      } else {
        await talkApi.listenStart(lang);
        setRunning(true);
      }
    } catch (e) {
      setError(errorText(e));
    }
  }, [running]);

  const silence = useCallback(() => { talkApi.silence().catch((e: unknown) => setError(errorText(e))); }, []);

  const lastPerson = turns.filter((x) => x.who === "person").at(-1);
  const lastAgent = turns.filter((x) => x.who === "agent").at(-1);
  const state: TalkState = speaking ? "speaking"
    : partial ? "hearing"
    : running && lastPerson && (!lastAgent || lastAgent.at < lastPerson.at) && agent?.agent_connected ? "thinking"
    : running ? "listening" : "off";
  const loudness = useCallback(
    () => (speaking ? (fed ? fedLevel.current : aloudPlayback()?.level() ?? 0) : micLevel.current),
    [speaking, fed],
  );

  return { state, running, heard, partial, saying, turns, lastPerson, lastAgent, agent, error, speaking, loudness, toggleMic, silence };
}
