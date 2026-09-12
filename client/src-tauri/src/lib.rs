mod bridge;
mod fs_commands;
mod fs_state;
pub mod fs_types;
pub mod node;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{Emitter, Manager};

/// Liveness probe for the webview <-> Rust bridge.
#[tauri::command]
fn ping() -> &'static str {
    "pong"
}

/// Where this client keeps its identities, replicas and assembled files.
///
/// `QFS_DATA_DIR` exists so two clients can run side by side on one laptop during a demo — the
/// backend takes an exclusive lock per identity directory, so a second app sharing the first's
/// data dir would refuse to start (docs/decisions/client-backend-embed.md).
fn data_dir(app: &tauri::App) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = match std::env::var_os("QFS_DATA_DIR") {
        Some(value) => PathBuf::from(value),
        None => app.path().app_data_dir()?,
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Built inside `setup` because the node needs an `AppHandle` to emit through, and the
            // handle only exists once the app is assembled. Every `backend://` event the webview
            // listens to comes out of this one callback.
            let handle = app.handle().clone();
            let emit: node::Emit = Arc::new(move |name: &str, payload: serde_json::Value| {
                // A failed emit means the window is gone; the next mount re-reads state anyway.
                let _ = handle.emit(name, payload);
            });
            app.manage(node::Node::start(data_dir(app)?, emit));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            bridge::daemon_status,
            bridge::list_servers,
            bridge::add_server,
            bridge::create_vault,
            bridge::join_vault,
            fs_commands::me,
            fs_commands::get_profile,
            fs_commands::set_profile,
            fs_commands::list_tree,
            fs_commands::create_node,
            fs_commands::rename_node,
            fs_commands::move_nodes,
            fs_commands::delete_nodes,
            fs_commands::duplicate_nodes,
            fs_commands::set_node_color,
            fs_commands::request_download,
            fs_commands::open_node,
            fs_commands::import_files,
            fs_commands::read_text_preview,
            fs_commands::get_access,
            fs_commands::set_access,
            fs_commands::get_history,
            fs_commands::list_recents,
            fs_commands::touch_recent,
            fs_commands::get_vault_meta,
            fs_commands::update_vault_meta,
            fs_commands::rotate_join_code,
            fs_commands::list_members,
            fs_commands::set_member_role,
            fs_commands::remove_member,
            fs_commands::delete_vault,
            fs_commands::leave_vault,
            fs_commands::get_presence,
            fs_commands::publish_presence,
            fs_commands::ask_agent
        ])
        .build(tauri::generate_context!())
        .expect("error while running QuantamFS")
        // The node runs on its own thread with its own Tokio runtime, so nothing stops it when
        // the window closes: without this the vault tasks are killed mid-write and the demo log
        // loses whatever is still buffered. `Exit` is the last event before the process ends.
        .run(|handle, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(node) = handle.try_state::<node::Node>() {
                    node.shutdown();
                }
            }
        });
}
