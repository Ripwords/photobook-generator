pub mod cluster;
pub mod commands;
pub mod db;
pub mod protocol;
pub mod ranking;
pub mod sidecar;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![commands::analyze_folder])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
