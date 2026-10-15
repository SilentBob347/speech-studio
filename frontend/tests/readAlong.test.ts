import { describe, expect, it } from "vitest";
import { sentenceAt, textWords, wordAt, wordTimes } from "../src/lib/readAlong";

describe("the word that sounds", () => {
  const text = "<|emotion:joy|>Привет, [pause 500ms] м+ир и **солнце**!";
  const word = (r: [number, number] | null) => (r ? text.slice(r[0], r[1]) : null);

  it("takes the words as written and leaves markup out", () => {
    expect(textWords(text).map((r) => word(r))).toEqual(["Привет", "м+ир", "и", "солнце"]);
  });

  it("follows the heard word times when there is one per word", () => {
    const times = wordTimes(text, 3, [["привет", 0, 0.6], ["мир", 1.2, 1.5], ["и", 1.5, 1.6], ["солнце", 1.7, 2.4]]);
    expect([0.1, 1.0, 1.55, 2.0].map((t) => word(wordAt(times, t)))).toEqual(["Привет", "Привет", "и", "солнце"]);
  });

  it("spreads the words over the take by their length otherwise", () => {
    const times = wordTimes(text, 2, null);
    expect(word(wordAt(times, 0))).toBe("Привет");
    expect(word(wordAt(times, 1.99))).toBe("солнце");
    expect(times[times.length - 1].end).toBeCloseTo(2);
  });

  it("finds the sentence around a word", () => {
    const s = "Раз два. Три «четыре»! Пять";
    const cut = (r: [number, number]) => s.slice(r[0], r[1]);
    expect(cut(sentenceAt(s, s.indexOf("два")))).toBe("Раз два.");
    expect(cut(sentenceAt(s, s.indexOf("четыре")))).toBe("Три «четыре»!");
    expect(cut(sentenceAt(s, s.indexOf("Пять")))).toBe("Пять");
  });
});
