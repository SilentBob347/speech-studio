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

const field = "w-full bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-3 py-2 text-[13px] focus:border-[var(--color-accent)] focus:outline-none";

/** Speech's start: a new project from a text, and the projects made before. */
export default function ProjectsHome({ onOpen }: { onOpen: (pid: string) => void }) {
  const { t, i18n } = useTranslation();
  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [split, setSplit] = useState<"paragraphs" | "lines">("paragraphs");
  const txtFiles = useRef<HTMLInputElement>(null);
  const [voice, setVoice] = useState<string | null>(null);
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
      const r = await speechApi.create({ name: name.trim() || undefined, text: "", voice, lang, files });
      onOpen(r.project_id);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await speechApi.create({ name: name.trim() || undefined, text, voice, lang, split });
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
          <textarea value={text} onChange={(e) => setText(e.target.value)} rows={12} placeholder={t("speech.textPlaceholder")} aria-label={t("speech.text")}
            className={`${field} resize-y leading-relaxed min-h-[220px]`} />
          <div className="flex flex-wrap items-center gap-2">
            <VoicePicker value={voice} onChange={setVoice} label={t("speech.voice")} />
            <select value={split} onChange={(e) => setSplit(e.target.value === "lines" ? "lines" : "paragraphs")} aria-label={t("speech.split")} title={t("speech.splitHint")} className="bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-2 py-1.5 text-[12px]">
              <option value="paragraphs">{t("speech.splitParagraphs")}</option>
              <option value="lines">{t("speech.splitLines")}</option>
            </select>
            <button onClick={() => txtFiles.current?.click()} disabled={busy} className="inline-flex items-center gap-1 px-2 py-1.5 rounded-lg border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)] disabled:opacity-40" title={t("speech.fromFilesHint")}><Files size={13} />{t("speech.fromFiles")}</button>
            <input ref={txtFiles} type="file" accept=".txt" multiple hidden onChange={(e) => { const l = e.target.files; if (l?.length) void createFromFiles(l); e.target.value = ""; }} />
            <select value={lang} onChange={(e) => setLang(e.target.value)} aria-label={t("speech.lang")} className="bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-lg px-2 py-1.5 text-[12px]">
              {LANG_CHOICES.map((l) => <option key={l} value={l}>{l === "auto" ? t("speech.langAuto") : l.toUpperCase()}</option>)}
            </select>
            <button onClick={create} disabled={busy} className="ml-auto inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">
              {busy ? <Loader2 size={14} className="animate-spin" /> : <Plus size={14} />}{t("speech.create")}
            </button>
          </div>
          <p className="text-[11px] text-[var(--color-muted)] leading-relaxed">{t("speech.markupHint")}</p>
          {error && <p role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
        </section>
        <section aria-labelledby="recent-title" className="space-y-2">
          <div className="flex flex-wrap items-center gap-1.5">
            <h2 id="recent-title" className="basis-full text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)]">{t("speech.recent")}</h2>
            <button onClick={() => setPodcast(true)} disabled={busy} className="inline-flex items-center gap-1 px-2 py-1 rounded-md border border-[var(--color-border)] text-[11px] whitespace-nowrap hover:border-[var(--color-accent)] disabled:opacity-40" title={t("podcast.openHint")}><Mic2 size={12} />{t("podcast.open")}</button>
            <button onClick={() => bookFile.current?.click()} disabled={busy} className="inline-flex items-center gap-1 px-2 py-1 rounded-md border border-[var(--color-border)] text-[11px] whitespace-nowrap hover:border-[var(--color-accent)] disabled:opacity-40" title={t("book.openHint")}><Library size={12} />{t("book.open")}</button>
            <input ref={bookFile} type="file" accept=".epub,.fb2,.pdf,.docx,.html,.htm,.md,.txt" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) void openBook(f); }} />
            <button onClick={() => subsFile.current?.click()} disabled={busy} className="inline-flex items-center gap-1 px-2 py-1 rounded-md border border-[var(--color-border)] text-[11px] whitespace-nowrap hover:border-[var(--color-accent)] disabled:opacity-40" title={t("speech.fromSubsHint")}><Captions size={12} />{t("speech.fromSubs")}</button>
            <input ref={subsFile} type="file" accept=".srt,.vtt,.ass,.ssa" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) void openSubs(f); }} />
            <button onClick={() => packFile.current?.click()} disabled={busy} className="inline-flex items-center gap-1 px-2 py-1 rounded-md border border-[var(--color-border)] text-[11px] whitespace-nowrap hover:border-[var(--color-accent)] disabled:opacity-40" title={t("speech.openPackHint")}><FolderOpen size={12} />{t("speech.openPack")}</button>
            <input ref={packFile} type="file" accept=".speechproj" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) void openPack(f); }} />
          </div>
          {projects === null && <Loader2 size={16} className="animate-spin text-[var(--color-muted)]" />}
          {projects?.length === 0 && <p className="text-[13px] text-[var(--color-muted)]">{t("speech.noProjects")}</p>}
          <ul className="space-y-1.5">
            {projects?.map((p) => (
              <li key={p.pid} className="group flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-accent)] transition-colors">
                <button onClick={() => onOpen(p.pid)} className="flex-1 min-w-0 text-left px-3 py-2">
                  <div className="text-[13px] font-medium truncate">{p.name || t("speech.untitled")}</div>
                  <div className="text-[11px] text-[var(--color-muted)]">{when(p.mtime)} · {t("speech.readyOf", { ready: p.ready ?? 0, total: p.blocks ?? 0 })}</div>
                </button>
                <button onClick={() => setRemoving(p)} {...named(t("speech.deleteProject"))} className="p-2 mr-1 rounded-md text-[var(--color-muted)] opacity-0 group-hover:opacity-100 focus:opacity-100 hover:text-[var(--color-warn)]"><Trash2 size={14} /></button>
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
