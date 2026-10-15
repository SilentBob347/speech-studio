import { describe, expect, it } from "vitest";
import { comboOf } from "../src/lib/shortcuts";

const press = (key: string, code: string, mods: Partial<Pick<KeyboardEvent, "ctrlKey" | "shiftKey" | "altKey" | "metaKey">> = {}) =>
  ({ key, code, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...mods }) as KeyboardEvent;

describe("comboOf", () => {
  it("names letters and digits by their place, whatever the layout", () => {
    expect(comboOf(press("л", "KeyK", { ctrlKey: true }))).toBe("Ctrl+K");
    expect(comboOf(press("k", "KeyK", { metaKey: true }))).toBe("Ctrl+K");
    expect(comboOf(press("1", "Digit1", { ctrlKey: true }))).toBe("Ctrl+1");
    expect(comboOf(press("Я", "KeyZ", { ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+Z");
  });
  it("keeps typed punctuation as it is, Shift inside it", () => {
    expect(comboOf(press("?", "Slash", { shiftKey: true }))).toBe("?");
    expect(comboOf(press(",", "Comma", { ctrlKey: true }))).toBe("Ctrl+,");
    expect(comboOf(press("б", "Comma", { ctrlKey: true }))).toBe("Ctrl+,");
  });
  it("names other keys and waits while only modifiers are held", () => {
    expect(comboOf(press("F1", "F1"))).toBe("F1");
    expect(comboOf(press("F10", "F10", { shiftKey: true }))).toBe("Shift+F10");
    expect(comboOf(press(" ", "Space", { altKey: true }))).toBe("Alt+Space");
    expect(comboOf(press("Control", "ControlLeft", { ctrlKey: true }))).toBeNull();
  });
});
