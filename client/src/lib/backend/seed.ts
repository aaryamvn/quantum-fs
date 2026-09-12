import type { OrchestrationServer } from "./types";

/**
 * Design-time data. Kept byte-for-byte in step with the seed in
 * `client/src-tauri/src/bridge.rs` so the mock and the Tauri build look identical.
 */
export const SEED_SERVERS: OrchestrationServer[] = [
  {
    id: "srv_1",
    name: "Server 1",
    address: "127.0.0.1:7447",
    peerId: "peer_a1f3c9d2",
    online: true,
    vaults: [
      { id: "vlt_1_1", serverId: "srv_1", name: "Vault 1", memberCount: 3, role: "owner" },
      { id: "vlt_1_2", serverId: "srv_1", name: "Vault 2", memberCount: 5, role: "member" },
      { id: "vlt_1_3", serverId: "srv_1", name: "Vault 3", memberCount: 2, role: "member" },
    ],
  },
  {
    id: "srv_2",
    name: "Server 2",
    address: "10.0.0.12:7447",
    peerId: "peer_7b4e21ac",
    online: true,
    vaults: [
      { id: "vlt_2_1", serverId: "srv_2", name: "Vault 1", memberCount: 4, role: "member" },
      { id: "vlt_2_2", serverId: "srv_2", name: "Vault 2", memberCount: 2, role: "owner" },
    ],
  },
];
