//! The command surface the webview calls: one `#[tauri::command]` per `BackendClient` method.
//!
//! WHY the shape is so uniform: `client/src/lib/backend/tauri.ts` is a thin `invoke` per method,
//! so every behavioral rule lives here or in [`crate::fs_state`], never in the UI. Each mutation
//! does the same three steps — lock, mutate, drop the guard, *then* emit — because a listener that
//! calls straight back into a command while the mutex is still held would deadlock the app.
//!
//! Search is deliberately absent: it runs client-side over the loaded tree in both runtimes
//! (docs/decisions/client-workspace.md), so there is nothing to mirror here.

use std::sync::MutexGuard;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::bridge::{BridgeState, Server};
use crate::fs_state::{FsDb, FsState};
use crate::fs_types::{
    AgentReply, AskAgentInput, CreateNodeInput, DeleteNodesInput, DuplicateNodesInput, FsChange,
    FsChangedPayload, FsNode, HistoryEvent, Member, MemberRole, MoveNodesInput, NodeAccess,
    PeerPresence, PresenceInput, PresencePayload, Recent, RenameNodeInput, SetAccessInput,
    SetNodeColorInput, VaultIdPayload, VaultMeta, VaultMetaPatch,
};

/// Deltas of one mutation, plus the member who caused them.
const FS_CHANGED: &str = "backend://fs-changed";
/// A vault's member list changed (role, removal).
const MEMBERS_CHANGED: &str = "backend://members-changed";
/// A vault's settings changed (name, description, join code, cleanup).
const VAULT_CHANGED: &str = "backend://vault-changed";
/// The recents list changed.
const RECENTS_CHANGED: &str = "backend://recents-changed";
/// One peer published presence; the payload carries the vault's whole peer list.
const PRESENCE: &str = "backend://presence";
/// The server/vault list changed — same event `bridge.rs` emits, since it is the same list.
const SERVERS_CHANGED: &str = "backend://servers-changed";

/// One tick of a simulated transfer; 100ms is the smallest step the progress ring can show.
const DOWNLOAD_TICK_MS: u64 = 100;
/// Long enough for the agent bar's typing indicator to read as thinking, short enough to not annoy.
const AGENT_LATENCY_MS: u64 = 650;

fn fs<'a>(state: &'a State<'_, FsState>) -> MutexGuard<'a, FsDb> {
    state.0.lock().expect("fs state poisoned")
}

fn bridge<'a>(state: &'a State<'_, BridgeState>) -> MutexGuard<'a, Vec<Server>> {
    state.0.lock().expect("bridge state poisoned")
}

/// A failed emit means the window is gone; the next mount re-reads state anyway.
fn emit_fs(app: &AppHandle, vault_id: &str, changes: Vec<FsChange>, actor: &str) {
    if changes.is_empty() {
        return;
    }
    let _ = app.emit(
        FS_CHANGED,
        FsChangedPayload {
            vault_id: vault_id.to_string(),
            changes,
            actor: actor.to_string(),
        },
    );
}

fn emit_vault(app: &AppHandle, event: &str, vault_id: &str) {
    let _ = app.emit(
        event,
        VaultIdPayload {
            vault_id: vault_id.to_string(),
        },
    );
}

/// Push a vault's name/server/member count into the server list `bridge.rs` owns.
/// Returns whether anything actually moved, so we only emit when the sidebar must redraw.
fn sync_vault_into_bridge(servers: &mut [Server], meta: &VaultMeta, member_count: u32) -> bool {
    let mut found: Option<(usize, usize)> = None;
    for (si, server) in servers.iter().enumerate() {
        if let Some(vi) = server.vaults.iter().position(|v| v.id == meta.id) {
            found = Some((si, vi));
            break;
        }
    }
    let Some((si, vi)) = found else {
        return false;
    };
    let target = servers.iter().position(|s| s.id == meta.server_id);

    // Moving a vault between servers: take it off the old one, hand it to the new one.
    if let Some(ti) = target {
        if ti != si {
            let mut vault = servers[si].vaults.remove(vi);
            vault.server_id = meta.server_id.clone();
            vault.name = meta.name.clone();
            vault.member_count = member_count;
            servers[ti].vaults.push(vault);
            return true;
        }
    }

    let vault = &mut servers[si].vaults[vi];
    let mut changed = false;
    if vault.name != meta.name {
        vault.name = meta.name.clone();
        changed = true;
    }
    if vault.member_count != member_count {
        vault.member_count = member_count;
        changed = true;
    }
    changed
}

