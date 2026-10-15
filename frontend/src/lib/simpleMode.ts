import { useEffect, useState } from "react";

const KEY = "speech-simple-mode";

/** The beginner's mode: the editor walks the steps from text to file and keeps only what they need. */
export function simpleModeOn(): boolean {
  try { return localStorage.getItem(KEY) === "1"; } catch { return false; }
}

export function setSimpleMode(on: boolean): void {
  try { localStorage.setItem(KEY, on ? "1" : "0"); } catch { /* the mode then lasts until the window reloads */ }
  window.dispatchEvent(new CustomEvent<boolean>(KEY, { detail: on }));
}

export function useSimpleMode(): boolean {
  const [on, setOn] = useState(simpleModeOn);
  useEffect(() => {
    const sync = (e: Event) => setOn((e as CustomEvent<boolean>).detail);
    window.addEventListener(KEY, sync);
    return () => window.removeEventListener(KEY, sync);
  }, []);
  return on;
}
