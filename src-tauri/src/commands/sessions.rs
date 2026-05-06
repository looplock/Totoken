use tauri::State;

use super::{ensure_storage_runtime_current, log_result, run_blocking};
use crate::db::repo::Repository;
use crate::error::AppResult;
use crate::models::{SessionListQuery, SessionListResponse};
use crate::settings;
use crate::state::AppState;
use std::collections::BTreeMap;

#[tauri::command]
pub async fn sessions_list(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    query: Option<SessionListQuery>,
) -> AppResult<SessionListResponse> {
    ensure_storage_runtime_current(&state)?;
    let pool = state.db_pool();
    let cost_estimation_policy = settings::get_cost_estimation_policy(&app)?;
    let result = run_blocking(move || {
        let repo = Repository::with_cost_estimation_policy(pool, cost_estimation_policy);
        repo.sessions_list(query)
    })
    .await;
    log_result(&state, "sessions", "list", result, BTreeMap::new())
}
