import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, ExternalLink, Share2 } from "lucide-react";
import { named } from "../lib/a11y";
import { LINKS } from "../lib/links";

/** Where a text and a link are posted, as VideoSOS and YuE2 Studio list them. */
const PLACES: Array<{ id: "telegram" | "x" | "vk" | "whatsapp" | "reddit"; url: (text: string, link: string) => string }> = [
  { id: "telegram", url: (text, link) => `https://t.me/share/url?url=${encodeURIComponent(link)}&text=${encodeURIComponent(text)}` },
  { id: "x", url: (text, link) => `https://twitter.com/intent/tweet?text=${encodeURIComponent(text)}&url=${encodeURIComponent(link)}` },
  { id: "vk", url: (text, link) => `https://vk.com/share.php?url=${encodeURIComponent(link)}&title=${encodeURIComponent(text)}` },
  { id: "whatsapp", url: (text, link) => `https://wa.me/?text=${encodeURIComponent(`${text}\n${link}`)}` },
  { id: "reddit", url: (text, link) => `https://reddit.com/submit?url=${encodeURIComponent(link)}&title=${encodeURIComponent(text)}` },
];

const canShareFiles = () => typeof navigator !== "undefined" && typeof navigator.share === "function" && typeof navigator.canShare === "function";

/** Shares a result: the file itself through the system's share sheet, or a line about it with the studio's link. */
export default function ShareMenu({ title, file }: { title: string; file: () => Promise<File> }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [note, setNote] = useState<{ text: string; error: boolean } | null>(null);
  const text = t("share.text", { title });
  useEffect(() => {
    if (!open) return;
    const close = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, [open]);
  const fail = (e: unknown) => setNote({ text: t("share.failed", { reason: e instanceof Error ? e.message : String(e) }), error: true });
  const sendFile = async () => {
    setOpen(false);
    const shared = await file();
    if (!navigator.canShare({ files: [shared] })) { setNote({ text: t("share.fileRefused"), error: true }); return; }
    await navigator.share({ files: [shared], title, text }).catch((e: unknown) => {
      // closing the share sheet is the person's choice, not a failure
      if (e instanceof DOMException && e.name === "AbortError") return;
      throw e;
    });
  };
  const item = "flex w-full items-center gap-2.5 px-3 py-2 rounded-md text-[12px] text-left text-[var(--color-text)] hover:bg-[var(--color-surface-2)] disabled:opacity-40";
  return (
    <div className="relative inline-flex items-center gap-1.5">
      <button onClick={() => setOpen((o) => !o)} aria-haspopup="menu" aria-expanded={open} {...named(t("share.title"))}
        className="p-1.5 rounded-lg border border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-text)]"><Share2 size={13} /></button>
      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div role="menu" aria-label={t("share.title")} className="absolute left-0 top-full mt-1 z-50 w-60 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-xl p-1.5">
            <button role="menuitem" disabled={!canShareFiles()} onClick={() => void sendFile().catch(fail)} className={item}>
              <Share2 size={13} className="text-[var(--color-muted)]" />{canShareFiles() ? t("share.file") : t("share.fileUnavailable")}
            </button>
            <div className="my-1 border-t border-[var(--color-border)]" />
            {PLACES.map((place) => (
              <a key={place.id} role="menuitem" href={place.url(text, LINKS.repo)} target="_blank" rel="noreferrer" onClick={() => setOpen(false)} className={item}>
                <ExternalLink size={13} className="text-[var(--color-muted)]" />{t(`share.places.${place.id}`)}
              </a>
            ))}
            <div className="my-1 border-t border-[var(--color-border)]" />
            <button role="menuitem" onClick={() => { setOpen(false); void navigator.clipboard.writeText(`${text}\n${LINKS.repo}`).then(() => setNote({ text: t("share.copied"), error: false }), fail); }} className={item}>
              <Copy size={13} className="text-[var(--color-muted)]" />{t("share.copyText")}
            </button>
          </div>
        </>
      )}
      {note && <span role={note.error ? "alert" : "status"} className={`text-[11px] ${note.error ? "text-[var(--color-warn)]" : "text-[var(--color-muted)]"}`}>{note.text}</span>}
    </div>
  );
}
