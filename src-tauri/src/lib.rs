pub mod book;
pub mod cluster;
pub mod commands;
pub mod db;
pub mod geometry;
pub mod project;
pub mod protocol;
pub mod ranking;
pub mod sidecar;
pub mod templates;

/// Builds the shared plugin/state chain used by both the real app (`run`,
/// below) and by tests that need a live `AppHandle` (e.g.
/// `tests/sidecar_worker_pool.rs`, `tests/cache_hit.rs`) -- so a diagnostic
/// wired up here is guaranteed to be exercised by those tests too, not just
/// declared and left unverified.
///
/// Without a registered logger, the `log` crate's `log::warn!`/`log::info!`
/// etc. calls throughout `commands.rs` and `sidecar.rs` are no-ops: they
/// cover unhashable files, sidecar retries, record-count mismatches and
/// notification failures -- exactly the diagnostics needed for the first
/// real run against RAW/HEIC photos. `tauri-plugin-log`'s default targets
/// are stdout plus a rotating file under the app's log directory, which is
/// enough to make those calls observable without any extra configuration.
pub fn builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(commands::AppState::default())
}

pub fn run() {
    builder()
        .invoke_handler(tauri::generate_handler![commands::analyze_folder])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
