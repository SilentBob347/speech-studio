import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown, ArrowUp, BookOpen, ChevronDown, Ear, FileUp, Loader2, Plus, SearchCheck, Trash2, X } from "lucide-react";
import { newEntry, speechApi, type DictEntry, type DictKind, type SpeechProject, type UnknownWord } from "../../lib/speech";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import VoicePicker from "./VoicePicker";
import { LANG_CHOICES } from "./langs";
import { followJob } from "../../lib/followJob";

type Tried = { before: { file: string; text: string }; after: { file: string; text: string } };

/** The first sentence of the project with the rule's word, for trying the rule in context. */
function phraseWith(project: SpeechProject, find: string): string {
  const word = find.trim().toLowerCase();
  if (!word) return "";
  for (const b of project.chapters.flatMap((c) => c.blocks)) {
    const at = b.text.toLowerCase().indexOf(word);
    if (at < 0) continue;
    const start = Math.max(0, ...[".", "!", "?", "\n"].map((m) => b.text.lastIndexOf(m, at) + 1));
    const ends = [".", "!", "?", "\n"].map((m) => b.text.indexOf(m, at)).filter((i) => i >= 0);
    let end = ends.length ? Math.min(...ends) + 1 : b.text.length;
    while (end < b.text.length && "»\"”’)".includes(b.text[end])) end += 1;
    return b.text.slice(start, end).trim();
  }
  return find.trim();
}

/** A rule by ear (ElevenLabs, TTS-Story): the phrase as written and with the rule, in the chosen voice. */
function TryByEar({ pid, project, entry }: { pid: string; project: SpeechProject; entry: DictEntry }) {
  const { t } = useTranslation();
  const [phrase, setPhrase] = useState("");
  const fallback = phraseWith(project, entry.find);
  const effective = phrase.trim() || fallback;
  const [voice, setVoice] = useState<string | null>(entry.voice || null);
  const [busy, setBusy] = useState(false);
  const [tried, setTried] = useState<Tried | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const { job_id } = await speechApi.tryRule(pid, { phrase: effective, rule: entry, voice });
      setTried((await followJob(job_id, "synth")) as Tried);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  return (
    <div className="sm:col-span-4 space-y-1.5 rounded-md border border-[var(--color-border)] p-2">
      <div className="flex flex-wrap items-center gap-1.5">
        <Ear size={13} className="text-[var(--color-accent)]" />
        <span className="font-medium text-[var(--color-text)]">{t("dict.tryTitle")}</span>
        <input value={phrase} onChange={(ev) => setPhrase(ev.target.value)} placeholder={fallback || t("dict.tryPhrase")} aria-label={t("dict.tryPhrase")} className={`${field} flex-1 min-w-[12rem]`} />
        <VoicePicker value={voice} onChange={setVoice} inherit={t("dict.tryProjectVoice")} label={t("dict.voice")} className={`${field} w-40`} />
        <button onClick={() => void run()} disabled={busy || !effective} className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[12px] font-medium disabled:opacity-40">
          {busy && <Loader2 size={12} className="animate-spin" />}{t("dict.tryRun")}
        </button>
      </div>
      {error && <p role="alert" className="text-[var(--color-warn)] break-words">{error}</p>}
      {tried && (["before", "after"] as const).map((k) => (
        <div key={k} className="flex flex-wrap items-center gap-2">
          <span className="w-28 shrink-0">{t(k === "before" ? "dict.tryBefore" : "dict.tryAfter")}</span>
          <audio controls preload="none" src={speechApi.tryUrl(pid, tried[k].file)} className="h-8" />
          <span className="mono break-all text-[var(--color-text)]">{tried[k].text}</span>
        </div>
      ))}
    </div>
  );
}

type Tab = "project" | "global";
const KINDS: DictKind[] = ["word", "regex", "phonemes"];
const field = "bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] focus:border-[var(--color-accent)] focus:outline-none";

