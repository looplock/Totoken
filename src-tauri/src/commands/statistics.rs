use tauri::State;

use super::{ensure_storage_runtime_current, log_result, run_blocking};
use crate::db::repo::Repository;
use crate::error::AppResult;
use crate::models::{StatisticsOverview, StatisticsQuery};
use crate::state::AppState;
use std::collections::BTreeMap;

#[tauri::command]
pub async fn statistics_get(
    state: State<'_, AppState>,
    query: Option<StatisticsQuery>,
) -> AppResult<StatisticsOverview> {
    ensure_storage_runtime_current(&state)?;
    let pool = state.db_pool();
    let result = run_blocking(move || {
        let repo = Repository::new(pool);
        repo.statistics_get(query)
    })
    .await;
    log_result(&state, "statistics", "get", result, BTreeMap::new())
}
