import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Copy, KeyRound, Loader2, Trash2 } from "lucide-react";
import { BASE, getCoded, postCoded, type ApiError } from "../../lib/api";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";

type KeyState = { configured: boolean; source: "environment" | "local_store" | null; environment_variable: string };
type ApiSettings = { key: KeyState; speech_models: { id: string; engine: string }[]; transcribe_models: string[] };
type Logged = { at: number; endpoint: "models" | "speech" | "transcriptions"; model: string; voice: string; status: number; ms: number; size: number; error: string | null };

const ENDPOINTS = ["", "speech", "transcriptions", "models"] as const;
const ROUTES: Record<Exclude<(typeof ENDPOINTS)[number], "">, string> = { speech: "/v1/audio/speech", transcriptions: "/v1/audio/transcriptions", models: "/v1/models" };
const PYTHON = "Python (openai)";

function CopyBlock({ label, text }: { label: string; text: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const copy = () => {
    navigator.clipboard.writeText(text).then(() => { setCopied(true); window.setTimeout(() => setCopied(false), 1500); }, (e: unknown) => console.error("[ERROR] copy failed:", e));
  };
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[11px] text-[var(--color-muted)]">{label}</span>
        <button type="button" onClick={copy} className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md border border-[var(--color-border)] text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-text)]">
          {copied ? <Check size={12} /> : <Copy size={12} />}{copied ? t("agent.copied") : t("agent.copy")}
        </button>
      </div>
      <pre className="overflow-x-auto rounded-md bg-[var(--color-surface)] border border-[var(--color-border)] px-2 py-1.5 mono text-[11px] whitespace-pre">{text}</pre>
    </div>
  );
}

/** The OpenAI-compatible API for other programs (Higgs-Ultimate's command center): its address, its key, examples and
 * the latest requests with a filter. */
