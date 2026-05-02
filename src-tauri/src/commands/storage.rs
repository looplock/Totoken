use tauri::AppHandle;

use crate::error::AppResult;
use crate::storage::{self, StorageConfigView};
use std::collections::BTreeMap;
use tauri::Manager;

use super::{log_command_error, log_result, run_blocking};

#[tauri::command]
pub async fn get_storage_config(app: AppHandle) -> AppResult<StorageConfigView> {
    let app_for_get = app.clone();
    let restart_required = app
        .state::<crate::state::AppState>()
        .storage_restart_required();
    let result = run_blocking(move || {
        let paths = storage::resolve_storage_paths(&app_for_get)?;
        Ok(paths.to_view(restart_required))
    })
    .await;
    log_result(
        app.state::<crate::state::AppState>().inner(),
        "storage",
        "get_config",
        result,
        BTreeMap::new(),
    )
}

#[tauri::command]
pub async fn set_storage_data_dir(
    app: AppHandle,
    data_dir: Option<String>,
) -> AppResult<StorageConfigView> {
    let app_for_set = app.clone();
    let data_dir_for_set = data_dir.clone();
    match run_blocking(move || storage::set_data_dir(&app_for_set, data_dir_for_set)).await {
        Ok(result) => {
            if result.restart_required {
                app.state::<crate::state::AppState>()
                    .mark_storage_restart_required();
            }
            Ok(result)
        }
        Err(error) => {
            let mut context = BTreeMap::new();
            if let Some(data_dir) = data_dir {
                context.insert("dataDir".to_string(), data_dir);
            }
            log_command_error(
                app.state::<crate::state::AppState>().inner(),
                "storage",
                "set_data_dir",
                &error,
                context,
            );
            Err(error)
        }
    }
}
