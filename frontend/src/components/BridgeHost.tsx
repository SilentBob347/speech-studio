import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { Bot } from "lucide-react";
import { api, type JobKind } from "../lib/api";
import en from "../locales/en.json";
import { useStore, MODES, type Mode } from "../store";
import { playSfx } from "../lib/sfx";
import { speechApi } from "../lib/speech";
import { requestOpen } from "../lib/openProject";
import { openEditor } from "../lib/editorBridge";
import { AGENT_ACTION, JOBS_CHANGED, bridgeError, isMine, label, takeArgs, textArg, useBridgeCommand, type BridgeArgs, type ChangeNotice } from "../lib/mcpBridge";

// Команды агента, которые пользователь видит подписью «Агент: …» (что только смотрит — не показываем).
const SHOWN = ["screenshot", "click", "type", "select", "press_key", "scroll", "navigate", "open_settings", "open_help", "editor_open", "editor_select"] as const;
type Shown = (typeof SHOWN)[number];

function actionLabel(t: TFunction<"t">, command: string, args: BridgeArgs): string | null {
  if (!(SHOWN as readonly string[]).includes(command)) return null;
  if (command === "navigate") {
    const view = args.view;
    return typeof view === "string" && (MODES as readonly string[]).includes(view) ? t("bridge.do.navigate", { name: t(`modes.${view as Mode}`) }) : null;
  }
  const target = typeof args.ref === "string" ? document.querySelector(`[data-mcp-ref="${CSS.escape(args.ref)}"]`) : null;
  const name = target ? label(target) : typeof args.text === "string" ? args.text : "";
  return t(`bridge.do.${command as Shown}`, { name });
}

// Продолжение джобы не знает заранее, какую джобу продолжит.
const jobLabel = (t: TFunction<"t">, kind: string | undefined): string =>
  kind === "resume" ? t("bridge.job.resume") : kind !== undefined && kind in en.jobs.kind ? t(`jobs.kind.${kind as JobKind}`) : t("bridge.job.other");

/** Follows a job an agent (or another window) started: its progress in the status bar. */
async function follow(t: TFunction<"t">, notice: ChangeNotice & { job_id: string }): Promise<void> {
  const kind = jobLabel(t, notice.kind);
  useStore.getState().pushActivity(t("bridge.jobStarted", { kind }), "agent");
  try {
    await api.watchJob(notice.job_id, (event) => {
      if (event.type !== "progress") return;
      useStore.getState().setProgress(event.stage || "", event.msg || "", event.pct ?? null);
    });
    useStore.getState().setProgress("done", t("bridge.jobDone", { kind }), null);
  } catch (problem) {
    useStore.getState().setProgress("error", t("bridge.jobFailed", { kind, error: String(problem) }), null);
  }
}

