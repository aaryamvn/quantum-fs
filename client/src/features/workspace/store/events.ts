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