export default function ApiSection() {
  const { t, i18n } = useTranslation();
  const [settings, setSettings] = useState<ApiSettings | null>(null);
  const [fresh, setFresh] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [rows, setRows] = useState<Logged[]>([]);
  const [endpoint, setEndpoint] = useState<(typeof ENDPOINTS)[number]>("");
  const [query, setQuery] = useState("");
  const [onlyErrors, setOnlyErrors] = useState(false);

  useEffect(() => { getCoded<ApiSettings>("/openai/settings").then(setSettings, (e: unknown) => setProblem(errorText(e))); }, []);
  const loadLog = useCallback(() => getCoded<{ requests: Logged[] }>("/openai/log").then((r) => setRows(r.requests), (e: unknown) => setProblem(errorText(e))), []);
  useEffect(() => {
    void loadLog();
    const timer = window.setInterval(() => void loadLog(), 3000);
    return () => window.clearInterval(timer);
  }, [loadLog]);

  const base = `${BASE || window.location.origin}/v1`;
  const key = fresh ?? (settings?.key.configured ? "<key>" : "local");
  const sample = t("api.sampleText");
  const curlSpeech = `curl ${base}/audio/speech \\\n  -H "Authorization: Bearer ${key}" -H "Content-Type: application/json" \\\n  -d '{"model": "tts-1", "voice": "<voice name>", "input": "${sample}", "response_format": "mp3"}' \\\n  -o speech.mp3`;
  const curlTranscribe = `curl ${base}/audio/transcriptions \\\n  -H "Authorization: Bearer ${key}" \\\n  -F file=@recording.wav -F model=whisper-1 -F response_format=verbose_json`;
  const python = `from openai import OpenAI\n\nclient = OpenAI(base_url="${base}", api_key="${key}")\nclient.audio.speech.create(model="tts-1", voice="<voice name>", input="${sample}").write_to_file("speech.mp3")\nprint(client.audio.transcriptions.create(model="whisper-1", file=open("recording.wav", "rb")).text)`;

  const makeKey = async () => {
    setBusy(true);
    try {
      const made = await postCoded<{ key: string; state: KeyState }>("/openai/key", {});
      setFresh(made.key);
      setSettings((s) => (s ? { ...s, key: made.state } : s));
      setProblem(null);
    } catch (e) { setProblem(errorText(e as ApiError)); } finally { setBusy(false); }
  };
  const removeKey = async () => {
    setBusy(true);
    try {
      const r = await fetch(`${BASE}/openai/key`, { method: "DELETE" });
      if (!r.ok) throw new Error(await r.text());
      const state = (await r.json()) as KeyState;
      setFresh(null);
      setSettings((s) => (s ? { ...s, key: state } : s));
      setProblem(null);
    } catch (e) { setProblem(errorText(e)); } finally { setBusy(false); }
  };
  const clearLog = () => { fetch(`${BASE}/openai/log`, { method: "DELETE" }).then(() => setRows([]), (e: unknown) => setProblem(errorText(e))); };

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return rows.filter((r) => (!endpoint || r.endpoint === endpoint) && (!onlyErrors || r.status >= 400) && (!q || `${r.model} ${r.voice} ${r.error ?? ""}`.toLowerCase().includes(q)));
  }, [rows, endpoint, query, onlyErrors]);
  const box = "bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] focus:border-[var(--color-accent)] focus:outline-none";
  const k = settings?.key;
  return (
    <div className="max-w-2xl space-y-3">
      <div className="px-2.5 py-2 rounded-lg bg-[var(--color-surface-2)] border border-[var(--color-border)] space-y-2.5">
        <p className="text-[12px] leading-5">{t("api.intro")}</p>
        <CopyBlock label={t("api.base")} text={base} />
        <div className="space-y-1.5" data-settings-part="key">
          <div className="flex flex-wrap items-center gap-2 text-[12px]">
            <KeyRound size={13} className="text-[var(--color-accent)]" />
            <span className="font-medium">{t("api.key")}</span>
            <span className="text-[11px] text-[var(--color-muted)]">
              {!k ? "…" : !k.configured ? t("api.keyNone") : k.source === "environment" ? t("api.keyEnv", { name: k.environment_variable }) : t("api.keySaved")}
            </span>
            <span className="ml-auto flex gap-1.5">
              {k?.source !== "environment" && (
                <button onClick={() => void makeKey()} disabled={busy || !k} className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[12px] font-semibold disabled:opacity-40">
                  {busy && <Loader2 size={12} className="animate-spin" />}{k?.configured ? t("api.keyNew") : t("api.keyMake")}
                </button>
              )}
              {k?.configured && k.source === "local_store" && <button onClick={() => void removeKey()} disabled={busy} {...named(t("api.keyRemove"))} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-warn)] disabled:opacity-40"><Trash2 size={13} /></button>}
            </span>
          </div>
          {fresh && <CopyBlock label={t("api.keyOnce")} text={fresh} />}
          <p className="text-[11px] text-[var(--color-muted)] leading-relaxed">{t("api.keyHint")}</p>
        </div>
        {settings && (
          <p className="text-[11px] text-[var(--color-muted)] leading-relaxed">
            {t("api.models", { speech: settings.speech_models.map((m) => `${m.id} → ${m.engine}`).join(", "), transcribe: settings.transcribe_models.join(", ") })}
          </p>
        )}
      </div>
      <div className="px-2.5 py-2 rounded-lg bg-[var(--color-surface-2)] border border-[var(--color-border)] space-y-2.5">
        <h3 className="text-[12px] font-semibold">{t("api.examples")}</h3>
        <CopyBlock label={t("api.exampleSpeech")} text={curlSpeech} />
        <CopyBlock label={t("api.exampleTranscribe")} text={curlTranscribe} />
        <CopyBlock label={PYTHON} text={python} />
      </div>
      <div className="px-2.5 py-2 rounded-lg bg-[var(--color-surface-2)] border border-[var(--color-border)] space-y-2">
        <div className="flex flex-wrap items-center gap-2">
          <h3 className="text-[12px] font-semibold mr-auto">{t("api.log")}</h3>
          <select value={endpoint} onChange={(e) => setEndpoint(e.target.value as (typeof ENDPOINTS)[number])} aria-label={t("api.logEndpoint")} className={box}>
            {ENDPOINTS.map((e) => <option key={e} value={e}>{e ? ROUTES[e] : t("api.logAll")}</option>)}
          </select>
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("api.logFilter")} aria-label={t("api.logFilter")} className={`${box} w-36`} />
          <label className="inline-flex items-center gap-1 text-[11px]"><input type="checkbox" checked={onlyErrors} onChange={(e) => setOnlyErrors(e.target.checked)} className="accent-[var(--color-accent)]" />{t("api.logErrors")}</label>
          <button onClick={clearLog} disabled={!rows.length} {...named(t("api.logClear"))} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-warn)] disabled:opacity-40"><Trash2 size={13} /></button>
        </div>
        {shown.length === 0 ? <p className="text-[11px] text-[var(--color-muted)] py-2">{rows.length ? t("api.logNoMatch") : t("api.logEmpty")}</p> : (
          <div className="overflow-x-auto">
            <table className="w-full text-[11px] tabnum">
              <thead className="text-[var(--color-muted)] text-left"><tr><th className="font-normal pr-2">{t("api.logTime")}</th><th className="font-normal pr-2">{t("api.logRoute")}</th><th className="font-normal pr-2">{t("api.logModel")}</th><th className="font-normal pr-2">{t("api.logStatus")}</th><th className="font-normal pr-2">{t("api.logMs")}</th></tr></thead>
              <tbody>
                {shown.map((r, i) => (
                  <tr key={`${r.at}-${i}`} className="border-t border-[var(--color-border)] align-top">
                    <td className="pr-2 py-1 mono whitespace-nowrap">{new Date(r.at).toLocaleTimeString(i18n.language)}</td>
                    <td className="pr-2 py-1 mono">{r.endpoint}</td>
                    <td className="pr-2 py-1 mono break-all">{[r.model, r.voice].filter(Boolean).join(" · ")}</td>
                    <td className={`pr-2 py-1 mono ${r.status >= 400 ? "text-[var(--color-warn)]" : ""}`} title={r.error ?? undefined}>{r.status}{r.error ? ` ${r.error}` : ""}</td>
                    <td className="pr-2 py-1 mono whitespace-nowrap">{r.ms}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
      {problem && <p role="alert" className="text-[11px] text-[var(--color-warn)] break-words">{problem}</p>}
    </div>
  );
}