fn set_bridge_member_count(servers: &mut [Server], vault_id: &str, count: u32) -> bool {
    for server in servers.iter_mut() {
        for vault in server.vaults.iter_mut() {
            if vault.id == vault_id {
                if vault.member_count == count {
                    return false;
                }
                vault.member_count = count;
                return true;
            }
        }
    }
    false
}

fn drop_vault_from_bridge(servers: &mut [Server], vault_id: &str) -> bool {
    let mut changed = false;
    for server in servers.iter_mut() {
        let before = server.vaults.len();
        server.vaults.retain(|v| v.id != vault_id);
        if server.vaults.len() != before {
            changed = true;
        }
    }
    changed
}

/* -------------------------------------------------------------------- reads */

#[tauri::command]
pub fn me(state: State<'_, FsState>) -> Result<Member, String> {
    fs(&state).me()
}

#[tauri::command]
pub fn list_tree(state: State<'_, FsState>, vault_id: String) -> Vec<FsNode> {
    fs(&state).list_tree(&vault_id)
}

#[tauri::command]
pub fn read_text_preview(
    state: State<'_, FsState>,
    vault_id: String,
    node_id: String,
    max_bytes: usize,
) -> Option<String> {
    let _ = vault_id;
    fs(&state).preview(&node_id, max_bytes)
}

#[tauri::command]
pub fn get_access(
    state: State<'_, FsState>,
    vault_id: String,
    node_id: String,
) -> Result<NodeAccess, String> {
    let _ = vault_id;
    fs(&state).get_access(&node_id)
}

#[tauri::command]
pub fn get_history(
    state: State<'_, FsState>,
    vault_id: String,
    node_id: String,
) -> Vec<HistoryEvent> {
    fs(&state).get_history(&vault_id, &node_id)
}

#[tauri::command]
pub fn list_recents(state: State<'_, FsState>) -> Vec<Recent> {
    fs(&state).list_recents()
}

#[tauri::command]
pub fn get_vault_meta(state: State<'_, FsState>, vault_id: String) -> Result<VaultMeta, String> {
    fs(&state).get_vault_meta(&vault_id)
}

#[tauri::command]
pub fn list_members(state: State<'_, FsState>, vault_id: String) -> Vec<Member> {
    fs(&state).list_members(&vault_id)
}

#[tauri::command]
pub fn get_presence(state: State<'_, FsState>, vault_id: String) -> Vec<PeerPresence> {
    fs(&state).get_presence(&vault_id)
}

/* ---------------------------------------------------------------- tree ops */

#[tauri::command]
pub fn create_node(
    app: AppHandle,
    state: State<'_, FsState>,
    input: CreateNodeInput,
) -> Result<FsNode, String> {
    let vault_id = input.vault_id.clone();
    let actor_hint = input.actor.clone();
    let (node, changes, actor) = {
        let mut db = fs(&state);
        let actor = actor_hint.unwrap_or_else(|| db.self_id());
        let (node, changes) = db.create_node(input)?;
        (node, changes, actor)
    };
    emit_fs(&app, &vault_id, changes, &actor);
    Ok(node)
}

#[tauri::command]
pub fn rename_node(
    app: AppHandle,
    state: State<'_, FsState>,
    input: RenameNodeInput,
) -> Result<FsNode, String> {
    let vault_id = input.vault_id.clone();
    let actor_hint = input.actor.clone();
    let (node, changes, actor) = {
        let mut db = fs(&state);
        let actor = actor_hint.unwrap_or_else(|| db.self_id());
        let (node, changes) = db.rename_node(input)?;
        (node, changes, actor)
    };
    emit_fs(&app, &vault_id, changes, &actor);
    Ok(node)
}

#[tauri::command]
pub fn move_nodes(
    app: AppHandle,
    state: State<'_, FsState>,
    input: MoveNodesInput,
) -> Result<Vec<FsNode>, String> {
    let vault_id = input.vault_id.clone();
    let actor_hint = input.actor.clone();
    let (nodes, changes, actor) = {
        let mut db = fs(&state);
        let actor = actor_hint.unwrap_or_else(|| db.self_id());
        let (nodes, changes) = db.move_nodes(input)?;
        (nodes, changes, actor)
    };
    emit_fs(&app, &vault_id, changes, &actor);
    Ok(nodes)
}

