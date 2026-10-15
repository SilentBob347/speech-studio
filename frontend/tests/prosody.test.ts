import { describe, expect, it } from "vitest";
import { cues, spans, withoutCue } from "../src/lib/prosody";

describe("prosody cues in a block's text", () => {
  const text = "Привет [pause 500ms] мир. [rate:0.8]Медленно тут[rate:1] и **важно**. <|prosody:pitch_low|>Ниже. Дальше.";

  it("finds pauses, tempo up to its closing tag, a sentence tag up to the sentence end and stress", () => {
    const list = cues(text);
    expect(list.map((c) => [c.kind, c.key, c.value ?? "", text.slice(c.from, c.to)])).toEqual([
      ["pause", "pauseMs", "500", "[pause 500ms]"],
      ["rate", "rate", "×0.8", "Медленно тут"],
      ["stress", "stress", "", "важно"],
      ["pitch", "pitch_low", "", "Ниже."],
    ]);
  });

  it("takes a cue off with its markup and leaves the rest of the text as it was", () => {
    const list = cues(text);
    expect(withoutCue(text, list[0])).toBe("Привет мир. [rate:0.8]Медленно тут[rate:1] и **важно**. <|prosody:pitch_low|>Ниже. Дальше.");
    expect(withoutCue(text, list[1])).toBe("Привет [pause 500ms] мир. Медленно тут и **важно**. <|prosody:pitch_low|>Ниже. Дальше.");
    expect(withoutCue(text, list[2])).toBe("Привет [pause 500ms] мир. [rate:0.8]Медленно тут[rate:1] и важно. <|prosody:pitch_low|>Ниже. Дальше.");
  });

  it("a tempo without a closing tag holds to the block's end, and tempo 1 is no cue", () => {
    const open = "Раз [rate:1.2]два три";
    expect(cues(open).map((c) => [c.kind, c.to])).toEqual([["rate", open.length]]);
    expect(cues("Раз [rate:1] два")).toEqual([]);
  });

  it("cuts the text into pieces that join back into it", () => {
    expect(spans(text, cues(text)).map((s) => s.text).join("")).toBe(text);
  });
});
