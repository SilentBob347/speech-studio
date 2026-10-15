import { useEffect, useState } from "react";

/** The window's shortcuts by action, as they come; the user can set others in Language and sounds. */
export const SHORTCUTS = {
  palette: "Ctrl+K",
  modeSpeech: "Ctrl+1",
  modeListening: "Ctrl+2",
  new: "Ctrl+N",
  settings: "Ctrl+,",
  help: "F1",
  fullscreen: "F10",
  shortcuts: "?",
  search: "Ctrl+F",
  undo: "Ctrl+Z",
  redo: "Ctrl+Shift+Z",
} as const;
export type ShortcutId = keyof typeof SHORTCUTS;
export const SHORTCUT_IDS = Object.keys(SHORTCUTS) as ShortcutId[];

const KEY = "speech-shortcuts";
const CHANGED = "speech:shortcuts-changed";

function own(): Partial<Record<ShortcutId, string>> {
  try { return JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Record<ShortcutId, string>>; } catch { return {}; }
}

export function binding(id: ShortcutId): string {
  return own()[id] ?? SHORTCUTS[id];
}

/** Sets an action's keys; null gives it back its own. */
export function setBinding(id: ShortcutId, combo: string | null): void {
  const all = own();
  if (combo === null || combo === SHORTCUTS[id]) delete all[id];
  else all[id] = combo;
  try { localStorage.setItem(KEY, JSON.stringify(all)); } catch { /* the keys then last until the window reloads */ }
  window.dispatchEvent(new Event(CHANGED));
}

/** The action that already has these keys, other than `id`. */
export function takenBy(combo: string, id: ShortcutId): ShortcutId | null {
  return SHORTCUT_IDS.find((other) => other !== id && binding(other) === combo) ?? null;
}

const MODIFIERS = new Set(["Control", "Shift", "Alt", "Meta"]);
// a key that types a letter of another alphabet in this layout is named by its place on an English keyboard
const PLACES: Record<string, string> = { Comma: ",", Period: ".", Slash: "/", Semicolon: ";", Quote: "'", BracketLeft: "[", BracketRight: "]", Backquote: "`", Minus: "-", Equal: "=", Backslash: "\\" };

/** The keys pressed as a combo like "Ctrl+Shift+Z": letters and digits by their place on the keyboard, so any layout
 * gives the same combo; other characters as typed, their Shift already in them. Only modifiers held: null. */
export function comboOf(e: KeyboardEvent): string | null {
  if (MODIFIERS.has(e.key)) return null;
  const letter = /^Key([A-Z])$/.exec(e.code)?.[1] ?? /^Digit(\d)$/.exec(e.code)?.[1];
  const typed = e.key.length === 1 && e.key.charCodeAt(0) > 127 ? PLACES[e.code] ?? e.key : e.key;
  const name = letter ?? (typed === " " ? "Space" : typed);
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey && (letter !== undefined || name.length > 1)) parts.push("Shift");
  parts.push(name.length === 1 ? name.toUpperCase() : name);
  return parts.join("+");
}

export function matches(e: KeyboardEvent, id: ShortcutId): boolean {
  return comboOf(e) === binding(id);
}

/** The current keys of every action, kept fresh when they change. */
export function useShortcuts(): Record<ShortcutId, string> {
  const read = () => Object.fromEntries(SHORTCUT_IDS.map((id) => [id, binding(id)])) as Record<ShortcutId, string>;
  const [all, setAll] = useState(read);
  useEffect(() => {
    const sync = () => setAll(read());
    window.addEventListener(CHANGED, sync);
    return () => window.removeEventListener(CHANGED, sync);
  }, []);
  return all;
}
