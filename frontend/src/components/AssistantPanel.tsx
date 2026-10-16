import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Bot, Check, CircleAlert, KeyRound, Loader2, Send, Square, Trash2, Wrench, X } from "lucide-react";
import { ASSISTANT_CHANGED, chatModels, sendChat, stopChat, useChatStore, type ChatEntry, type ChatModel } from "../lib/chat";
import { openSettings } from "../lib/settingsNav";
import { openProjectId } from "../lib/openProject";
import { useEscapeLayer } from "../lib/escape";
import { askCloud } from "../lib/cloudConsent";
import { named } from "../lib/a11y";
import { errorText } from "../lib/voices";
import { useStore } from "../store";
import { Tip } from "./Hint";

const SUGGESTIONS = ["what", "voice", "check", "listen"] as const;
const LIST_ITEM = /^\s*(?:[-*•]|\d+[.)])\s+/;

function usd(amount: number): string {
  if (amount === 0) return "$0";
  return amount >= 0.01 ? `$${amount.toFixed(2)}` : `$${amount.toFixed(4)}`;
}

/** An answer's markdown as the panel shows it: paragraphs with their line breaks, bullet and numbered lists. */
function AnswerText({ text }: { text: string }) {
  return (
    <div className="space-y-2 text-[12px] leading-relaxed">
      {text.split(/\n\s*\n/).filter((block) => block.trim()).map((block, b) => {
        const lines = block.split("\n").filter((line) => line.trim());
        if (lines.every((line) => LIST_ITEM.test(line))) {
          const ordered = /^\s*\d/.test(lines[0] ?? "");
          const List = ordered ? "ol" : "ul";
          return <List key={b} className={`space-y-1 pl-5 ${ordered ? "list-decimal" : "list-disc"}`}>{lines.map((line, l) => <li key={l}>{line.replace(LIST_ITEM, "")}</li>)}</List>;
        }
        return <p key={b} className="whitespace-pre-wrap break-words">{lines.join("\n")}</p>;
      })}
    </div>
  );
}

function ToolChip({ tool }: { tool: ChatEntry["tools"][number] }) {
  const { t } = useTranslation();
  return (
    <li className="flex min-w-0 items-center gap-1.5 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-2)] px-2 py-1 text-[11px]" title={tool.summary || undefined}>
      {tool.state === "running" ? <Loader2 size={12} className="shrink-0 animate-spin text-[var(--color-accent)]" aria-label={t("chat.toolRunning")} />
        : tool.state === "ok" ? <Check size={12} className="shrink-0 text-[var(--color-accent)]" aria-label={t("chat.toolDone")} />
        : <CircleAlert size={12} className="shrink-0 text-[var(--color-warn)]" aria-label={t("chat.toolFailed")} />}
      <Wrench size={12} className="shrink-0 text-[var(--color-muted)]" aria-hidden />
      <span className="mono truncate">{tool.name}</span>
    </li>
  );
}

/**
 * The Assistant panel (VideoSOS chat): it works the studio with the tools an agent has over MCP, on the OpenRouter
 * key or through the connected agent, and the person sees each step; the conversation, the model and its cost stay
 * in the window.
 */
