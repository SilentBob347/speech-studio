import { useEffect, useState } from "react";
import i18n from "i18next";

/** The talk screen's choices, shared by the window and the widget: a change in one shows in the other. */
export type TalkPrefs = {
  style: string;
  lang: string;
  /** the recogniser: parakeet, nemotron or whisper; empty is the one in the settings */
  asr: string;
  /** the microphone by the studio's name for it; empty is the system's default */
  device: string;
  volume: number;
  muted: boolean;
  /** the speaker the agent's voice plays on, by the browser's device id; empty is the system's default */
  output: string;
};

/** Only what the person chose is kept, so a default they never touched can still follow the studio. */
const KEY = "speech.talk.chosen";
const CHANGED = "speech-talk-prefs";
const DEFAULTS: TalkPrefs = { style: "aura", lang: "auto", asr: "", device: "", volume: 1, muted: false, output: "" };

/** Until the person picks one, the language spoken is the studio's own: told nothing, a recogniser guesses the language
 * of every phrase afresh and gets short ones wrong. */
function defaults(): TalkPrefs {
  return { ...DEFAULTS, lang: (i18n.resolvedLanguage ?? i18n.language ?? "auto").slice(0, 2) };
}

function chosen(): Partial<TalkPrefs> {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<TalkPrefs>;
  } catch {
    return {};
  }
}

export function talkPrefs(): TalkPrefs {
  return { ...defaults(), ...chosen() };
}

export function setTalkPrefs(patch: Partial<TalkPrefs>): void {
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...chosen(), ...patch }));
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
