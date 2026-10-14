import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Keyboard, X } from "lucide-react";
import { named } from "../../lib/a11y";
import { openHelp, openSettings } from "../../lib/settingsNav";
import { useStore } from "../../store";

// Фокус в поле ввода или открытая модалка -> горячие клавиши окна не перехватываем.
function typing(e: KeyboardEvent): boolean {
  const el = e.target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el.isContentEditable;
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
  const rows: [string, string][] = [
    ["Ctrl + 1", t("hotkeys.modeSpeech")],
    ["Ctrl + 2", t("hotkeys.modeListening")],
    ["Ctrl + N", t("hotkeys.new")],
    ["Ctrl + ,", t("hotkeys.settings")],
    ["F1", t("hotkeys.helpWindow")],
    ["F10", t("hotkeys.fsWindow")],
    ["?", t("hotkeys.help")],
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
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key === "F10") { e.preventDefault(); toggleFullscreen(); return; }
      if (typing(e) || modalOpen()) return;
      const s = useStore.getState();
      const ctrl = e.ctrlKey || e.metaKey;
      if (e.key === "F1") { e.preventDefault(); openHelp(); return; }
      if (ctrl && e.key === ",") { e.preventDefault(); openSettings("models"); return; }
      if (s.stage !== "ready") { if (e.key === "?") setShown(true); return; }
      if (ctrl && e.key === "1") { e.preventDefault(); s.setMode("speech"); }
      else if (ctrl && e.key === "2") { e.preventDefault(); s.setMode("listening"); }
      else if (ctrl && e.key.toLowerCase() === "n") { e.preventDefault(); s.startFresh(); }
      else if (e.key === "?") setShown(true);
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);
  return shown ? <ShortcutsHelp onClose={() => setShown(false)} /> : null;
}
