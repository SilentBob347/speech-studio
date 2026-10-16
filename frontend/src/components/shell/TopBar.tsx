import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AudioLines, Bot, ChevronDown, Ear, HelpCircle, Loader2, MoreHorizontal, Plus, Radio, ScrollText, Settings, Users, type LucideIcon } from "lucide-react";
import { LANGS, setLang, type Lang } from "../../lib/i18n";
import { OPEN_HELP_EVENT, OPEN_SETTINGS_EVENT, SETTINGS_SECTIONS } from "../../lib/settingsNav";
import { JOURNAL_CHANGED, bridgeError, takeArgs, textArg, useBridgeCommand, useChanged } from "../../lib/mcpBridge";
import { errorText } from "../../lib/voices";
import { useCommands, type Command } from "../../lib/commands";
import { setSimpleMode, useSimpleMode } from "../../lib/simpleMode";
import { useShortcuts } from "../../lib/shortcuts";
import { useStore, type Activity, type Mode } from "../../store";
import type { JobKind } from "../../lib/api";
import en from "../../locales/en.json";
import SettingsModal from "../settings/SettingsModal";
import WhatsNew from "../WhatsNew";
import ProxySection from "../ProxySection";
import AgentPanel from "../AgentPanel";
import CloudProviders from "../cloud/CloudProviders";
import ModelsSection from "../setup/ModelsSection";
import PresetsSection from "../setup/PresetsSection";
import HelpModal from "./HelpModal";
import VoiceLibrary from "../voices/VoiceLibrary";
import AssistantPanel from "../AssistantPanel";

function LanguageSwitcher() {
  const { t, i18n } = useTranslation();
  return (
    <select value={i18n.language as Lang} onChange={(e) => setLang(e.target.value as Lang)} aria-label={t("prefs.uiLanguage")}
      className="bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-sm">
      {LANGS.map((l) => <option key={l} value={l}>{l.toUpperCase()}</option>)}
    </select>
  );
}

const MODE_ITEMS: readonly { id: Mode; icon: LucideIcon; label: "modes.speech" | "modes.listening" | "modes.talk"; hint: "modes.speechHint" | "modes.listeningHint" | "modes.talkHint" }[] = [
  { id: "speech", icon: AudioLines, label: "modes.speech", hint: "modes.speechHint" },
  { id: "listening", icon: Ear, label: "modes.listening", hint: "modes.listeningHint" },
  { id: "talk", icon: Radio, label: "modes.talk", hint: "modes.talkHint" },
];

