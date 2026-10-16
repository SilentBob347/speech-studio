import { getCoded, postCoded, sendCoded } from "./api";

export type SayPhrase = { id: string; text: string; state: "said" | "waiting" | "failed" | "interrupted"; first_ms: number | null; seconds: number | null };
export type HeardLine = { start: number; end: number; text: string };
export type LiveHeard = { running: boolean; lines: HeardLine[]; partial: { start: number; text: string } | null; error: string | null; muted: boolean };

/** The spoken conversation: the microphone heard live, and what an agent says aloud through the studio. */
export const talkApi = {
  listenStart: (lang: string, device?: string) => postCoded<{ pid: string }>("/speech/listen", { lang, device }),
  listenStop: () => sendCoded<unknown>("DELETE", "/speech/listen"),
  mute: (muted: boolean) => postCoded<{ muted: boolean }>("/speech/listen/mute", { muted }),
  type: (text: string) => postCoded<{ ok: boolean }>("/speech/listen/text", { text }),
  heard: () => getCoded<LiveHeard>("/listen/record/live"),
  level: () => getCoded<{ level: number }>("/record/level"),
  session: (id: string) => getCoded<{ session: string; open: boolean; phrases: SayPhrase[] }>(`/speech/say/${encodeURIComponent(id)}`),
  silence: () => sendCoded<unknown>("DELETE", "/speech/say"),
};
