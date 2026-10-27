import type { TFunction } from "i18next";

/** The names of timoncool's programs by their id (CONTRACT.md), for "taken from <program>". */
export const PROGRAMS: Record<string, string> = {
  yue2: "YuE2 Studio", minimax: "MiniMax Music3 Studio", ace: "ACE-Step Studio", dub: "Dub Studio",
  speech: "Speech Studio", dungeon: "Dungeon Ultimate", bulka: "Bulka", videosos: "VideoSOS",
};

/** This program's id: a file taken from it comes from another install of Speech Studio, not from this one. */
export const OWN_PROGRAM = "speech";

/** "Taken from <program>" for a component a neighbour gave. */
export const takenFromLabel = (t: TFunction, id: string): string =>
  t(id === OWN_PROGRAM ? "setup.takenFromOwn" : "setup.takenFrom", { app: PROGRAMS[id] ?? id });
