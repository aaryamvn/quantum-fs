/**
 * The `BackendClient` used whenever there is no daemon: `npm run dev` in a plain
 * browser, and every screenshot run.
 *
 * It is a thin adapter, on purpose. The home screen's server/vault logic stayed
 * exactly as it was written — same constants, same ids, same error strings — and
 * everything the workspace needs is delegated to {@link FsEngine}, so there is
 * one place where file system behavior is decided and one place where the seam's
 * shape is decided. When the real daemon arrives, this file is what gets deleted;
 * `client.ts` and the UI do not move (docs/decisions/client-workspace.md).
 *
 * Mutations resolve synchronously — `async` only because the interface is
 * promise-shaped. Nothing here simulates network latency: the mock exists to make
 * the UI feel like Finder, and Finder does not spin.
 */

import { devQuery } from "@/lib/devQuery";

import type { BackendClient } from "../client";
import type {
  AskAgentInput,
  CreateNodeInput,
  DeleteNodesInput,
  DuplicateNodesInput,
  MoveNodesInput,
  PresenceInput,
  RenameNodeInput,
  SetAccessInput,
  SetNodeColorInput,
  VaultMetaPatch,
} from "../client";
import { SEED_SERVERS } from "../seed";
import type {
  BackendEvent,
  CreateVaultInput,
  DaemonStatus,
  JoinCode,
  MemberRole,
  NodeId,
  OrchestrationServer,
  PeerId,
  ProfilePatch,
  SearchQuery,
  Vault,
  VaultId,
} from "../types";

import { FsEngine } from "./engine";
import type { MockSimulation } from "./engine";
import { loadSeed } from "./seedLoader";

export { FsEngine } from "./engine";
export type { FsEngineOptions, MockSimulation } from "./engine";
export { loadSeed, SEED_SELF } from "./seedLoader";
export type { FsSeed } from "./seedLoader";

/** Smallest vault a server will provision: 256 MiB. */
const MIN_QUOTA_BYTES = 268435456;
/** Capacity given to a server the user adds by hand, or one invented by a join code: 128 GiB. */
const DEFAULT_CAPACITY_BYTES = 137438953472;
/** Quota assumed for a vault reached through a join code: 1 GiB. */
const JOINED_VAULT_QUOTA_BYTES = 1073741824;
const MAX_VAULT_NAME = 40;

/** A join code is this many Base32 characters (docs/decisions/client-backend-embed.md). */
const JOIN_CODE_LENGTH = 6;
/** Bounds on the host token in a printed connect string, in Base32 characters. */
const MIN_TOKEN_LENGTH = 16;
const MAX_TOKEN_LENGTH = 32;
/** RFC 4648 Base32, unpadded — the alphabet codes and tokens are printed in. */
const BASE32 = /^[A-Z2-7]+$/;
/** `ip:port`, with the ip an IPv4 dotted quad or a bracketed IPv6 literal. */
const ENDPOINT = /^(?:(\d{1,3}(?:\.\d{1,3}){3})|\[[0-9A-Fa-f:]*:[0-9A-Fa-f.:]*\]):(\d{1,5})$/;

/** The `ip:port` half of a connect string — ranges checked, not just the shape. */
function isEndpoint(text: string): boolean {
  const match = ENDPOINT.exec(text);
  if (!match) return false;
  const [, v4, port] = match;
  if (v4 && v4.split(".").some((octet) => Number(octet) > 255)) return false;
  const portNumber = Number(port);
  return portNumber >= 1 && portNumber <= 65535;
}

/** The token half of a connect string: 16-32 Base32 characters. */
function isToken(text: string): boolean {
  return text.length >= MIN_TOKEN_LENGTH && text.length <= MAX_TOKEN_LENGTH && BASE32.test(text);
}

/**
 * Where the demo's side door is hung.
 *
 * Non-enumerable, so `BackendClient` stays exactly the interface it claims to be:
 * nothing that iterates the client sees it, and TypeScript never offers it to UI
 * code by accident.
 */
const SIMULATION_KEY = "simulate";

interface WithSimulation {
  [SIMULATION_KEY]?: MockSimulation;
}

/**
 * The demo hooks on a client, or `null` for any client that has none.
 *
 * Returning `null` rather than throwing is what lets `withDemo` be wrapped around
 * the Tauri client harmlessly: no simulation, no scripted peers, everything else
 * unchanged.
 */
