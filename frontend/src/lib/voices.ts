import { useCallback, useEffect, useState } from "react";
import { api, ApiError, type VoiceEntry, type VoiceListing } from "./api";
import { VOICES_CHANGED, useChanged } from "./mcpBridge";

/** A voice name as people read it: the pack's `RU_Female_` prefix and the underscores go. */
export const prettyVoice = (name: string) => name.replace(/^[a-z]{2}_(female|famale|male)_/i, "").replace(/_/g, " ");

const RECENT = "speech.recentVoices";

/** The library voices last picked in this window, newest first. */
export function recentVoices(): string[] {
  try { return (JSON.parse(localStorage.getItem(RECENT) ?? "[]") as string[]).slice(0, 5); } catch { return []; }
}

export function rememberVoice(name: string): void {
  try { localStorage.setItem(RECENT, JSON.stringify([name, ...recentVoices().filter((v) => v !== name)].slice(0, 5))); } catch { /* the list stays as it was */ }
}

/** The library voice an engine that only clones starts with: in the project's language, a favourite first, then one of
 * the pack; an engine that needs the reference text takes only voices that have it. Null when the library has none. */
export function defaultLibraryVoice(listing: VoiceListing | null, lang: string, needsText: boolean): string | null {
  const fit = (listing?.voices ?? []).filter((v) => v.card.kind !== "preset" && (!needsText || !!v.transcript?.trim()) && (!lang || lang === "auto" || v.card.lang === lang));
  const rank = (v: VoiceEntry) => (v.card.favorite ? 0 : v.card.source === "pack" ? 1 : 2);
  return [...fit].sort((a, b) => rank(a) - rank(b))[0]?.name ?? null;
}

export const errorText =(e: unknown) => (e instanceof ApiError ? e.detail || e.code : e instanceof Error ? e.message : String(e));

/** The voice library, read again whenever the studio says the voices changed. */
export function useVoices(): { listing: VoiceListing | null; error: string | null; reload: () => void; patch: (v: VoiceEntry) => void } {
  const [listing, setListing] = useState<VoiceListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => {
    api.voices().then((l) => { setListing(l); setError(null); }, (e: unknown) => setError(errorText(e)));
  }, []);
  useEffect(() => { reload(); }, [reload]);
  useChanged(VOICES_CHANGED, reload);
  const patch = useCallback((v: VoiceEntry) => {
    setListing((cur) => (cur ? { ...cur, voices: cur.voices.map((x) => (x.name === v.name ? v : x)) } : cur));
  }, []);
  return { listing, error, reload, patch };
}
