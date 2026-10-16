import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { HelpCircle } from "lucide-react";

const WIDTH = 280;
const GAP = 6;

/** A faint «?» beside a control: hover, focus or a tap opens what the control does and how it works; leaving, Esc or
 * another tap closes it. The text is drawn over the window so a scrolling panel does not clip it. */
export default function Hint({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  const [place, setPlace] = useState<{ left: number; top?: number; bottom?: number } | null>(null);
  const icon = useRef<HTMLSpanElement>(null);
  const id = useId();
  useEffect(() => {
    if (!open) return;
    const measure = () => {
      const r = icon.current?.getBoundingClientRect();
      if (!r) return;
      const left = Math.min(Math.max(8, r.left + r.width / 2 - WIDTH / 2), window.innerWidth - WIDTH - 8);
      setPlace(r.top > 140 ? { left, bottom: window.innerHeight - r.top + GAP } : { left, top: r.bottom + GAP });
    };
    measure();
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); setOpen(false); } };
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    window.addEventListener("keydown", esc, true);
    return () => { window.removeEventListener("resize", measure); window.removeEventListener("scroll", measure, true); window.removeEventListener("keydown", esc, true); };
  }, [open]);
  return (
    <>
      <span ref={icon} tabIndex={0} role="button" aria-label={text} aria-describedby={open ? id : undefined}
        onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)} onFocus={() => setOpen(true)} onBlur={() => setOpen(false)}
        onClick={(e) => { e.preventDefault(); e.stopPropagation(); setOpen((v) => !v); }}
        className={`cursor-help inline-flex align-middle rounded-full transition hover:opacity-100 hover:text-[var(--color-accent-2)] focus-visible:opacity-100 focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--color-accent)] ${open ? "opacity-100 text-[var(--color-accent-2)]" : "opacity-40 text-[var(--color-muted)]"}`}>
        <HelpCircle size={12} />
      </span>
      {open && place && createPortal(
        <span id={id} role="tooltip" style={{ left: place.left, top: place.top, bottom: place.bottom, width: WIDTH }}
          className="fixed z-[90] pointer-events-none rounded-lg border border-[var(--color-border)] bg-[var(--color-overlay)] px-2.5 py-1.5 text-[12px] leading-snug text-[var(--color-text)] normal-case tracking-normal font-normal shadow-xl anim-pop">
          {text}
        </span>,
        document.body,
      )}
    </>
  );
}
