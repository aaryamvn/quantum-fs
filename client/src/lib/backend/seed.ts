import type { OrchestrationServer } from "./types";

/**
 * Design-time data. Kept byte-for-byte in step with the seed in
 * `client/src-tauri/src/bridge.rs` so the mock and the Tauri build look identical.
 */
export const SEED_SERVERS: OrchestrationServer[] = [
  {
    id: "srv_1",
    name: "Server 1",
    address: "192.168.1.24:7447",
    peerId: "peer_a1f3c9d2",
    online: true,
    capacityBytes: 274877906944,
    vaults: [
      {
        id: "vlt_1_1",
        serverId: "srv_1",
        name: "Design Assets",
        memberCount: 5,
        usedBytes: 1288490189,
        quotaBytes: 4294967296,
        role: "owner",
      },
      {
        id: "vlt_1_2",
        serverId: "srv_1",
        name: "Hackathon Build",
        memberCount: 5,
        usedBytes: 671088640,
        quotaBytes: 2147483648,
        role: "member",
      },
      {
        id: "vlt_1_3",
        serverId: "srv_1",
        name: "Family Photos",
        memberCount: 2,
        usedBytes: 9019431322,
        quotaBytes: 17179869184,
        role: "member",
      },
    ],
  },
  {
    id: "srv_2",
    name: "Server 2",
    address: "10.0.0.12:7447",
    peerId: "peer_7b4e21ac",
    online: true,
    capacityBytes: 549755813888,
    vaults: [
      {
        id: "vlt_2_1",
        serverId: "srv_2",
        name: "Research Papers",
        memberCount: 4,
        usedBytes: 327155712,
        quotaBytes: 1073741824,
        role: "member",
      },
      {
        id: "vlt_2_2",
        serverId: "srv_2",
        name: "Backups",
        memberCount: 2,
        usedBytes: 26413435289,
        quotaBytes: 68719476736,
        role: "owner",
      },
    ],
  },
];
