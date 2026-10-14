import { describe, expect, it } from "vitest";
import { activeToken, mergeNext, setLineText, splitLine, toggleCut, tokensOf, cutSeconds, type ListenLine } from "../src/lib/listen";

const line = (id: string, words: [string, number, number][], extra: Partial<ListenLine> = {}): ListenLine => ({
  id, start: words[0][1], end: words[words.length - 1][2], speaker: "0", text: words.map((w) => w[0]).join(" "),
  words: words.map(([text, start, end]) => ({ text, start, end, cut: false })), approx: false, flag: null, evidence: null, hidden: false, ...extra,
});

const lines = [line("l0", [["Hello", 0, 0.5], ["there", 0.6, 1.0]]), line("l1", [["skip", 1.2, 1.5]], { hidden: true }), line("l2", [["General", 2.0, 2.6], ["Kenobi", 2.7, 3.2]])];

describe("transcript editing", () => {
  it("hidden lines give no tokens and the sounding word is found by time", () => {
    const tokens = tokensOf(lines);
    expect(tokens.map((t) => t.text)).toEqual(["Hello", "there", "General", "Kenobi"]);
    expect(activeToken(tokens, 0.7)).toBe(1);
    expect(activeToken(tokens, 2.65)).toBe(2);
    expect(activeToken(tokens, 5)).toBe(-1);
    expect(activeToken(tokens, 0.5999), "a seek lands just before the word").toBe(1);
  });

  it("marks a range and returns it when all of it is marked", () => {
    const tokens = tokensOf(lines);
    const marked = toggleCut(lines, tokens, 3, 1);
    expect(marked[0].words[1].cut && marked[2].words[0].cut && marked[2].words[1].cut).toBe(true);
    expect(cutSeconds(marked)).toBeCloseTo(0.4 + 0.6 + 0.5);
    const back = toggleCut(marked, tokensOf(marked), 1, 3);
    expect(back.flatMap((l) => l.words).some((w) => w.cut)).toBe(false);
  });

  it("splits, merges and retexts lines keeping word times", () => {
    const split = splitLine(lines, 2, 1);
    expect(split.map((l) => l.text)).toEqual(["Hello there", "skip", "General", "Kenobi"]);
    expect(split[3].start).toBe(2.7);
    expect(new Set(split.map((l) => l.id)).size).toBe(4);
    expect(mergeNext(split, 2)[2].text).toBe("General Kenobi");
    expect(setLineText(lines[0], "Hullo there").words[0]).toEqual({ text: "Hullo", start: 0, end: 0.5, cut: false });
    const respread = setLineText(lines[0], "Hello over there");
    expect(respread.approx && respread.words.length === 3 && respread.words[2].end === 1.0).toBe(true);
  });
});
