import en from "../locales/en.json";

/** What the block's markup does to the delivery, found in its text: pauses, tempo, pitch and stress. */
export type CueKind = "pause" | "rate" | "pitch" | "stress";
export type CueKey = keyof typeof en.prosody;

export type Cue = {
  kind: CueKind;
  /** The text the cue colours; a pause colours its own tag. */
  from: number;
  to: number;
  /** The markup that makes the cue: removing these spans removes it. */
  tags: [number, number][];
  /** Its name in the `prosody` locale section, and the value the name takes. */
  key: CueKey;
  value?: string;
};

const PAUSE = /\[\s*pause\s+(\d+(?:[.,]\d+)?)\s*(ms|s)?\s*\]|sil\s*<\[\s*(\d+)\s*\]>|<\[\s*(tiny|small|medium|large|huge)\s*\]>|<\|prosody:(pause|long_pause)\|>/gi;
const RATE = /\[\s*rate\s*:\s*(\d+(?:[.,]\d+)?)\s*\]/gi;
const SENTENCE_TAG = /<\|prosody:(speed_very_slow|speed_slow|speed_fast|speed_very_fast|pitch_low|pitch_high|expressive_high|expressive_low)\|>/g;
const ACCENT = /\*\*([^*\n]{1,80})\*\*/g;
const EMPHASIS = /<emphasis>([\s\S]*?)<\/emphasis>/g;
const CONTEXT_MS: Record<string, number> = { tiny: 100, small: 250, medium: 500, large: 1000, huge: 2000 };

/** Where the sentence that goes on at `at` ends: after its closing mark, or at the text's end. */
function sentenceEnd(text: string, at: number): number {
  const m = /[.!?…]+["»”)]*(?=\s|$)/.exec(text.slice(at));
  return m ? at + m.index + m[0].length : text.length;
}

function pauseMs(m: RegExpExecArray): number {
  if (m[1]) return parseFloat(m[1].replace(",", ".")) * (m[2]?.toLowerCase() === "s" ? 1000 : 1);
  if (m[3]) return Number(m[3]);
  if (m[4]) return CONTEXT_MS[m[4].toLowerCase()];
  return m[5] === "long_pause" ? 1000 : 500;
}

/** Each cue of the text in its order. A tempo holds until the next tempo tag or the block's end, as the server reads it. */
export function cues(text: string): Cue[] {
  const out: Cue[] = [];
  for (const m of text.matchAll(PAUSE)) {
    const at = m.index ?? 0;
    const ms = Math.round(pauseMs(m as RegExpExecArray));
    out.push({ kind: "pause", from: at, to: at + m[0].length, tags: [[at, at + m[0].length]], key: ms >= 1000 ? "pauseS" : "pauseMs", value: String(ms >= 1000 ? ms / 1000 : ms) });
  }
  const rates = [...text.matchAll(RATE)];
  rates.forEach((m, i) => {
    const rate = parseFloat(m[1].replace(",", "."));
    const at = m.index ?? 0;
    const next = rates[i + 1];
    const end = next ? next.index ?? text.length : text.length;
    if (rate === 1) return;
    const tags: [number, number][] = [[at, at + m[0].length]];
    if (next && parseFloat(next[1].replace(",", ".")) === 1) tags.push([end, end + next[0].length]);
    out.push({ kind: "rate", from: at + m[0].length, to: end, tags, key: "rate", value: `×${rate}` });
  });
  for (const m of text.matchAll(SENTENCE_TAG)) {
    const at = m.index ?? 0;
    const from = at + m[0].length;
    out.push({ kind: m[1].startsWith("speed") ? "rate" : "pitch", from, to: sentenceEnd(text, from), tags: [[at, from]], key: m[1] as CueKey });
  }
  for (const re of [ACCENT, EMPHASIS]) {
    for (const m of text.matchAll(re)) {
      const at = m.index ?? 0;
      const open = re === ACCENT ? 2 : "<emphasis>".length;
      const close = re === ACCENT ? 2 : "</emphasis>".length;
      const end = at + m[0].length;
      out.push({ kind: "stress", from: at + open, to: end - close, tags: [[at, at + open], [end - close, end]], key: "stress" });
    }
  }
  return out.sort((a, b) => a.from - b.from || a.to - b.to);
}

/** The text cut at every cue edge, each piece with the cues over it, for colouring a copy of the text. */
export function spans(text: string, list: Cue[]): { text: string; kinds: CueKind[] }[] {
  const edges = new Set<number>([0, text.length]);
  for (const c of list) { edges.add(c.from); edges.add(c.to); }
  const cuts = [...edges].filter((n) => n >= 0 && n <= text.length).sort((a, b) => a - b);
  const out: { text: string; kinds: CueKind[] }[] = [];
  for (let i = 0; i + 1 < cuts.length; i++) {
    const [a, b] = [cuts[i], cuts[i + 1]];
    if (a === b) continue;
    out.push({ text: text.slice(a, b), kinds: [...new Set(list.filter((c) => c.from <= a && c.to >= b).map((c) => c.kind))] });
  }
  return out;
}

/** The text without the cue's markup; a space a tag stood between is kept once. */
export function withoutCue(text: string, cue: Cue): string {
  let out = text;
  for (const [a, b] of [...cue.tags].sort((x, y) => y[0] - x[0])) {
    const end = out[a - 1] === " " && out[b] === " " ? b + 1 : b;
    out = out.slice(0, a) + out.slice(end);
  }
  return out.trim();
}
