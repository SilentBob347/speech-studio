import { aloudPlayback } from "./livePlayback";

const CHANNEL = "speech-talk-feed";
const HELLO_EVERY_MS = 1000;
const GONE_AFTER_MS = 3000;
/** The bands the widget is sent; a look wanting fewer folds them. */
export const FED_BANDS = 32;

type Message = { kind: "hello" } | { kind: "level"; speaking: boolean; level: number; bands: number[] };

/** The main window feeds the talk widget how loud the agent's voice is: the widget only draws, the sound plays here,
 * once. Fed while a widget has said hello within the last few seconds. */
export function serveTalkFeed(): () => void {
  const channel = new BroadcastChannel(CHANNEL);
  let heard = 0;
  channel.onmessage = (event: MessageEvent<Message>) => {
    if (event.data.kind === "hello") heard = performance.now();
  };
  const id = window.setInterval(() => {
    if (performance.now() - heard > GONE_AFTER_MS) return;
    const player = aloudPlayback();
    channel.postMessage({ kind: "level", speaking: player?.speaking ?? false, level: player?.level() ?? 0, bands: player?.bands(FED_BANDS) ?? [] } satisfies Message);
  }, 40);
  return () => { window.clearInterval(id); channel.close(); };
}

/** The widget's side: says hello while open and keeps the last level the main window sent. */
export function receiveTalkFeed(onLevel: (speaking: boolean, level: number, bands: number[]) => void): () => void {
  const channel = new BroadcastChannel(CHANNEL);
  channel.onmessage = (event: MessageEvent<Message>) => {
    if (event.data.kind === "level") onLevel(event.data.speaking, event.data.level, event.data.bands);
  };
  const hello = () => channel.postMessage({ kind: "hello" } satisfies Message);
  hello();
  const id = window.setInterval(hello, HELLO_EVERY_MS);
  return () => { window.clearInterval(id); channel.close(); };
}
