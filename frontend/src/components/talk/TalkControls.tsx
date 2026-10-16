import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { AudioLines, ChevronUp, Mic, MicOff, SendHorizontal, Square, Volume2, VolumeX, X } from "lucide-react";
import { named } from "../../lib/a11y";
import { api, BASE } from "../../lib/api";
import { aloudPlayback } from "../../lib/livePlayback";
import { talkApi } from "../../lib/talk";
import { errorText } from "../../lib/voices";
import LevelMeter from "./LevelMeter";
import { setTalkPrefs, useTalkPrefs } from "../../lib/talkPrefs";
import { LANG_CHOICES } from "../speech/langs";
import { Tip } from "../Hint";
import type { useTalk } from "./useTalk";

type Talk = ReturnType<typeof useTalk>;

const round_ = "grid place-items-center rounded-full transition-colors disabled:opacity-40";
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

type Sinkable = AudioContext & { setSinkId?: (id: string) => Promise<void> };

/** Plays a test on the chosen speaker at the agent's volume, and how loud it plays: a chime, or what the microphone
 * test heard. */
class SpeakerTest {
  private ctx: Sinkable | null = null;
  private meter: AnalyserNode | null = null;
  private gain: GainNode | null = null;
  private element: HTMLAudioElement | null = null;

  private async open(output: string, volume: number) {
    this.ctx ??= new AudioContext() as Sinkable;
    if (!this.meter) {
      this.meter = this.ctx.createAnalyser();
      this.meter.fftSize = 1024;
      this.gain = this.ctx.createGain();
      this.meter.connect(this.gain).connect(this.ctx.destination);
    }
    if (this.gain) this.gain.gain.value = volume;
    if (this.ctx.setSinkId) await this.ctx.setSinkId(output);
    if (this.ctx.state === "suspended") await this.ctx.resume();
    return this.ctx;
  }

  async chime(output: string, volume: number) {
    const ctx = await this.open(output, volume);
    const start = ctx.currentTime;
    [523.25, 659.25, 783.99].forEach((hz, i) => {
      const tone = ctx.createOscillator();
      const env = ctx.createGain();
      tone.frequency.value = hz;
      env.gain.setValueAtTime(0, start + i * 0.18);
      env.gain.linearRampToValueAtTime(0.5, start + i * 0.18 + 0.02);
      env.gain.exponentialRampToValueAtTime(0.001, start + i * 0.18 + 0.6);
      tone.connect(env).connect(this.meter as AnalyserNode);
      tone.start(start + i * 0.18);
      tone.stop(start + i * 0.18 + 0.65);
    });
  }

  async play(url: string, output: string, volume: number): Promise<void> {
    const ctx = await this.open(output, volume);
    this.element?.pause();
    const element = new Audio(url);
    element.crossOrigin = "anonymous";
    this.element = element;
    ctx.createMediaElementSource(element).connect(this.meter as AnalyserNode);
    await element.play();
    await new Promise<void>((done) => { element.onended = () => done(); element.onpause = () => done(); });
  }

  peak(): number {
    if (!this.meter) return 0;
    const data = new Float32Array(this.meter.fftSize);
    this.meter.getFloatTimeDomainData(data);
    let peak = 0;
    for (const v of data) peak = Math.max(peak, Math.abs(v));
    return peak;
  }

  close() {
    this.element?.pause();
    void this.ctx?.close();
    this.ctx = null;
  }
}

/** The microphone and the speaker as call apps set them up: each with a live level meter, the microphone heard back
 * (Discord's mic test), the speaker tested with a chime (Zoom's speaker test), the agent's volume and the language. */
