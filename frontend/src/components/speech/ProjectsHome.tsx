import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AudioLines, Captions, Files, FlaskConical, FolderDown, FolderOpen, Library, Loader2, Mic2, Plus, Sparkles, Trash2 } from "lucide-react";
import BookImport from "./BookImport";
import PodcastWriter from "./PodcastWriter";
import { speechApi, type BookPreview, type ProjectCard } from "../../lib/speech";
import { errorText } from "../../lib/voices";
import { PROJECTS_CHANGED, useChanged } from "../../lib/mcpBridge";
import { named } from "../../lib/a11y";
import ConfirmDialog from "../ConfirmDialog";
import VoicePicker from "./VoicePicker";
import { LANG_CHOICES } from "./langs";
import { useEngines } from "../../lib/engines";
import { api } from "../../lib/api";
import { CLOUD_ENGINE_NAMES, askCloud } from "../../lib/cloudConsent";
import BatchList, { type BatchItem } from "../BatchList";
import RenderLibrary from "./RenderLibrary";
import QuickSpeak from "./QuickSpeak";
import type { ExportFormat } from "../../lib/speech";
import Hint from "../Hint";

const FORMATS: ExportFormat[] = ["mp3", "wav", "flac", "ogg", "m4a"];

const pick = "w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-2 py-1.5 text-[12px]";
const start = "flex items-start gap-2.5 text-left rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2.5 hover:border-[var(--color-accent)] disabled:opacity-40 transition-colors";
const field = "w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-3 py-2 text-[13px] focus:border-[var(--color-accent)] focus:outline-none";

