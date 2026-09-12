/**
 * Every key the icon grid answers, in one listener.
 *
 * Finder is usable without a pointer and so is this: arrows walk the grid by the
 * *rendered* column count (measured from the container, so it is the same number
 * `auto-fill` produced and a resize needs no state), ⇧ extends from the anchor
 * the store already tracks, and the object verbs sit on the same combos macOS
 * trained everyone to expect.
 *
 * Bound to the canvas element rather than the window: a shortcut that fires while
 * the sidebar or the chat bar has focus is a bug, and the canvas is a real focus
 * target (`tabIndex=0`) precisely so this stays scoped. State is read through
 * `getState()` at keypress time — subscribing here would re-bind the listener on
 * every selection change for no gain, and a stale closure would move the wrong tile.
 *
 * ⌘↑ (enclosing folder) and ⌘I (get info) are deliberately absent: they belong to
 * `keyboard/useWorkspaceShortcuts`, whose window listener this one bubbles to, and
 * handling them here as well fired each of them twice.
 */

import { useEffect, useRef } from "react";
import type { RefObject } from "react";

import type { NodeId } from "@/lib/backend";
import { isEditableTarget, matchesShortcut } from "@/lib/keys";

import { useWorkspace } from "../store";
import { columnsFor } from "./canvasGeometry";

export function useGridKeyboard(
  container: RefObject<HTMLElement | null>,
  orderedIds: NodeId[],
): void {
  const ordered = useRef(orderedIds);
  ordered.current = orderedIds;

  useEffect(() => {
    const el = container.current;
    if (!el) return;

    const onKeyDown = (e: KeyboardEvent): void => {
      // A rename field types the same letters and arrows this handler claims.
      if (isEditableTarget(e)) return;

      const ids = ordered.current;
      const store = useWorkspace.getState();
      const focused = store.focusedId ?? store.selection[store.selection.length - 1] ?? null;
      const index = focused === null ? -1 : ids.indexOf(focused);
      const selection = store.selection;

      /** Land on an index, clamped; ⇧ grows the range instead of replacing it. */
      const moveTo = (next: number, extend: boolean): void => {
        if (ids.length === 0) return;
        const id = ids[Math.max(0, Math.min(ids.length - 1, next))];
        if (id === undefined) return;
        if (extend) store.rangeSelect(id, ids);
        else store.select([id]);
        store.setFocused(id);
        e.preventDefault();
      };

      /** Nothing focused yet: the first arrow lands on the first tile, whichever it is. */
      const step = (delta: number, extend: boolean): void => {
        if (index === -1) moveTo(0, false);
        else moveTo(index + delta, extend);
      };

      const columns = columnsFor(el.clientWidth);

      // Modified combos first: ⌘↓ and ⇧↓ must never fall through to a plain move.
      if (matchesShortcut(e, "mod+down") || matchesShortcut(e, "mod+o")) {
        const id = focused ?? (selection.length === 1 ? selection[0] : null);
        if (id !== null) {
          store.openNode(id);
          e.preventDefault();
        }
        return;
      }
      if (matchesShortcut(e, "mod+a")) {
        store.selectAll(ids);
        e.preventDefault();
        return;
      }
      if (matchesShortcut(e, "mod+d")) {
        if (selection.length > 0) void store.duplicateNodes(selection);
        e.preventDefault();
        return;
      }
      if (matchesShortcut(e, "mod+c")) {
        if (selection.length > 0) store.copy(selection);
        e.preventDefault();
        return;
      }
      if (matchesShortcut(e, "mod+v")) {
        void store.paste();
        e.preventDefault();
        return;
      }
      if (
        matchesShortcut(e, "mod+backspace") ||
        matchesShortcut(e, "delete") ||
        matchesShortcut(e, "backspace")
      ) {
        if (selection.length > 0) store.requestDelete(selection);
        e.preventDefault();
        return;
      }

      if (matchesShortcut(e, "left")) return step(-1, false);
      if (matchesShortcut(e, "shift+left")) return step(-1, true);
      if (matchesShortcut(e, "right")) return step(1, false);
      if (matchesShortcut(e, "shift+right")) return step(1, true);
      if (matchesShortcut(e, "up")) return step(-columns, false);
      if (matchesShortcut(e, "shift+up")) return step(-columns, true);
      if (matchesShortcut(e, "down")) return step(columns, false);
      if (matchesShortcut(e, "shift+down")) return step(columns, true);
      if (matchesShortcut(e, "home")) return moveTo(0, false);
      if (matchesShortcut(e, "shift+home")) return moveTo(0, true);
      if (matchesShortcut(e, "end")) return moveTo(ids.length - 1, false);
      if (matchesShortcut(e, "shift+end")) return moveTo(ids.length - 1, true);

      // Finder's Enter: rename in place. Opening is ⌘↓, and people who learn one
      // learn the other — a return key that opened things would make renaming
      // reachable only from a menu.
      if (matchesShortcut(e, "enter")) {
        const id = focused ?? (selection.length === 1 ? selection[0] : null);
        if (id !== null) {
          store.startRename(id);
          e.preventDefault();
        }
        return;
      }

      // Quick Look's key, standing in for the preview until there is one.
      if (matchesShortcut(e, "space")) {
        if (selection.length === 1) store.openModal({ kind: "info", nodeId: selection[0] });
        e.preventDefault();
        return;
      }

      if (matchesShortcut(e, "escape")) {
        if (store.selection.length > 0) store.clearSelection();
        e.preventDefault();
      }
    };

    el.addEventListener("keydown", onKeyDown);
    return () => el.removeEventListener("keydown", onKeyDown);
  }, [container]);
}
