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
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use tauri::WindowEvent;

const LEGACY_RUNTIME_LOG_DIR_NAME: &str = "logs";
const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_MENU_SHOW_ID: &str = "show";
const TRAY_MENU_QUIT_ID: &str = "quit";

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
            setup_system_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != MAIN_WINDOW_LABEL {
                return;
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                if settings::should_hide_to_tray_on_close(window.app_handle()) {
                    api.prevent_close();
                    if let Err(error) = window.hide() {
                        log::warn!("failed to hide main window to system tray: {error}");
                    }
                }
            }
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

fn setup_system_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text(TRAY_MENU_SHOW_ID, "Show Totoken")
        .separator()
        .text(TRAY_MENU_QUIT_ID, "Quit")
        .build()?;

    let mut builder = TrayIconBuilder::with_id(settings::TRAY_ICON_ID)
        .menu(&menu)
        .tooltip("Totoken")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id();
            if id == TRAY_MENU_SHOW_ID {
                show_main_window(app);
            } else if id == TRAY_MENU_QUIT_ID {
                app.exit(0);
            }
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => show_main_window(tray.app_handle()),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    settings::sync_tray_visibility(app.handle());
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };

    if let Err(error) = window.show() {
        log::warn!("failed to show main window from system tray: {error}");
    }
    if let Err(error) = window.unminimize() {
        log::warn!("failed to unminimize main window from system tray: {error}");
    }
    if let Err(error) = window.set_focus() {
        log::warn!("failed to focus main window from system tray: {error}");
    }
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
