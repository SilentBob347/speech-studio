import { useEffect, useLayoutEffect, useRef } from "react";

/** What the open editor (a Speech project or a Listening recording) tells the agent's editor_* commands. */
export type OpenEditor = {
  kind: "speech" | "listening";
  pid: string;
  /** What the agent reads: the project's name, its blocks or lines, what is selected and playing. */
  state: () => Record<string, unknown>;
  /** Selects a block or a line by id and brings it into view; false when there is none. */
  select: (id: string) => boolean;
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
    const published: OpenEditor = { kind, pid, state: () => latest.current.state(), select: (id) => latest.current.select(id) };
    current = published;
    return () => {
      if (current === published) current = null;
    };
  }, [kind, pid]);
}
