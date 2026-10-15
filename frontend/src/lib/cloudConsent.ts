import { create } from "zustand";
import { api } from "./api";

/** The stages that can send text or audio out of this computer (SmartSub, Open Notebook: ask before the first time). */
export type CloudStage = "speech" | "listening" | "director" | "assistant";
export const CLOUD_STAGES: readonly CloudStage[] = ["speech", "listening", "director", "assistant"];
export const consentKey = (stage: CloudStage) => `cloud_ok_${stage}`;

type Asking = { stage: CloudStage; provider: string; answer: (ok: boolean) => void };

/** The question the window shows now, answered by CloudConsentHost. */
export const useCloudConsent = create<{ asking: Asking | null }>(() => ({ asking: null }));

/** The cloud providers of the engines, by engine id, as the person knows them. */
export const CLOUD_ENGINE_NAMES: Record<string, string> = { openrouter: "OpenRouter", google: "Gemini", fal: "fal.ai", runware: "Runware" };

/** The director (podcast, characters) goes to the cloud only when its language model is OpenRouter. */
export async function askDirector(): Promise<boolean> {
  const caps = await api.capabilities();
  return caps.selection?.llm_provider === "openrouter" ? askCloud("director", "OpenRouter") : true;
}

/** Whether the stage may go to the cloud: asked once per stage, the answer kept in the studio's settings. */
export async function askCloud(stage: CloudStage, provider: string): Promise<boolean> {
  const caps = await api.capabilities();
  if (caps.selection?.[consentKey(stage)] === "1") return true;
  const ok = await new Promise<boolean>((answer) => useCloudConsent.setState({ asking: { stage, provider, answer } }));
  useCloudConsent.setState({ asking: null });
  if (ok) await api.setSelection(consentKey(stage), "1");
  return ok;
}

/** The terms of cloning a voice, shown before the first clone: a voice of one's own or with its owner's leave. */
export const useCloneTerms = create<{ asking: ((ok: boolean) => void) | null }>(() => ({ asking: null }));

/** Whether a voice may be cloned now: the terms are accepted once, and the answer is kept in the studio's settings. */
export async function askCloneTerms(): Promise<boolean> {
  const caps = await api.capabilities();
  if (caps.selection?.clone_terms_ok === "1") return true;
  const ok = await new Promise<boolean>((answer) => useCloneTerms.setState({ asking: answer }));
  useCloneTerms.setState({ asking: null });
  if (ok) await api.setSelection("clone_terms_ok", "1");
  return ok;
}
