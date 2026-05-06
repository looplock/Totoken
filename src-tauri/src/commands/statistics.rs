use tauri::State;

use super::{ensure_storage_runtime_current, log_result, run_blocking};
use crate::db::repo::Repository;
use crate::error::AppResult;
use crate::models::{StatisticsOverview, StatisticsQuery};
use crate::settings;
use crate::state::AppState;
use std::collections::BTreeMap;

#[tauri::command]
pub async fn statistics_get(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    query: Option<StatisticsQuery>,
) -> AppResult<StatisticsOverview> {
    ensure_storage_runtime_current(&state)?;
    let pool = state.db_pool();
    let cost_estimation_policy = settings::get_cost_estimation_policy(&app)?;
    let result = run_blocking(move || {
        let repo = Repository::with_cost_estimation_policy(pool, cost_estimation_policy);
        repo.statistics_get(query)
    })
    .await;
    log_result(&state, "statistics", "get", result, BTreeMap::new())
}
