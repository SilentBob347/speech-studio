import { describe, expect, it } from "vitest";
import { wordDiff, words } from "../src/lib/wordDiff";

describe("the words a check compares", () => {
  it("drops tags, pauses and stress marks and reads ё as е", () => {
    expect(words("Зам+ок <|emotion:joy|>ёлка [pause 500ms] Ёж!")).toEqual(["замок", "елка", "еж"]);
  });

  it("aligns a missed, a changed and an extra word", () => {
    expect(wordDiff(words("я принёс тебе круассан"), words("я принес круасан")).filter((s) => s.op !== "same")).toEqual([
      { op: "deleted", expected: "тебе" },
      { op: "substituted", expected: "круассан", heard: "круасан" },
    ]);
    expect(wordDiff(words("я принёс"), words("вот я принес")).filter((s) => s.op !== "same")).toEqual([{ op: "inserted", heard: "вот" }]);
  });
});
