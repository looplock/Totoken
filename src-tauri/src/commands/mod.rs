pub mod app_data;
pub mod messages;
pub mod model_catalog;
pub mod scan;
pub mod scan_records;
pub mod sessions;
pub mod settings;
pub mod sources;
pub mod statistics;
pub mod storage;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use std::collections::BTreeMap;

pub(crate) fn log_command_error(
    state: &AppState,
    source: &str,
    action: &str,
    error: &AppError,
    context: BTreeMap<String, String>,
) {
    state.log_error(
        "command",
        source,
        action,
        format!("{source}.{action} failed"),
        Some(error.to_string()),
        context,
    );
}

pub(crate) fn log_result<T>(
    state: &AppState,
    source: &str,
    action: &str,
    result: AppResult<T>,
    context: BTreeMap<String, String>,
) -> AppResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            log_command_error(state, source, action, &error, context);
            Err(error)
        }
    }
}

pub(crate) fn ensure_storage_runtime_current(state: &AppState) -> AppResult<()> {
    if state.storage_restart_required() {
        return Err(AppError::validation(
            "storage directory changed; restart the app before using database-backed features",
        ));
    }
    Ok(())
}

pub use app_data::{app_data_get_item_detail, app_data_get_overview, app_data_run_action};
pub use messages::{messages_ensure_session_index, messages_list};
pub use model_catalog::{get_model_sync_status, list_models, refresh_model_catalog};
pub use scan::start_scan;
pub use scan_records::scan_records_list;
pub use sessions::sessions_list;
pub use settings::{
    settings_auto_scan_status, settings_get, settings_reset, settings_scheduler_preview,
    settings_update,
};
pub use sources::{sources_list, sources_update};
pub use statistics::statistics_get;
pub use storage::{get_storage_config, set_storage_data_dir};

async fn run_blocking<T, F>(task: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| AppError::internal(format!("blocking task join failure: {error}")))?
}
