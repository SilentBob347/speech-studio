import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronUp, Replace, X } from "lucide-react";
import type { Block } from "../../lib/speech";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";

type Found = { re: RegExp | null; error: string | null };

function compile(find: string, regex: boolean, cased: boolean): Found {
  if (!find) return { re: null, error: null };
  try {
    const source = regex ? find : find.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    return { re: new RegExp(source, cased ? "gu" : "giu"), error: null };
  } catch (e) {
    return { re: null, error: e instanceof Error ? e.message : String(e) };
  }
}

/** Find and replace over the project's blocks; with a regex, $1 in the replacement inserts a group. Replaced blocks go stale by themselves. */
export default function SearchReplace({ blocks, onReplace, onJump, onFound, onClose }: {
  blocks: Block[];
  onReplace: (changes: Record<string, string>) => void;
  onJump: (id: string) => void;
  /** What the blocks highlight: the pattern and the block the last jump went to. */
  onFound: (re: RegExp | null, current: string | null) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  useEscapeLayer(onClose, true, false);
  const [find, setFind] = useState("");
  const [replace, setReplace] = useState("");
  const [regex, setRegex] = useState(false);
  const [cased, setCased] = useState(false);
  const [at, setAt] = useState(0);
  const { re, error } = useMemo(() => compile(find, regex, cased), [find, regex, cased]);
  const hits = useMemo(() => {
    if (!re) return [];
    return blocks.flatMap((b) => {
      const n = [...b.text.matchAll(re)].filter((m) => m[0].length > 0).length;
      return n ? [{ id: b.id, n }] : [];
    });
  }, [blocks, re]);
  const total = hits.reduce((s, h) => s + h.n, 0);
  const current = hits.length ? hits[Math.min(at, hits.length - 1)].id : null;
  useEffect(() => onFound(re, current), [re, current, onFound]);
  const jump = (dir: 1 | -1) => {
    if (!hits.length) return;
    const next = (at + dir + hits.length) % hits.length;
    setAt(next);
    onJump(hits[next].id);
  };
  const replaceIn = (ids: string[]) => {
    if (!re) return;
    const changes: Record<string, string> = {};
    for (const b of blocks.filter((x) => ids.includes(x.id))) {
      const text = b.text.replace(re, replace);
      if (text !== b.text) changes[b.id] = text;
    }
    onReplace(changes);
  };
  const field = "bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-md px-2 py-1 text-[12px] focus:border-[var(--color-accent)] focus:outline-none";
  const toggle = (on: boolean) => `px-1.5 py-1 rounded-md border text-[11px] mono ${on ? "border-[var(--color-accent)] text-[var(--color-accent)]" : "border-[var(--color-border)] text-[var(--color-muted)]"}`;
  return (
    <div role="search" className="flex flex-wrap items-center gap-2 px-5 py-2 border-b border-[var(--color-border)] bg-[var(--color-surface)]/80">
      <input autoFocus value={find} onChange={(e) => { setFind(e.target.value); setAt(0); }} onKeyDown={(e) => { if (e.key === "Enter") jump(e.shiftKey ? -1 : 1); }}
        placeholder={t("search.find")} aria-label={t("search.find")} className={`${field} w-56`} />
      <button onClick={() => setRegex((v) => !v)} aria-pressed={regex} className={toggle(regex)} title={t("search.regex")}>.*</button>
      <button onClick={() => setCased((v) => !v)} aria-pressed={cased} className={toggle(cased)} title={t("search.case")}>Aa</button>
      <input value={replace} onChange={(e) => setReplace(e.target.value)} placeholder={t("search.replace")} aria-label={t("search.replace")} className={`${field} w-56`} />
      <button onClick={() => jump(-1)} disabled={!hits.length} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("search.prev"))}><ChevronUp size={14} /></button>
      <button onClick={() => jump(1)} disabled={!hits.length} className="p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)] disabled:opacity-30" {...named(t("search.next"))}><ChevronDown size={14} /></button>
      <button onClick={() => hits[at] && replaceIn([hits[at].id])} disabled={!hits.length} className="inline-flex items-center gap-1 px-2 py-1 rounded-md border border-[var(--color-border)] text-[11px] hover:border-[var(--color-accent)] disabled:opacity-30"><Replace size={12} />{t("search.replaceHere")}</button>
      <button onClick={() => replaceIn(hits.map((h) => h.id))} disabled={!hits.length} className="inline-flex items-center gap-1 px-2 py-1 rounded-md bg-[var(--color-accent)] text-[var(--color-on-accent)] text-[11px] disabled:opacity-30">{t("search.replaceAll")}</button>
      <span className={`text-[11px] tabnum ${error ? "text-[var(--color-warn)]" : "text-[var(--color-muted)]"}`}>
        {error ? t("search.badRegex") : find ? t("search.count", { n: total, blocks: hits.length }) : ""}
      </span>
      <button onClick={onClose} className="ml-auto p-1 rounded text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("a11y.close"))}><X size={14} /></button>
    </div>
  );
}
