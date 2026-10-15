import { describe as suite, expect, it } from "vitest";
import { NO_ATTRS, describe } from "../src/lib/voiceAttributes";

suite("describe", () => {
  it("puts the chosen attributes into an English description", () => {
    expect(describe({ ...NO_ATTRS, age: "middle", gender: "male", pitch: "low", timbre: ["warm", "husky"], manner: ["calm"], pace: "slow", accent: "British" }))
      .toBe("A middle-aged male voice, low-pitched, with a warm and husky timbre. Calm delivery; speaks slowly. British accent.");
  });
  it("says an before a vowel and leaves out what is not chosen", () => {
    expect(describe({ ...NO_ATTRS, age: "senior", gender: "female" })).toBe("An elderly female voice.");
    expect(describe(NO_ATTRS)).toBe("A voice.");
    expect(describe({ ...NO_ATTRS, age: "senior", gender: "male", timbre: ["deep"], manner: ["calm"] })).toBe("An elderly male voice, with a deep timbre. Calm delivery.");
  });
});
