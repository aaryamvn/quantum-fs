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
 * The scripted multiplayer demo is the one wrapper around it; deleting this call and
 * `./demo/` removes it entirely (docs/decisions/client-workspace.md).
 */
export function createBackend(): BackendClient {
  const inner = isTauri() ? createTauriBackend() : createMockBackend();
  return withDemo(inner, devQuery.demo);
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
export { BackendProvider, useBackend } from "./BackendProvider";
export type { BackendContextValue } from "./BackendProvider";
