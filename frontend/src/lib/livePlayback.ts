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
  | { kind: "start" | "restart"; project: string; block: string }
  | { kind: "chunk"; project: string; block: string; sr: number; data: string }
  | { kind: "end"; project: string; block: string; ok: boolean; first_ms: number | null }
  | { kind: "lagged"; missed: number };

function samplesOf(data: string): Float32Array<ArrayBuffer> {
  const bin = atob(data);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new Float32Array(bytes.buffer, 0, bytes.length >> 2);
}

/** Plays the pieces of one project's blocks back to back as they arrive; a restarted take drops what was queued. */
export class LivePlayer {
  private ctx: AudioContext | null = null;
  private cursor = 0;
  private playing: AudioBufferSourceNode[] = [];

  constructor(private readonly pid: string, private readonly makeContext: () => AudioContext = () => new AudioContext()) {}

  handle(ev: LiveEvent): void {
    if (ev.kind === "lagged" || ev.project !== this.pid) return;
    if (ev.kind === "restart") { this.stop(); return; }
    if (ev.kind !== "chunk") return;
    this.ctx ??= this.makeContext();
    const samples = samplesOf(ev.data);
    const buffer = this.ctx.createBuffer(1, samples.length, ev.sr);
    buffer.copyToChannel(samples, 0);
    const node = this.ctx.createBufferSource();
    node.buffer = buffer;
    node.connect(this.ctx.destination);
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
