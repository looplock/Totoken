use tauri::{AppHandle, Manager};

use super::{ensure_storage_runtime_current, log_result, run_blocking};
use crate::error::AppResult;
use crate::pricing::backfill_missing_estimated_costs;
use crate::scanner::scheduler::SchedulerPreviewView;
use crate::settings::{self, SchedulerSettings, SettingsState};
use crate::state::{AppState, AutoScanStatusView};
use std::collections::BTreeMap;

#[tauri::command]
pub async fn settings_get(app: AppHandle) -> AppResult<SettingsState> {
    let app_for_get = app.clone();
    let result = run_blocking(move || settings::get_settings(&app_for_get)).await;
    log_result(
        app.state::<AppState>().inner(),
        "settings",
        "get",
        result,
        BTreeMap::new(),
    )
}

#[tauri::command]
pub async fn settings_update(app: AppHandle, settings: SettingsState) -> AppResult<SettingsState> {
    ensure_storage_runtime_current(app.state::<AppState>().inner())?;
    let previous_unknown_model_fallback_enabled = settings::get_settings(&app)
        .map(|current| {
            current
                .cost_estimation
                .bill_unknown_models_with_default_pricing
        })
        .unwrap_or(false);
    let app_for_save = app.clone();
    let result = run_blocking(move || settings::update_settings(&app_for_save, settings)).await;
    match log_result(
        app.state::<AppState>().inner(),
        "settings",
        "update",
        result,
        BTreeMap::new(),
    ) {
        Ok(saved) => {
            if !previous_unknown_model_fallback_enabled
                && saved
                    .cost_estimation
                    .bill_unknown_models_with_default_pricing
            {
                let pool = app.state::<AppState>().db_pool();
                let cost_estimation_policy = settings::cost_estimation_policy(&saved);
                if let Err(error) = run_blocking(move || {
                    let mut conn = pool.get()?;
                    backfill_missing_estimated_costs(&mut conn, cost_estimation_policy)
                })
                .await
                {
                    log::warn!("failed to backfill unknown-model fallback costs after settings update: {error}");
                }
            }
            app.state::<crate::state::AppState>()
                .notify_auto_scan_settings_changed();
            Ok(saved)
        }
        Err(error) => Err(error),
    }
}

#[tauri::command]
pub async fn settings_reset(app: AppHandle) -> AppResult<SettingsState> {
    ensure_storage_runtime_current(app.state::<AppState>().inner())?;
    let app_for_reset = app.clone();
    let result = run_blocking(move || settings::reset_settings(&app_for_reset)).await;
    match log_result(
        app.state::<AppState>().inner(),
        "settings",
        "reset",
        result,
        BTreeMap::new(),
    ) {
        Ok(defaults) => {
            app.state::<crate::state::AppState>()
                .notify_auto_scan_settings_changed();
            Ok(defaults)
        }
        Err(error) => Err(error),
    }
}

#[tauri::command]
pub async fn settings_auto_scan_status(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> AppResult<AutoScanStatusView> {
    ensure_storage_runtime_current(&state)?;
    let app_state = state.inner().clone();
    let state_for_task = app_state.clone();
    let result = run_blocking(move || settings::get_auto_scan_status(&app, &state_for_task)).await;
    log_result(
        &app_state,
        "settings",
        "auto_scan_status",
        result,
        BTreeMap::new(),
    )
}

#[tauri::command]
pub async fn settings_scheduler_preview(
    state: tauri::State<'_, AppState>,
    scheduler: SchedulerSettings,
) -> AppResult<SchedulerPreviewView> {
    ensure_storage_runtime_current(&state)?;
    let app_state = state.inner().clone();
    let state_for_task = app_state.clone();
    let result =
        run_blocking(move || settings::preview_scheduler(&state_for_task, scheduler)).await;
    log_result(
        &app_state,
        "settings",
        "scheduler_preview",
        result,
        BTreeMap::new(),
    )
}
