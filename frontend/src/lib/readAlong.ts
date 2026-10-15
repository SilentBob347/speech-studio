/** Which word of a block sounds at a moment of its take: by the word times the take's check heard when they are one
 * per word of the text, else by the words' length spread over the take. */

/** A word of the text as written, `[from, to)`; markup (tags, pauses, stress and accent marks) is no word. */
export function textWords(text: string): [number, number][] {
  const markup: [number, number][] = [];
  for (const m of text.matchAll(/\[[^\]\n]*\]|<[^<>\n]*>/g)) markup.push([m.index ?? 0, (m.index ?? 0) + m[0].length]);
  const out: [number, number][] = [];
  for (const m of text.matchAll(/[\p{L}\p{N}]+(?:[-'’+][\p{L}\p{N}]+)*/gu)) {
    const at = m.index ?? 0;
    if (!markup.some(([a, b]) => at >= a && at < b)) out.push([at, at + m[0].length]);
  }
  return out;
}

/** When each word starts and ends in the take, in seconds. */
export function wordTimes(text: string, duration: number, timed?: [string, number, number][] | null): { at: [number, number]; start: number; end: number }[] {
  const words = textWords(text);
  if (timed && timed.length === words.length) return words.map((at, i) => ({ at, start: timed[i][1], end: timed[i][2] }));
  const weight = words.map(([a, b]) => b - a + 1);
  const total = weight.reduce((s, w) => s + w, 0) || 1;
  let t = 0;
  return words.map((at, i) => {
    const start = t;
    t += (weight[i] / total) * duration;
    return { at, start, end: t };
  });
}

/** The word sounding at `t`: the last one started by then. */
export function wordAt(times: { at: [number, number]; start: number }[], t: number): [number, number] | null {
  let found: [number, number] | null = null;
  for (const w of times) {
    if (w.start > t) break;
    found = w.at;
  }
  return found;
}

/** The sentence of `text` holding position `at`: from the end of the sentence before it to its own closing mark. */
export function sentenceAt(text: string, at: number): [number, number] {
  let start = 0;
  for (const m of text.slice(0, at).matchAll(/[.!?…]+["»”)]*\s+|\n+/g)) start = (m.index ?? 0) + m[0].length;
  const end = /[.!?…]+["»”)]*(?=\s|$)|\n/.exec(text.slice(at));
  return [start, end ? at + end.index + (end[0] === "\n" ? 0 : end[0].length) : text.length];
}
