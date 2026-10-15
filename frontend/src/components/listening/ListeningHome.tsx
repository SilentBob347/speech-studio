import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Ear, FileAudio, Loader2, Mic, Square, Trash2 } from "lucide-react";
import { api } from "../../lib/api";
import { listenApi, clock, type ListenCard } from "../../lib/listen";
import { speechApi } from "../../lib/speech";
import { errorText } from "../../lib/voices";
import { PROJECTS_CHANGED, useChanged } from "../../lib/mcpBridge";
import { named } from "../../lib/a11y";
import ConfirmDialog from "../ConfirmDialog";
import UrlImport from "../UrlImport";

const MEDIA = "audio/*,video/*,.mkv,.avi,.wma,.opus,.flac,.m4a";

/** Listening's start: a recording from a file (several at once go to recognition one after another), a link or the
 * microphone, and the recordings opened before. */
export default function ListeningHome({ onOpen }: { onOpen: (pid: string) => void }) {
  const { t, i18n } = useTranslation();
  const [projects, setProjects] = useState<ListenCard[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [drag, setDrag] = useState(false);
  const [removing, setRemoving] = useState<ListenCard | null>(null);
  const [batch, setBatch] = useState<{ done: number; total: number } | null>(null);
  const file = useRef<HTMLInputElement>(null);
  const load = () => {
    speechApi.list().then((r) => setProjects(r.projects.filter((p): p is ListenCard => p.kind === "listening") as ListenCard[]), (e: unknown) => setError(errorText(e)));
  };
  useEffect(load, []);
  useChanged(PROJECTS_CHANGED, load);

  const upload = async (files: File[]) => {
    if (!files.length) return;
    setBusy(true);
    setError(null);
    try {
      if (files.length === 1) { onOpen((await listenApi.upload(files[0])).pid); return; }
      setBatch({ done: 0, total: files.length });
      const failed: string[] = [];
      for (const [k, f] of files.entries()) {
        try { await listenApi.transcribe((await listenApi.upload(f)).pid); } catch (e) { failed.push(`${f.name}: ${errorText(e)}`); }
        setBatch({ done: k + 1, total: files.length });
        load();
      }
      if (failed.length) setError(failed.join("\n"));
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const when = (s: number) => new Date(s * 1000).toLocaleString(i18n.language, { dateStyle: "medium", timeStyle: "short" });

  return (
    <main className="flex-1 min-h-0 overflow-y-auto">
      <div className="max-w-5xl mx-auto px-6 py-8 grid gap-8 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        <section aria-labelledby="listen-new" className="space-y-4">
          <h1 id="listen-new" className="text-xl font-bold tracking-tight flex items-center gap-2"><Ear size={20} className="text-[var(--color-accent)]" />{t("listen.newTitle")}</h1>
          <p className="text-[13px] text-[var(--color-muted)] leading-relaxed">{t("listen.newHint")}</p>
          <div
            onDragOver={(e) => { e.preventDefault(); setDrag(true); }}
            onDragLeave={() => setDrag(false)}
            onDrop={(e) => { e.preventDefault(); setDrag(false); void upload([...e.dataTransfer.files]); }}
            className={`rounded-xl border-2 border-dashed px-5 py-8 grid place-items-center text-center gap-2 transition-colors ${drag ? "border-[var(--color-accent)] bg-[color-mix(in_oklab,var(--color-accent)_8%,transparent)]" : "border-[var(--color-border)]"}`}
          >
            {busy ? <Loader2 size={22} className="animate-spin text-[var(--color-accent)]" /> : <FileAudio size={22} className="text-[var(--color-accent)]" />}
            <p className="text-[13px]">{busy ? (batch ? t("listen.batchUploading", { n: batch.done + 1 > batch.total ? batch.total : batch.done + 1, total: batch.total }) : t("listen.uploading")) : t("listen.drop")}</p>
            <button onClick={() => file.current?.click()} disabled={busy} className="px-3 py-1.5 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">{t("listen.pickFile")}</button>
            <input ref={file} type="file" accept={MEDIA} multiple hidden onChange={(e) => { const all = [...(e.target.files ?? [])]; e.target.value = ""; void upload(all); }} />
            <p className="text-[11px] text-[var(--color-muted)]">{t("listen.formats")}</p>
            {!busy && batch && <p role="status" className="text-[12px] text-[var(--color-accent)]">{t("listen.batchQueued", { n: batch.total })}</p>}
          </div>
          <UrlImport onReady={(f) => onOpen(f.pid)} />
          <MicRecorder onDone={onOpen} onError={setError} />
          {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
        </section>
        <section aria-labelledby="listen-list" className="space-y-3">
          <h2 id="listen-list" className="text-[13px] uppercase tracking-[0.14em] text-[var(--color-muted)]">{t("listen.projects")}</h2>
          {projects === null ? <Loader2 size={16} className="animate-spin text-[var(--color-muted)]" /> : projects.length === 0 ? (
            <p className="text-[13px] text-[var(--color-muted)]">{t("listen.noProjects")}</p>
          ) : (
            <ul className="space-y-1.5">
              {projects.map((p) => (
                <li key={p.pid} className="group flex items-center gap-2 rounded-lg border border-[var(--color-border)] hover:border-[var(--color-accent)] transition-colors">
                  <button onClick={() => onOpen(p.pid)} className="flex-1 min-w-0 text-left px-3 py-2">
                    <span className="block text-[13px] font-medium truncate">{p.name}</span>
                    <span className="block text-[11px] text-[var(--color-muted)] tabnum">
                      {[clock(p.duration), p.transcribed ? t("listen.cardLines", { n: p.lines, speakers: p.speakers }) : t("listen.cardNew"), when(p.created || p.mtime)].join(" · ")}
                    </span>
                  </button>
                  <button onClick={() => setRemoving(p)} className="p-2 mr-1 rounded text-[var(--color-muted)] opacity-0 group-hover:opacity-100 focus:opacity-100 hover:text-[var(--color-warn)]" {...named(t("listen.remove", { name: p.name }))}><Trash2 size={14} /></button>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
      {removing && (
        <ConfirmDialog title={t("speech.deleteProject")} message={t("listen.removeMessage", { name: removing.name })} confirmLabel={t("speech.delete")} danger
          onCancel={() => setRemoving(null)} onConfirm={async () => { await speechApi.remove(removing.pid); setRemoving(null); load(); }} />
      )}
    </main>
  );
}

/** A recording from a microphone straight into a new project: device, level, time. */
function MicRecorder({ onDone, onError }: { onDone: (pid: string) => void; onError: (e: string) => void }) {
  const { t } = useTranslation();
  const [devices, setDevices] = useState<string[] | null>(null);
  const [device, setDevice] = useState("");
  const [recording, setRecording] = useState(false);
  const [level, setLevel] = useState(0);
  const [seconds, setSeconds] = useState(0);
  const timer = useRef<number | null>(null);
  useEffect(() => {
    api.recordDevices().then((d) => { setDevices(d.devices); setDevice(d.devices[0] ?? ""); }, () => setDevices([]));
    return () => { if (timer.current) window.clearInterval(timer.current); };
  }, []);
  const start = async () => {
    try {
      await listenApi.recordStart(device || undefined);
      setRecording(true);
      const began = Date.now();
      timer.current = window.setInterval(() => {
        setSeconds(Math.floor((Date.now() - began) / 1000));
        api.recordLevel().then((l) => setLevel(l.level), () => undefined);
      }, 120);
    } catch (e) { onError(errorText(e)); }
  };
  const stop = async () => {
    if (timer.current) window.clearInterval(timer.current);
    timer.current = null;
    setRecording(false);
    setLevel(0);
    try { onDone((await listenApi.recordStop()).pid); } catch (e) { onError(errorText(e)); }
  };
  return (
    <div className="rounded-xl border border-[var(--color-border)] px-4 py-3 space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <Mic size={15} className="text-[var(--color-accent)]" />
        <span className="text-[13px] font-medium">{t("listen.mic")}</span>
        <select value={device} onChange={(e) => setDevice(e.target.value)} disabled={recording || !devices?.length} aria-label={t("voices.device")}
          className="ml-auto max-w-[55%] bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px]">
          {devices === null && <option>…</option>}
          {devices?.length === 0 && <option value="">{t("voices.noDevices")}</option>}
          {devices?.map((d) => <option key={d} value={d}>{d}</option>)}
        </select>
      </div>
      <div className="flex items-center gap-3">
        <div className="flex-1 h-1.5 rounded-full bg-[var(--color-surface-2)] overflow-hidden" role="meter" aria-label={t("voices.level")} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(Math.min(1, level * 1.4) * 100)}>
          <div className="h-full bg-[var(--color-accent)] transition-[width] duration-100" style={{ width: `${Math.min(100, level * 140)}%` }} />
        </div>
        <span className="mono text-[12px] tabnum w-12 text-right text-[var(--color-muted)]">{clock(seconds)}</span>
        {recording ? (
          <button onClick={() => void stop()} className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[var(--color-warn)] text-[var(--color-on-accent)] text-[12px] font-semibold"><Square size={12} />{t("listen.micStop")}</button>
        ) : (
          <button onClick={() => void start()} disabled={!devices?.length} className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)] disabled:opacity-40"><Mic size={12} />{t("listen.micStart")}</button>
        )}
      </div>
    </div>
  );
}
