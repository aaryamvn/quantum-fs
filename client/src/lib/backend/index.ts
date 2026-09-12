import { isTauri } from "@tauri-apps/api/core";

import { devQuery } from "@/lib/devQuery";

import type { BackendClient } from "./client";
import withDemo from "./demo";
import { createMockBackend } from "./mock";
import { createTauriBackend } from "./tauri";

/**
 * Picks the implementation for the current runtime: the Rust bridge inside the
 * Tauri shell, the seeded in-memory mock in a plain browser.
 *
 * The scripted multiplayer demo wraps ONLY the mock. Inside Tauri the vaults, the
 * members and the peers are real, and the script would invent moves and cursors on
 * top of someone's actual files; the browser is where it belongs (design work and
 * screenshots). Deleting this call and `./demo/` removes it entirely
 * (docs/decisions/client-workspace.md).
 */
export function createBackend(): BackendClient {
  const inner = isTauri() ? createTauriBackend() : withDemo(createMockBackend(), devQuery.demo);
  return inner;
}

export type {
  ActorInput,
  AskAgentInput,
  BackendClient,
  CreateNodeInput,
  DeleteNodesInput,
  DuplicateNodesInput,
  MoveNodesInput,
  PresenceInput,
  RenameNodeInput,
  SetAccessInput,
  SetNodeColorInput,
  VaultMetaPatch,
} from "./client";
export type {
  AccessEntry,
  AccessLevel,
  AgentReply,
  Availability,
  BackendEvent,
  CreateVaultInput,
  DaemonStatus,
  FolderColor,
  FsChange,
  FsNode,
  HistoryEvent,
  HistoryKind,
  JoinCode,
  Member,
  MemberRole,
  NodeAccess,
  NodeId,
  NodeKind,
  OrchestrationServer,
  PeerCursor,
  PeerId,
  PeerPresence,
  Profile,
  ProfilePatch,
  Recent,
  RemoteOp,
  SearchHit,
  SearchQuery,
  ServerId,
  Vault,
  VaultId,
  VaultMeta,
} from "./types";
export { FOLDER_COLORS } from "./types";
export { SEED_SERVERS } from "./seed";
export { createMockBackend } from "./mock";
export { createTauriBackend } from "./tauri";
export { BackendProvider, useBackend, useBackendClient } from "./BackendProvider";
export type { BackendContextValue } from "./BackendProvider";