#[tauri::command]
pub fn delete_nodes(
    app: AppHandle,
    state: State<'_, FsState>,
    input: DeleteNodesInput,
) -> Result<(), String> {
    let vault_id = input.vault_id.clone();
    let actor_hint = input.actor.clone();
    let (changes, recents_changed, actor) = {
        let mut db = fs(&state);
        let actor = actor_hint.unwrap_or_else(|| db.self_id());
        let (changes, recents_changed) = db.delete_nodes(input)?;
        (changes, recents_changed, actor)
    };
    emit_fs(&app, &vault_id, changes, &actor);
    if recents_changed {
        let _ = app.emit(RECENTS_CHANGED, ());
    }
    Ok(())
}

#[tauri::command]
pub fn duplicate_nodes(
    app: AppHandle,
    state: State<'_, FsState>,
    input: DuplicateNodesInput,
) -> Result<Vec<FsNode>, String> {
    let vault_id = input.vault_id.clone();
    let actor_hint = input.actor.clone();
    let (nodes, changes, actor) = {
        let mut db = fs(&state);
        let actor = actor_hint.unwrap_or_else(|| db.self_id());
        let (nodes, changes) = db.duplicate_nodes(input)?;
        (nodes, changes, actor)
    };
    emit_fs(&app, &vault_id, changes, &actor);
    Ok(nodes)
}

#[tauri::command]
pub fn set_node_color(
    app: AppHandle,
    state: State<'_, FsState>,
    input: SetNodeColorInput,
) -> Result<FsNode, String> {
    let vault_id = input.vault_id.clone();
    let actor_hint = input.actor.clone();
    let (node, changes, actor) = {
        let mut db = fs(&state);
        let actor = actor_hint.unwrap_or_else(|| db.self_id());
        let (node, changes) = db.set_node_color(input)?;
        (node, changes, actor)
    };
    emit_fs(&app, &vault_id, changes, &actor);
    Ok(node)
}

/// Start a simulated transfer of a remote file and return immediately; progress arrives
/// as `fs-changed` upserts from a ticker thread, exactly as a real fetch would report it.
#[tauri::command]
pub fn request_download(
    app: AppHandle,
    state: State<'_, FsState>,
    vault_id: String,
    node_id: String,
) -> Result<(), String> {
    let (started, actor) = {
        let mut db = fs(&state);
        let actor = db.self_id();
        (db.begin_download(&node_id)?, actor)
    };
    let Some((change, duration)) = started else {
        return Ok(());
    };
    emit_fs(&app, &vault_id, vec![change], &actor);

    let handle = app.clone();
    let step = DOWNLOAD_TICK_MS as f64 / duration;
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(DOWNLOAD_TICK_MS));
            // Lock, advance, drop — never emit while the mutex is held.
            let tick = {
                let state = handle.state::<FsState>();
                let mut db = state.0.lock().expect("fs state poisoned");
                db.tick_download(&node_id, step)
            };
            // The node was deleted (or someone else finished it) while we were sleeping.
            let Some((change, done)) = tick else {
                break;
            };
            emit_fs(&handle, &vault_id, vec![change], &actor);
            if done {
                break;
            }
        }
    });
    Ok(())
}

/* ------------------------------------------------------------ access/history */

/// A permission change is not an edit, so `modifiedAt` stays put — but the node still goes
/// out as an upsert, because the inspector reads its access badge from the tree.
#[tauri::command]
pub fn set_access(
    app: AppHandle,
    state: State<'_, FsState>,
    input: SetAccessInput,
) -> Result<NodeAccess, String> {
    let vault_id = input.vault_id.clone();
    let (access, changes, actor) = {
        let mut db = fs(&state);
        let actor = db.self_id();
        let (access, changes) = db.set_access(input)?;
        (access, changes, actor)
    };
    emit_fs(&app, &vault_id, changes, &actor);
    Ok(access)
}

#[tauri::command]
pub fn touch_recent(
    app: AppHandle,
    state: State<'_, FsState>,
    vault_id: String,
    node_id: String,
) -> Result<(), String> {
    fs(&state).touch_recent(&vault_id, &node_id);
    let _ = app.emit(RECENTS_CHANGED, ());
    Ok(())
}

/* ------------------------------------------------------------------ vaults */

