import { BASE, getCoded, postCoded, sendCoded } from "./api";

export type CloudProvider = "fal" | "runware";
export type CloudTask = "speech" | "transcribe";
export type CloudRate = { amount: number; unit: string; label?: string | null };
export type CloudModel = {
  provider: CloudProvider; id: string; name: string; description: string; tasks: string[];
  created: number | null; status: string; price: string | null; rates?: CloudRate[];
};
export type CloudChoice = { value: unknown; label: string };
export type CloudField = {
  key: string; label: string; description?: string; required: boolean; default?: unknown; advanced: boolean; role?: string | null;
  type: "text" | "textarea" | "integer" | "number" | "boolean" | "select" | "media" | "size";
  min?: number | null; max?: number | null; max_length?: number | null; options?: CloudChoice[];
};
export type CloudForm = { provider: CloudProvider; id: string; fields: CloudField[]; price: { rates: CloudRate[]; note?: string | null }; delivery: string };
export type KeyState = { configured: boolean; source: "environment" | "local_store" | null; environment_variable: string };
export type ProviderState = { models: number; updated: number | null; refreshing: boolean; error: string | null; key: KeyState };
export type SpendGroup = { key: string; usd: number; runs: number; unpriced: number };
export type Spend = { total: number; today: number; unpriced: number; providers: SpendGroup[]; models: SpendGroup[]; days: SpendGroup[]; daily_limit_usd: number };

export const cloudApi = {
  catalog: (task: CloudTask) => getCoded<{ models: CloudModel[]; providers: Record<CloudProvider, ProviderState> }>(`/cloud/catalog?task=${task}`),
  refresh: () => fetch(`${BASE}/cloud/catalog/refresh`, { method: "POST" }),
  form: (provider: CloudProvider, id: string) => getCoded<CloudForm>(`/cloud/form?provider=${provider}&id=${encodeURIComponent(id)}`),
  estimate: (provider: CloudProvider, id: string, values: Record<string, unknown>, runs: number, chars: number) =>
    postCoded<{ usd: number | null; exact: boolean | null; note: string | null }>("/cloud/estimate", { provider, id, values, runs, chars }),
  keys: () => getCoded<Record<CloudProvider, KeyState>>("/cloud/keys"),
  setKey: (provider: CloudProvider, apiKey: string) => sendCoded<{ state: KeyState; balance: number | null }>("PUT", `/cloud/keys/${provider}`, { api_key: apiKey }),
  deleteKey: (provider: CloudProvider) => sendCoded<KeyState>("DELETE", `/cloud/keys/${provider}`),
  spend: (project?: string) => getCoded<Spend>(`/cloud/spend${project ? `?project=${encodeURIComponent(project)}` : ""}`),
};

const FAVOURITES = "speech-studio:cloud-favourites";

export function favourites(): Set<string> {
  try { return new Set(JSON.parse(localStorage.getItem(FAVOURITES) ?? "[]") as string[]); } catch { return new Set(); }
}

export function setFavourite(key: string, on: boolean): Set<string> {
  const all = favourites();
  if (on) all.add(key); else all.delete(key);
  try { localStorage.setItem(FAVOURITES, JSON.stringify([...all])); } catch { /* favourites stay for this window only */ }
  return all;
}

/** Values of the form a project keeps: everything but the text field and media. */
export function editableFields(form: CloudForm): CloudField[] {
  return form.fields.filter((f) => f.role !== "prompt" && f.type !== "media");
}
