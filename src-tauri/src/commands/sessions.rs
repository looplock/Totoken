use tauri::State;

use super::{ensure_storage_runtime_current, log_result, run_blocking};
use crate::db::repo::Repository;
use crate::error::AppResult;
use crate::models::{SessionListQuery, SessionListResponse};
use crate::state::AppState;
use std::collections::BTreeMap;

#[tauri::command]
pub async fn sessions_list(
    state: State<'_, AppState>,
    query: Option<SessionListQuery>,
) -> AppResult<SessionListResponse> {
    ensure_storage_runtime_current(&state)?;
    let pool = state.db_pool();
    let result = run_blocking(move || {
        let repo = Repository::new(pool);
        repo.sessions_list(query)
    })
    .await;
    log_result(&state, "sessions", "list", result, BTreeMap::new())
}
