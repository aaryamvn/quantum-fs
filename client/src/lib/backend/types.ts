/**
 * Domain types for the daemon bridge.
 *
 * These mirror `client/src-tauri/src/bridge.rs` exactly; the Rust side serializes
 * with `#[serde(rename_all = "camelCase")]` so the wire shape matches these names.
 * Terminology follows docs/decisions/net-vault-join-directory.md.
 */

/** Cryptographic member identity (crypto-identity-selfcert). Never a MAC or email. */
export type PeerId = string;

/** Identifier of an orchestration server (the designated host H of a set of vaults). */
export type ServerId = string;

/** Identifier of a vault (one shared file system with a member list). */
export type VaultId = string;

/** High-entropy base32 code that the central directory maps to (server, vault). */
export type JoinCode = string;

export interface Vault {
  id: VaultId;
  serverId: ServerId;
  name: string;
  memberCount: number;
  role: "owner" | "member";
}

export interface OrchestrationServer {
  id: ServerId;
  name: string;
  address: string;
  peerId: PeerId;
  online: boolean;
  vaults: Vault[];
}

export interface DaemonStatus {
  running: boolean;
  version: string | null;
  peerId: PeerId | null;
  dataDir: string | null;
}

/** Push notifications from the Rust side (or the mock) to the webview. */
export type BackendEvent =
  | { type: "servers-changed" }
  | { type: "daemon-status"; status: DaemonStatus };
