import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AudioLines, Bot, ChevronDown, Ear, HelpCircle, Loader2, Plus, ScrollText, Settings, type LucideIcon } from "lucide-react";
import { LANGS, setLang, type Lang } from "../../lib/i18n";
import { OPEN_HELP_EVENT, OPEN_SETTINGS_EVENT, SETTINGS_SECTIONS } from "../../lib/settingsNav";
import { bridgeError, takeArgs, textArg, useBridgeCommand } from "../../lib/mcpBridge";
import { useStore, type Mode } from "../../store";
import SettingsModal from "../settings/SettingsModal";
import WhatsNew from "../WhatsNew";
import ProxySection from "../ProxySection";
import AgentPanel from "../AgentPanel";
import ModelsSection from "../setup/ModelsSection";
import PresetsSection from "../setup/PresetsSection";
import HelpModal from "./HelpModal";

function LanguageSwitcher() {
  const { t, i18n } = useTranslation();
  return (
    <select value={i18n.language as Lang} onChange={(e) => setLang(e.target.value as Lang)} aria-label={t("prefs.uiLanguage")}
      className="bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-sm">
      {LANGS.map((l) => <option key={l} value={l}>{l.toUpperCase()}</option>)}
    </select>
  );
}

const MODE_ITEMS: readonly { id: Mode; icon: LucideIcon; label: "modes.speech" | "modes.listening"; hint: "modes.speechHint" | "modes.listeningHint" }[] = [
  { id: "speech", icon: AudioLines, label: "modes.speech", hint: "modes.speechHint" },
  { id: "listening", icon: Ear, label: "modes.listening", hint: "modes.listeningHint" },
];

function ModeSwitcher() {
  const { t } = useTranslation();
  const mode = useStore((s) => s.mode);
  const setMode = useStore((s) => s.setMode);
  return (
    <div role="tablist" aria-label={t("modes.label")} className="flex items-center gap-0.5 rounded-lg bg-[var(--color-surface-2)] border border-[var(--color-border)] p-0.5">
      {MODE_ITEMS.map(({ id, icon: Icon, label, hint }) => {
        const active = mode === id;
        return (
          <button key={id} type="button" role="tab" aria-selected={active} onClick={() => setMode(id)} title={t(hint)}
            className={`flex h-7 items-center gap-1.5 rounded-md px-3 text-[12px] font-medium transition-colors ${active ? "bg-[var(--color-surface)] text-[var(--color-text)] shadow-sm" : "text-[var(--color-muted)] hover:text-[var(--color-text)]"}`}>
            <Icon size={14} className={active ? "text-[var(--color-accent)]" : ""} />
            <span>{t(label)}</span>
          </button>
        );
      })}
    </div>
  );
}

