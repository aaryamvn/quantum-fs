//! The command surface the webview calls: one `#[tauri::command]` per `BackendClient` method.
//!
//! WHY the shape is so uniform: `client/src/lib/backend/tauri.ts` is a thin `invoke` per method,
//! so every behavioral rule lives in the embedded node (`src/node/`), never in the UI and no longer
//! in this file. Each command is one `await` into that runtime, which owns the replica, the host
//! connections and the identities (docs/decisions/client-backend-embed.md).
//!
//! Nothing here emits: the node pushes `backend://` events itself, off its own heartbeat, so a
//! change another member made reaches the webview without any command having been called.
//!
//! Search is deliberately absent: it runs client-side over the loaded tree in both runtimes
//! (docs/decisions/client-workspace.md), so there is nothing to mirror here.

use std::path::PathBuf;

use tauri::State;

use crate::node::Node;
use crate::fs_types::{
    AgentReply, AskAgentInput, CreateNodeInput, DeleteNodesInput, DuplicateNodesInput, FsNode,
    HistoryEvent, ImportFilesInput, Member, MemberRole, MoveNodesInput, NodeAccess, PeerPresence,
    PresenceInput, Recent, RenameNodeInput, SetAccessInput, SetNodeColorInput, VaultMeta,
    VaultMetaPatch,
};

/// The OS file picker, as AppleScript. `choose file` returns aliases, so the POSIX paths are
/// assembled one per line — the only shape a shell round trip can carry back losslessly.
#[cfg(target_os = "macos")]
const CHOOSE_FILES_SCRIPT: &str = "set fs to choose file with multiple selections allowed with prompt \"Import into QuantumFS\"\nset out to \"\"\nrepeat with f in fs\nset out to out & POSIX path of f & linefeed\nend repeat\nreturn out";

/* -------------------------------------------------------------------- reads */

#[tauri::command]
pub async fn me(node: State<'_, Node>) -> Result<Member, String> {
    node.me().await
}

#[tauri::command]
pub async fn list_tree(node: State<'_, Node>, vault_id: String) -> Result<Vec<FsNode>, String> {
    node.list_tree(vault_id).await
}

#[tauri::command]
pub async fn read_text_preview(
    node: State<'_, Node>,
    vault_id: String,
    node_id: String,
    max_bytes: usize,
) -> Result<Option<String>, String> {
    node.read_text_preview(vault_id, node_id, max_bytes).await
}

#[tauri::command]
pub async fn get_access(
    node: State<'_, Node>,
    vault_id: String,
    node_id: String,
) -> Result<NodeAccess, String> {
    node.get_access(vault_id, node_id).await
}

#[tauri::command]
pub async fn get_history(
    node: State<'_, Node>,
    vault_id: String,
    node_id: String,
) -> Result<Vec<HistoryEvent>, String> {
    node.get_history(vault_id, node_id).await
}

#[tauri::command]
pub async fn list_recents(node: State<'_, Node>) -> Result<Vec<Recent>, String> {
    node.list_recents().await
}

#[tauri::command]
pub async fn get_vault_meta(
    node: State<'_, Node>,
    vault_id: String,
) -> Result<VaultMeta, String> {
    node.get_vault_meta(vault_id).await
}

#[tauri::command]
pub async fn list_members(node: State<'_, Node>, vault_id: String) -> Result<Vec<Member>, String> {
    node.list_members(vault_id).await
}

#[tauri::command]
pub async fn get_presence(
    node: State<'_, Node>,
    vault_id: String,
) -> Result<Vec<PeerPresence>, String> {
    node.get_presence(vault_id).await
}

/* ---------------------------------------------------------------- tree ops */

#[tauri::command]
pub async fn create_node(
    node: State<'_, Node>,
    input: CreateNodeInput,
) -> Result<FsNode, String> {
    node.create_node(input).await
}

#[tauri::command]
pub async fn rename_node(
    node: State<'_, Node>,
    input: RenameNodeInput,
) -> Result<FsNode, String> {
    node.rename_node(input).await
}

#[tauri::command]
pub async fn move_nodes(
    node: State<'_, Node>,
    input: MoveNodesInput,
) -> Result<Vec<FsNode>, String> {
    node.move_nodes(input).await
}

#[tauri::command]
pub async fn delete_nodes(node: State<'_, Node>, input: DeleteNodesInput) -> Result<(), String> {
    node.delete_nodes(input).await
}

#[tauri::command]
pub async fn duplicate_nodes(
    node: State<'_, Node>,
    input: DuplicateNodesInput,
) -> Result<Vec<FsNode>, String> {
    node.duplicate_nodes(input).await
}

#[tauri::command]
pub async fn set_node_color(
    node: State<'_, Node>,
    input: SetNodeColorInput,
) -> Result<FsNode, String> {
    node.set_node_color(input).await
}

/// Start pulling a remote file and return at once; progress arrives as `fs-changed`
/// upserts from the node's heartbeat, so the UI never polls.
#[tauri::command]
pub async fn request_download(
    node: State<'_, Node>,
    vault_id: String,
    node_id: String,
) -> Result<(), String> {
    node.request_download(vault_id, node_id).await
}