/** Speech's start: a new project from a text, and the projects made before. */
export default function ProjectsHome({ onOpen }: { onOpen: (pid: string) => void }) {
  const { t, i18n } = useTranslation();
  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [split, setSplit] = useState<"paragraphs" | "lines">("paragraphs");
  const txtFiles = useRef<HTMLInputElement>(null);
  const batchFiles = useRef<HTMLInputElement>(null);
  const [batch, setBatch] = useState<BatchItem[] | null>(null);
  const [batchRunning, setBatchRunning] = useState(false);
  const [format, setFormat] = useState<ExportFormat>("mp3");
  const [voice, setVoice] = useState<string | null>(null);
  const [engine, setEngine] = useState("higgs");
  const engines = useEngines();
  const [lang, setLang] = useState("auto");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [projects, setProjects] = useState<ProjectCard[] | null>(null);
  const [find, setFind] = useState("");
  const [tab, setTab] = useState<"projects" | "renders">("projects");
  const [removing, setRemoving] = useState<ProjectCard | null>(null);
  const packFile = useRef<HTMLInputElement>(null);
  const subsFile = useRef<HTMLInputElement>(null);
  const bookFile = useRef<HTMLInputElement>(null);
  const [book, setBook] = useState<BookPreview | null>(null);
  const [podcast, setPodcast] = useState(false);
  const [quick, setQuick] = useState(false);
  const openBook = async (f: File) => {
    setBusy(true);
    setError(null);
    try { setBook(await speechApi.bookPreview(f)); } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const openSubs = async (f: File) => {
    setBusy(true);
    setError(null);
    try { onOpen((await speechApi.fromSubtitles(f, name.trim() || undefined)).project_id); } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const openPack = async (f: File) => {
    setBusy(true);
    setError(null);
    try { onOpen((await speechApi.importPackage(f)).project_id); } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const load = () => { speechApi.list().then((r) => setProjects(r.projects.filter((p) => p.kind === "speech")), (e: unknown) => setError(errorText(e))); };
  useEffect(load, []);
  useChanged(PROJECTS_CHANGED, load);

  const createFromFiles = async (list: FileList) => {
    setBusy(true);
    setError(null);
    try {
      const files = await Promise.all([...list].map(async (f) => ({ name: f.name, text: await f.text() })));
      const r = await speechApi.create({ name: name.trim() || undefined, text: "", voice, lang, files, engine });
      onOpen(r.project_id);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  /** Every text file becomes its own project with the settings above and is voiced, one after another. */
  const runBatch = async (list: FileList) => {
    const files = [...list];
    const provider = CLOUD_ENGINE_NAMES[engine];
    if (provider && !(await askCloud("speech", provider))) return;
    setError(null);
    setBatch(files.map((f) => ({ name: f.name, status: "queued", pct: 0, pid: null, files: [] })));
    setBatchRunning(true);
    for (const [i, f] of files.entries()) {
      const upd = (patch: Partial<BatchItem>) => setBatch((xs) => xs && xs.map((x, j) => (j === i ? { ...x, ...patch } : x)));
      try {
        upd({ status: "working", pct: 0 });
        const r = await speechApi.create({ name: f.name.replace(/\.[^.]+$/, ""), text: await f.text(), voice, lang, split, engine });
        upd({ pid: r.project_id });
        const { job_id } = await speechApi.synth(r.project_id, {});
        const result = (await api.watchJob(job_id, (e) => { if (e.type === "progress") upd({ pct: e.pct ?? 0, detail: e.msg || undefined }); })) as { failed?: { error: string }[] };
        if (result.failed?.length) upd({ status: "error", error: t("batch.blocksFailed", { n: result.failed.length, error: result.failed[0].error }) });
        else upd({ status: "done", pct: 100, detail: undefined });
      } catch (e) {
        upd({ status: "error", error: errorText(e) });
      }
    }
    setBatchRunning(false);
    load();
  };
  /** Exports every voiced project of the batch in one format and copies the files into one folder. */
  const saveBatch = async () => {
    let folder: string | undefined;
    let copied = 0;
    for (const [i, it] of (batch ?? []).entries()) {
      if (it.status !== "done" || !it.pid) continue;
      setBatch((xs) => xs && xs.map((x, j) => (j === i ? { ...x, status: "working", pct: 0, detail: t("batch.exporting") } : x)));
      try {
        const { job_id } = await speechApi.export(it.pid, format);
        const r = (await api.watchJob(job_id, () => undefined)) as { file: string };
        const out = await speechApi.exportsToFolder(it.pid, [r.file], folder);
        if (!out.folder) return null;
        folder = out.folder;
        copied += out.copied ?? 0;
      } finally {
        setBatch((xs) => xs && xs.map((x, j) => (j === i ? { ...x, status: "done", pct: 100, detail: undefined } : x)));
      }
    }
    return folder ? { folder, copied } : null;
  };
  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await speechApi.create({ name: name.trim() || undefined, text, voice, lang, split, engine });
      onOpen(r.project_id);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const createCorpus = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await speechApi.create({ text: "", corpus: "ru_hard", voice, engine });
      onOpen(r.project_id);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const when = (s: number) => new Date(s * 1000).toLocaleString(i18n.language, { dateStyle: "medium", timeStyle: "short" });

  return (
    <main className="flex-1 min-h-0 overflow-y-auto">
      <div className="max-w-5xl mx-auto px-6 py-8 grid gap-8 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        <section aria-labelledby="new-title" className="space-y-3">
          <h1 id="new-title" className="text-xl font-bold tracking-tight flex items-center gap-2"><AudioLines size={20} className="text-[var(--color-accent)]" />{t("speech.newTitle")}</h1>
          <p className="text-[13px] text-[var(--color-muted)] leading-relaxed">{t("speech.newHint")}</p>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder={t("speech.namePlaceholder")} aria-label={t("speech.name")} className={field} maxLength={120} />
          <textarea value={text} onChange={(e) => setText(e.target.value)} rows={8} placeholder={t("speech.textPlaceholder")} aria-label={t("speech.text")}
            className={`${field} resize-y leading-relaxed min-h-[160px]`} />
          {!text.trim() && (
            <button onClick={() => { setText(t("speech.sample")); setSplit("paragraphs"); }} className="-mt-1 text-[12px] text-[var(--color-accent)] hover:underline">{t("speech.sampleInsert")}</button>
          )}
          <div className="grid gap-2 grid-cols-2">
            <label className="text-[11px] text-[var(--color-muted)] space-y-1">
              <span className="block">{t("speech.voice")} <Hint text={t("hint.newVoice")} /></span>
              <VoicePicker value={voice} onChange={setVoice} label={t("speech.voice")} engine={engine} className={pick} />
            </label>
            <label className="text-[11px] text-[var(--color-muted)] space-y-1">
              <span className="block">{t("speech.engine")} <Hint text={t("hint.engine")} /></span>
              <select value={engine} onChange={(e) => { setEngine(e.target.value); setVoice(null); }} className={pick}>
                {engines?.map((e) => <option key={e.id} value={e.id} disabled={!!e.unavailable} title={e.unavailable ?? undefined}>{e.name}</option>)}
              </select>
            </label>
            <label className="text-[11px] text-[var(--color-muted)] space-y-1">
              <span className="block">{t("speech.splitBy")} <Hint text={t("speech.splitHint")} /></span>
              <select value={split} onChange={(e) => setSplit(e.target.value === "lines" ? "lines" : "paragraphs")} className={pick}>
                <option value="paragraphs">{t("speech.splitParagraphs")}</option>
                <option value="lines">{t("speech.splitLines")}</option>
              </select>
            </label>
            <label className="text-[11px] text-[var(--color-muted)] space-y-1">
              <span className="block">{t("speech.lang")} <Hint text={t("hint.lang")} /></span>
              <select value={lang} onChange={(e) => setLang(e.target.value)} className={pick}>
                {LANG_CHOICES.map((l) => <option key={l} value={l}>{l === "auto" ? t("speech.langAuto") : l.toUpperCase()}</option>)}
              </select>
            </label>
          </div>
          <div className="flex items-start gap-3">
            <p className="flex-1 text-[11px] text-[var(--color-muted)] leading-relaxed">{t("speech.markupHint")}</p>
            <button onClick={create} disabled={busy} className="shrink-0 inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">
              {busy ? <Loader2 size={14} className="animate-spin" /> : <Plus size={14} />}{t("speech.create")}
            </button>
          </div>
          {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
          {batch && (
            <BatchList title={t("batch.speechTitle")} items={batch} running={batchRunning} onOpen={onOpen} onSaveAll={saveBatch} onClose={() => setBatch(null)}
              options={
                <label className="inline-flex items-center gap-1.5 text-[12px] text-[var(--color-muted)]">{t("speech.format")}
                  <select value={format} onChange={(e) => setFormat(e.target.value as ExportFormat)} className="bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-1.5 py-1 text-[12px] text-[var(--color-text)]">
                    {FORMATS.map((f) => <option key={f} value={f}>{f.toUpperCase()}</option>)}
                  </select>
                </label>
              } />
          )}
          <h2 className="pt-3 text-[13px] font-semibold">{t("speech.startWith")}</h2>
          <div className="grid gap-2 sm:grid-cols-2">
            <button onClick={() => txtFiles.current?.click()} disabled={busy} className={start}><Files size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("speech.fromFiles")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("speech.fromFilesHint")}</span></span></button>
            <button onClick={() => batchFiles.current?.click()} disabled={busy || batchRunning} className={start}><FolderDown size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("batch.speechOpen")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("batch.speechHint")}</span></span></button>
            <button onClick={() => setQuick(true)} disabled={busy} className={start}><Sparkles size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("quick.title")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("quick.hint")}</span></span></button>
            <button onClick={() => setPodcast(true)} disabled={busy} className={start}><Mic2 size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("podcast.open")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("podcast.openHint")}</span></span></button>
            <button onClick={() => bookFile.current?.click()} disabled={busy} className={start}><Library size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("book.open")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("book.openHint")}</span></span></button>
            <button onClick={() => subsFile.current?.click()} disabled={busy} className={start}><Captions size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("speech.fromSubs")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("speech.fromSubsHint")}</span></span></button>
            <button onClick={() => packFile.current?.click()} disabled={busy} className={start}><FolderOpen size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("speech.openPack")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("speech.openPackHint")}</span></span></button>
            <button onClick={() => void createCorpus()} disabled={busy} className={start}><FlaskConical size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("speech.corpus")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("speech.corpusHint")}</span></span></button>
          </div>
          <input ref={batchFiles} type="file" accept=".txt,.md" multiple hidden onChange={(e) => { const l = e.target.files; if (l?.length) void runBatch(l); e.target.value = ""; }} />
          <input ref={txtFiles} type="file" accept=".txt" multiple hidden onChange={(e) => { const l = e.target.files; if (l?.length) void createFromFiles(l); e.target.value = ""; }} />
          <input ref={bookFile} type="file" accept=".epub,.fb2,.pdf,.docx,.html,.htm,.md,.txt" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) void openBook(f); }} />
          <input ref={subsFile} type="file" accept=".srt,.vtt,.ass,.ssa" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) void openSubs(f); }} />
          <input ref={packFile} type="file" accept=".speechproj" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) void openPack(f); }} />
        </section>
        <section aria-labelledby="recent-title" className="space-y-2">
          <div role="tablist" aria-labelledby="recent-title" className="flex items-center gap-1">
            <h2 id="recent-title" className="sr-only">{t("speech.recent")}</h2>
            {(["projects", "renders"] as const).map((id) => (
              <button key={id} role="tab" aria-selected={tab === id} onClick={() => setTab(id)}
                className={`px-2.5 py-1 rounded-md text-[13px] font-semibold ${tab === id ? "bg-[var(--color-surface-2)] text-[var(--color-text)]" : "text-[var(--color-muted)] hover:text-[var(--color-text)]"}`}>{id === "projects" ? t("speech.recent") : t("renders.title")}</button>
            ))}
          </div>
          {(tab === "renders" || (projects?.length ?? 0) > 5) && (
            <input value={find} onChange={(e) => setFind(e.target.value)} placeholder={tab === "renders" ? t("renders.find") : t("speech.findProject")} aria-label={tab === "renders" ? t("renders.find") : t("speech.findProject")}
              className="w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-3 py-1.5 text-[12px] focus:border-[var(--color-accent)] focus:outline-none" />
          )}
          {tab === "renders" && <RenderLibrary find={find} onOpen={onOpen} />}
          {tab === "projects" && <>
          {projects === null && <Loader2 size={16} className="animate-spin text-[var(--color-muted)]" />}
          {projects?.length === 0 && <p className="text-[13px] text-[var(--color-muted)]">{t("speech.noProjects")}</p>}
          <ul className="space-y-1.5">
            {projects?.filter((p) => !find.trim() || (p.name || "").toLowerCase().includes(find.trim().toLowerCase())).map((p) => (
              <li key={p.pid} className="group flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-accent)] transition-colors">
                <button onClick={() => onOpen(p.pid)} className="flex-1 min-w-0 text-left px-3 py-2">
                  <div className="text-[13px] font-medium truncate">{p.name || t("speech.untitled")}</div>
                  <div className="text-[11px] text-[var(--color-muted)]">{when(p.mtime)} · {t("speech.readyOf", { ready: p.ready ?? 0, total: p.blocks ?? 0 })}</div>
                </button>
                <button onClick={() => setRemoving(p)} {...named(t("speech.deleteProject"))} className="p-2 mr-1 rounded-md text-[var(--color-muted)] pointer-fine:opacity-0 pointer-fine:group-hover:opacity-100 focus:opacity-100 hover:text-[var(--color-warn)]"><Trash2 size={14} /></button>
              </li>
            ))}
          </ul>
          </>}
        </section>
      </div>
      {removing && (
        <ConfirmDialog title={t("speech.deleteProject")} message={t("speech.deleteProjectMessage", { name: removing.name || t("speech.untitled") })} confirmLabel={t("speech.delete")} danger
          onCancel={() => setRemoving(null)} onConfirm={async () => { await speechApi.remove(removing.pid); setRemoving(null); load(); }} />
      )}
      {quick && <QuickSpeak onClose={() => setQuick(false)} onOpen={(pid) => { setQuick(false); onOpen(pid); }} />}
      {podcast && <PodcastWriter onClose={() => setPodcast(false)} onCreated={(pid) => { setPodcast(false); onOpen(pid); }} />}
      {book && <BookImport preview={book} onClose={() => setBook(null)} onCreated={(pid) => { setBook(null); onOpen(pid); }} />}
    </main>
  );
}
