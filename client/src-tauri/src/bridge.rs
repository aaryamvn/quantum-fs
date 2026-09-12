//! Webview <-> daemon bridge.
//!
//! Mirrors `client/src/lib/backend/types.ts`. Today the state is seeded in memory;
//! when `qfsd` exists this module keeps the socket to it and the webview keeps
//! calling exactly these commands (docs/decisions/client-stack.md).

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

/// Event emitted after any mutation of the server/vault list.
const SERVERS_CHANGED: &str = "backend://servers-changed";

/// Smallest vault a server will provision: 256 MiB.
const MIN_QUOTA_BYTES: u64 = 268_435_456;
/// Capacity given to a server the user adds by hand, or one invented by a join code: 128 GiB.
const DEFAULT_CAPACITY_BYTES: u64 = 137_438_953_472;
/// Quota assumed for a vault reached through a join code: 1 GiB.
const JOINED_VAULT_QUOTA_BYTES: u64 = 1_073_741_824;
const MAX_VAULT_NAME: usize = 40;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Member,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vault {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub member_count: u32,
    /// Bytes used by the vault's files. Always <= `quota_bytes`.
    pub used_bytes: u64,
    /// Bytes of the server's capacity allocated to this vault.
    pub quota_bytes: u64,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: String,
    pub name: String,
    pub address: String,
    pub peer_id: String,
    pub online: bool,
    /// Total provisionable storage on the server; the sum of vault quotas cannot exceed it.
    pub capacity_bytes: u64,
    pub vaults: Vec<Vault>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatus {
    pub running: bool,
    pub version: Option<String>,
    pub peer_id: Option<String>,
    pub data_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddServerInput {
    pub name: String,
    pub address: String,
}

/// Arguments for creating a vault: a name plus the slice of server capacity it gets.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVaultInput {
    pub server_id: String,
    pub name: String,
    pub quota_bytes: u64,
}

/// Managed state: the server/vault list the webview reads.
pub struct BridgeState(pub Mutex<Vec<Server>>);

impl BridgeState {
    /// Same data as `client/src/lib/backend/seed.ts`.
    pub fn seeded() -> Self {
        BridgeState(Mutex::new(vec![
            Server {
                id: "srv_1".into(),
                name: "Server 1".into(),
                address: "192.168.1.24:7447".into(),
                peer_id: "peer_a1f3c9d2".into(),
                online: true,
                capacity_bytes: 274_877_906_944,
                vaults: vec![
                    Vault {
                        id: "vlt_1_1".into(),
                        server_id: "srv_1".into(),
                        name: "Design Assets".into(),
                        member_count: 5,
                        used_bytes: 1_288_490_189,
                        quota_bytes: 4_294_967_296,
                        role: Role::Owner,
                    },
                    Vault {
                        id: "vlt_1_2".into(),
                        server_id: "srv_1".into(),
                        name: "Hackathon Build".into(),
                        member_count: 5,
                        used_bytes: 671_088_640,
                        quota_bytes: 2_147_483_648,
                        role: Role::Member,
                    },
                    Vault {
                        id: "vlt_1_3".into(),
                        server_id: "srv_1".into(),
                        name: "Family Photos".into(),
                        member_count: 2,
                        used_bytes: 9_019_431_322,
                        quota_bytes: 17_179_869_184,
                        role: Role::Member,
                    },
                ],
            },
            Server {
                id: "srv_2".into(),
                name: "Server 2".into(),
                address: "10.0.0.12:7447".into(),
                peer_id: "peer_7b4e21ac".into(),
                online: true,
                capacity_bytes: 549_755_813_888,
                vaults: vec![
                    Vault {
                        id: "vlt_2_1".into(),
                        server_id: "srv_2".into(),
                        name: "Research Papers".into(),
                        member_count: 4,
                        used_bytes: 327_155_712,
                        quota_bytes: 1_073_741_824,
                        role: Role::Member,
                    },
                    Vault {
                        id: "vlt_2_2".into(),
                        server_id: "srv_2".into(),
                        name: "Backups".into(),
                        member_count: 2,
                        used_bytes: 26_413_435_289,
                        quota_bytes: 68_719_476_736,
                        role: Role::Owner,
                    },
                ],
            },
        ]))
    }
}

fn servers<'a>(state: &'a State<'_, BridgeState>) -> std::sync::MutexGuard<'a, Vec<Server>> {
    state.0.lock().expect("bridge state poisoned")
}

fn notify(app: &AppHandle) {
    // A failed emit means the window is gone; the next mount re-reads state anyway.
    let _ = app.emit(SERVERS_CHANGED, ());
}

#[tauri::command]
pub fn daemon_status() -> DaemonStatus {
    DaemonStatus {
        running: false,
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        peer_id: None,
        data_dir: None,
    }
}

#[tauri::command]
pub fn list_servers(state: State<'_, BridgeState>) -> Vec<Server> {
    servers(&state).clone()
}

#[tauri::command]
pub fn add_server(
    app: AppHandle,
    state: State<'_, BridgeState>,
    input: AddServerInput,
) -> Server {
    let server = {
        let mut list = servers(&state);
        let n = list.len() + 1;
        let server = Server {
            id: format!("srv_{n}"),
            name: input.name,
            address: input.address,
            peer_id: format!("peer_{n}"),
            online: true,
            capacity_bytes: DEFAULT_CAPACITY_BYTES,
            vaults: Vec::new(),
        };
        list.push(server.clone());
        server
    };
    notify(&app);
    server
}

#[tauri::command]
pub fn create_vault(
    app: AppHandle,
    state: State<'_, BridgeState>,
    input: CreateVaultInput,
) -> Result<Vault, String> {
    let vault = {
        let mut list = servers(&state);
        let server = list
            .iter_mut()
            .find(|s| s.id == input.server_id)
            .ok_or_else(|| format!("Unknown server: {}", input.server_id))?;
        let name = input.name.trim().to_string();
        if name.is_empty() || name.chars().count() > MAX_VAULT_NAME {
            return Err("Invalid vault name".to_string());
        }
        // A vault's quota is carved out of what the server has not already promised.
        let allocated: u64 = server.vaults.iter().map(|v| v.quota_bytes).sum();
        let free = server.capacity_bytes.saturating_sub(allocated);
        if input.quota_bytes < MIN_QUOTA_BYTES || input.quota_bytes > free {
            return Err("Not enough space on this server".to_string());
        }
        let vault = Vault {
            id: format!("{}_{}", server.id.replace("srv", "vlt"), server.vaults.len() + 1),
            server_id: server.id.clone(),
            name,
            member_count: 1,
            used_bytes: 0,
            quota_bytes: input.quota_bytes,
            role: Role::Owner,
        };
        server.vaults.push(vault.clone());
        vault
    };
    notify(&app);
    Ok(vault)
}

#[tauri::command]
pub fn join_vault(
    app: AppHandle,
    state: State<'_, BridgeState>,
    code: String,
) -> Result<Vault, String> {
    // The central directory only maps a 6-character code -> (server, vault);
    // anything that is not exactly six A-Z/0-9 characters never resolves.
    let normalized = code.trim().to_ascii_uppercase();
    if normalized.len() != 6
        || !normalized
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err("Invalid join code".to_string());
    }
    let vault = {
        let mut list = servers(&state);
        let n = list.len() + 1;
        let vault = Vault {
            id: format!("vlt_{n}_1"),
            server_id: format!("srv_{n}"),
            name: "Joined Vault".into(),
            member_count: 1,
            used_bytes: 0,
            quota_bytes: JOINED_VAULT_QUOTA_BYTES,
            role: Role::Member,
        };
        list.push(Server {
            id: format!("srv_{n}"),
            name: "Directory result".into(),
            address: "unknown".into(),
            peer_id: format!("peer_{n}"),
            online: false,
            capacity_bytes: DEFAULT_CAPACITY_BYTES,
            vaults: vec![vault.clone()],
        });
        vault
    };
    notify(&app);
    Ok(vault)
}
