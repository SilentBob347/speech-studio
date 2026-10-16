import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ChevronUp, Mic, MicOff, PhoneOff, Phone, SendHorizontal, Square, Volume2, VolumeX } from "lucide-react";
import { named } from "../../lib/a11y";
import { api } from "../../lib/api";
import { setTalkPrefs, useTalkPrefs } from "../../lib/talkPrefs";
import { LANG_CHOICES } from "../speech/langs";
import { Tip } from "../Hint";
import type { useTalk } from "./useTalk";

type Talk = ReturnType<typeof useTalk>;

const round = "grid place-items-center rounded-full transition-colors disabled:opacity-40";
const quiet = "border border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-accent)]";

/** The speakers the browser can play on; names come only once the page may use a microphone. */
function useSpeakers() {
  const [speakers, setSpeakers] = useState<MediaDeviceInfo[] | null>(null);
  const look = useCallback(() => {
    navigator.mediaDevices.enumerateDevices().then((all) => setSpeakers(all.filter((d) => d.kind === "audiooutput")), () => setSpeakers([]));
  }, []);
  useEffect(() => {
    look();
    navigator.mediaDevices.addEventListener("devicechange", look);
    return () => navigator.mediaDevices.removeEventListener("devicechange", look);
  }, [look]);
  const name = useCallback(async () => {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    for (const track of stream.getTracks()) track.stop();
    look();
  }, [look]);
  return { speakers, name };
}

/** The microphone, the language spoken, the agent's voice and the speaker it plays on. */
function TalkSettings({ running, up }: { running: boolean; up: boolean }) {
  const { t } = useTranslation();
  const prefs = useTalkPrefs();
  const [devices, setDevices] = useState<string[] | null>(null);
  const { speakers, name } = useSpeakers();
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    api.recordDevices().then((d) => setDevices(d.devices), (e: unknown) => { setDevices([]); setProblem(String(e)); });
  }, []);
  const canChoose = typeof (AudioContext.prototype as { setSinkId?: unknown }).setSinkId === "function";
  const unnamed = !!speakers?.length && speakers.every((s) => !s.label);
  const field = "w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1.5 text-[12px] disabled:opacity-50";
  const label = "block space-y-1 text-[11px] text-[var(--color-muted)]";

  return (
    <div role="dialog" aria-label={t("talk.settings")} className={`absolute left-0 z-50 w-72 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-xl p-3 space-y-3 text-[var(--color-text)] ${up ? "bottom-full mb-2" : "top-full mt-2"}`}>
      <label className={label}>{t("talk.device")}
        <select value={prefs.device} disabled={running} onChange={(e) => setTalkPrefs({ device: e.target.value })} className={field}>
          <option value="">{t("talk.deviceDefault")}</option>
          {devices?.map((d) => <option key={d} value={d}>{d}</option>)}
        </select>
      </label>
      <label className={label}>{t("talk.lang")}
        <select value={prefs.lang} disabled={running} onChange={(e) => setTalkPrefs({ lang: e.target.value })} className={field}>
          {LANG_CHOICES.map((l) => <option key={l} value={l}>{l === "auto" ? t("talk.langAuto") : l.toUpperCase()}</option>)}
        </select>
      </label>
      <div className={label}>
        <span>{t("talk.agentVoice")}</span>
        <div className="flex items-center gap-2">
          <Tip text={prefs.muted ? t("talk.agentUnmute") : t("talk.agentMute")}>
            <button onClick={() => setTalkPrefs({ muted: !prefs.muted })} aria-pressed={prefs.muted} className={`${round} ${quiet} w-8 h-8 shrink-0`} {...named(prefs.muted ? t("talk.agentUnmute") : t("talk.agentMute"))}>
              {prefs.muted ? <VolumeX size={14} /> : <Volume2 size={14} />}
            </button>
          </Tip>
          <input type="range" min={0} max={1} step={0.05} value={prefs.volume} disabled={prefs.muted} onChange={(e) => setTalkPrefs({ volume: Number(e.target.value) })} className="flex-1 accent-[var(--color-accent)]" {...named(t("talk.volume"))} />
          <span className="mono w-9 text-right text-[11px]">{Math.round(prefs.volume * 100)}%</span>
        </div>
      </div>
      <label className={label}>{t("talk.output")}
        <select value={prefs.output} disabled={!canChoose} onChange={(e) => setTalkPrefs({ output: e.target.value })} className={field}>
          <option value="">{t("talk.outputDefault")}</option>
          {speakers?.filter((s) => s.deviceId && s.deviceId !== "default").map((s, i) => <option key={s.deviceId} value={s.deviceId}>{s.label || `${t("talk.output")} ${i + 1}`}</option>)}
        </select>
        {!canChoose && <span className="block">{t("talk.outputUnsupported")}</span>}
        {canChoose && unnamed && (
          <span className="flex items-center justify-between gap-2">
            <span>{t("talk.outputNamesHint")}</span>
            <button onClick={() => { name().catch((e: unknown) => setProblem(String(e))); }} className="shrink-0 rounded-md border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-text)] hover:border-[var(--color-accent)]">{t("talk.outputNames")}</button>
          </span>
        )}
      </label>
      {problem && <p role="alert" className="text-[11px] text-[var(--color-warn)]">{problem}</p>}
    </div>
  );
}