#[tauri::command]
pub fn update_vault_meta(
    app: AppHandle,
    state: State<'_, FsState>,
    bridge_state: State<'_, BridgeState>,
    vault_id: String,
    patch: VaultMetaPatch,
) -> Result<VaultMeta, String> {
    // The vault row lives in `bridge.rs`'s list, so a server move has to be validated
    // against it before the settings are written; the ids are read under their own lock.
    let known: Vec<String> = bridge(&bridge_state).iter().map(|s| s.id.clone()).collect();
    let (update, members, actor) = {
        let mut db = fs(&state);
        let actor = db.self_id();
        let update = db.update_vault_meta(&vault_id, patch, &known)?;
        let members = db.member_count(&vault_id);
        (update, members, actor)
    };
    if update.renamed || update.rehomed {
        let mut servers = bridge(&bridge_state);
        sync_vault_into_bridge(&mut servers, &update.meta, members);
    }

    emit_fs(&app, &vault_id, update.changes, &actor);
    emit_vault(&app, VAULT_CHANGED, &vault_id);
    if update.renamed || update.rehomed {
        let _ = app.emit(SERVERS_CHANGED, ());
    }
    Ok(update.meta)
}

#[tauri::command]
pub fn rotate_join_code(
    app: AppHandle,
    state: State<'_, FsState>,
    vault_id: String,
) -> Result<String, String> {
    let code = fs(&state).rotate_join_code(&vault_id)?;
    emit_vault(&app, VAULT_CHANGED, &vault_id);
    Ok(code)
}

#[tauri::command]
pub fn set_member_role(
    app: AppHandle,
    state: State<'_, FsState>,
    vault_id: String,
    peer_id: String,
    role: MemberRole,
) -> Result<Member, String> {
    let member = fs(&state).set_member_role(&vault_id, &peer_id, role)?;
    emit_vault(&app, MEMBERS_CHANGED, &vault_id);
    let _ = app.emit(SERVERS_CHANGED, ());
    Ok(member)
}

#[tauri::command]
pub fn remove_member(
    app: AppHandle,
    state: State<'_, FsState>,
    bridge_state: State<'_, BridgeState>,
    vault_id: String,
    peer_id: String,
) -> Result<(), String> {
    let (remaining, peers) = {
        let mut db = fs(&state);
        let remaining = db.remove_member(&vault_id, &peer_id)?;
        (remaining, db.get_presence(&vault_id))
    };
    {
        let mut servers = bridge(&bridge_state);
        set_bridge_member_count(&mut servers, &vault_id, remaining);
    }
    emit_vault(&app, MEMBERS_CHANGED, &vault_id);
    let _ = app.emit(SERVERS_CHANGED, ());
    let _ = app.emit(
        PRESENCE,
        PresencePayload {
            vault_id: vault_id.clone(),
            peers,
        },
    );
    Ok(())
}

#[tauri::command]
pub fn delete_vault(
    app: AppHandle,
    state: State<'_, FsState>,
    bridge_state: State<'_, BridgeState>,
    vault_id: String,
) -> Result<(), String> {
    fs(&state).delete_vault(&vault_id)?;
    {
        let mut servers = bridge(&bridge_state);
        drop_vault_from_bridge(&mut servers, &vault_id);
    }
    let _ = app.emit(SERVERS_CHANGED, ());
    Ok(())
}

#[tauri::command]
pub fn leave_vault(
    app: AppHandle,
    state: State<'_, FsState>,
    bridge_state: State<'_, BridgeState>,
    vault_id: String,
) -> Result<(), String> {
    fs(&state).leave_vault(&vault_id)?;
    {
        let mut servers = bridge(&bridge_state);
        drop_vault_from_bridge(&mut servers, &vault_id);
    }
    let _ = app.emit(SERVERS_CHANGED, ());
    Ok(())
}

/* ---------------------------------------------------------------- presence */

#[tauri::command]
pub fn publish_presence(
    app: AppHandle,
    state: State<'_, FsState>,
    input: PresenceInput,
) -> Result<(), String> {
    let vault_id = input.vault_id.clone();
    let peers = fs(&state).publish_presence(input);
    let _ = app.emit(PRESENCE, PresencePayload { vault_id, peers });
    Ok(())
}

/* ------------------------------------------------------------------- agent */

/// Canned answer for the agent bar. `async` so the deliberate think-time never blocks
/// the main thread, and the lock is taken only after the sleep.
#[tauri::command(async)]
pub fn ask_agent(state: State<'_, FsState>, input: AskAgentInput) -> Result<AgentReply, String> {
    // Deliberate think-time, taken before the lock so no other command waits on it.
    thread::sleep(Duration::from_millis(AGENT_LATENCY_MS));
    let (id, text) = fs(&state).agent_reply(&input.folder_id)?;
    Ok(AgentReply { id, text })
}