// Статус-строка в шапке: текущее действие приложения + разворот по клику в полный журнал (что делалось).
function StatusBar() {
  const { t } = useTranslation();
  const activities = useStore((s) => s.activities);
  const progress = useStore((s) => s.progress);
  const firstRun = useStore((s) => s.stage === "setup");
  const [open, setOpen] = useState(false);
  const last = activities[activities.length - 1];
  const busy = progress.pct != null || (!!progress.stage && !["", "done", "error"].includes(progress.stage));
  const text = busy ? (progress.msg || last?.text || t("status.working")) : (last?.text || t(firstRun ? "status.setupNeeded" : "status.idle"));
  const errored = !busy && last?.kind === "error";
  const fmt = (ms: number) => new Date(ms).toLocaleTimeString();
  return (
    <div className="relative flex-1 min-w-0 flex justify-center px-3">
      <span className="sr-only" role="status" aria-live="polite">{text}</span>
      <button onClick={() => setOpen((o) => !o)} title={t("status.log")} aria-expanded={open}
        className="inline-flex items-center gap-2 max-w-full px-3 py-1 rounded-md text-[12px] text-[var(--color-muted)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-2)] transition-colors">
        {busy
          ? <Loader2 size={13} className="animate-spin text-[var(--color-accent)] shrink-0" />
          : <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${errored ? "bg-[var(--color-warn)]" : "bg-[var(--color-accent)]"}`} />}
        <span className="truncate">{text}</span>
        {activities.length > 0 && <ChevronDown size={13} className={`shrink-0 transition-transform ${open ? "rotate-180" : ""}`} />}
      </button>
      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div role="log" aria-label={t("status.log")} className="absolute top-full mt-1 left-1/2 -translate-x-1/2 z-50 w-[min(560px,92vw)] max-h-[60vh] overflow-y-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] shadow-xl p-1.5">
            <div className="flex items-center justify-between px-2 py-1 mb-1 border-b border-[var(--color-border)]">
              <span className="text-[11px] uppercase tracking-wide text-[var(--color-muted)] inline-flex items-center gap-1.5"><ScrollText size={13} />{t("status.log")}</span>
              {activities.length > 0 && <button onClick={() => useStore.setState({ activities: [] })} className="text-[11px] text-[var(--color-muted)] hover:text-[var(--color-text)] transition-colors">{t("status.clear")}</button>}
            </div>
            {activities.length === 0 && <div className="text-[11px] text-[var(--color-muted)]/60 py-3 text-center">—</div>}
            {[...activities].reverse().map((a, i) => (
              <div key={i} className="flex items-start gap-2 px-2 py-1 text-[12px]">
                <span className="mono text-[10px] text-[var(--color-muted)]/70 tabnum shrink-0 mt-[3px]">{fmt(a.t)}</span>
                {a.kind === "agent"
                  ? <Bot size={12} aria-label={t("bridge.agent")} className="shrink-0 mt-[3px] text-[var(--color-accent)]" />
                  : <span className={`w-1.5 h-1.5 rounded-full shrink-0 mt-1.5 ${a.kind === "error" ? "bg-[var(--color-warn)]" : a.kind === "done" ? "bg-[var(--color-accent)]" : "bg-[var(--color-muted)]"}`} />}
                <span className={`leading-snug break-words min-w-0 ${a.kind === "error" ? "text-[var(--color-warn)]" : "text-[var(--color-text)]"}`}>{a.text}</span>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

const settingsRequest = (target: string) => (cur: { request: number } | null) => ({ target, request: (cur?.request ?? 0) + 1 });

export default function TopBar() {
  const { t } = useTranslation();
  const ready = useStore((s) => s.stage === "ready");
  const startFresh = useStore((s) => s.startFresh);
  // Открытые настройки: цель «раздел[:часть]» и номер запроса — повторный запрос той же цели снова ведёт к ней.
  const [settings, setSettings] = useState<{ target: string; request: number } | null>(null);
  const [help, setHelp] = useState(false);
  const openSettingsAt = (target: string) => setSettings(settingsRequest(target));
  useEffect(() => {
    const open = (e: Event) => setSettings(settingsRequest((e as CustomEvent<string>).detail));
    const openHelp = () => setHelp(true);
    window.addEventListener(OPEN_SETTINGS_EVENT, open);
    window.addEventListener(OPEN_HELP_EVENT, openHelp);
    return () => {
      window.removeEventListener(OPEN_SETTINGS_EVENT, open);
      window.removeEventListener(OPEN_HELP_EVENT, openHelp);
    };
  }, []);
  useBridgeCommand("open_settings", (args) => {
    takeArgs("ui_open_settings", args, ["section"]);
    const section = textArg("ui_open_settings", args, "section") ?? "models";
    if (!(SETTINGS_SECTIONS as readonly string[]).includes(section)) throw bridgeError("bad_value", { command: "ui_open_settings", field: "section", expected: SETTINGS_SECTIONS.join(", ") });
    openSettingsAt(section);
    return { open: "settings", section };
  });
  useBridgeCommand("open_help", (args) => { takeArgs("ui_open_help", args, []); setHelp(true); return { open: "help" }; });
  return (
    <header className="flex items-center gap-3 px-5 h-14 border-b border-[var(--color-border)] bg-[var(--color-surface)]">
      <span className="flex items-center gap-2 font-bold tracking-tight shrink-0">
        <img src="/favicon.svg" alt="" width={20} height={20} className="rounded-[5px] shadow-[0_0_10px_rgba(198,242,78,0.25)]" />
        {t("app.name")}
        <span className="font-normal text-[11px] text-[var(--color-muted)] tracking-normal" title={t("app.name") + " v" + __APP_VERSION__}>v{__APP_VERSION__}</span>
      </span>
      {ready && <ModeSwitcher />}
      <StatusBar />
      <div className="flex items-center gap-2 shrink-0">
        <div id="dock-slot" className="flex items-center gap-2 shrink-0" />
        {ready && (
          <button onClick={startFresh} title={t("nav.newTitle")}
            className="inline-flex items-center gap-1 px-3 py-1.5 rounded-lg bg-[var(--color-surface-2)] border border-[var(--color-border)] text-[12px] font-bold text-[var(--color-text)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors shadow-sm">
            <Plus size={13} />{t("nav.new")}
          </button>
        )}
        <WhatsNew />
        <button onClick={() => setHelp(true)} title={t("help.title")} aria-label={t("help.title")}
          className="p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] transition-colors"><HelpCircle size={18} /></button>
        <button onClick={() => openSettingsAt("models")} title={t("prefs.title")} aria-label={t("prefs.title")}
          className="p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)] transition-colors"><Settings size={18} /></button>
        <LanguageSwitcher />
      </div>
      {help && <HelpModal onClose={() => setHelp(false)} />}
      {settings !== null && (
        <SettingsModal target={settings.target} request={settings.request} onClose={() => setSettings(null)}
          panes={{ models: <><PresetsSection /><ModelsSection /></>, cloud: <ModelsSection part="cloud" />, network: <ProxySection />, agent: <AgentPanel /> }} />
      )}
    </header>
  );
}
