mod bridge;

/// Liveness probe for the webview <-> Rust bridge.
#[tauri::command]
fn ping() -> &'static str {
    "pong"
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(bridge::BridgeState::seeded())
        .invoke_handler(tauri::generate_handler![
            ping,
            bridge::daemon_status,
            bridge::list_servers,
            bridge::add_server,
            bridge::create_vault,
            bridge::join_vault
        ])
        .run(tauri::generate_context!())
        .expect("error while running QuantamFS");
}
