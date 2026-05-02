pub mod app_data;
pub mod commands;
pub mod config;
pub mod db;
pub mod error;
pub mod events;
pub mod model_catalog;
pub mod models;
pub mod pricing;
pub mod scanner;
pub mod session;
pub mod settings;
pub mod source_settings;
pub mod sources;
pub mod state;
pub mod storage;
pub mod utils;

use std::fs;
use std::path::Path;

use state::AppState;
use tauri::Manager;

const LEGACY_RUNTIME_LOG_DIR_NAME: &str = "logs";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let storage_paths = storage::resolve_storage_paths(app.handle())?;
            cleanup_legacy_runtime_log_dir(storage_paths.data_dir());
            let pool = db::init_db(&storage_paths)?;
            let app_state = AppState::new(pool);
            scanner::scheduler::start_auto_scan_worker(app.handle().clone(), app_state.clone());

            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_data::app_data_get_overview,
            commands::app_data::app_data_get_item_detail,
            commands::app_data::app_data_run_action,
            commands::messages::messages_ensure_session_index,
            commands::messages::messages_list,
            commands::model_catalog::list_models,
            commands::model_catalog::refresh_model_catalog,
            commands::model_catalog::get_model_sync_status,
            commands::scan_records::scan_records_list,
            commands::settings::settings_get,
            commands::settings::settings_update,
            commands::settings::settings_reset,
            commands::settings::settings_auto_scan_status,
            commands::settings::settings_scheduler_preview,
            commands::sessions::sessions_list,
            commands::statistics::statistics_get,
            commands::scan::start_scan,
            commands::storage::get_storage_config,
            commands::storage::set_storage_data_dir,
            commands::sources::sources_list,
            commands::sources::sources_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn cleanup_legacy_runtime_log_dir(data_dir: &Path) {
    let path = data_dir.join(LEGACY_RUNTIME_LOG_DIR_NAME);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return;
    };

    if metadata.file_type().is_symlink() {
        log::warn!(
            "skipping legacy runtime log cleanup for symlink: {}",
            path.display()
        );
        return;
    }

    if metadata.is_dir() {
        if let Err(error) = fs::remove_dir_all(&path) {
            log::warn!(
                "failed to remove legacy runtime log directory {}: {error}",
                path.display()
            );
        }
    }
}
