/**
 * The workspace's global keyboard shortcuts — the ones that belong to the app,
 * not to the grid.
 *
 * Search, navigation, creation, settings and info are reachable from anywhere in
 * the workspace, so they hang off one window listener here instead of being
 * scattered across the units that happen to render the matching button. The
 * canvas keeps its own shortcuts (arrows, Enter, Space, ⌘A/C/V/D/⌫, ⌘↓) because
 * they only mean anything while the grid has focus and they need the grid's
 * ordered ids; nothing in this file duplicates them.
 *
 * Two rules keep it from stealing keystrokes it has no right to:
 *  - a keystroke aimed at a field is left alone (`isEditableTarget`), with ⌘K and
 *    Escape excepted so search stays reachable while a rename is in flight;
 *  - while a modal is open the only handled combo is ⌘K (closing search). Escape
 *    never reaches us at all — `@/lib/layers` owns it on the capture phase and
 *    swallows it for the top layer — so the Escape branch below is exactly the
 *    "nothing is layered" case.
 *
 * `preventDefault()` fires only when a combo was actually handled, so unhandled
 * browser/OS shortcuts keep working.
 */

import { useEffect } from "react";

import { isEditableTarget, matchesShortcut } from "@/lib/keys";

import { useWorkspace } from "../store";

/** Combo → human label, for a future help sheet and for menu accessories. */
export const SHORTCUTS: { combo: string; label: string }[] = [
  { combo: "mod+k", label: "Search" },
  { combo: "mod+[", label: "Back" },
  { combo: "mod+]", label: "Forward" },
  { combo: "mod+up", label: "Enclosing folder" },
  { combo: "mod+n", label: "New file" },
  { combo: "mod+shift+n", label: "New folder" },
  { combo: "mod+,", label: "Vault settings" },
  { combo: "mod+i", label: "Get info" },
  { combo: "mod+shift+h", label: "History" },
  { combo: "escape", label: "Dismiss" },
];

/**
 * Register the global shortcuts for as long as the workspace is mounted.
 *
 * Everything is read through `useWorkspace.getState()` inside the handler rather
 * than subscribed to: the listener is registered once and never re-registered,
 * so a selection change or a navigation does not churn a window listener.
 */
export function useWorkspaceShortcuts(): void {
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent): void {
      const s = useWorkspace.getState();
      const editable = isEditableTarget(e);

      // ⌘K outranks both a focused field and an open modal: it is the way out of
      // search as well as the way in.
      if (matchesShortcut(e, "mod+k")) {
        e.preventDefault();
        if (s.modal?.kind === "search") s.closeModal();
        else if (s.modal === null) s.openModal({ kind: "search" });
        return;
      }

      // A layered surface (modal, menu, popover) already consumed Escape on the
      // capture phase; reaching here means nothing is layered.
      if (e.key === "Escape" && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        if (s.contextMenu !== null) {
          e.preventDefault();
          s.closeContextMenu();
        }
        // Otherwise the canvas clears the selection; leave the event alone.
        return;
      }

      if (editable) return;
      // Every remaining combo would act on a surface the dialog covers.
      if (s.modal !== null) return;

      if (matchesShortcut(e, "mod+[")) {
        e.preventDefault();
        s.back();
        return;
      }
      if (matchesShortcut(e, "mod+]")) {
        e.preventDefault();
        s.forward();
        return;
      }
      if (matchesShortcut(e, "mod+up")) {
        e.preventDefault();
        s.up();
        return;
      }

      // ⇧⌘N first: the exact-modifier match makes the order irrelevant, but the
      // reading order matches the keycaps.
      if (matchesShortcut(e, "mod+shift+n")) {
        e.preventDefault();
        void s.createNode("folder");
        return;
      }
      if (matchesShortcut(e, "mod+n")) {
        e.preventDefault();
        void s.createNode("file");
        return;
      }

      if (matchesShortcut(e, "mod+,")) {
        if (s.vaultId === null) return;
        e.preventDefault();
        s.openModal({ kind: "vault-settings", vaultId: s.vaultId, tab: "general" });
        return;
      }

      // Info follows the Finder: one thing selected means that thing, nothing
      // selected means the folder you are standing in. A multi-selection has no
      // single subject, so it is left unhandled rather than guessed at.
      if (matchesShortcut(e, "mod+i")) {
        const target =
          s.selection.length === 1 ? s.selection[0] : s.selection.length === 0 ? s.folderId : null;
        if (target === null) return;
        e.preventDefault();
        s.openModal({ kind: "info", nodeId: target });
        return;
      }

      if (matchesShortcut(e, "mod+shift+h")) {
        if (s.folderId === null) return;
        e.preventDefault();
        s.openModal({ kind: "history", nodeId: s.folderId });
      }
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