function ModeSwitcher() {
  const { t } = useTranslation();
  const keys = useShortcuts();
  const mode = useStore((s) => s.mode);
  const setMode = useStore((s) => s.setMode);
  return (
    <div role="tablist" aria-label={t("modes.label")} className="flex items-center gap-0.5 rounded-lg bg-[var(--color-surface-2)] border border-[var(--color-border)] p-0.5">
      {MODE_ITEMS.map(({ id, icon: Icon, label, hint }) => {
        const active = mode === id;
        return (
          <button key={id} type="button" role="tab" aria-selected={active} onClick={() => setMode(id)} title={t(hint, { keys: id === "speech" ? keys.modeSpeech : id === "listening" ? keys.modeListening : "" })} aria-label={t(label)}
            className={`flex h-7 items-center gap-1.5 rounded-md px-3 text-[12px] font-medium transition-colors ${active ? "bg-[var(--color-surface)] text-[var(--color-text)] shadow-sm" : "text-[var(--color-muted)] hover:text-[var(--color-text)]"}`}>
            <Icon size={14} className={active ? "text-[var(--color-accent)]" : ""} />
            <span className="hidden md:inline">{t(label)}</span>
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
  const loadJournal = useStore((s) => s.loadJournal);
  const reload = useCallback(() => {
    loadJournal().catch((problem: unknown) => {
      useStore.setState((s) => ({ activities: [...s.activities, { t: Date.now(), text: t("status.journalFailed", { error: errorText(problem) }), kind: "error" }] }));
    });
  }, [loadJournal, t]);
  useEffect(reload, [reload]);
  useChanged(JOURNAL_CHANGED, reload);
  const words = (a: Activity) => {
    if (!a.verb) return a.text;
    const verb = a.verb in en.journal.verb ? t(`journal.verb.${a.verb as keyof typeof en.journal.verb}`) : a.verb;
    const kind = a.noun && a.noun in en.journal.kind ? t(`journal.kind.${a.noun as keyof typeof en.journal.kind}`) : (a.noun ?? "");
    return t("journal.line", { verb, kind, target: a.target ? ` ${t("journal.quoted", { name: a.target })}` : "" });
  };
  const progress = useStore((s) => s.progress);
  const firstRun = useStore((s) => s.stage === "setup");
  const [open, setOpen] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const tick = window.setInterval(() => setNow(Date.now()), 2000);
    return () => window.clearInterval(tick);
  }, []);
  useEffect(() => {
    if (!open) return;
    const close = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, [open]);
  const [seen, setSeen] = useState(0);
  const last = activities[activities.length - 1];
  const busy = progress.pct != null || (!!progress.stage && !["", "done", "error"].includes(progress.stage));
  const stageText = progress.stage in en.jobs.kind ? t(`jobs.kind.${progress.stage as JobKind}`) : t("status.working");
  const eta = (() => {
    const left = progress.eta;
    if (left == null) return "";
    return ` · ${t("status.eta", { time: left >= 3600 ? `${Math.floor(left / 3600)}:${String(Math.floor((left % 3600) / 60)).padStart(2, "0")}:${String(left % 60).padStart(2, "0")}` : `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}` })}`;
  })();
  const busyText = `${progress.msg || stageText}${progress.pct != null ? ` · ${progress.pct}%` : ""}${eta}`;
  // a finished step stays in the bar for a moment; an error stays until the log is opened
  const errored = !busy && last?.kind === "error" && last.t > seen;
  const recent = !!last && (errored || now - last.t < 10_000);
  const text = busy ? busyText : (recent && last ? words(last) : t(firstRun ? "status.setupNeeded" : "status.idle"));
  const fmt = (ms: number) => new Date(ms).toLocaleTimeString();
  return (
    <div className="relative flex-1 min-w-0 flex justify-center px-3">
      <span className="sr-only" role="status" aria-live="polite">{text}</span>
      <button onClick={() => { setOpen((o) => !o); setSeen(Date.now()); }} title={t("status.log")} aria-label={`${t("status.log")}: ${text}`} aria-expanded={open}
        className="inline-flex items-center gap-2 max-w-full px-3 py-1 rounded-md text-[12px] text-[var(--color-muted)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-2)] transition-colors">
        {busy
          ? <Loader2 size={13} className="animate-spin text-[var(--color-accent)] shrink-0" />
          : <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${errored ? "bg-[var(--color-warn)]" : "bg-[var(--color-accent)]"}`} />}
        <span className="truncate hidden sm:inline">{text}</span>
        {activities.length > 0 && <ChevronDown size={13} className={`shrink-0 transition-transform ${open ? "rotate-180" : ""}`} />}
      </button>
      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div role="log" aria-label={t("status.log")} className="fixed inset-x-2 top-14 sm:absolute sm:inset-x-auto sm:top-full sm:mt-1 sm:left-1/2 sm:-translate-x-1/2 z-50 sm:w-[min(560px,92vw)] max-h-[60vh] overflow-y-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] shadow-xl p-1.5">
            <div className="flex items-center justify-between px-2 py-1 mb-1 border-b border-[var(--color-border)]">
              <span className="text-[11px] uppercase tracking-wide text-[var(--color-muted)] inline-flex items-center gap-1.5"><ScrollText size={13} />{t("status.log")}</span>
              {activities.length > 0 && <button onClick={() => { useStore.getState().clearJournal().catch((problem: unknown) => useStore.setState((s) => ({ activities: [...s.activities, { t: Date.now(), text: errorText(problem), kind: "error" }] }))); }} className="text-[11px] text-[var(--color-muted)] hover:text-[var(--color-text)] transition-colors">{t("status.clear")}</button>}
            </div>
            {activities.length === 0 && <div className="text-[11px] text-[var(--color-muted)]/60 py-3 text-center">—</div>}
            {[...activities].reverse().map((a, i) => (
              <div key={i} className="flex items-start gap-2 px-2 py-1 text-[12px]">
                <span className="mono text-[10px] text-[var(--color-muted)]/70 tabnum shrink-0 mt-[3px]">{fmt(a.t)}</span>
                {a.kind === "agent"
                  ? <Bot size={12} aria-label={t("bridge.agent")} className="shrink-0 mt-[3px] text-[var(--color-accent)]" />
                  : <span className={`w-1.5 h-1.5 rounded-full shrink-0 mt-1.5 ${a.kind === "error" ? "bg-[var(--color-warn)]" : a.kind === "done" ? "bg-[var(--color-accent)]" : "bg-[var(--color-muted)]"}`} />}
                <span className={`leading-snug break-words min-w-0 ${a.kind === "error" ? "text-[var(--color-warn)]" : "text-[var(--color-text)]"}`}>{words(a)}</span>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

const settingsRequest = (target: string) => (cur: { request: number } | null) => ({ target, request: (cur?.request ?? 0) + 1 });

/** The header's actions on a narrow screen, in one menu: the bar keeps the logo, the mode and the status. */
function MoreMenu({ onVoices, onAssistant, onNew, onHelp, onSettings }: { onVoices: () => void; onAssistant: () => void; onNew: () => void; onHelp: () => void; onSettings: () => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open) return;
    const close = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, [open]);
  const item = (Icon: LucideIcon, label: string, act: () => void) => (
    <button role="menuitem" onClick={() => { setOpen(false); act(); }}
      className="flex w-full items-center gap-3 px-3 py-3 rounded-md text-[14px] text-[var(--color-text)] hover:bg-[var(--color-surface-2)]">
      <Icon size={16} className="text-[var(--color-muted)]" />{label}
    </button>
  );
  return (
    <div className="relative md:hidden shrink-0">
      <button onClick={() => setOpen((o) => !o)} aria-haspopup="menu" aria-expanded={open} aria-label={t("nav.more")} title={t("nav.more")}
        className="grid place-items-center w-11 h-11 rounded-lg text-[var(--color-text)] hover:bg-[var(--color-surface-2)]"><MoreHorizontal size={20} /></button>
      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div role="menu" aria-label={t("nav.more")} className="absolute right-0 top-full mt-1 z-50 w-64 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-xl p-1.5">
            {item(Users, t("voices.open"), onVoices)}
            {item(Bot, t("chat.title"), onAssistant)}
            {item(Plus, t("nav.new"), onNew)}
            {item(HelpCircle, t("help.title"), onHelp)}
            {item(Settings, t("prefs.title"), onSettings)}
            <div className="px-3 py-2"><LanguageSwitcher /></div>
          </div>
        </>
      )}
    </div>
  );
}

export default function TopBar() {
  const { t } = useTranslation();
  const ready = useStore((s) => s.stage === "ready");
  const startFresh = useStore((s) => s.startFresh);
  // Открытые настройки: цель «раздел[:часть]» и номер запроса — повторный запрос той же цели снова ведёт к ней.
  const [settings, setSettings] = useState<{ target: string; request: number } | null>(null);
  const [help, setHelp] = useState(false);
  const [voices, setVoices] = useState(false);
  const [assistant, setAssistant] = useState(false);
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
  const group = t("palette.groups.window");
  const keys = useShortcuts();
  const setMode = useStore((s) => s.setMode);
  const simple = useSimpleMode();
  useCommands("topbar", [
    ...(ready ? [
      { id: "mode-speech", label: t("hotkeys.modeSpeech"), group, run: () => setMode("speech") },
      { id: "mode-listening", label: t("hotkeys.modeListening"), group, run: () => setMode("listening") },
      { id: "new", label: t("nav.newTitle", { keys: keys.new }), group, run: startFresh },
      { id: "voices", label: t("voices.title"), group, run: () => setVoices(true) },
      { id: "assistant", label: t("chat.title"), group, run: () => setAssistant((open) => !open) },
    ] : []),
    { id: "help", label: t("help.title"), group, run: () => setHelp(true) },
    { id: "simple", label: simple ? t("simple.turnOff") : t("simple.turnOn"), group, run: () => setSimpleMode(!simple) },
    ...SETTINGS_SECTIONS.map((section): Command => ({ id: `settings-${section}`, label: `${t("prefs.title")}: ${t(`prefs.sections.${section}`)}`, group, run: () => openSettingsAt(section) })),
  ]);
  return (
    <header className="flex items-center gap-2 px-3 md:gap-3 md:px-5 h-14 border-b border-[var(--color-border)] bg-[var(--color-surface)]">
      <span className="flex items-center gap-2 font-bold tracking-tight shrink-0">
        <img src="/favicon.svg" alt="" width={20} height={20} className="rounded-[5px] shadow-[0_0_10px_color-mix(in_oklab,var(--color-accent)_30%,transparent)]" />
        <span className="hidden sm:inline">{t("app.name")}</span>
        <span className="hidden lg:inline font-normal text-[11px] text-[var(--color-muted)] tracking-normal" title={t("app.name") + " v" + __APP_VERSION__}>v{__APP_VERSION__}</span>
      </span>
      {ready && <ModeSwitcher />}
      <StatusBar />
      {ready && <MoreMenu onVoices={() => setVoices(true)} onAssistant={() => setAssistant((open) => !open)} onNew={startFresh} onHelp={() => setHelp(true)} onSettings={() => openSettingsAt("models")} />}
      <div className="hidden md:flex items-center gap-1.5 md:gap-2 shrink-0">
        <div id="dock-slot" className="flex items-center gap-2 shrink-0" />
        {ready && (
          <button onClick={() => setVoices(true)} title={t("voices.title")} aria-label={t("voices.title")}
            className="inline-flex items-center gap-1.5 px-2 xl:px-3 py-1.5 rounded-lg border border-[var(--color-border)] text-[12px] font-medium text-[var(--color-text)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors">
            <Users size={13} /><span className="hidden xl:inline">{t("voices.open")}</span>
          </button>
        )}
        {ready && (
          <button onClick={() => setAssistant((open) => !open)} title={t("chat.title")} aria-label={t("chat.title")} aria-pressed={assistant}
            className={`inline-flex items-center gap-1.5 px-2 xl:px-3 py-1.5 rounded-lg border text-[12px] font-medium transition-colors ${assistant ? "border-[var(--color-accent)] text-[var(--color-accent)]" : "border-[var(--color-border)] text-[var(--color-text)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"}`}>
            <Bot size={13} /><span className="hidden xl:inline">{t("chat.title")}</span>
          </button>
        )}
        {ready && (
          <button onClick={startFresh} title={t("nav.newTitle", { keys: keys.new })} aria-label={t("nav.newTitle", { keys: keys.new })}
            className="inline-flex items-center gap-1 px-2 xl:px-3 py-1.5 rounded-lg bg-[var(--color-surface-2)] border border-[var(--color-border)] text-[12px] font-bold text-[var(--color-text)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors shadow-sm">
            <Plus size={13} /><span className="hidden xl:inline">{t("nav.new")}</span>
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
      {voices && <VoiceLibrary onClose={() => setVoices(false)} />}
      {assistant && <AssistantPanel onClose={() => setAssistant(false)} />}
      {settings !== null && (
        <SettingsModal target={settings.target} request={settings.request} onClose={() => setSettings(null)}
          panes={{ models: <><PresetsSection /><ModelsSection /></>, cloud: <><ModelsSection part="cloud" /><CloudProviders /></>, network: <ProxySection />, agent: <AgentPanel /> }} />
      )}
    </header>
  );
}
