import { useEffect, useState } from "react";

/** An action of the window the command palette lists: its name, where it comes from, and what it does. */
export type Command = { id: string; label: string; group: string; run: () => void; disabled?: boolean };

const sources = new Map<string, Command[]>();
const listeners = new Set<() => void>();
const tell = () => listeners.forEach((l) => l());

/** Puts a part of the window's commands into the palette while the part is shown. */
export function useCommands(source: string, commands: Command[]): void {
  const key = commands.map((c) => `${c.id}:${c.label}:${c.disabled ? 1 : 0}`).join("|");
  useEffect(() => {
    sources.set(source, commands);
    tell();
    return () => { sources.delete(source); tell(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, key]);
  useEffect(() => { sources.set(source, commands); });
}

/** Every command the window offers now. */
export function useAllCommands(): Command[] {
  const [, setTick] = useState(0);
  useEffect(() => {
    const l = () => setTick((n) => n + 1);
    listeners.add(l);
    return () => { listeners.delete(l); };
  }, []);
  return [...sources.values()].flat();
}
