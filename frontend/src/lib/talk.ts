import { getCoded, postCoded, sendCoded } from "./api";

export type SayPhrase = { id: string; text: string; state: "said" | "waiting" | "failed"; first_ms: number | null; seconds: number | null };
export type HeardLine = { start: number; end: number; text: string };
export type LiveHeard = { running: boolean; lines: HeardLine[]; partial: { start: number; text: string } | null; error: string | null };

/** The spoken conversation: the microphone heard live, and what an agent says aloud through the studio. */
export const talkApi = {
  listenStart: (lang: string) => postCoded<{ pid: string }>("/speech/listen", { lang }),
  listenStop: () => sendCoded<unknown>("DELETE", "/speech/listen"),
  heard: () => getCoded<LiveHeard>("/listen/record/live"),
  level: () => getCoded<{ level: number }>("/record/level"),
  session: (id: string) => getCoded<{ session: string; open: boolean; phrases: SayPhrase[] }>(`/speech/say/${encodeURIComponent(id)}`),
  silence: () => sendCoded<unknown>("DELETE", "/speech/say"),
};
