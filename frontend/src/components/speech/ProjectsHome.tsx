import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AudioLines, Captions, Files, FolderOpen, Library, Loader2, Mic2, Plus, Trash2 } from "lucide-react";
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
  const [voice, setVoice] = useState<string | null>(null);
  const [engine, setEngine] = useState("higgs");
  const engines = useEngines();
  const [lang, setLang] = useState("auto");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [projects, setProjects] = useState<ProjectCard[] | null>(null);
  const [removing, setRemoving] = useState<ProjectCard | null>(null);
  const packFile = useRef<HTMLInputElement>(null);
  const subsFile = useRef<HTMLInputElement>(null);
  const bookFile = useRef<HTMLInputElement>(null);
  const [book, setBook] = useState<BookPreview | null>(null);
  const [podcast, setPodcast] = useState(false);
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
  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await speechApi.create({ name: name.trim() || undefined, text, voice, lang, split, engine });
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
          <div className="grid gap-2 grid-cols-2 sm:grid-cols-4">
            <label className="text-[11px] text-[var(--color-muted)] space-y-1">
              <span className="block">{t("speech.voice")}</span>
              <VoicePicker value={voice} onChange={setVoice} label={t("speech.voice")} engine={engine} className={pick} />
            </label>
            <label className="text-[11px] text-[var(--color-muted)] space-y-1">
              <span className="block">{t("speech.engine")}</span>
              <select value={engine} onChange={(e) => { setEngine(e.target.value); setVoice(null); }} className={pick}>
                {engines?.map((e) => <option key={e.id} value={e.id} disabled={!!e.unavailable} title={e.unavailable ?? undefined}>{e.name}</option>)}
              </select>
            </label>
            <label className="text-[11px] text-[var(--color-muted)] space-y-1">
              <span className="block">{t("speech.splitBy")}</span>
              <select value={split} onChange={(e) => setSplit(e.target.value === "lines" ? "lines" : "paragraphs")} title={t("speech.splitHint")} className={pick}>
                <option value="paragraphs">{t("speech.splitParagraphs")}</option>
                <option value="lines">{t("speech.splitLines")}</option>
              </select>
            </label>
            <label className="text-[11px] text-[var(--color-muted)] space-y-1">
              <span className="block">{t("speech.lang")}</span>
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
          <h2 className="pt-3 text-[13px] font-semibold">{t("speech.startWith")}</h2>
          <div className="grid gap-2 sm:grid-cols-2">
            <button onClick={() => txtFiles.current?.click()} disabled={busy} className={start}><Files size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("speech.fromFiles")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("speech.fromFilesHint")}</span></span></button>
            <button onClick={() => setPodcast(true)} disabled={busy} className={start}><Mic2 size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("podcast.open")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("podcast.openHint")}</span></span></button>
            <button onClick={() => bookFile.current?.click()} disabled={busy} className={start}><Library size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("book.open")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("book.openHint")}</span></span></button>
            <button onClick={() => subsFile.current?.click()} disabled={busy} className={start}><Captions size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("speech.fromSubs")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("speech.fromSubsHint")}</span></span></button>
            <button onClick={() => packFile.current?.click()} disabled={busy} className={start}><FolderOpen size={15} className="mt-0.5 shrink-0 text-[var(--color-accent)]" /><span><span className="block text-[13px] font-medium">{t("speech.openPack")}</span><span className="block text-[11px] text-[var(--color-muted)]">{t("speech.openPackHint")}</span></span></button>
          </div>
          <input ref={txtFiles} type="file" accept=".txt" multiple hidden onChange={(e) => { const l = e.target.files; if (l?.length) void createFromFiles(l); e.target.value = ""; }} />
          <input ref={bookFile} type="file" accept=".epub,.fb2,.pdf,.docx,.html,.htm,.md,.txt" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) void openBook(f); }} />
          <input ref={subsFile} type="file" accept=".srt,.vtt,.ass,.ssa" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) void openSubs(f); }} />
          <input ref={packFile} type="file" accept=".speechproj" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) void openPack(f); }} />
        </section>
        <section aria-labelledby="recent-title" className="space-y-2">
          <h2 id="recent-title" className="text-[13px] font-semibold">{t("speech.recent")}</h2>
          {projects === null && <Loader2 size={16} className="animate-spin text-[var(--color-muted)]" />}
          {projects?.length === 0 && <p className="text-[13px] text-[var(--color-muted)]">{t("speech.noProjects")}</p>}
          <ul className="space-y-1.5">
            {projects?.map((p) => (
              <li key={p.pid} className="group flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-accent)] transition-colors">
                <button onClick={() => onOpen(p.pid)} className="flex-1 min-w-0 text-left px-3 py-2">
                  <div className="text-[13px] font-medium truncate">{p.name || t("speech.untitled")}</div>
                  <div className="text-[11px] text-[var(--color-muted)]">{when(p.mtime)} · {t("speech.readyOf", { ready: p.ready ?? 0, total: p.blocks ?? 0 })}</div>
                </button>
                <button onClick={() => setRemoving(p)} {...named(t("speech.deleteProject"))} className="p-2 mr-1 rounded-md text-[var(--color-muted)] pointer-fine:opacity-0 pointer-fine:group-hover:opacity-100 focus:opacity-100 hover:text-[var(--color-warn)]"><Trash2 size={14} /></button>
              </li>
            ))}
          </ul>
        </section>
      </div>
      {removing && (
        <ConfirmDialog title={t("speech.deleteProject")} message={t("speech.deleteProjectMessage", { name: removing.name || t("speech.untitled") })} confirmLabel={t("speech.delete")} danger
          onCancel={() => setRemoving(null)} onConfirm={async () => { await speechApi.remove(removing.pid); setRemoving(null); load(); }} />
      )}
      {podcast && <PodcastWriter onClose={() => setPodcast(false)} onCreated={(pid) => { setPodcast(false); onOpen(pid); }} />}
      {book && <BookImport preview={book} onClose={() => setBook(null)} onCreated={(pid) => { setBook(null); onOpen(pid); }} />}
    </main>
  );
}
