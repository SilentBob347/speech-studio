/**
 * The Assistant panel (VideoSOS chat): the conversation, the model it runs on and what it cost, kept in this window's
 * storage; a message goes to the server, which runs the model with the studio's tools (or hands it to the connected
 * agent) and streams back the text, each tool call and how it ended.
 */

import { create } from "zustand";
import { persist } from "zustand/middleware";
import { BASE, getCoded } from "./api";

export type ChatTool = { id: string; name: string; args: unknown; state: "running" | "ok" | "error"; summary: string };
export type ChatEntry = { id: string; role: "user" | "assistant"; text: string; tools: ChatTool[]; error: string | null };
export type ChatModel = { id: string; name: string; pricing?: { prompt?: string; completion?: string } | null };
export type ChatModels = { models: ChatModel[]; default: string | null; assistant: "openrouter" | "agent"; has_key: boolean };

type ChatState = {
  entries: ChatEntry[];
  model: string;
  cost: number;
  busy: boolean;
  setModel: (model: string) => void;
  clear: () => void;
};

let stopper: AbortController | null = null;

export const useChatStore = create<ChatState>()(
  persist(
    (set) => ({
      entries: [],
      model: "",
      cost: 0,
      busy: false,
      setModel: (model) => set({ model }),
      clear: () => set({ entries: [], cost: 0 }),
    }),
    { name: "speech-studio:chat", partialize: (state) => ({ entries: state.entries, model: state.model, cost: state.cost }) },
  ),
);

const newId = () => crypto.randomUUID().slice(0, 8);

function update(entryId: string, change: (entry: ChatEntry) => ChatEntry): void {
  useChatStore.setState((state) => ({ entries: state.entries.map((entry) => (entry.id === entryId ? change(entry) : entry)) }));
}

export const chatModels = (): Promise<ChatModels> => getCoded<ChatModels>("/chat/models");

/** Sent when the person changes who answers, so an open panel reads it again. */
export const ASSISTANT_CHANGED = "speech:assistant-changed";

/** Stops the answer being written; the server stops with it once the stream is gone. */
export function stopChat(): void {
  stopper?.abort();
}

type ChatEvent = { type: string; text?: string; id?: string; name?: string; args?: unknown; ok?: boolean; summary?: string; message?: string; cost?: number };

/** Sends the person's message and follows the answer as the server streams it. */
export async function sendChat(text: string, projectId: string | null, language: string): Promise<void> {
  const state = useChatStore.getState();
  if (state.busy || !text.trim()) return;
  const history = state.entries.filter((entry) => entry.text.trim()).map((entry) => ({ role: entry.role, content: entry.text }));
  const asked: ChatEntry = { id: newId(), role: "user", text: text.trim(), tools: [], error: null };
  const answer: ChatEntry = { id: newId(), role: "assistant", text: "", tools: [], error: null };
  useChatStore.setState({ entries: [...state.entries, asked, answer], busy: true });
  stopper = new AbortController();
  try {
    const response = await fetch(`${BASE}/chat`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model: state.model, messages: [...history, { role: "user", content: asked.text }], project_id: projectId, language }),
      signal: stopper.signal,
    });
    if (!response.ok || !response.body) {
      const body = await response.text();
      let message = body;
      try { const parsed = JSON.parse(body) as { detail?: string; error?: string }; message = parsed.detail || parsed.error || body; } catch { /* the answer is plain text */ }
      throw new Error(message || `${response.status}`);
    }
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    // the server tells the answer's cost so far after every step; the total of the chat grows by what is new
    let counted = 0;
    const addCost = (sofar: number | undefined) => {
      const now = sofar ?? counted;
      useChatStore.setState((current) => ({ cost: current.cost + (now - counted) }));
      counted = now;
    };
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      let end: number;
      while ((end = buffer.indexOf("\n\n")) !== -1) {
        const block = buffer.slice(0, end);
        buffer = buffer.slice(end + 2);
        const data = block.split("\n").filter((line) => line.startsWith("data:")).map((line) => line.slice(5).trimStart()).join("\n");
        if (!data) continue;
        const event = JSON.parse(data) as ChatEvent;
        if (event.type === "text") update(answer.id, (entry) => ({ ...entry, text: entry.text + (event.text ?? "") }));
        else if (event.type === "tool")
          update(answer.id, (entry) => ({ ...entry, tools: [...entry.tools, { id: event.id ?? newId(), name: event.name ?? "", args: event.args, state: "running", summary: "" }] }));
        else if (event.type === "tool_done")
          update(answer.id, (entry) => ({ ...entry, tools: entry.tools.map((tool) => (tool.id === event.id ? { ...tool, state: event.ok ? "ok" : "error", summary: event.summary ?? "" } : tool)) }));
        else if (event.type === "cost") addCost(event.cost);
        else if (event.type === "done" || event.type === "error") {
          addCost(event.cost);
          if (event.type === "error") update(answer.id, (entry) => ({ ...entry, error: event.message ?? "" }));
        }
      }
    }
  } catch (error) {
    if (!(error instanceof DOMException && error.name === "AbortError")) update(answer.id, (entry) => ({ ...entry, error: error instanceof Error ? error.message : String(error) }));
  } finally {
    stopper = null;
    // a tool still running when the answer stopped never finished
    update(answer.id, (entry) => ({ ...entry, tools: entry.tools.map((tool) => (tool.state === "running" ? { ...tool, state: "error" } : tool)) }));
    useChatStore.setState({ busy: false });
  }
}