/** The conversation's controls, as voice modes have them: start or end, mute the microphone (M), the microphone and
 * sound, stop the agent, and a field to write to the agent instead of saying it. */
export default function TalkControls({ talk, compact = false, children }: { talk: Talk; compact?: boolean; children?: ReactNode }) {
  const { t } = useTranslation();
  const [menu, setMenu] = useState(false);
  const [draft, setDraft] = useState("");
  const { running, muted, speaking } = talk;

  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenu(false);
      const typing = e.target instanceof HTMLElement && (e.target.closest("input, textarea, select, [contenteditable]") !== null);
      if (!typing && running && !e.ctrlKey && !e.metaKey && !e.altKey && e.code === "KeyM") {
        e.preventDefault();
        void talk.toggleMute();
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [running, talk]);

  const send = async () => {
    const text = draft.trim();
    if (!text) return;
    if (await talk.type(text)) setDraft("");
  };

  const size = compact ? "w-9 h-9" : "w-11 h-11";
  const main = compact ? "w-11 h-11" : "w-14 h-14";
  return (
    <div className={`flex flex-col items-center gap-3 ${compact ? "" : "w-full max-w-xl"}`}>
      {!compact && (
        <form onSubmit={(e) => { e.preventDefault(); void send(); }} className="flex w-full items-center gap-2 rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] pl-4 pr-1 py-1 focus-within:border-[var(--color-accent)]">
          <input value={draft} onChange={(e) => setDraft(e.target.value)} disabled={!running} placeholder={t("talk.typePlaceholder")} className="flex-1 min-w-0 bg-transparent text-[13px] outline-none disabled:opacity-50" {...named(t("talk.typePlaceholder"))} />
          <button type="submit" disabled={!running || !draft.trim()} className={`${round} w-8 h-8 bg-[var(--color-accent)] text-[var(--color-on-accent)]`} {...named(t("talk.send"))}>
            <SendHorizontal size={14} />
          </button>
        </form>
      )}
      <div className="flex items-center gap-2">
        <div className="relative">
          <Tip text={t("talk.settings")}>
            <button onClick={() => setMenu((m) => !m)} aria-haspopup="dialog" aria-expanded={menu} className={`${round} ${quiet} ${size}`} {...named(t("talk.settings"))}>
              <ChevronUp size={16} className={`transition-transform ${menu ? "" : "rotate-180"}`} />
            </button>
          </Tip>
          {menu && (
            <>
              <div className="fixed inset-0 z-40" onClick={() => setMenu(false)} />
              <TalkSettings running={running} up />
            </>
          )}
        </div>
        {running && (
          <Tip text={t("talk.muteHint")}>
            <button onClick={() => void talk.toggleMute()} aria-pressed={muted} className={`${round} ${size} ${muted ? "bg-[var(--color-warn)] text-[var(--color-bg)]" : quiet}`} {...named(muted ? t("talk.unmute") : t("talk.mute"))}>
              {muted ? <MicOff size={16} /> : <Mic size={16} />}
            </button>
          </Tip>
        )}
        <Tip text={running ? t("talk.micOffHint") : t("talk.micOnHint")}>
          <button onClick={() => void talk.toggleMic()} className={`${round} ${main} ${running ? "bg-[var(--color-bad)] text-white" : "bg-[var(--color-accent)] text-[var(--color-on-accent)]"}`} {...named(running ? t("talk.micOff") : t("talk.micOn"))}>
            {running ? <PhoneOff size={compact ? 18 : 22} /> : <Phone size={compact ? 18 : 22} />}
          </button>
        </Tip>
        <Tip text={t("talk.silenceHint")}>
          <button onClick={talk.silence} disabled={!speaking} className={`${round} ${quiet} ${size}`} {...named(t("talk.silence"))}>
            <Square size={14} />
          </button>
        </Tip>
        {children}
      </div>
    </div>
  );
}
