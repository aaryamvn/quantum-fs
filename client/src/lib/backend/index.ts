import { isTauri } from "@tauri-apps/api/core";

import type { BackendClient } from "./client";
import { createMockBackend } from "./mock";
import { createTauriBackend } from "./tauri";

/**
 * Picks the implementation for the current runtime: the Rust bridge inside the
 * Tauri shell, the seeded in-memory mock in a plain browser.
 */
export function createBackend(): BackendClient {
  return isTauri() ? createTauriBackend() : createMockBackend();
}

export type { BackendClient } from "./client";
export type {
  BackendEvent,
  CreateVaultInput,
  DaemonStatus,
  JoinCode,
  OrchestrationServer,
  PeerId,
  ServerId,
  Vault,
  VaultId,
} from "./types";
export { SEED_SERVERS } from "./seed";
export { createMockBackend } from "./mock";
export { createTauriBackend } from "./tauri";
export { BackendProvider, useBackend } from "./BackendProvider";
export type { BackendContextValue } from "./BackendProvider";
