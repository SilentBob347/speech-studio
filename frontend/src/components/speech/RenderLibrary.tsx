import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { FileJson, Loader2, Pause, Play, SquarePen, X } from "lucide-react";
import { speechApi, type RecipeBlock, type Render } from "../../lib/speech";
import { errorText } from "../../lib/voices";
import { named } from "../../lib/a11y";
import { useEscapeLayer } from "../../lib/escape";
import FileActions from "./FileActions";

function RecipeDialog({ render, onClose }: { render: Render; onClose: () => void }) {
  const { t } = useTranslation();
  const [blocks, setBlocks] = useState<RecipeBlock[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEscapeLayer(onClose);
  useEffect(() => {
    if (!render.recipe) return;
    fetch(speechApi.exportUrl(render.pid, render.recipe))
      .then((r) => { if (!r.ok) throw new Error(`${r.status} ${r.statusText}`); return r.json() as Promise<{ blocks: RecipeBlock[] }>; })
      .then((r) => setBlocks(r.blocks), (e: unknown) => setError(errorText(e)));
  }, [render]);
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center bg-black/50 p-4" onClick={onClose}>
      <div role="dialog" aria-modal="true" aria-label={t("renders.recipe")} onClick={(e) => e.stopPropagation()}
        className="w-[min(94vw,760px)] max-h-[86vh] flex flex-col rounded-xl glass-panel anim-pop overflow-hidden">
        <div className="flex items-center gap-2 px-4 py-3 border-b border-[var(--color-border)]">
          <h2 className="font-semibold truncate">{t("renders.recipeOf", { file: render.file })}</h2>
          <button onClick={onClose} className="ml-auto p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("a11y.close"))}><X size={16} /></button>
        </div>
        <div className="flex-1 min-h-0 overflow-auto px-4 py-3">
          {error && <p role="alert" className="text-[12px] text-[var(--color-warn)]">{error}</p>}
          {!blocks && !error && <Loader2 size={16} className="animate-spin text-[var(--color-muted)]" />}
          {blocks && (
            <table className="w-full text-[12px]">
              <thead className="text-left text-[var(--color-muted)]">
                <tr><th className="py-1 pr-2 font-medium">{t("renders.block")}</th><th className="py-1 pr-2 font-medium">{t("renders.take")}</th><th className="py-1 pr-2 font-medium">{t("renders.engine")}</th><th className="py-1 pr-2 font-medium">{t("renders.voice")}</th><th className="py-1 font-medium">{t("renders.seed")}</th></tr>
              </thead>
              <tbody>
                {blocks.map((b) => (
                  <tr key={b.block} className="border-t border-[var(--color-border)] align-top">
                    <td className="py-1.5 pr-2"><span className="block text-[11px] text-[var(--color-muted)]">{b.speaker}</span><span className="line-clamp-2">{b.text}</span></td>
                    <td className="py-1.5 pr-2 tabnum">#{b.take}</td>
                    <td className="py-1.5 pr-2 whitespace-nowrap">{b.engine}</td>
                    <td className="py-1.5 pr-2">{b.voice || t("renders.baseVoice")}</td>
                    <td className="py-1.5 tabnum">{b.seed}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>
    </div>
  );
}

/** Every audio file built for any project, newest first: listen, save, show in the folder, see how it was made. */
export default function RenderLibrary({ find, onOpen }: { find: string; onOpen: (pid: string) => void }) {
  const { t, i18n } = useTranslation();
  const [renders, setRenders] = useState<Render[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [playing, setPlaying] = useState<string | null>(null);
  const [recipe, setRecipe] = useState<Render | null>(null);
  const audio = useRef<HTMLAudioElement | null>(null);
  useEffect(() => { speechApi.renders().then((r) => setRenders(r.renders), (e: unknown) => setError(errorText(e))); }, []);
  useEffect(() => () => audio.current?.pause(), []);
  const key = (r: Render) => `${r.pid}/${r.file}`;
  const play = (r: Render) => {
    audio.current?.pause();
    if (playing === key(r)) { setPlaying(null); return; }
    const a = new Audio(speechApi.exportUrl(r.pid, r.file));
    a.onended = () => setPlaying(null);
    audio.current = a;
    setPlaying(key(r));
    a.play().catch((e: unknown) => { setPlaying(null); setError(errorText(e)); });
  };
  const words = find.trim().toLowerCase();
  const shown = renders?.filter((r) => !words || `${r.file} ${r.project}`.toLowerCase().includes(words));
  const when = (s: number) => new Date(s * 1000).toLocaleString(i18n.language, { dateStyle: "medium", timeStyle: "short" });
  return (
    <div className="space-y-1.5">
      {error && <p role="alert" className="text-[12px] text-[var(--color-warn)]">{error}</p>}
      {renders === null && !error && <Loader2 size={16} className="animate-spin text-[var(--color-muted)]" />}
      {shown?.length === 0 && <p className="text-[13px] text-[var(--color-muted)]">{renders?.length ? t("renders.noneFound") : t("renders.none")}</p>}
      <ul className="space-y-1.5">
        {shown?.map((r) => (
          <li key={key(r)} data-mcp-row={`render ${r.file} (${r.project})`} className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 space-y-1.5">
            <div className="flex items-center gap-2">
              <button onClick={() => play(r)} className="grid place-items-center w-7 h-7 shrink-0 rounded-full bg-[var(--color-surface-2)] text-[var(--color-accent)]" {...named(playing === key(r) ? t("renders.stop") : t("renders.play"))}>
                {playing === key(r) ? <Pause size={13} /> : <Play size={13} />}
              </button>
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium truncate" title={r.file}>{r.file}</div>
                <div className="text-[11px] text-[var(--color-muted)] truncate">{r.project} · {when(r.mtime)} · {(r.bytes / 1_048_576).toFixed(1)} {t("units.mb")}</div>
              </div>
              <button onClick={() => onOpen(r.pid)} className="p-1.5 rounded-md text-[var(--color-muted)] hover:text-[var(--color-text)]" {...named(t("renders.openProject"))}><SquarePen size={14} /></button>
            </div>
            <div className="flex flex-wrap items-center gap-1.5">
              <FileActions pid={r.pid} file={r.file} label={t("renders.save")} />
              {r.recipe && <button onClick={() => setRecipe(r)} className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border border-[var(--color-border)] text-[12px] hover:border-[var(--color-accent)]"><FileJson size={13} />{t("renders.recipe")}</button>}
            </div>
          </li>
        ))}
      </ul>
      {recipe && <RecipeDialog render={recipe} onClose={() => setRecipe(null)} />}
    </div>
  );
}
