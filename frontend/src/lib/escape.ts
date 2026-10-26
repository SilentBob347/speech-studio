import { useEffect, useLayoutEffect, useRef } from "react";

// Открытые окна в порядке открытия: Escape достаётся только последнему (подтверждение внутри настроек
// закрывает себя, а не настройки).
const layers: { close: () => void }[] = [];

/** Fired when a window opens over the page: what the page plays stops, its controls being under that window. */
export const LAYER_OPENED = "speech:layer-opened";

function onKey(e: KeyboardEvent) {
  if (e.key !== "Escape" || layers.length === 0) return;
  e.stopPropagation();
  e.preventDefault();
  layers[layers.length - 1].close();
}

/** While mounted and `active`, Escape runs `onEscape` when this is the window opened last. `covers`: the window lies
 * over the page's player (a dialog), not beside it (a side panel, a bar in the page). */
export function useEscapeLayer(onEscape: () => void, active = true, covers = true): void {
  const latest = useRef(onEscape);
  useLayoutEffect(() => {
    latest.current = onEscape;
  });
  useEffect(() => {
    if (!active) return;
    const layer = { close: () => latest.current() };
    if (layers.length === 0) window.addEventListener("keydown", onKey, true);
    layers.push(layer);
    if (covers) window.dispatchEvent(new Event(LAYER_OPENED));
    return () => {
      layers.splice(layers.indexOf(layer), 1);
      if (layers.length === 0) window.removeEventListener("keydown", onKey, true);
    };
  }, [active, covers]);
}
