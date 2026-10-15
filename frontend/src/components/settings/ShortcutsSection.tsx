import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { RotateCcw } from "lucide-react";
import { named } from "../../lib/a11y";
import { SHORTCUTS, SHORTCUT_IDS, comboOf, setBinding, takenBy, useShortcuts, type ShortcutId } from "../../lib/shortcuts";

const LABELS = {
  palette: "hotkeys.palette", modeSpeech: "hotkeys.modeSpeech", modeListening: "hotkeys.modeListening", new: "hotkeys.new",
  settings: "hotkeys.settings", help: "hotkeys.helpWindow", fullscreen: "hotkeys.fsWindow", shortcuts: "hotkeys.help",
  search: "hotkeys.search", undo: "hotkeys.undo", redo: "hotkeys.redo",
} as const satisfies Record<ShortcutId, string>;

/** The window's shortcuts, each one set by pressing the new keys; Escape gives up, the arrow puts back its own. */
export default function ShortcutsSection() {
  const { t } = useTranslation();
  const keys = useShortcuts();
  const [recording, setRecording] = useState<ShortcutId | null>(null);
  const [clash, setClash] = useState<{ id: ShortcutId; with: ShortcutId } | null>(null);
  useEffect(() => {
    if (!recording) return;
    const take = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.key === "Escape") { setRecording(null); return; }
      const combo = comboOf(e);
      if (!combo) return;
      const other = takenBy(combo, recording);
      if (other) { setClash({ id: recording, with: other }); return; }
      setClash(null);
      setBinding(recording, combo);
      setRecording(null);
    };
    window.addEventListener("keydown", take, { capture: true });
    return () => window.removeEventListener("keydown", take, { capture: true });
  }, [recording]);
  return (
    <div className="max-w-2xl rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)]/40">
      <div className="px-3 py-2.5 border-b border-[var(--color-border)]">
        <span className="block text-[13px] font-medium">{t("hotkeys.title")}</span>
        <span className="block text-[11px] leading-snug text-[var(--color-muted)]">{t("hotkeys.setHint")}</span>
      </div>
      <ul className="divide-y divide-[var(--color-border)]">
        {SHORTCUT_IDS.map((id) => (
          <li key={id} className="flex items-center gap-2 px-3 py-1.5" data-mcp-row={`shortcut ${t(LABELS[id])}: ${keys[id]}`}>
            <span className="flex-1 min-w-0 text-[12px]">{t(LABELS[id])}</span>
            {clash?.id === id && recording === id && <span role="alert" className="text-[11px] text-[var(--color-warn)]">{t("hotkeys.taken", { action: t(LABELS[clash.with]) })}</span>}
            <button onClick={() => { setClash(null); setRecording(recording === id ? null : id); }} aria-pressed={recording === id}
              className={`min-w-24 px-2 py-0.5 rounded border mono text-[11px] ${recording === id ? "border-[var(--color-accent)] text-[var(--color-accent)]" : "border-[var(--color-border)] text-[var(--color-text)] hover:border-[var(--color-accent)]"}`}
              {...named(t("hotkeys.change", { action: t(LABELS[id]) }))}>{recording === id ? t("hotkeys.pressKeys") : keys[id]}</button>
            <button onClick={() => setBinding(id, null)} disabled={keys[id] === SHORTCUTS[id]} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30"
              {...named(t("hotkeys.reset", { keys: SHORTCUTS[id] }))}><RotateCcw size={12} /></button>
          </li>
        ))}
      </ul>
    </div>
  );
}
