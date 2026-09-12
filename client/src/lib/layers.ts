import { useEffect, useRef } from "react";

export type LayerKind = "modal" | "popover" | "menu" | "tooltip";

export interface LayerHandle {
  /** Remove this layer from the stack. Idempotent, so StrictMode's double cleanup is safe. */
  release(): void;
  /** True while this layer is the one Escape would dismiss. */
  isTop(): boolean;
}

interface Layer {
  kind: LayerKind;
  onDismiss(): void;
}

/**
 * Ordered newest-last. A single module-level stack is the only way nested
 * layers can agree on who owns Escape: a popover opened from inside a modal
 * must close itself and leave the modal standing, which per-component window
 * listeners cannot arrange between themselves.
 */
const stack: Layer[] = [];
let listening = false;

/**
 * Only the top layer reacts, and it swallows the event outright —
 * `stopImmediatePropagation` also silences the other capture listeners on
 * window (the Modal's Tab trap among them) so one keypress never dismisses two
 * layers at once.
 */
function onKeyDown(e: KeyboardEvent): void {
  if (e.key !== "Escape") return;
  const top = stack[stack.length - 1];
  if (!top) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  top.onDismiss();
}

function sync(): void {
  const wanted = stack.length > 0;
  if (wanted === listening) return;
  listening = wanted;
  if (wanted) window.addEventListener("keydown", onKeyDown, true);
  else window.removeEventListener("keydown", onKeyDown, true);
}

/**
 * Register a dismissable layer. Tooltips are deliberately not dismissable —
 * they are transient hints, and letting one eat the Escape that was meant for
 * the dialog underneath it would be a bug, so they get an inert handle instead
 * of a stack entry and callers need no special case.
 */
export function pushLayer(kind: LayerKind, onDismiss: () => void): LayerHandle {
  if (kind === "tooltip") {
    return { release() {}, isTop: () => false };
  }

  const layer: Layer = { kind, onDismiss };
  stack.push(layer);
  sync();

  let released = false;
  return {
    release() {
      if (released) return;
      released = true;
      const i = stack.indexOf(layer);
      if (i !== -1) stack.splice(i, 1);
      sync();
    },
    isTop: () => stack[stack.length - 1] === layer,
  };
}

/**
 * Push while `open`, release on close or unmount. The dismiss callback is read
 * through a ref so a caller passing an inline arrow does not churn the stack
 * order on every render — re-pushing would quietly promote the layer above its
 * own children.
 */
export function useLayer(open: boolean, kind: LayerKind, onDismiss: () => void): void {
  const latest = useRef(onDismiss);
  latest.current = onDismiss;

  useEffect(() => {
    if (!open) return;
    const layer = pushLayer(kind, () => latest.current());
    return () => layer.release();
  }, [open, kind]);
}
