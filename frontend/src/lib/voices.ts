import { useCallback, useEffect, useState } from "react";
import { api, ApiError, type VoiceEntry, type VoiceListing } from "./api";
import { VOICES_CHANGED, useChanged } from "./mcpBridge";

/** A voice name as people read it: the pack's `RU_Female_` prefix and the underscores go. */
export const prettyVoice = (name: string) => name.replace(/^[a-z]{2}_(female|famale|male)_/i, "").replace(/_/g, " ");

export const errorText = (e: unknown) => (e instanceof ApiError ? e.detail || e.code : e instanceof Error ? e.message : String(e));

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
