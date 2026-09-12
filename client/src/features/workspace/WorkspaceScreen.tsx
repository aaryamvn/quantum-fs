/**
 * The vault, assembled.
 *
 * Every panel below is self-contained — each one reads the workspace store for
 * itself and none of them takes state as a prop — so this file is deliberately
 * only three things: the frame they sit in, the handful of hooks that have to be
 * mounted exactly once, and the dev switches that drop a screenshot straight
 * into a deep state.
 *
 * The frame is a rail, then a column: sidebar on the left at a fixed width, and
 * everything else stacked under one top bar so the crumbs, the presence cluster
 * and search span the canvas *and* the inspector. The center column is the one
 * `position: relative` box in the app — the chat bar, the chat thread and the
 * toasts all anchor to it rather than to the window, which is what keeps them
 * clear of the rail and the details pane at every width.
 *
 * The overlay layers come last and are `fixed`: a drag ghost has to cross the
 * whole window, so it cannot live inside the scroller it is drawn over. It is
 * mounted unconditionally and renders nothing until there is something to show;
 * the modals do the same, each self-gating on the store's single `modal` slot.
 *
 * The top bar, not `DragRegion`, is the window's drag handle here: App hides the
 * fixed z-50 strip while the workspace is up, because that strip would sit over
 * the top bar's own buttons and swallow their clicks.
 */

import { useEffect, useRef } from "react";

import { ToastStack } from "@/components/ui/Toast";
import { devQuery } from "@/lib/devQuery";
import type { NodeId } from "@/lib/backend";

import { ActionBar } from "./actionbar/ActionBar";
import { Canvas } from "./canvas/Canvas";
import { ChatBar } from "./chat/ChatBar";
import { ChatThread } from "./chat/ChatThread";
import { DragLayer } from "./drag/DragLayer";
import { useDragController } from "./drag/useDragController";
import { Inspector } from "./inspector/Inspector";
import { useWorkspaceShortcuts } from "./keyboard/useWorkspaceShortcuts";
import { CHATBAR_CLEARANCE, Z } from "./layout";
import { ContextMenu } from "./menus/ContextMenu";
import { AccessModal } from "./modals/AccessModal";
import { ConfirmDeleteModal } from "./modals/ConfirmDeleteModal";
import { HistoryModal } from "./modals/HistoryModal";
import { InfoModal } from "./modals/InfoModal";
import { SearchModal } from "./modals/search/SearchModal";
import { VaultSettingsModal } from "./modals/settings/VaultSettingsModal";
import { ShareModal } from "./modals/ShareModal";
import { Sidebar } from "./sidebar/Sidebar";
import { useWorkspace, useWorkspaceEvents } from "./store";
import type { VaultSettingsTab } from "./store";
import { PresenceAvatars } from "./topbar/PresenceAvatars";
import { TopBar } from "./topbar/TopBar";

/**
 * The panels arrive one after another rather than all at once — rail, bar, pane
 * — so the frame reads as assembling around the canvas instead of blinking on.
 * Each unit owns its own slide; all this does is offset them.
 */
const STAGGER = 0.06;

const SETTINGS_TABS: VaultSettingsTab[] = [
  "general",
  "members",
  "sharing",
  "storage",
  "advanced",
];

/** Where `?ui=context:<id>` puts the menu: a repeatable spot, well inside the grid. */
const DEV_MENU_AT = { x: 640, y: 360 };

function isSettingsTab(value: string): value is VaultSettingsTab {
  return (SETTINGS_TABS as string[]).includes(value);
}

/**
 * Apply one `?ui=` switch, once the tree is loaded.
 *
 * Every surface it can open is keyed off a real node id, so it has to wait for
 * `treeLoaded`: opening the info modal on an id the store has never heard of
 * renders an empty dialog and reads as a broken screenshot rather than a missing
 * fixture.
 */
