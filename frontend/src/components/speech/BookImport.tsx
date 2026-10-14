import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { BookOpen, ChevronDown, Loader2, X } from "lucide-react";
import { speechApi, type BookPreview } from "../../lib/speech";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import VoicePicker from "./VoicePicker";


/** The structure of a book before synthesis: which chapters to voice, their titles and text, the voice; then a project. */
export default function BookImport({ preview, onClose, onCreated }: { preview: BookPreview; onClose: () => void; onCreated: (pid: string) => void }) {
  const { t } = useTranslation();
  useEscapeLayer(onClose);
  const duration = (secs: number) => {
    const m = Math.max(1, Math.round(secs / 60));
    return m < 60 ? t("book.minutes", { m }) : t("book.hours", { h: Math.floor(m / 60), m: m % 60 });
  };
  const [picked, setPicked] = useState<Record<number, boolean>>(() => Object.fromEntries(preview.chapters.map((c) => [c.index, !c.skip])));
  const [titles, setTitles] = useState<Record<number, string>>(() => Object.fromEntries(preview.chapters.map((c) => [c.index, c.title])));
  const [texts, setTexts] = useState<Record<number, string>>({});
  const [open, setOpen] = useState<number | null>(null);
  const [name, setName] = useState(preview.title);
  const [voice, setVoice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const chosen = preview.chapters.filter((c) => picked[c.index]);
  const total = useMemo(() => chosen.reduce((s, c) => s + c.est_secs, 0), [chosen]);
  const edit = async (index: number) => {
    if (open === index) { setOpen(null); return; }
    setOpen(index);
    if (texts[index] === undefined) {
      try { const c = await speechApi.bookChapter(preview.id, index); setTexts((x) => ({ ...x, [index]: c.text })); } catch (e) { setError(errorText(e)); }
    }
  };
  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await speechApi.fromBook({
        id: preview.id, name: name.trim() || undefined, voice,
        chapters: chosen.map((c) => ({ index: c.index, title: titles[c.index], text: texts[c.index] })),
      });
      onCreated(r.project_id);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const field = "bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] focus:border-[var(--color-accent)] focus:outline-none";
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade" onClick={() => { if (!busy) onClose(); }}>
      <div role="dialog" aria-modal="true" aria-labelledby="book-title" className="w-[min(96vw,860px)] h-[min(90vh,780px)] flex flex-col rounded-xl glass-panel anim-pop" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center gap-3 px-5 pt-4 pb-3 border-b border-[var(--color-border)]">
          <BookOpen size={16} className="text-[var(--color-accent)]" />
          <h2 id="book-title" className="font-semibold">{t("book.title")}</h2>
          <span className="text-[12px] text-[var(--color-muted)] truncate">{[preview.author, preview.lang.toUpperCase()].filter(Boolean).join(" · ")}</span>
          <button onClick={onClose} disabled={busy} {...named(t("a11y.close"))} className="ml-auto text-[var(--color-muted)] hover:text-[var(--color-text)]"><X size={16} /></button>
        </div>
        <div className="flex flex-wrap items-center gap-2 px-5 py-2.5">
          <input value={name} onChange={(e) => setName(e.target.value)} aria-label={t("speech.name")} className={`${field} flex-1 min-w-48 text-[13px]`} />
          <VoicePicker value={voice} onChange={setVoice} label={t("speech.voice")} />
          <button onClick={() => setPicked(Object.fromEntries(preview.chapters.map((c) => [c.index, true])))} className="px-2 py-1 rounded-md border border-[var(--color-border)] text-[11px] hover:border-[var(--color-accent)]">{t("book.all")}</button>
          <button onClick={() => setPicked(Object.fromEntries(preview.chapters.map((c) => [c.index, !c.skip])))} className="px-2 py-1 rounded-md border border-[var(--color-border)] text-[11px] hover:border-[var(--color-accent)]">{t("book.suggested")}</button>
        </div>
        <ol className="flex-1 min-h-0 overflow-y-auto px-5 pb-3 space-y-1">
          {preview.chapters.map((c) => (
            <li key={c.index} className={`rounded-lg border ${picked[c.index] ? "border-[var(--color-border)]" : "border-dashed border-[var(--color-border)] opacity-60"}`}>
              <div className="flex items-center gap-2 px-2 py-1.5">
                <input type="checkbox" checked={!!picked[c.index]} onChange={(e) => setPicked((p) => ({ ...p, [c.index]: e.target.checked }))} {...named(t("book.voiceIt"))} className="w-4 h-4 accent-[var(--color-accent)]" />
                <input value={titles[c.index] ?? ""} onChange={(e) => setTitles((x) => ({ ...x, [c.index]: e.target.value }))} placeholder={t("book.untitled")} aria-label={t("book.chapterTitle")} className={`${field} flex-1 min-w-0`} />
                {c.skip && <span className="text-[10px] text-[var(--color-warn)]" title={t("book.skipHint")}>{t(`book.skip_${c.skip}`)}</span>}
                <span className="w-24 text-right text-[11px] tabnum text-[var(--color-muted)]">{duration(c.est_secs)}</span>
                <button onClick={() => void edit(c.index)} aria-expanded={open === c.index} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("book.editText"))}><ChevronDown size={13} className={open === c.index ? "rotate-180" : ""} /></button>
              </div>
              {open === c.index ? (
                texts[c.index] === undefined ? <Loader2 size={13} className="m-2 animate-spin text-[var(--color-muted)]" /> : (
                  <textarea value={texts[c.index]} onChange={(e) => setTexts((x) => ({ ...x, [c.index]: e.target.value }))} rows={10} aria-label={t("book.chapterText")}
                    className={`${field} mx-2 mb-2 w-[calc(100%-1rem)] resize-y leading-relaxed text-[12px]`} />
                )
              ) : (
                <p className="px-9 pb-1.5 text-[11px] text-[var(--color-muted)] line-clamp-2">{c.head}</p>
              )}
            </li>
          ))}
        </ol>
        <div className="flex flex-wrap items-center gap-3 px-5 py-3 border-t border-[var(--color-border)]">
          <span className="text-[12px] text-[var(--color-muted)]">{t("book.summary", { n: chosen.length, of: preview.chapters.length, time: duration(total) })}</span>
          {error && <span role="alert" className="text-[12px] text-[var(--color-warn)] break-words">{error}</span>}
          <button onClick={() => void create()} disabled={busy || !chosen.length} className="ml-auto inline-flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[13px] font-semibold disabled:opacity-40">
            {busy && <Loader2 size={14} className="animate-spin" />}{t("book.create")}
          </button>
        </div>
      </div>
    </div>
  );
}
