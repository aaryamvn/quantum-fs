import { useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent, MouseEvent as ReactMouseEvent } from "react";

import type { FsNode } from "@/lib/backend";
import { selectionRangeForRename } from "@/lib/path";

import { TILE_LABEL_GAP } from "../layout";
import { useWorkspace } from "../store";

export interface TileLabelProps {
  node: FsNode;
  selected: boolean;
  renaming: boolean;
  /**
   * A double-click on the name means rename, never open. The event travels with
   * it so the canvas can mark it handled and skip its own delegated open.
   */
  onDoubleClick(e: ReactMouseEvent): void;
}

/** How long a refused name's reason stands before the field goes quiet again. */
const ERROR_MS = 1600;

/**
 * Shared by the static label and the field that replaces it. The two have to
 * agree to the pixel or the tile visibly twitches at the moment you commit to
 * renaming — which is the moment you are looking straight at it.
 */
const METRICS = "text-[12.5px] leading-[15px] text-center rounded-[6px]";

/**
 * The name under the icon, and the field it becomes.
 *
 * Two things make this Finder rather than a form. First, renaming happens *in
 * place*: no dialog, no row expansion, the field lands exactly where the text
 * was, so the gesture costs no re-orientation. Second, the selection on open is
 * the base name only for a file — you almost never mean to retype `.hdr`, and a
 * rename that eats the extension is a rename that breaks the file.
 *
 * A refused name does not close the field or raise a toast. The field shakes,
 * says why in one line, and stays open with your text intact, because the only
 * useful next action is to fix the name you are already holding — and a toast
 * would be somewhere else on screen, telling you about a field you are looking
 * at.
 */
export function TileLabel({ node, selected, renaming, onDoubleClick }: TileLabelProps) {
  const commitRename = useWorkspace((s) => s.commitRename);
  const cancelRename = useWorkspace((s) => s.cancelRename);

  const inputRef = useRef<HTMLInputElement | null>(null);
  const [value, setValue] = useState(node.name);
  const [error, setError] = useState<string | null>(null);
  /** Escape has already closed the field; the blur it causes must not re-commit. */
  const abandoned = useRef(false);
  /** One commit in flight at a time: blur fires while the Enter commit is awaiting. */
  const busy = useRef(false);

  // Reopening on a different node (or after a peer renamed it) starts from the
  // truth, never from whatever the last edit left behind.
  useEffect(() => {
    if (!renaming) return;
    abandoned.current = false;
    busy.current = false;
    setValue(node.name);
    setError(null);
    // A frame later: autoFocus has run, and the range survives the focus call.
    const frame = requestAnimationFrame(() => {
      const el = inputRef.current;
      if (!el) return;
      const [start, end] = selectionRangeForRename(node.name, node.kind);
      el.setSelectionRange(start, end);
    });
    return () => cancelAnimationFrame(frame);
  }, [renaming, node.id, node.name, node.kind]);

  useEffect(() => {
    if (error === null) return;
    const timer = window.setTimeout(() => setError(null), ERROR_MS);
    return () => window.clearTimeout(timer);
  }, [error]);

  /** Restart the CSS shake on an element that must keep its focus and caret. */
  const shake = useCallback(() => {
    const el = inputRef.current;
    if (!el) return;
    el.classList.remove("shake");
    // Force a reflow so the removed animation is actually torn down first.
    void el.offsetWidth;
    el.classList.add("shake");
  }, []);

  const commit = useCallback(async () => {
    if (abandoned.current || busy.current) return;
    busy.current = true;
    const message = await commitRename(node.id, value);
    busy.current = false;
    if (message === null) return;
    setError(message);
    shake();
    inputRef.current?.focus();
  }, [commitRename, node.id, shake, value]);

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      void commit();
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      abandoned.current = true;
      cancelRename();
      return;
    }
    if (e.key === "Tab") {
      // Not prevented: the canvas owns where focus goes next.
      void commit();
      return;
    }
    // Everything else is typing, and typing must never reach the canvas's
    // single-key shortcuts (Space to preview, Delete to remove).
    e.stopPropagation();
  };

  if (renaming) {
    return (
      <div
        className="relative flex w-full flex-col items-center"
        style={{ marginTop: TILE_LABEL_GAP }}
      >
        <input
          ref={inputRef}
          data-testid="rename-input"
          value={value}
          autoFocus
          spellCheck={false}
          autoComplete="off"
          aria-label="Name"
          aria-invalid={error !== null}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={onKeyDown}
          onBlur={() => void commit()}
          // The tile owns selection and drag; a press inside the field is neither.
          onPointerDown={(e) => e.stopPropagation()}
          onDoubleClick={(e) => e.stopPropagation()}
          onContextMenu={(e) => e.stopPropagation()}
          onAnimationEnd={() => inputRef.current?.classList.remove("shake")}
          className={`${METRICS} w-[104px] min-h-[19px] border border-violet/70 bg-bg px-[4px]
            text-fg outline-none`}
        />
        {error !== null ? (
          <span
            data-testid="rename-error"
            role="alert"
            className="pointer-events-none absolute top-[21px] left-1/2 -translate-x-1/2
              whitespace-nowrap text-[11px] leading-none text-coral"
          >
            {error}
          </span>
        ) : null}
      </div>
    );
  }

  return (
    <span
      data-testid="tile-label"
      title={node.name}
      onDoubleClick={onDoubleClick}
      style={{ marginTop: TILE_LABEL_GAP }}
      className={`${METRICS} max-w-[104px] overflow-hidden px-[6px] py-[2px] break-words
        [display:-webkit-box] [overflow-wrap:anywhere] [-webkit-box-orient:vertical]
        [-webkit-line-clamp:2]
        transition-colors duration-[160ms] ease-[cubic-bezier(0.2,0.8,0.2,1)]
        ${selected ? "bg-violet/[0.45] text-fg" : "text-fg-2"}`}
    >
      {node.name}
    </span>
  );
}