/// Assemble a file's bytes if they are not local yet, then hand the copy to the OS
/// default application for its type.
#[tauri::command]
pub async fn open_node(
    node: State<'_, Node>,
    vault_id: String,
    node_id: String,
) -> Result<(), String> {
    node.open_node(vault_id, node_id).await
}

/// Copy files from the OS into a vault folder.
///
/// `input.paths` absent means the user has not chosen yet, so the native picker runs here rather
/// than in the webview: the drag-and-drop plugin is off (`dragDropEnabled: false`) and a webview
/// `<input type=file>` would hand us bytes instead of paths, which for multi-gigabyte imports is
/// the difference between a copy and a second copy through IPC. Cancelling is not an error — the
/// chooser reports error -128 and the import resolves as an empty list, so the UI shows nothing;
/// any other chooser failure surfaces as an error instead of a silent no-op.
#[tauri::command]
pub async fn import_files(
    node: State<'_, Node>,
    input: ImportFilesInput,
) -> Result<Vec<FsNode>, String> {
    let paths: Vec<PathBuf> = match input.paths {
        Some(given) => given.into_iter().map(PathBuf::from).collect(),
        None => match choose_files().await? {
            Some(chosen) => chosen,
            None => return Ok(Vec::new()),
        },
    };
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    node.import_files(input.vault_id, input.parent_id, paths).await
}

/// The native multi-file chooser. `None` = the user cancelled.
#[cfg(target_os = "macos")]
async fn choose_files() -> Result<Option<Vec<PathBuf>>, String> {
    use std::process::Command;

    // `osascript` blocks for as long as the sheet is up, which is unbounded: it must not sit on
    // an async worker that other commands are waiting for.
    let output = tauri::async_runtime::spawn_blocking(|| {
        Command::new("osascript").arg("-e").arg(CHOOSE_FILES_SCRIPT).output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("Couldn't open the file chooser: {e}"))?;

    // A cancel is error -128 ("User canceled"). The exit code alone cannot be trusted for it —
    // osascript reports 1 for any script error — so a non-zero exit without -128 in stderr is a
    // real failure and its message is worth surfacing rather than swallowing as "nothing chosen".
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-128") {
            return Ok(None);
        }
        return Err(format!("Couldn't open the file chooser: {}", stderr.trim()));
    }
    Ok(Some(parse_chosen_paths(&String::from_utf8_lossy(&output.stdout))))
}

#[cfg(not(target_os = "macos"))]
async fn choose_files() -> Result<Option<Vec<PathBuf>>, String> {
    Err("Choosing files isn't supported on this platform yet".to_string())
}

/// One POSIX path per line; blank lines are the trailing linefeed the script always adds.
///
/// Only the line ending is stripped, never surrounding whitespace: a basename may legally begin or
/// end with a space, and trimming it would hand `import_files` a path that does not exist.
#[cfg(target_os = "macos")]
fn parse_chosen_paths(stdout: &str) -> Vec<PathBuf> {
    stdout
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

/* ------------------------------------------------------------ access/history */

#[tauri::command]
pub async fn set_access(
    node: State<'_, Node>,
    input: SetAccessInput,
) -> Result<NodeAccess, String> {
    node.set_access(input).await
}

#[tauri::command]
pub async fn touch_recent(
    node: State<'_, Node>,
    vault_id: String,
    node_id: String,
) -> Result<(), String> {
    node.touch_recent(vault_id, node_id).await
}

/* ------------------------------------------------------------------ vaults */

#[tauri::command]
pub async fn update_vault_meta(
    node: State<'_, Node>,
    vault_id: String,
    patch: VaultMetaPatch,
) -> Result<VaultMeta, String> {
    node.update_vault_meta(vault_id, patch).await
}

#[tauri::command]
pub async fn rotate_join_code(
    node: State<'_, Node>,
    vault_id: String,
) -> Result<String, String> {
    node.rotate_join_code(vault_id).await
}

#[tauri::command]
pub async fn set_member_role(
    node: State<'_, Node>,
    vault_id: String,
    peer_id: String,
    role: MemberRole,
) -> Result<Member, String> {
    node.set_member_role(vault_id, peer_id, role).await
}

#[tauri::command]
pub async fn remove_member(
    node: State<'_, Node>,
    vault_id: String,
    peer_id: String,
) -> Result<(), String> {
    node.remove_member(vault_id, peer_id).await
}

#[tauri::command]
pub async fn delete_vault(node: State<'_, Node>, vault_id: String) -> Result<(), String> {
    node.delete_vault(vault_id).await
}

#[tauri::command]
pub async fn leave_vault(node: State<'_, Node>, vault_id: String) -> Result<(), String> {
    node.leave_vault(vault_id).await
}

/* ---------------------------------------------------------------- presence */

#[tauri::command]
pub async fn publish_presence(
    node: State<'_, Node>,
    input: PresenceInput,
) -> Result<(), String> {
    node.publish_presence(input).await
}

/* ------------------------------------------------------------------- agent */

#[tauri::command]
pub async fn ask_agent(
    node: State<'_, Node>,
    input: AskAgentInput,
) -> Result<AgentReply, String> {
    node.ask_agent(input).await
}
