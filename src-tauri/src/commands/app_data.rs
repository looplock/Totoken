use tauri::{AppHandle, Manager};

use super::{ensure_storage_runtime_current, log_result, run_blocking};
use crate::app_data;
use crate::error::AppResult;
use crate::models::{
    AppDataActionOutcomeView, AppDataItemDetailView, AppDataMaintenanceAction, AppDataOverviewView,
};
use crate::state::AppState;
use std::collections::BTreeMap;

#[tauri::command]
pub async fn app_data_get_overview(app: AppHandle) -> AppResult<AppDataOverviewView> {
    let app_clone = app.clone();
    let restart_required = app.state::<AppState>().storage_restart_required();
    let result = run_blocking(move || app_data::get_overview(&app_clone, restart_required)).await;
    log_result(
        app.state::<AppState>().inner(),
        "app_data",
        "get_overview",
        result,
        BTreeMap::new(),
    )
}

#[tauri::command]
pub async fn app_data_get_item_detail(
    app: AppHandle,
    relative_path: Option<String>,
) -> AppResult<AppDataItemDetailView> {
    let app_clone = app.clone();
    let mut context = BTreeMap::new();
    if let Some(relative_path) = relative_path.clone() {
        context.insert("relativePath".to_string(), relative_path);
    }
    let result = run_blocking(move || app_data::get_item_detail(&app_clone, relative_path)).await;
    log_result(
        app.state::<AppState>().inner(),
        "app_data",
        "get_item_detail",
        result,
        context,
    )
}

#[tauri::command]
pub async fn app_data_run_action(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    action: AppDataMaintenanceAction,
) -> AppResult<AppDataActionOutcomeView> {
    ensure_storage_runtime_current(&state)?;
    let action_name = format!("{action:?}");
    let app_clone = app.clone();
    let state_clone = state.inner().clone();

    let result = run_blocking(move || app_data::run_action(&app_clone, &state_clone, action)).await;
    let mut context = BTreeMap::new();
    context.insert("action".to_string(), action_name);
    log_result(state.inner(), "app_data", "run_action", result, context)
}
