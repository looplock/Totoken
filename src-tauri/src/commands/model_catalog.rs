use tauri::State;

use crate::error::AppResult;
use crate::model_catalog::{
    get_model_sync_status as load_model_sync_status, list_models as load_models,
    refresh_model_catalog as run_model_catalog_refresh, ModelCatalogListQuery,
    ModelCatalogListResponse, ModelCatalogSyncRunView, ModelCatalogSyncStatusView,
};
use crate::settings;
use crate::state::AppState;
use std::collections::BTreeMap;

use super::{ensure_storage_runtime_current, log_command_error, log_result, run_blocking};

#[tauri::command]
pub async fn list_models(
    state: State<'_, AppState>,
    query: Option<ModelCatalogListQuery>,
) -> AppResult<ModelCatalogListResponse> {
    ensure_storage_runtime_current(&state)?;
    let pool = state.db_pool();
    let result = run_blocking(move || load_models(pool, query)).await;
    log_result(&state, "model_catalog", "list", result, BTreeMap::new())
}

#[tauri::command]
pub async fn refresh_model_catalog(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ModelCatalogSyncRunView> {
    ensure_storage_runtime_current(&state)?;
    let cost_estimation_policy = settings::get_cost_estimation_policy(&app)?;
    match run_model_catalog_refresh(state.db_pool(), cost_estimation_policy).await {
        Ok(result) => Ok(result),
        Err(error) => {
            log_command_error(&state, "model_catalog", "refresh", &error, BTreeMap::new());
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn get_model_sync_status(
    state: State<'_, AppState>,
) -> AppResult<ModelCatalogSyncStatusView> {
    ensure_storage_runtime_current(&state)?;
    let pool = state.db_pool();
    let result = run_blocking(move || load_model_sync_status(pool)).await;
    log_result(
        &state,
        "model_catalog",
        "get_sync_status",
        result,
        BTreeMap::new(),
    )
}