function applyDevUi(raw: string): void {
  const store = useWorkspace.getState();
  const colon = raw.indexOf(":");
  const kind = colon === -1 ? raw : raw.slice(0, colon);
  const arg = colon === -1 ? "" : raw.slice(colon + 1);
  const nodeId = arg as NodeId;

  switch (kind) {
    case "search":
      store.openModal({ kind: "search" });
      return;
    case "settings": {
      if (store.vaultId === null) return;
      const tab = isSettingsTab(arg) ? arg : "general";
      store.openModal({ kind: "vault-settings", vaultId: store.vaultId, tab });
      return;
    }
    case "context":
      store.openContextMenu(DEV_MENU_AT.x, DEV_MENU_AT.y, arg === "" ? null : nodeId);
      return;
    case "info":
      if (arg === "") return;
      store.openModal({ kind: "info", nodeId });
      return;
    case "history":
      if (arg === "") return;
      store.openModal({ kind: "history", nodeId });
      return;
    case "access":
      if (arg === "") return;
      store.openModal({ kind: "access", nodeId });
      return;
    case "share":
      if (arg === "") return;
      store.openModal({ kind: "share", nodeId });
      return;
    case "delete":
      if (arg === "") return;
      store.requestDelete([nodeId]);
      return;
    default:
      // `icons` is handled by App before the workspace exists; anything else is a typo.
      return;
  }
}

export function WorkspaceScreen() {
  // Mounted once for the life of the workspace: the backend feed, the global
  // keymap, and the one drag controller the canvas hands its tiles.
  useWorkspaceEvents();
  useWorkspaceShortcuts();
  const dragController = useDragController();

  const treeLoaded = useWorkspace((s) => s.treeLoaded);
  const toasts = useWorkspace((s) => s.toasts);
  const dismissToast = useWorkspace((s) => s.dismissToast);

  // Fires for the first loaded tree only. A ref rather than state because the
  // switch has no rendered form, and StrictMode's double effect must not open
  // the same dialog twice.
  const uiApplied = useRef(false);
  useEffect(() => {
    if (!treeLoaded || uiApplied.current) return;
    if (devQuery.ui === null) return;
    uiApplied.current = true;
    applyDevUi(devQuery.ui);
  }, [treeLoaded]);

  return (
    <div
      data-testid="workspace"
      className="fixed inset-0 flex bg-bg"
      style={{ zIndex: Z.canvas }}
    >
      <Sidebar />

      <div className="flex min-w-0 flex-1 flex-col">
        <TopBar delay={STAGGER} trailing={<PresenceAvatars />} />

        <div className="flex min-h-0 flex-1">
          {/*
            The containing block for everything that floats over the canvas.
            `relative` and never `overflow: hidden`: the chat bar's glow and the
            toasts paint outside their own footprints on purpose.
          */}
          <main data-testid="center" className="relative flex min-w-0 flex-1 flex-col">
            <ActionBar />
            <Canvas drag={dragController} />

            <ChatThread />
            <ChatBar />

            {/*
              Bottom-left, but clear of the chat bar rather than beside it: the
              bar is 640px wide and centered, so at a 1400px window a toast at the
              canvas's bottom edge lands on top of it. `CHATBAR_CLEARANCE` is the
              same number the canvas keeps free, so the stack rises with the bar
              if the bar ever changes height. One above the bar's `Z.chrome`, so a
              toast that does overlap is readable rather than half-covered.
            */}
            <div
              className="pointer-events-none absolute left-[20px]"
              style={{ bottom: CHATBAR_CLEARANCE + 8, zIndex: Z.chrome + 1 }}
            >
              <ToastStack toasts={toasts} onDismiss={dismissToast} />
            </div>
          </main>

          <Inspector delay={STAGGER * 2} />
        </div>
      </div>

      {/* Fixed overlays, in stacking order: carried tiles, then the menu and the
          dialogs. Each renders null when idle. */}
      <DragLayer />
      <ContextMenu />

      <InfoModal />
      <HistoryModal />
      <AccessModal />
      <ShareModal />
      <ConfirmDeleteModal />
      <VaultSettingsModal />
      <SearchModal />
    </div>
  );
}

export default WorkspaceScreen;
