import type {
  BackendEvent,
  CreateVaultInput,
  DaemonStatus,
  JoinCode,
  OrchestrationServer,
  Vault,
} from "./types";

/**
 * The single seam between the webview and the local `qfsd` daemon.
 *
 * The webview never speaks to the daemon directly (docs/decisions/client-stack.md):
 * inside Tauri this is implemented by Rust commands that own the daemon socket,
 * outside Tauri by an in-memory mock with seeded data.
 */
export interface BackendClient {
  status(): Promise<DaemonStatus>;
  listServers(): Promise<OrchestrationServer[]>;
  addServer(input: { name: string; address: string }): Promise<OrchestrationServer>;
  createVault(input: CreateVaultInput): Promise<Vault>;
  joinVault(code: JoinCode): Promise<Vault>;
  /** Returns an unsubscribe function. */
  subscribe(listener: (e: BackendEvent) => void): () => void;
}
