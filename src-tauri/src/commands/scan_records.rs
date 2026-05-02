use tauri::State;

use super::{ensure_storage_runtime_current, log_result, run_blocking};
use crate::db::repo::Repository;
use crate::error::AppResult;
use crate::models::{ScanRecordsListQuery, ScanRecordsListResponse};
use crate::state::AppState;
use std::collections::BTreeMap;

#[tauri::command]
pub async fn scan_records_list(
    state: State<'_, AppState>,
    query: Option<ScanRecordsListQuery>,
) -> AppResult<ScanRecordsListResponse> {
    ensure_storage_runtime_current(&state)?;
    let pool = state.db_pool();
    let result = run_blocking(move || {
        let repo = Repository::new(pool);
        repo.scan_records_list(query)
    })
    .await;
    log_result(&state, "scan_records", "list", result, BTreeMap::new())
}
