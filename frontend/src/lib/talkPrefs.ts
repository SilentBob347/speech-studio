import { useEffect, useState } from "react";

/** The talk screen's choices, shared by the window and the widget: a change in one shows in the other. */
export type TalkPrefs = {
  style: string;
  lang: string;
  /** the microphone by the studio's name for it; empty is the system's default */
  device: string;
  volume: number;
  muted: boolean;
  /** the speaker the agent's voice plays on, by the browser's device id; empty is the system's default */
  output: string;
};

const KEY = "speech.talk.prefs";
const CHANGED = "speech-talk-prefs";
const DEFAULTS: TalkPrefs = { style: "aura", lang: "auto", device: "", volume: 1, muted: false, output: "" };

export function talkPrefs(): TalkPrefs {
  try {
    return { ...DEFAULTS, ...(JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<TalkPrefs>) };
  } catch {
    return DEFAULTS;
  }
}

export function setTalkPrefs(patch: Partial<TalkPrefs>): void {
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...talkPrefs(), ...patch }));
  } catch {
    return;
  }
  window.dispatchEvent(new Event(CHANGED));
}

/** Calls `cb` whenever the choices change, in this window or another one of the studio. */
export function onTalkPrefs(cb: (p: TalkPrefs) => void): () => void {
  const here = () => cb(talkPrefs());
  const there = (e: StorageEvent) => { if (e.key === KEY) cb(talkPrefs()); };
  window.addEventListener(CHANGED, here);
  window.addEventListener("storage", there);
  return () => { window.removeEventListener(CHANGED, here); window.removeEventListener("storage", there); };
}

export function useTalkPrefs(): TalkPrefs {
  const [prefs, setPrefs] = useState(talkPrefs);
  useEffect(() => onTalkPrefs(setPrefs), []);
  return prefs;
}
