import type { BackendClient } from "./client";
import { SEED_SERVERS } from "./seed";
import type {
  BackendEvent,
  CreateVaultInput,
  DaemonStatus,
  JoinCode,
  OrchestrationServer,
  Vault,
} from "./types";

/** Smallest vault a server will provision: 256 MiB. */
const MIN_QUOTA_BYTES = 268435456;
/** Capacity given to a server the user adds by hand, or one invented by a join code: 128 GiB. */
const DEFAULT_CAPACITY_BYTES = 137438953472;
/** Quota assumed for a vault reached through a join code: 1 GiB. */
const JOINED_VAULT_QUOTA_BYTES = 1073741824;
const MAX_VAULT_NAME = 40;

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
        capacityBytes: DEFAULT_CAPACITY_BYTES,
        vaults: [],
      };
      servers.push(server);
      emit({ type: "servers-changed" });
      return structuredClone(server);
    },

    async createVault(input: CreateVaultInput) {
      const server = servers.find((s) => s.id === input.serverId);
      if (!server) throw new Error(`Unknown server: ${input.serverId}`);
      const name = input.name.trim();
      if (name.length === 0 || name.length > MAX_VAULT_NAME) {
        throw new Error("Invalid vault name");
      }
      // A vault's quota is carved out of what the server has not already promised.
      const allocated = server.vaults.reduce((sum, v) => sum + v.quotaBytes, 0);
      const free = server.capacityBytes - allocated;
      if (input.quotaBytes < MIN_QUOTA_BYTES || input.quotaBytes > free) {
        throw new Error("Not enough space on this server");
      }
      const vault: Vault = {
        id: `${server.id.replace("srv", "vlt")}_${server.vaults.length + 1}`,
        serverId: server.id,
        name,
        memberCount: 1,
        usedBytes: 0,
        quotaBytes: input.quotaBytes,
        role: "owner",
      };
      server.vaults.push(vault);
      emit({ type: "servers-changed" });
      return structuredClone(vault);
    },

    async joinVault(code: JoinCode) {
      // The central directory only maps a 6-character code -> (server, vault);
      // anything that is not exactly six A-Z/0-9 characters never resolves.
      const normalized = code.trim().toUpperCase();
      if (!/^[A-Z0-9]{6}$/.test(normalized)) throw new Error("Invalid join code");
      const n = servers.length + 1;
      const vault: Vault = {
        id: `vlt_${n}_1`,
        serverId: `srv_${n}`,
        name: "Joined Vault",
        memberCount: 1,
        usedBytes: 0,
        quotaBytes: JOINED_VAULT_QUOTA_BYTES,
        role: "member",
      };
      servers.push({
        id: `srv_${n}`,
        name: "Directory result",
        address: "unknown",
        peerId: `peer_${n}`,
        online: false,
        capacityBytes: DEFAULT_CAPACITY_BYTES,
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
