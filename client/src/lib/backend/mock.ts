import type { BackendClient } from "./client";
import { SEED_SERVERS } from "./seed";
import type {
  BackendEvent,
  DaemonStatus,
  JoinCode,
  OrchestrationServer,
  ServerId,
  Vault,
} from "./types";

/**
 * In-memory `BackendClient` used outside Tauri (plain `npm run dev`) so every screen
 * renders with realistic data for design work. Mutations are synchronous and local.
 */
export function createMockBackend(): BackendClient {
  const servers: OrchestrationServer[] = structuredClone(SEED_SERVERS);
  const listeners = new Set<(e: BackendEvent) => void>();

  const emit = (e: BackendEvent) => {
    for (const listener of listeners) listener(e);
  };

  const status: DaemonStatus = {
    running: false,
    version: null,
    peerId: null,
    dataDir: null,
  };

  return {
    async status() {
      return { ...status };
    },

    async listServers() {
      return structuredClone(servers);
    },

    async addServer(input: { name: string; address: string }) {
      const n = servers.length + 1;
      const server: OrchestrationServer = {
        id: `srv_${n}`,
        name: input.name,
        address: input.address,
        peerId: `peer_${n}`,
        online: true,
        vaults: [],
      };
      servers.push(server);
      emit({ type: "servers-changed" });
      return structuredClone(server);
    },

    async createVault(serverId: ServerId, name: string) {
      const server = servers.find((s) => s.id === serverId);
      if (!server) throw new Error(`Unknown server: ${serverId}`);
      const vault: Vault = {
        id: `${server.id.replace("srv", "vlt")}_${server.vaults.length + 1}`,
        serverId: server.id,
        name,
        memberCount: 1,
        role: "owner",
      };
      server.vaults.push(vault);
      emit({ type: "servers-changed" });
      return structuredClone(vault);
    },

    async joinVault(code: JoinCode) {
      // The central directory only maps code -> (server, vault); a short code never resolves.
      if (code.length < 8) throw new Error("Invalid join code");
      const n = servers.length + 1;
      const vault: Vault = {
        id: `vlt_${n}_1`,
        serverId: `srv_${n}`,
        name: "Joined vault",
        memberCount: 1,
        role: "member",
      };
      servers.push({
        id: `srv_${n}`,
        name: "Directory result",
        address: "unknown",
        peerId: `peer_${n}`,
        online: false,
        vaults: [vault],
      });
      emit({ type: "servers-changed" });
      return structuredClone(vault);
    },

    subscribe(listener: (e: BackendEvent) => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}
