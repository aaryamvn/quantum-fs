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

/// Managed state: the server/vault list the webview reads.
pub struct BridgeState(pub Mutex<Vec<Server>>);

impl BridgeState {
    /// Same data as `client/src/lib/backend/seed.ts`.
    pub fn seeded() -> Self {
        BridgeState(Mutex::new(vec![
            Server {
                id: "srv_1".into(),
                name: "Server 1".into(),
                address: "127.0.0.1:7447".into(),
                peer_id: "peer_a1f3c9d2".into(),
                online: true,
                vaults: vec![
                    Vault {
                        id: "vlt_1_1".into(),
                        server_id: "srv_1".into(),
                        name: "Vault 1".into(),
                        member_count: 3,
                        role: Role::Owner,
                    },
                    Vault {
                        id: "vlt_1_2".into(),
                        server_id: "srv_1".into(),
                        name: "Vault 2".into(),
                        member_count: 5,
                        role: Role::Member,
                    },
                    Vault {
                        id: "vlt_1_3".into(),
                        server_id: "srv_1".into(),
                        name: "Vault 3".into(),
                        member_count: 2,
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
                vaults: vec![
                    Vault {
                        id: "vlt_2_1".into(),
                        server_id: "srv_2".into(),
                        name: "Vault 1".into(),
                        member_count: 4,
                        role: Role::Member,
                    },
                    Vault {
                        id: "vlt_2_2".into(),
                        server_id: "srv_2".into(),
                        name: "Vault 2".into(),
                        member_count: 2,
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
    server_id: String,
    name: String,
) -> Result<Vault, String> {
    let vault = {
        let mut list = servers(&state);
        let server = list
            .iter_mut()
            .find(|s| s.id == server_id)
            .ok_or_else(|| format!("Unknown server: {server_id}"))?;
        let vault = Vault {
            id: format!("{}_{}", server.id.replace("srv", "vlt"), server.vaults.len() + 1),
            server_id: server.id.clone(),
            name,
            member_count: 1,
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
    // The central directory only maps code -> (server, vault); a short code never resolves.
    if code.len() < 8 {
        return Err("Invalid join code".to_string());
    }
    let vault = {
        let mut list = servers(&state);
        let n = list.len() + 1;
        let vault = Vault {
            id: format!("vlt_{n}_1"),
            server_id: format!("srv_{n}"),
            name: "Joined vault".into(),
            member_count: 1,
            role: Role::Member,
        };
        list.push(Server {
            id: format!("srv_{n}"),
            name: "Directory result".into(),
            address: "unknown".into(),
            peer_id: format!("peer_{n}"),
            online: false,
            vaults: vec![vault.clone()],
        });
        vault
    };
    notify(&app);
    Ok(vault)
}