/** Pronunciation dictionary of the project and of the studio: order, kinds, scope, counts, Balabolka import, unknown words. */
export default function DictionaryDialog({ pid, project, onChange, onClose }: {
  pid: string;
  project: SpeechProject;
  onChange: (f: (p: SpeechProject) => SpeechProject) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  useEscapeLayer(onClose);
  const [tab, setTab] = useState<Tab>("project");
  const [global, setGlobal] = useState<DictEntry[] | null>(null);
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [filter, setFilter] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const [unknown, setUnknown] = useState<UnknownWord[] | null>(null);
  const [looking, setLooking] = useState(false);
  const file = useRef<HTMLInputElement>(null);
  const dirtyGlobal = useRef(false);

  useEffect(() => {
    speechApi.dictionary().then((r) => setGlobal(r.entries), (e: unknown) => setError(errorText(e)));
  }, []);
  const projectEntries = project.dictionary;
  useEffect(() => {
    const timer = window.setTimeout(() => {
      speechApi.dictionaryCounts(pid).then((r) => setCounts(r.counts), () => undefined);
    }, 400);
    return () => window.clearTimeout(timer);
  }, [pid, projectEntries, global]);
  useEffect(() => {
    if (!global || !dirtyGlobal.current) return;
    const timer = window.setTimeout(() => {
      setSaving(true);
      speechApi.saveDictionary(global).then(() => { dirtyGlobal.current = false; setError(null); }, (e: unknown) => setError(errorText(e))).finally(() => setSaving(false));
    }, 600);
    return () => window.clearTimeout(timer);
  }, [global]);

  const entries = useMemo(() => (tab === "project" ? projectEntries : global ?? []), [tab, projectEntries, global]);
  const setEntries = (f: (list: DictEntry[]) => DictEntry[], target: Tab = tab) => {
    if (target === "project") onChange((p) => { p.dictionary = f([...p.dictionary]); return p; });
    else { dirtyGlobal.current = true; setGlobal((g) => f([...(g ?? [])])); }
  };
  const patch = (id: string, change: Partial<DictEntry>) => setEntries((list) => list.map((e) => (e.id === id ? { ...e, ...change } : e)));
  const move = (id: string, dir: -1 | 1) => setEntries((list) => {
    const i = list.findIndex((e) => e.id === id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= list.length) return list;
    [list[i], list[j]] = [list[j], list[i]];
    return list;
  });
  const add = (find = "", say = "", target: Tab = tab) => {
    const e = newEntry(find, say);
    setTab(target);
    setEntries((list) => [e, ...list], target);
    setOpen(e.id);
    setFilter("");
  };
  const shown = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return q ? entries.filter((e) => e.find.toLowerCase().includes(q) || e.say.toLowerCase().includes(q)) : entries;
  }, [entries, filter]);

  const importFile = async (f: File) => {
    setError(null);
    try {
      const r = await speechApi.importDictionary(f);
      setEntries((list) => [...list, ...r.entries]);
    } catch (e) { setError(errorText(e)); }
  };
  const findUnknown = async () => {
    setLooking(true);
    setError(null);
    try { setUnknown((await speechApi.unknownWords(pid)).words); } catch (e) { setError(errorText(e)); } finally { setLooking(false); }
  };

  const tabBtn = (id: Tab, label: string, n: number) => (
    <button role="tab" aria-selected={tab === id} onClick={() => setTab(id)}
      className={`px-3 py-1.5 rounded-md text-[12px] font-medium border transition-colors ${tab === id ? "border-[var(--color-accent)] bg-[color-mix(in_oklab,var(--color-accent)_14%,transparent)]" : "border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-text)]"}`}>
      {label} <span className="tabnum text-[var(--color-muted)]">{n}</span>
    </button>
  );
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center glass-scrim anim-fade" onClick={onClose}>
      <div role="dialog" aria-modal="true" aria-labelledby="dict-title" className="w-[min(96vw,1000px)] h-[min(90vh,760px)] flex flex-col rounded-xl glass-panel anim-pop" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center gap-3 px-5 pt-4 pb-3 border-b border-[var(--color-border)]">
          <BookOpen size={16} className="text-[var(--color-accent)]" />
          <h2 id="dict-title" className="font-semibold">{t("dict.title")}</h2>
          <div role="tablist" className="flex gap-1.5">
            {tabBtn("project", t("dict.project"), projectEntries.length)}
            {tabBtn("global", t("dict.global"), global?.length ?? 0)}
          </div>
          {saving && <Loader2 size={13} className="animate-spin text-[var(--color-muted)]" />}
          <button onClick={onClose} {...named(t("a11y.close"))} className="ml-auto text-[var(--color-muted)] hover:text-[var(--color-text)]"><X size={16} /></button>
        </div>
        <div className="flex flex-wrap items-center gap-2 px-5 py-2.5">
          <button onClick={() => add()} className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[12px] font-medium"><Plus size={13} />{t("dict.add")}</button>
          <button onClick={() => file.current?.click()} className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)]" title={t("dict.importHint")}><FileUp size={13} />{t("dict.import")}</button>
          <input ref={file} type="file" accept=".dic,.rex,.bxd" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) void importFile(f); }} />
          <button onClick={() => void findUnknown()} disabled={looking} className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)] disabled:opacity-40" title={t("dict.unknownHint")}>
            {looking ? <Loader2 size={13} className="animate-spin" /> : <SearchCheck size={13} />}{t("dict.unknown")}
          </button>
          <input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={t("dict.search")} aria-label={t("dict.search")} className={`${field} ml-auto w-48`} />
        </div>
        {error && <p role="alert" className="mx-5 mb-2 px-3 py-2 rounded-lg border border-[var(--color-warn)]/40 text-[12px] text-[var(--color-warn)] break-words">{error}</p>}
        <div className="flex-1 min-h-0 flex">
          <div className="flex-1 min-w-0 overflow-y-auto px-5 pb-4">
            <p className="text-[11px] text-[var(--color-muted)] mb-2">{t(tab === "project" ? "dict.projectHint" : "dict.globalHint")}</p>
            {!shown.length && <p className="py-10 text-center text-[13px] text-[var(--color-muted)]">{filter ? t("dict.nothingFound") : t("dict.empty")}</p>}
            <ol className="space-y-1">
              {shown.map((e, i) => {
                const n = counts[e.id] ?? 0;
                const expanded = open === e.id;
                return (
                  <li key={e.id} className={`rounded-lg border ${e.enabled ? "border-[var(--color-border)]" : "border-dashed border-[var(--color-border)] opacity-60"}`}>
                    <div className="flex items-center gap-1.5 px-2 py-1.5">
                      <input type="checkbox" checked={e.enabled} onChange={(ev) => patch(e.id, { enabled: ev.target.checked })} {...named(t("dict.enabled"))} className="w-4 h-4 accent-[var(--color-accent)]" />
                      <select value={e.kind} onChange={(ev) => patch(e.id, { kind: ev.target.value as DictKind })} aria-label={t("dict.kind")} className={`${field} w-28`}>
                        {KINDS.map((k) => <option key={k} value={k}>{t(`dict.kind_${k}`)}</option>)}
                      </select>
                      <input value={e.find} onChange={(ev) => patch(e.id, { find: ev.target.value })} placeholder={t("dict.find")} aria-label={t("dict.find")} className={`${field} flex-1 min-w-0 mono`} />
                      <span className="text-[var(--color-muted)]">→</span>
                      <input value={e.say} onChange={(ev) => patch(e.id, { say: ev.target.value })} placeholder={t(e.kind === "phonemes" ? "dict.sayPhonemes" : "dict.say")} aria-label={t("dict.say")} className={`${field} flex-1 min-w-0 mono`} />
                      <span title={t("dict.count")} className={`w-9 text-right tabnum text-[11px] ${n ? "text-[var(--color-accent)]" : "text-[var(--color-muted)]"}`}>{n}</span>
                      <button onClick={() => move(e.id, -1)} disabled={i === 0 || !!filter} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("dict.up"))}><ArrowUp size={12} /></button>
                      <button onClick={() => move(e.id, 1)} disabled={i === shown.length - 1 || !!filter} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("dict.down"))}><ArrowDown size={12} /></button>
                      <button onClick={() => setOpen(expanded ? null : e.id)} aria-expanded={expanded} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("dict.more"))}><ChevronDown size={13} className={expanded ? "rotate-180" : ""} /></button>
                      <button onClick={() => setEntries((list) => list.filter((x) => x.id !== e.id))} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-warn)]" {...named(t("dict.remove"))}><Trash2 size={12} /></button>
                    </div>
                    {expanded && (
                      <div className="grid gap-2 sm:grid-cols-4 px-2 pb-2 text-[11px] text-[var(--color-muted)]">
                        <label className="flex items-center gap-1.5"><input type="checkbox" checked={e.case_sensitive} onChange={(ev) => patch(e.id, { case_sensitive: ev.target.checked })} className="accent-[var(--color-accent)]" />{t("dict.caseSensitive")}</label>
                        <label className="flex items-center gap-1.5"><input type="checkbox" checked={e.whole_word} disabled={e.kind === "regex"} onChange={(ev) => patch(e.id, { whole_word: ev.target.checked })} className="accent-[var(--color-accent)]" />{t("dict.wholeWord")}</label>
                        <label>{t("dict.lang")}
                          <select value={e.lang} onChange={(ev) => patch(e.id, { lang: ev.target.value })} className={`${field} mt-1 w-full`}>
                            <option value="">{t("dict.any")}</option>
                            {LANG_CHOICES.filter((l) => l !== "auto").map((l) => <option key={l} value={l}>{l.toUpperCase()}</option>)}
                          </select>
                        </label>
                        <label>{t("dict.voice")}
                          <VoicePicker value={e.voice || null} onChange={(v) => patch(e.id, { voice: v ?? "" })} inherit={t("dict.any")} label={t("dict.voice")} className={`${field} mt-1 w-full`} />
                        </label>
                        <TryByEar pid={pid} project={project} entry={e} />
                        <label className="sm:col-span-4">{t("dict.note")}
                          <input value={e.note} onChange={(ev) => patch(e.id, { note: ev.target.value })} className={`${field} mt-1 w-full`} />
                        </label>
                        <p className="sm:col-span-4">{t(`dict.kindHint_${e.kind}`)}{e.source !== "manual" ? ` · ${t("dict.from", { source: e.source })}` : ""}</p>
                      </div>
                    )}
                  </li>
                );
              })}
            </ol>
          </div>
          {unknown && (
            <aside aria-label={t("dict.unknown")} className="w-64 shrink-0 border-l border-[var(--color-border)] overflow-y-auto px-3 py-2">
              <div className="flex items-center justify-between mb-1.5">
                <h3 className="text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)]">{t("dict.unknown")}</h3>
                <button onClick={() => setUnknown(null)} className="text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("a11y.close"))}><X size={13} /></button>
              </div>
              {!unknown.length && <p className="text-[12px] text-[var(--color-muted)]">{t("dict.unknownNone")}</p>}
              <ul className="space-y-0.5">
                {unknown.map((w) => (
                  <li key={w.word} className="group flex items-center gap-1.5 text-[12px]">
                    <span className="flex-1 min-w-0 truncate" title={t(`dict.why_${w.why}`)}>{w.word}</span>
                    <span className="tabnum text-[10px] text-[var(--color-muted)]">{w.count}</span>
                    <button onClick={() => add(w.word, w.word, "project")} className="p-0.5 rounded text-[var(--color-muted)] hover:text-[var(--color-accent)]" {...named(t("dict.addWord", { word: w.word }))}><Plus size={12} /></button>
                  </li>
                ))}
              </ul>
            </aside>
          )}
        </div>
      </div>
    </div>
  );
}