// Окно для агента на уровне приложения: режимы, сообщения пользователю, подпись к каждому действию агента и
// прогресс джоб, которые агент запустил мимо окна.
export default function BridgeHost() {
  const { t } = useTranslation();
  const [shown, setShown] = useState<{ key: number; text: string; tone: "agent" | "info" | "success" | "error" } | null>(null);

  useEffect(() => {
    if (!shown) return;
    const timer = window.setTimeout(() => setShown(null), shown.tone === "agent" ? 2600 : 6000);
    return () => window.clearTimeout(timer);
  }, [shown]);

  useEffect(() => {
    const onAction = (e: Event) => {
      const { command, args } = (e as CustomEvent<{ command: string; args: BridgeArgs }>).detail;
      const text = actionLabel(t, command, args);
      if (text) setShown({ key: Date.now(), text, tone: "agent" });
    };
    const onJob = (e: Event) => {
      const notice = (e as CustomEvent<ChangeNotice>).detail;
      if (isMine(notice) || !notice.job_id) return;
      void follow(t, { ...notice, job_id: notice.job_id });
    };
    window.addEventListener(AGENT_ACTION, onAction);
    window.addEventListener(JOBS_CHANGED, onJob);
    return () => {
      window.removeEventListener(AGENT_ACTION, onAction);
      window.removeEventListener(JOBS_CHANGED, onJob);
    };
  }, [t]);

  useBridgeCommand("navigate", (args) => {
    takeArgs("ui_navigate", args, ["view"]);
    const view = textArg("ui_navigate", args, "view");
    if (!view || !(MODES as readonly string[]).includes(view)) throw bridgeError("bad_value", { command: "ui_navigate", field: "view", expected: MODES.join(" or ") });
    const s = useStore.getState();
    if (s.stage !== "ready") throw bridgeError("bad_value", { command: "ui_navigate", field: "view", expected: `the studio to be past ${s.stage}` });
    s.setMode(view as Mode);
    return { screen: view };
  });

  useBridgeCommand("editor_open", async (args) => {
    takeArgs("editor_open", args, ["pid"]);
    const pid = textArg("editor_open", args, "pid");
    if (!pid) throw bridgeError("missing", { command: "editor_open", field: "pid" });
    const s = useStore.getState();
    if (s.stage !== "ready") throw bridgeError("bad_value", { command: "editor_open", field: "pid", expected: `the studio to be past ${s.stage}` });
    const card = (await speechApi.list()).projects.find((p) => p.pid === pid);
    if (!card) throw bridgeError("no_project", { pid, reason: "there is no such project" });
    const mode: Mode = card.kind === "listening" ? "listening" : "speech";
    s.setMode(mode);
    requestOpen(mode, pid);
    return { opened: pid, kind: mode, name: card.name };
  });

  useBridgeCommand("editor_state", (args) => {
    takeArgs("editor_state", args, []);
    const editor = openEditor();
    return { mode: useStore.getState().mode, open: editor ? { kind: editor.kind, pid: editor.pid, ...editor.state() } : null };
  });

  useBridgeCommand("editor_select", (args) => {
    takeArgs("editor_select", args, ["id"]);
    const id = textArg("editor_select", args, "id");
    if (!id) throw bridgeError("missing", { command: "editor_select", field: "id" });
    const editor = openEditor();
    if (!editor) throw bridgeError("no_screen", { command: "editor_select" });
    if (!editor.select(id)) throw bridgeError("no_line", { id });
    return { selected: id, pid: editor.pid };
  });

  useBridgeCommand("notify", (args) => {
    takeArgs("ui_notify", args, ["text", "tone"]);
    const text = (textArg("ui_notify", args, "text") ?? "").trim();
    if (!text) throw bridgeError("missing", { command: "ui_notify", field: "text" });
    const tone = textArg("ui_notify", args, "tone") ?? "info";
    if (tone !== "info" && tone !== "success" && tone !== "error") throw bridgeError("bad_value", { command: "ui_notify", field: "tone", expected: "info, success or error" });
    useStore.getState().pushActivity(text, tone === "error" ? "error" : tone === "success" ? "done" : "agent");
    playSfx(tone === "error" ? "error" : tone === "success" ? "success" : "notify");
    setShown({ key: Date.now(), text, tone });
    return { shown: true };
  });

  if (!shown) return null;
  const ring = shown.tone === "error" ? "border-[var(--color-warn)]" : "border-[var(--color-accent)]";
  return (
    <div key={shown.key} role="status" aria-live="polite"
      className={`fixed top-16 left-1/2 -translate-x-1/2 z-[80] pointer-events-none max-w-[min(92vw,640px)] inline-flex items-center gap-2 px-3.5 py-1.5 rounded-full border ${ring} bg-[var(--color-surface)] shadow-xl text-[12px] text-[var(--color-text)] anim-pop`}>
      <Bot size={14} className="text-[var(--color-accent)] shrink-0" />
      <span className="font-semibold shrink-0">{t("bridge.agent")}</span>
      <span className="truncate">{shown.text}</span>
    </div>
  );
}
