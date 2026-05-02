use tauri::{AppHandle, Manager, State};

use super::{ensure_storage_runtime_current, log_result, run_blocking};
use crate::error::AppResult;
use crate::source_settings::{self, SourceSettingsPatch, SourceSettingsStateView};
use crate::state::AppState;
use std::collections::BTreeMap;

#[tauri::command]
pub async fn sources_list(app: AppHandle) -> AppResult<SourceSettingsStateView> {
    let app_for_list = app.clone();
    let state = app.state::<AppState>();
    let store = state.source_settings_store();
    let result =
        run_blocking(move || source_settings::list_source_settings(&app_for_list, &store)).await;
    log_result(
        app.state::<AppState>().inner(),
        "sources",
        "list",
        result,
        BTreeMap::new(),
    )
}

#[tauri::command]
pub async fn sources_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    patch: SourceSettingsPatch,
) -> AppResult<SourceSettingsStateView> {
    ensure_storage_runtime_current(&state)?;
    let app_for_update = app.clone();
    let store = state.source_settings_store();
    let id_for_update = id.clone();
    let result = run_blocking(move || {
        source_settings::update_source_setting(&app_for_update, &store, &id_for_update, patch)
    })
    .await;
    let mut context = BTreeMap::new();
    context.insert("id".to_string(), id);

    match log_result(
        app.state::<AppState>().inner(),
        "sources",
        "update",
        result,
        context,
    ) {
        Ok(saved) => {
            app.state::<crate::state::AppState>()
                .notify_auto_scan_refresh();
            Ok(saved)
        }
        Err(error) => Err(error),
    }
}
