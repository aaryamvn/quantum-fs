//! Webview <-> node bridge: the server, vault and daemon commands.
//!
//! WHY this file is so thin: the state it used to fake now lives in the embedded backend node
//! (`src/node/`, docs/decisions/client-backend-embed.md), which owns the admin connections to the
//! hosts and every identity this client holds. Each command here is one `await` into that runtime,
//! so the webview's `client/src/lib/backend/tauri.ts` keeps calling exactly these names.
//!
//! Wire shapes live in [`crate::fs_types`], which is also where the node reads and writes them.

use tauri::State;

use crate::fs_types::{AddServerInput, CreateVaultInput, DaemonStatus, Server, Vault};
use crate::node::Node;

/// Whether the node came up, and the identity and data directory it came up with.
#[tauri::command]
pub async fn daemon_status(node: State<'_, Node>) -> Result<DaemonStatus, String> {
    node.status().await
}

/// Every host this client knows, each with the vaults it holds for us.
#[tauri::command]
pub async fn list_servers(node: State<'_, Node>) -> Result<Vec<Server>, String> {
    node.list_servers().await
}

/// Register a host from the `IP:PORT/TOKEN` connect string it printed on startup.
#[tauri::command]
pub async fn add_server(node: State<'_, Node>, input: AddServerInput) -> Result<Server, String> {
    node.add_server(input.name, input.address).await
}

/// Ask a host to provision a vault and join it as its owner.
#[tauri::command]
pub async fn create_vault(node: State<'_, Node>, input: CreateVaultInput) -> Result<Vault, String> {
    node.create_vault(input).await
}

/// Join an existing vault from a join code, or a `directory:port/code` string.
#[tauri::command]
pub async fn join_vault(node: State<'_, Node>, code: String) -> Result<Vault, String> {
    node.join_vault(code).await
}
