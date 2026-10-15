import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Keyboard, X } from "lucide-react";
import { named } from "../../lib/a11y";
import { openHelp, openSettings } from "../../lib/settingsNav";
import { useStore } from "../../store";
import { useCommands } from "../../lib/commands";
import CommandPalette from "./CommandPalette";
import { matches, typingIn, useShortcuts } from "../../lib/shortcuts";

// Фокус в поле ввода или открытая модалка -> горячие клавиши окна не перехватываем.
function typing(e: KeyboardEvent): boolean {
  return typingIn(e.target) || e.target instanceof HTMLSelectElement;
}

const modalOpen = () => document.querySelector('[aria-modal="true"]') !== null;

// Полноэкранное окно через браузерный Fullscreen API: окно грузится с http://127.0.0.1, IPC Tauri здесь не нужен.
function toggleFullscreen() {
  if (document.fullscreenElement) document.exitFullscreen().catch((e: unknown) => console.warn("[hotkeys] fullscreen exit:", e));
  else document.documentElement.requestFullscreen().catch((e: unknown) => console.warn("[hotkeys] fullscreen:", e));
}

function ShortcutsHelp({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  useEffect(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", h); return () => window.removeEventListener("keydown", h);
  }, [onClose]);
  const keys = useShortcuts();
  const rows: [string, string][] = [
    [keys.modeSpeech, t("hotkeys.modeSpeech")],
    [keys.modeListening, t("hotkeys.modeListening")],
    [keys.new, t("hotkeys.new")],
    [keys.settings, t("hotkeys.settings")],
    [keys.palette, t("hotkeys.palette")],
    [keys.search, t("hotkeys.search")],
    [keys.undo, t("hotkeys.undo")],
    [keys.redo, t("hotkeys.redo")],
    [keys.help, t("hotkeys.helpWindow")],
    [keys.fullscreen, t("hotkeys.fsWindow")],
    [keys.shortcuts, t("hotkeys.help")],
    ["Esc", t("hotkeys.esc")],
  ];
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade" onClick={onClose}>
      <div role="dialog" aria-modal="true" aria-labelledby="hotkeys-title" className="w-[min(92vw,480px)] rounded-xl glass-panel anim-pop overflow-hidden" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center gap-2 px-4 py-3 border-b border-[var(--color-border)]">
          <Keyboard size={16} className="text-[var(--color-accent)]" />
          <span id="hotkeys-title" className="text-[14px] font-semibold">{t("hotkeys.title")}</span>
          <button onClick={onClose} {...named(t("a11y.close"))} className="ml-auto text-[var(--color-muted)] hover:text-[var(--color-text)]"><X size={16} /></button>
        </div>
        <div className="p-2">
          {rows.map(([key, act]) => (
            <div key={key} className="flex items-center justify-between px-3 py-1.5 rounded-lg hover:bg-[var(--color-surface-2)] transition-colors">
              <span className="text-[13px] text-[var(--color-text)]">{act}</span>
              <kbd className="mono text-[11px] px-2 py-0.5 rounded bg-[var(--color-surface-2)] border border-[var(--color-border)] text-[var(--color-muted)] shrink-0 ml-4">{key}</kbd>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

/** The window's own shortcuts: modes, a fresh start, settings, help, full screen and the `?` list of them all. */
export default function Hotkeys() {
  const { t } = useTranslation();
  const [shown, setShown] = useState(false);
  const [palette, setPalette] = useState(false);
  const group = t("palette.groups.window");
  useCommands("hotkeys", [
    { id: "fullscreen", label: t("hotkeys.fsWindow"), group, run: toggleFullscreen },
    { id: "shortcuts", label: t("hotkeys.title"), group, run: () => setShown(true) },
  ]);
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (matches(e, "fullscreen")) { e.preventDefault(); toggleFullscreen(); return; }
      if (matches(e, "palette") && !modalOpen()) { e.preventDefault(); setPalette(true); return; }
      if (typing(e) || modalOpen()) return;
      const s = useStore.getState();
      if (matches(e, "help")) { e.preventDefault(); openHelp(); return; }
      if (matches(e, "settings")) { e.preventDefault(); openSettings("models"); return; }
      if (s.stage !== "ready") { if (matches(e, "shortcuts")) setShown(true); return; }
      if (matches(e, "modeSpeech")) { e.preventDefault(); s.setMode("speech"); }
      else if (matches(e, "modeListening")) { e.preventDefault(); s.setMode("listening"); }
      else if (matches(e, "new")) { e.preventDefault(); s.startFresh(); }
      else if (matches(e, "shortcuts")) setShown(true);
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);
  return (
    <>
      {shown && <ShortcutsHelp onClose={() => setShown(false)} />}
      {palette && <CommandPalette onClose={() => setPalette(false)} />}
    </>
  );
}
