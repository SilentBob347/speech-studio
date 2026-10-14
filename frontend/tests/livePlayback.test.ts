import { describe, expect, it } from "vitest";
import { LivePlayer, livePlaybackEnabled, setLivePlaybackEnabled } from "../src/lib/livePlayback";

class FakeNode {
  buffer: { duration: number } | null = null;
  startedAt: number | null = null;
  stopped = false;
  onended: (() => void) | null = null;
  connect() {}
  start(at: number) { this.startedAt = at; }
  stop() { this.stopped = true; }
}

function fakeContext() {
  const nodes: FakeNode[] = [];
  const buffers: { samples: Float32Array; rate: number }[] = [];
  const ctx = {
    currentTime: 10,
    destination: {},
    createBuffer: (_channels: number, length: number, rate: number) => {
      const b = { samples: new Float32Array(length), rate, duration: length / rate, copyToChannel(s: Float32Array) { b.samples.set(s); } };
      buffers.push(b);
      return b;
    },
    createBufferSource: () => { const n = new FakeNode(); nodes.push(n); return n; },
    close: async () => {},
  };
  return { ctx: ctx as unknown as AudioContext, nodes, buffers };
}

const piece = (values: number[]) => btoa(String.fromCharCode(...new Uint8Array(new Float32Array(values).buffer)));

describe("live playback", () => {
  it("plays the pieces of its project back to back and drops a restarted take", () => {
    const fake = fakeContext();
    const player = new LivePlayer("p1", () => fake.ctx);
    player.handle({ kind: "start", project: "p1", block: "b1" });
    player.handle({ kind: "chunk", project: "p1", block: "b1", sr: 4, data: piece([0.5, -0.5, 0.25, 0]) });
    player.handle({ kind: "chunk", project: "p1", block: "b1", sr: 4, data: piece([1, 1]) });
    player.handle({ kind: "chunk", project: "other", block: "x", sr: 4, data: piece([1]) });
    expect(fake.nodes).toHaveLength(2);
    expect(Array.from(fake.buffers[0].samples)).toEqual([0.5, -0.5, 0.25, 0]);
    expect(fake.nodes[0].startedAt).toBeCloseTo(10.05);
    expect(fake.nodes[1].startedAt).toBeCloseTo(11.05);
    player.handle({ kind: "restart", project: "p1", block: "b1" });
    expect(fake.nodes.every((n) => n.stopped)).toBe(true);
    player.handle({ kind: "chunk", project: "p1", block: "b1", sr: 4, data: piece([0]) });
    expect(fake.nodes[2].startedAt).toBeCloseTo(10.05);
  });

  it("remembers the setting, on by default", () => {
    localStorage.removeItem("speech-live-playback");
    expect(livePlaybackEnabled()).toBe(true);
    setLivePlaybackEnabled(false);
    expect(livePlaybackEnabled()).toBe(false);
    setLivePlaybackEnabled(true);
  });
});
