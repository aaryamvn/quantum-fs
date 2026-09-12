import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { BackendClient } from "./client";
import type {
  BackendEvent,
  DaemonStatus,
  JoinCode,
  OrchestrationServer,
  ServerId,
  Vault,
} from "./types";

/** Event the Rust side emits after any mutation of the server/vault list. */
const SERVERS_CHANGED = "backend://servers-changed";

/**
 * `BackendClient` backed by the Rust commands in `client/src-tauri/src/bridge.rs`.
 * Rust owns the socket to `qfsd`; this file is the only place that names its commands.
 */
export function createTauriBackend(): BackendClient {
  return {
    status() {
      return invoke<DaemonStatus>("daemon_status");
    },

    listServers() {
      return invoke<OrchestrationServer[]>("list_servers");
    },

    addServer(input: { name: string; address: string }) {
      return invoke<OrchestrationServer>("add_server", { input });
    },

    createVault(serverId: ServerId, name: string) {
      return invoke<Vault>("create_vault", { serverId, name });
    },

    joinVault(code: JoinCode) {
      return invoke<Vault>("join_vault", { code });
    },

    subscribe(listener: (e: BackendEvent) => void) {
      const unlisten = listen(SERVERS_CHANGED, () => {
        listener({ type: "servers-changed" });
      });
      return () => {
        void unlisten.then((off) => off());
      };
    },
  };
}