export function getSimulation(client: BackendClient): MockSimulation | null {
  return (client as WithSimulation)[SIMULATION_KEY] ?? null;
}

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

  // `?onboard=1` is the design-QA door onto the first launch: the profile starts
  // unnamed, so the onboarding screen is what the browser opens on.
  const engine = new FsEngine(loadSeed(), emit, servers, { onboarding: devQuery.onboard });

  const client: BackendClient = {
    async status() {
      return { ...status };
    },

    async listServers() {
      return structuredClone(servers);
    },

    async addServer(input: { name: string; address: string }) {
      // The host prints `ip:port/TOKEN`. The token is the joining secret, not part of
      // the address, so it is validated and then dropped
      // (docs/decisions/client-backend-embed.md).
      const raw = input.address.trim();
      const slash = raw.lastIndexOf("/");
      const endpoint = slash === -1 ? raw : raw.slice(0, slash);
      const token = slash === -1 ? null : raw.slice(slash + 1);
      if (!isEndpoint(endpoint)) throw new Error("Invalid server address");
      if (token !== null && !isToken(token)) throw new Error("Invalid server address");
      const n = servers.length + 1;
      const server: OrchestrationServer = {
        id: `srv_${n}`,
        name: input.name,
        address: endpoint,
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
        // Brand new: this client is its only member, so its pledge is the whole
        // surplus over the host's quota.
        capacityBytes: input.quotaBytes + engine.getProfile().contributionBytes,
        role: "owner",
      };
      server.vaults.push(vault);
      emit({ type: "servers-changed" });
      return structuredClone(vault);
    },

    async joinVault(code: JoinCode) {
      // Six Base32 characters and nothing else: the daemon resolves the code through
      // the central directory it knows, so the code never carries an address and
      // joining never requires adding a server first
      // (docs/decisions/client-backend-embed.md).
      const joinCode = code.trim().toUpperCase();
      if (joinCode.length !== JOIN_CODE_LENGTH || !BASE32.test(joinCode)) {
        throw new Error("Invalid join code");
      }
      const n = servers.length + 1;
      const vault: Vault = {
        id: `vlt_${n}_1`,
        serverId: `srv_${n}`,
        name: "Joined Vault",
        memberCount: 1,
        usedBytes: 0,
        quotaBytes: JOINED_VAULT_QUOTA_BYTES,
        capacityBytes: JOINED_VAULT_QUOTA_BYTES + engine.getProfile().contributionBytes,
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

    // Workspace surface of the seam. Every one of these is a straight delegation:
    // the engine owns the rules, this file owns only the promise wrapper.

    async me() {
      return engine.me();
    },

    async getProfile() {
      return engine.getProfile();
    },

    async setProfile(patch: ProfilePatch) {
      return engine.setProfile(patch);
    },

    async listTree(vaultId: VaultId) {
      return engine.listTree(vaultId);
    },

    async createNode(input: CreateNodeInput) {
      return engine.createNode(input);
    },

    async renameNode(input: RenameNodeInput) {
      return engine.renameNode(input);
    },

    async moveNodes(input: MoveNodesInput) {
      return engine.moveNodes(input);
    },

    async deleteNodes(input: DeleteNodesInput) {
      engine.deleteNodes(input);
    },

    async duplicateNodes(input: DuplicateNodesInput) {
      return engine.duplicateNodes(input);
    },

    async setNodeColor(input: SetNodeColorInput) {
      return engine.setNodeColor(input);
    },

    async requestDownload(vaultId: VaultId, nodeId: NodeId) {
      engine.requestDownload(vaultId, nodeId);
    },

    async openFile(vaultId: VaultId, nodeId: NodeId) {
      engine.openFile(vaultId, nodeId);
    },

    async importFiles(vaultId: VaultId, parentId: NodeId, paths?: string[]) {
      return engine.importFiles(vaultId, parentId, paths);
    },

    async readTextPreview(_vaultId: VaultId, nodeId: NodeId, maxBytes: number) {
      return engine.readTextPreview(nodeId, maxBytes);
    },

    async getAccess(_vaultId: VaultId, nodeId: NodeId) {
      return engine.getAccess(nodeId);
    },

    async setAccess(input: SetAccessInput) {
      return engine.setAccess(input);
    },

    async getHistory(vaultId: VaultId, nodeId: NodeId) {
      return engine.getHistory(vaultId, nodeId);
    },

    async listRecents() {
      return engine.listRecents();
    },

    async touchRecent(vaultId: VaultId, nodeId: NodeId) {
      engine.touchRecent(vaultId, nodeId);
    },

    async search(query: SearchQuery) {
      return engine.search(query);
    },

    async getVaultMeta(vaultId: VaultId) {
      return engine.getVaultMeta(vaultId);
    },

    async updateVaultMeta(vaultId: VaultId, patch: VaultMetaPatch) {
      return engine.updateVaultMeta(vaultId, patch);
    },

    async rotateJoinCode(vaultId: VaultId) {
      return engine.rotateJoinCode(vaultId);
    },

    async listMembers(vaultId: VaultId) {
      return engine.listMembers(vaultId);
    },

    async setMemberRole(vaultId: VaultId, peerId: PeerId, role: MemberRole) {
      return engine.setMemberRole(vaultId, peerId, role);
    },

    async removeMember(vaultId: VaultId, peerId: PeerId) {
      engine.removeMember(vaultId, peerId);
    },

    async deleteVault(vaultId: VaultId) {
      engine.deleteVault(vaultId);
    },

    async leaveVault(vaultId: VaultId) {
      engine.leaveVault(vaultId);
    },

    async getPresence(vaultId: VaultId) {
      return engine.getPresence(vaultId);
    },

    async publishPresence(input: PresenceInput) {
      engine.publishPresence(input);
    },

    async askAgent(input: AskAgentInput) {
      return engine.askAgent(input);
    },
  };

  Object.defineProperty(client, SIMULATION_KEY, {
    value: engine.simulate,
    enumerable: false,
    configurable: false,
    writable: false,
  });

  return client;
}
