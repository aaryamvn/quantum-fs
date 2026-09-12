/**
 * The one place backend events reach the workspace store.
 *
 * Mounted once by the workspace screen. Everything the daemon (or the mock, or
 * the scripted demo) pushes arrives here and is turned into exactly one store
 * action, which is what makes a change a peer made and a change we made take the
 * same path into the UI — the local op sends the command, the event paints it.
 *
 * The handler reads state through `getState()` rather than through a subscription
 * so it never closes over a stale vault id: the effect re-runs only when the
 * client itself changes, and a vault switch mid-flight is filtered by comparing
 * the event's vault to the one currently open.
 */

import { useEffect } from "react";

import { useShellNotice } from "@/features/home/shellNotice";

import { useWorkspace } from "./workspaceStore";

export function useWorkspaceEvents(): void {
  const client = useWorkspace((s) => s.client);

  useEffect(() => {
    if (!client) return;

    // `subscribe` returns its own unsubscribe, so StrictMode's mount/unmount/mount
    // leaves exactly one listener behind.
    return client.subscribe((event) => {
      const store = useWorkspace.getState();
      const openVaultId = store.vaultId;

      switch (event.type) {
        case "fs-changed":
          if (event.vaultId !== openVaultId) return;
          store.applyChanges(event.changes, event.actor);
          return;
        case "remote-op":
          // Nothing animates a peer's move any more: the matching `fs-changed`
          // is what reflects it, so the announcement itself is dropped.
          return;
        case "presence":
          if (event.vaultId !== openVaultId) return;
          store.setPresence(event.peers);
          return;
        case "members-changed":
          if (event.vaultId !== openVaultId) return;
          void store.refreshMembers();
          return;
        case "vault-changed":
          if (event.vaultId !== openVaultId) return;
          void store.refreshVaultMeta();
          return;
        case "recents-changed":
          // Recents span every vault, so this one is never filtered.
          void store.refreshRecents();
          return;
        case "vault-removed":
          // Kicked, or the host forgot the vault. Either way the reason goes to
          // the shell notice and not to a toast: the workspace's stack unmounts
          // with the screen, so a toast would be posted into a component that is
          // about to disappear and the user would never see the sentence.
          if (event.vaultId !== openVaultId) {
            // A vault we were not looking at still vanished from the home list,
            // and a row disappearing with no account of why reads as a bug.
            useShellNotice.getState().show(event.reason, "info");
            return;
          }
          // Notice first, teardown second: the home screen mounts off the back
          // of `qfs:home` and reads the notice that is already standing.
          useShellNotice.getState().show(event.reason, "error");
          store.closeVault();
          // The workspace screen has nothing left to draw, so the shell is asked
          // to go back home the same way the sidebar asks it to.
          window.dispatchEvent(new CustomEvent("qfs:home"));
          return;
        case "demo-reset":
          if (event.vaultId !== openVaultId) return;
          store.bumpDemoReset();
          return;
        case "servers-changed":
        case "daemon-status":
          // Home screen concerns; the workspace has no server list to refresh.
          return;
      }
    });
  }, [client]);
}