export default function AssistantPanel({ onClose }: { onClose: () => void }) {
  const { t, i18n } = useTranslation();
  useEscapeLayer(onClose);
  const mode = useStore((s) => s.mode);
  const entries = useChatStore((s) => s.entries);
  const model = useChatStore((s) => s.model);
  const cost = useChatStore((s) => s.cost);
  const busy = useChatStore((s) => s.busy);
  const [models, setModels] = useState<ChatModel[]>([]);
  const [who, setWho] = useState<{ assistant: "openrouter" | "agent"; hasKey: boolean } | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    document.documentElement.toggleAttribute("data-assistant", true);
    return () => document.documentElement.removeAttribute("data-assistant");
  }, []);
  useEffect(() => {
    const load = () => chatModels().then((answer) => {
      setModels(answer.models);
      setWho({ assistant: answer.assistant, hasKey: answer.has_key });
      setProblem(null);
      const current = useChatStore.getState().model;
      if ((!current || !answer.models.some((each) => each.id === current)) && answer.default) useChatStore.getState().setModel(answer.default);
    }, (e: unknown) => setProblem(errorText(e)));
    void load();
    window.addEventListener(ASSISTANT_CHANGED, load);
    return () => window.removeEventListener(ASSISTANT_CHANGED, load);
  }, []);
  useEffect(() => { end.current?.scrollIntoView({ block: "end" }); }, [entries]);
  const byAgent = who?.assistant === "agent";
  const send = async (text: string) => {
    if (!text.trim() || busy) return;
    if (!byAgent && !(await askCloud("assistant", "OpenRouter"))) return;
    setDraft("");
    void sendChat(text, openProjectId(mode), i18n.language);
  };
  const box = "bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] focus:border-[var(--color-accent)] focus:outline-none";
  const body = () => {
    if (!who) return problem ? <p role="alert" className="p-4 text-[12px] text-[var(--color-warn)] break-words">{problem}</p> : <div className="p-6 grid place-items-center"><Loader2 size={16} className="animate-spin text-[var(--color-muted)]" /></div>;
    if (!who.hasKey && !byAgent) return (
      <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center text-[12px] text-[var(--color-muted)]">
        <Bot size={28} className="text-[var(--color-accent)]" aria-hidden />
        <p className="leading-relaxed">{t("chat.needsKey")}</p>
        <div className="flex flex-wrap justify-center gap-2">
          <button onClick={() => openSettings("cloud:key")} className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-[var(--color-accent)] text-[var(--color-on-accent)] font-semibold"><KeyRound size={13} />{t("chat.openKeys")}</button>
          <button onClick={() => openSettings("agent")} className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md border border-[var(--color-border)] hover:border-[var(--color-accent)]"><Bot size={13} />{t("chat.useAgent")}</button>
        </div>
      </div>
    );
    return (
      <>
        <div className="flex items-center gap-2 border-b border-[var(--color-border)] px-3 py-2">
          {byAgent ? (
            <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[12px] text-[var(--color-muted)]"><Bot size={13} className="shrink-0 text-[var(--color-accent)]" aria-hidden /><span className="truncate">{t("chat.byAgent")}</span></span>
          ) : (
            <>
              <select value={model} onChange={(e) => useChatStore.getState().setModel(e.target.value)} aria-label={t("chat.model")} className={`${box} min-w-0 flex-1`}>
                {!models.some((m) => m.id === model) && <option value={model}>{model || t("chat.model")}</option>}
                {models.map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}
              </select>
              <Tip text={t("chat.costHint")} className="shrink-0 inline-flex"><span className="mono text-[11px] tabnum text-[var(--color-muted)]">{usd(cost)}</span></Tip>
            </>
          )}
          <button disabled={busy || entries.length === 0} onClick={() => useChatStore.getState().clear()} {...named(t("chat.clear"))} className="p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-warn)] disabled:opacity-40"><Trash2 size={14} /></button>
        </div>
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-3 py-3" aria-live="polite">
          {entries.length === 0 && (
            <div className="space-y-2 pt-1">
              <p className="text-[12px] leading-relaxed text-[var(--color-muted)]">{t(byAgent ? "chat.introAgent" : "chat.intro")}</p>
              <div className="flex flex-col gap-1.5">
                {SUGGESTIONS.map((key) => (
                  <button key={key} type="button" onClick={() => send(t(`chat.suggestions.${key}`))} className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-2)] px-2.5 py-1.5 text-left text-[12px] hover:border-[var(--color-accent)]">{t(`chat.suggestions.${key}`)}</button>
                ))}
              </div>
            </div>
          )}
          {entries.map((entry) => entry.role === "user" ? (
            <div key={entry.id} className="ml-6 rounded-lg bg-[color-mix(in_oklab,var(--color-accent)_15%,transparent)] px-3 py-2 text-[12px] leading-relaxed whitespace-pre-wrap break-words">{entry.text}</div>
          ) : (
            <div key={entry.id} className="space-y-1.5">
              {entry.tools.length > 0 && <ul className="space-y-1" aria-label={t("chat.tools")}>{entry.tools.map((tool) => <ToolChip key={tool.id} tool={tool} />)}</ul>}
              {entry.text && <AnswerText text={entry.text} />}
              {!entry.text && !entry.error && busy && entry === entries[entries.length - 1] && entry.tools.every((tool) => tool.state !== "running") && <Loader2 size={14} className="animate-spin text-[var(--color-accent)]" aria-label={t("chat.thinking")} />}
              {entry.error && <p role="alert" className="flex gap-1 break-words text-[11px] text-[var(--color-warn)]"><CircleAlert size={12} className="mt-px shrink-0" />{entry.error}</p>}
            </div>
          ))}
          <div ref={end} />
        </div>
        <form onSubmit={(e) => { e.preventDefault(); send(draft); }} className="border-t border-[var(--color-border)] p-2">
          <div className="flex items-end gap-2">
            <textarea value={draft} onChange={(e) => setDraft(e.target.value)} rows={2}
              onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(draft); } }}
              placeholder={t("chat.placeholder")} aria-label={t("chat.placeholder")} className={`${box} max-h-40 min-h-[52px] flex-1 resize-none`} />
            {busy
              ? <button type="button" onClick={stopChat} {...named(t("chat.stop"))} className="h-9 w-9 shrink-0 grid place-items-center rounded-md border border-[var(--color-border)] hover:border-[var(--color-warn)]"><Square size={14} /></button>
              : <button type="submit" disabled={!draft.trim() || (!model && !byAgent)} {...named(t("chat.send"))} className="h-9 w-9 shrink-0 grid place-items-center rounded-md bg-[var(--color-accent)] text-[var(--color-on-accent)] disabled:opacity-40"><Send size={14} /></button>}
          </div>
          <p className="mt-1 text-[11px] text-[var(--color-muted)]">{t(byAgent ? "chat.footnoteAgent" : "chat.footnote")}</p>
        </form>
      </>
    );
  };
  return (
    <aside role="complementary" aria-label={t("chat.title")} className="fixed top-14 right-0 bottom-0 z-[45] w-[min(420px,100vw)] flex flex-col border-l border-[var(--color-border)] bg-[var(--color-surface)] shadow-2xl anim-fade">
      <div className="flex items-center justify-between gap-2 px-3 h-11 border-b border-[var(--color-border)]">
        <h2 className="inline-flex items-center gap-1.5 text-[13px] font-semibold"><Bot size={15} className="text-[var(--color-accent)]" />{t("chat.title")}</h2>
        <button onClick={onClose} {...named(t("a11y.close"))} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]"><X size={15} /></button>
      </div>
      {body()}
    </aside>
  );
}
