import { useEffect, useLayoutEffect, useRef } from "react";

/** What the open editor (a Speech project or a Listening recording) tells the agent's editor_* commands. */
export type OpenEditor = {
  kind: "speech" | "listening";
  pid: string;
  /** What the agent reads: the project's name, its blocks or lines, what is selected and playing. */
  state: () => Record<string, unknown>;
  /** Selects a block or a line by id and brings it into view; false when there is none. */
  select: (id: string) => boolean;
  /** The editor's player, for the agent's player_* commands. */
  player: EditorPlayer;
};

/** What the agent asks the open editor's player to play: a block or a line, from it on, or from a moment. */
export type PlayRequest = { id?: string; on?: boolean; from?: number };

/** Each throws the bridge's error the agent reads when it cannot do what was asked. */
export type EditorPlayer = {
  state: () => Record<string, unknown>;
  play: (asked: PlayRequest) => void;
  pause: () => void;
  seek: (seconds: number) => void;
  set: (settings: { volume?: number; rate?: number }) => void;
};

let current: OpenEditor | null = null;

export const openEditor = (): OpenEditor | null => current;

/** Publishes the editor while the calling component is mounted, always with its latest closures. */
export function usePublishEditor(editor: OpenEditor): void {
  const latest = useRef(editor);
  useLayoutEffect(() => {
    latest.current = editor;
  });
  const { kind, pid } = editor;
  useEffect(() => {
    const published: OpenEditor = {
      kind,
      pid,
      state: () => latest.current.state(),
      select: (id) => latest.current.select(id),
      player: {
        state: () => latest.current.player.state(),
        play: (asked) => latest.current.player.play(asked),
        pause: () => latest.current.player.pause(),
        seek: (seconds) => latest.current.player.seek(seconds),
        set: (settings) => latest.current.player.set(settings),
      },
    };
    current = published;
    return () => {
      if (current === published) current = null;
    };
  }, [kind, pid]);
}
