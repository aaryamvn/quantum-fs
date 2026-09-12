mod bridge;
mod fs_commands;
mod fs_state;
mod fs_types;

use std::sync::Mutex;

/// Liveness probe for the webview <-> Rust bridge.
#[tauri::command]
fn ping() -> &'static str {
    "pong"
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(bridge::BridgeState::seeded())
        .manage(fs_state::FsState(Mutex::new(fs_state::FsDb::seeded())))
        .invoke_handler(tauri::generate_handler![
            ping,
            bridge::daemon_status,
            bridge::list_servers,
            bridge::add_server,
            bridge::create_vault,
            bridge::join_vault,
            fs_commands::me,
            fs_commands::list_tree,
            fs_commands::create_node,
            fs_commands::rename_node,
            fs_commands::move_nodes,
            fs_commands::delete_nodes,
            fs_commands::duplicate_nodes,
            fs_commands::set_node_color,
            fs_commands::request_download,
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
        .run(tauri::generate_context!())
        .expect("error while running QuantamFS");
}
