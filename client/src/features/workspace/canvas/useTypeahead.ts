/**
 * Type a few letters, land on the file — the oldest file-manager gesture there is.
 *
 * The buffer is what makes it work: "re" must reach *report.pdf* even though "r"
 * alone already matched *readme*, so keys accumulate and only a 700ms pause (the
 * span macOS uses) starts a new word. Prefix wins over substring because that is
 * how the eye scans a sorted grid; the substring pass is the fallback that saves
 * a search for "2024" in "budget-2024.xlsx".
 *
 * It only ever *selects*. Scrolling the match into view is the canvas's job,
 * which it already does for every focus change, so one behavior covers the
 * keyboard, the typeahead and a freshly created node.
 */

import { useEffect, useRef } from "react";
import type { RefObject } from "react";

import type { NodeId } from "@/lib/backend";
import { isEditableTarget } from "@/lib/keys";

import { useWorkspace } from "../store";

/** How long a word stays open. Longer feels stuck; shorter breaks two-letter names. */
const RESET_MS = 700;

export function useTypeahead(
  container: RefObject<HTMLElement | null>,
  orderedIds: NodeId[],
  names: string[],
): void {
  const ordered = useRef(orderedIds);
  ordered.current = orderedIds;
  const labels = useRef(names);
  labels.current = names;

  useEffect(() => {
    const el = container.current;
    if (!el) return;

    let buffer = "";
    let timer: ReturnType<typeof setTimeout> | null = null;

    const clear = (): void => {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      buffer = "";
    };

    const onKeyDown = (e: KeyboardEvent): void => {
      if (isEditableTarget(e)) return;
      // Modified keys are shortcuts; space is Quick Look, never the start of a word.
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key.length !== 1 || e.key === " ") return;

      buffer += e.key.toLowerCase();
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(clear, RESET_MS);

      const ids = ordered.current;
      const list = labels.current;
      let hit = -1;
      for (let i = 0; i < list.length && i < ids.length; i++) {
        if (list[i].toLowerCase().startsWith(buffer)) {
          hit = i;
          break;
        }
      }
      if (hit === -1) {
        for (let i = 0; i < list.length && i < ids.length; i++) {
          if (list[i].toLowerCase().includes(buffer)) {
            hit = i;
            break;
          }
        }
      }
      if (hit === -1) return;

      useWorkspace.getState().select([ids[hit]]);
      e.preventDefault();
    };

    el.addEventListener("keydown", onKeyDown);
    return () => {
      el.removeEventListener("keydown", onKeyDown);
      clear();
    };
  }, [container]);
}
