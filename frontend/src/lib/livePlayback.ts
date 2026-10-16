import { useEffect } from "react";
import { BASE } from "./api";

const KEY = "speech-live-playback";

export function livePlaybackEnabled(): boolean {
  return localStorage.getItem(KEY) !== "0";
}
export function setLivePlaybackEnabled(on: boolean): void {
  localStorage.setItem(KEY, on ? "1" : "0");
  window.dispatchEvent(new Event(KEY));
}

export type LiveEvent =
  | { kind: "start" | "restart"; project: string; block: string; aloud?: boolean }
  | { kind: "chunk"; project: string; block: string; sr: number; data: string; aloud?: boolean }
  | { kind: "end"; project: string; block: string; ok: boolean; first_ms: number | null; aloud?: boolean }
  | { kind: "stop"; project: string; aloud: true }
  | { kind: "lagged"; missed: number };

function samplesOf(data: string): Float32Array<ArrayBuffer> {
  const bin = atob(data);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new Float32Array(bytes.buffer, 0, bytes.length >> 2);
}

/** Plays pieces back to back as they arrive: one project's blocks (`pid`), or with `null` whatever is voiced aloud on
 * any screen (an agent speaking through the studio); a restarted take or a stop drops what was queued. */
export class LivePlayer {
  private ctx: AudioContext | null = null;
  private out: AnalyserNode | null = null;
  private cursor = 0;
  private playing: AudioBufferSourceNode[] = [];
  private readonly pid: string | null;
  private readonly makeContext: () => AudioContext;

  constructor(pid: string | null, makeContext: () => AudioContext = () => new AudioContext()) {
    this.pid = pid;
    this.makeContext = makeContext;
  }

  private accepts(ev: Exclude<LiveEvent, { kind: "lagged" }>): boolean {
    return this.pid === null ? ev.aloud === true : !ev.aloud && ev.project === this.pid;
  }

  /** Whether sound is queued or playing now. */
  get speaking(): boolean {
    return this.playing.length > 0;
  }

  /** The loudness of what plays now, 0..1. */
  level(): number {
    if (!this.out) return 0;
    const data = new Float32Array(this.out.fftSize);
    this.out.getFloatTimeDomainData(data);
    let sum = 0;
    for (const v of data) sum += v * v;
    return Math.min(1, Math.sqrt(sum / data.length) * 4);
  }

  handle(ev: LiveEvent): void {
    if (ev.kind === "lagged" || !this.accepts(ev)) return;
    if (ev.kind === "restart" || ev.kind === "stop") { this.stop(); return; }
    if (ev.kind !== "chunk") return;
    this.ctx ??= this.makeContext();
    if (!this.out) {
      this.out = this.ctx.createAnalyser();
      this.out.fftSize = 1024;
      this.out.connect(this.ctx.destination);
    }
    if (this.ctx.state === "suspended") void this.ctx.resume();
    const samples = samplesOf(ev.data);
    const buffer = this.ctx.createBuffer(1, samples.length, ev.sr);
    buffer.copyToChannel(samples, 0);
    const node = this.ctx.createBufferSource();
    node.buffer = buffer;
    node.connect(this.out);
    const at = Math.max(this.ctx.currentTime + 0.05, this.cursor);
    node.start(at);
    this.cursor = at + buffer.duration;
    this.playing.push(node);
    node.onended = () => { this.playing = this.playing.filter((s) => s !== node); };
  }

  stop(): void {
    for (const s of this.playing) s.stop();
    this.playing = [];
    this.cursor = 0;
  }

  close(): void {
    this.stop();
    void this.ctx?.close();
    this.ctx = null;
    this.out = null;
  }
}

/** Plays the blocks of project `pid` while the engine voices them, one after another, when the setting is on. */
export function useLivePlayback(pid: string): void {
  useEffect(() => {
    const player = new LivePlayer(pid);
    let source: EventSource | null = null;
    const sync = () => {
      if (livePlaybackEnabled() && !source) {
        source = new EventSource(`${BASE}/speech/live`);
        source.onmessage = (msg: MessageEvent<string>) => player.handle(JSON.parse(msg.data) as LiveEvent);
      } else if (!livePlaybackEnabled() && source) {
        source.close();
        source = null;
        player.stop();
      }
    };
    sync();
    window.addEventListener(KEY, sync);
    return () => {
      window.removeEventListener(KEY, sync);
      source?.close();
      player.close();
    };
  }, [pid]);
}

let aloudPlayer: LivePlayer | null = null;

/** The player of what an agent says aloud through the studio, while the window is open: its state and loudness. */
export function aloudPlayback(): LivePlayer | null {
  return aloudPlayer;
}

/** Plays whatever is voiced aloud (`say_*`, `speak_text` with aloud) on any screen, for as long as the window is open;
 * the open connection is what tells the server there is somewhere to play it. */
export function useAloudPlayback(): void {
  useEffect(() => {
    const player = new LivePlayer(null);
    aloudPlayer = player;
    const source = new EventSource(`${BASE}/speech/live`);
    source.onmessage = (msg: MessageEvent<string>) => player.handle(JSON.parse(msg.data) as LiveEvent);
    return () => {
      source.close();
      player.close();
      aloudPlayer = null;
    };
  }, []);
}
