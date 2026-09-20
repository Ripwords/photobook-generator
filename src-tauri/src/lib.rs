pub mod agent;
pub mod book;
pub mod cache;
pub mod cluster;
pub mod commands;
pub mod db;
pub mod export;
pub mod geometry;
pub mod place_names;
pub mod preview;
pub mod print_spec;
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(commands::AppState::default())
        .manage(agent::request::ModelRequests::new())
}

pub fn run() {
    builder()
        .invoke_handler(tauri::generate_handler![
            commands::analyze_folders,
            commands::forget_run,
            commands::list_drafts,
            commands::save_draft,
            commands::delete_draft,
            commands::apply_photo_overrides,
            commands::recommend_book,
            commands::place_chapters,
            commands::place_names,
            commands::generate_book,
            commands::default_print_spec,
            commands::check_print_spec,
            commands::export_book,
            commands::list_projects,
            commands::open_project,
            commands::book_layout,
            commands::folder_check,
            commands::import_photo,
            commands::agent_view,
            commands::agent_edit,
            commands::set_api_key,
            commands::clear_api_key,
            commands::api_key_status,
            commands::model_request,
            commands::cancel_model_request,
            commands::edit_book,
            commands::slot_candidates,
            commands::cover_candidates,
            commands::delete_project,
            commands::restore_project,
            commands::rename_project,
            commands::set_favourite,
            commands::reveal_in_finder,
            commands::cache_status,
            commands::set_cache_limit,
            commands::clear_unused_cache,
        ])
        .setup(|app| {
            commands::spawn_cache_enforcement(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