function TalkSettings({ running }: { running: boolean }) {
  const { t } = useTranslation();
  const prefs = useTalkPrefs();
  const [devices, setDevices] = useState<string[] | null>(null);
  const { speakers, name } = useSpeakers();
  const [problem, setProblem] = useState<string | null>(null);
  const [hearing, setHearing] = useState(false);
  const [round, setRound] = useState(0);
  const micPeak = useRef(0);
  const tester = useRef<SpeakerTest | null>(null);
  tester.current ??= new SpeakerTest();

  useEffect(() => {
    api.recordDevices().then((d) => setDevices(d.devices), (e: unknown) => { setDevices([]); setProblem(errorText(e)); });
    return () => tester.current?.close();
  }, []);

  // the microphone heard while this is open: the conversation's own when it runs, a test recording otherwise
  useEffect(() => {
    if (running || hearing) return;
    let open = false;
    talkApi.micTest(prefs.device).then(() => { open = true; setProblem(null); }, (e: unknown) => setProblem(errorText(e)));
    return () => {
      if (open) talkApi.micTestStop().catch((e: unknown) => console.error("[ERROR] mic test stop:", e));
    };
  }, [running, hearing, prefs.device, round]);

  useEffect(() => {
    const id = window.setInterval(() => { talkApi.level().then((l) => { micPeak.current = l.level; }, () => { micPeak.current = 0; }); }, 50);
    return () => window.clearInterval(id);
  }, []);

  const hearBack = async () => {
    setProblem(null);
    setHearing(true);
    try {
      await talkApi.micTestStop();
      await (tester.current as SpeakerTest).play(`${BASE}/record/test/audio?at=${Date.now()}`, prefs.output, prefs.muted ? 0 : prefs.volume);
    } catch (e) {
      setProblem(errorText(e));
    } finally {
      setHearing(false);
      setRound((r) => r + 1);
    }
  };

  const outPeak = () => Math.max(tester.current?.peak() ?? 0, aloudPlayback()?.level() ?? 0);
  const canChoose = typeof (AudioContext.prototype as { setSinkId?: unknown }).setSinkId === "function";
  const unnamed = !!speakers?.length && speakers.every((s) => !s.label);
  const field = "w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1.5 text-[12px] disabled:opacity-50";
  const label = "block space-y-1.5 text-[11px] text-[var(--color-muted)]";
  const small = "shrink-0 rounded-md border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-text)] hover:border-[var(--color-accent)] disabled:opacity-40";

  return (
    <div role="dialog" aria-label={t("talk.settings")} className="absolute left-0 bottom-full mb-2 z-50 w-80 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-xl p-3 space-y-4 text-[var(--color-text)]">
      <div className={label}>
        <span>{t("talk.device")}</span>
        <select value={prefs.device} disabled={running || hearing} onChange={(e) => setTalkPrefs({ device: e.target.value })} className={field} {...named(t("talk.device"))}>
          <option value="">{t("talk.deviceDefault")}</option>
          {devices?.map((d) => <option key={d} value={d}>{d}</option>)}
        </select>
        <LevelMeter peak={() => micPeak.current} label={t("talk.inputLevel")} />
        <span className="flex items-center justify-between gap-2">
          <span>{hearing ? t("talk.hearingBack") : t("talk.speakToTest")}</span>
          <button onClick={() => void hearBack()} disabled={running || hearing} className={small}>{t("talk.hearBack")}</button>
        </span>
      </div>
      <label className={label}>{t("talk.lang")}
        <select value={prefs.lang} disabled={running} onChange={(e) => setTalkPrefs({ lang: e.target.value })} className={field}>
          {LANG_CHOICES.map((l) => <option key={l} value={l}>{l === "auto" ? t("talk.langAuto") : l.toUpperCase()}</option>)}
        </select>
      </label>
      <div className={label}>
        <span>{t("talk.output")}</span>
        <select value={prefs.output} disabled={!canChoose} onChange={(e) => setTalkPrefs({ output: e.target.value })} className={field} {...named(t("talk.output"))}>
          <option value="">{t("talk.outputDefault")}</option>
          {speakers?.filter((s) => s.deviceId && s.deviceId !== "default").map((s, i) => <option key={s.deviceId} value={s.deviceId}>{s.label || `${t("talk.output")} ${i + 1}`}</option>)}
        </select>
        {!canChoose && <span className="block">{t("talk.outputUnsupported")}</span>}
        {canChoose && unnamed && (
          <span className="flex items-center justify-between gap-2">
            <span>{t("talk.outputNamesHint")}</span>
            <button onClick={() => { name().catch((e: unknown) => setProblem(errorText(e))); }} className={small}>{t("talk.outputNames")}</button>
          </span>
        )}
        <LevelMeter peak={outPeak} label={t("talk.outputLevel")} />
        <div className="flex items-center gap-2">
          <Tip text={prefs.muted ? t("talk.agentUnmute") : t("talk.agentMute")}>
            <button onClick={() => setTalkPrefs({ muted: !prefs.muted })} aria-pressed={prefs.muted} className={`${round_} ${quiet} w-8 h-8 shrink-0`} {...named(prefs.muted ? t("talk.agentUnmute") : t("talk.agentMute"))}>
              {prefs.muted ? <VolumeX size={14} /> : <Volume2 size={14} />}
            </button>
          </Tip>
          <input type="range" min={0} max={1} step={0.05} value={prefs.volume} disabled={prefs.muted} onChange={(e) => setTalkPrefs({ volume: Number(e.target.value) })} className="flex-1 accent-[var(--color-accent)]" {...named(t("talk.volume"))} />
          <span className="mono w-9 text-right text-[11px]">{Math.round(prefs.volume * 100)}%</span>
          <button onClick={() => { (tester.current as SpeakerTest).chime(prefs.output, prefs.muted ? 0 : prefs.volume).catch((e: unknown) => setProblem(errorText(e))); }} className={small}>{t("talk.testSpeaker")}</button>
        </div>
      </div>
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
    <div className={`shrink-0 flex flex-col items-center gap-3 ${compact ? "" : "w-full max-w-xl"}`}>
      {!compact && (
        <form onSubmit={(e) => { e.preventDefault(); void send(); }} className="flex w-full items-center gap-2 rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] pl-4 pr-1 py-1 focus-within:border-[var(--color-accent)]">
          <input value={draft} onChange={(e) => setDraft(e.target.value)} disabled={!running} placeholder={t("talk.typePlaceholder")} className="flex-1 min-w-0 bg-transparent text-[13px] outline-none disabled:opacity-50" {...named(t("talk.typePlaceholder"))} />
          <button type="submit" disabled={!running || !draft.trim()} className={`${round_} w-8 h-8 bg-[var(--color-accent)] text-[var(--color-on-accent)]`} {...named(t("talk.send"))}>
            <SendHorizontal size={14} />
          </button>
        </form>
      )}
      <div className="flex items-center gap-2">
        <div className="relative">
          <Tip text={t("talk.settings")}>
            <button onClick={() => setMenu((m) => !m)} aria-haspopup="dialog" aria-expanded={menu} className={`${round_} ${quiet} ${size}`} {...named(t("talk.settings"))}>
              <ChevronUp size={16} className={`transition-transform ${menu ? "" : "rotate-180"}`} />
            </button>
          </Tip>
          {menu && (
            <>
              <div className="fixed inset-0 z-40" onClick={() => setMenu(false)} />
              <TalkSettings running={running} />
            </>
          )}
        </div>
        {running && (
          <Tip text={t("talk.muteHint")}>
            <button onClick={() => void talk.toggleMute()} aria-pressed={muted} className={`${round_} ${size} ${muted ? "bg-[var(--color-warn)] text-[var(--color-bg)]" : quiet}`} {...named(muted ? t("talk.unmute") : t("talk.mute"))}>
              {muted ? <MicOff size={16} /> : <Mic size={16} />}
            </button>
          </Tip>
        )}
        <Tip text={running ? t("talk.micOffHint") : t("talk.micOnHint")}>
          <button onClick={() => void talk.toggleMic()} className={`${round_} ${main} ${running ? "bg-[var(--color-bad)] text-white" : "bg-[var(--color-accent)] text-[var(--color-on-accent)]"}`} {...named(running ? t("talk.micOff") : t("talk.micOn"))}>
            {running ? <X size={compact ? 18 : 22} /> : <AudioLines size={compact ? 18 : 22} />}
          </button>
        </Tip>
        <Tip text={t("talk.silenceHint")}>
          <button onClick={talk.silence} disabled={!speaking} className={`${round_} ${quiet} ${size}`} {...named(t("talk.silence"))}>
            <Square size={14} />
          </button>
        </Tip>
        {children}
      </div>
    </div>
  );
}
