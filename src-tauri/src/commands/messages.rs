use tauri::State;

use super::{ensure_storage_runtime_current, log_result, run_blocking};
use crate::db::repo::Repository;
use crate::error::AppResult;
use crate::models::{MessageListQuery, MessageListResponse};
use crate::settings;
use crate::state::AppState;
use std::collections::BTreeMap;

#[tauri::command]
pub async fn messages_list(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    query: Option<MessageListQuery>,
) -> AppResult<MessageListResponse> {
    ensure_storage_runtime_current(&state)?;
    let pool = state.db_pool();
    let cost_estimation_policy = settings::get_cost_estimation_policy(&app)?;
    let result = run_blocking(move || {
        let repo = Repository::with_cost_estimation_policy(pool, cost_estimation_policy);
        repo.messages_list(query)
    })
    .await;
    log_result(&state, "messages", "list", result, BTreeMap::new())
}

#[tauri::command]
pub async fn messages_ensure_session_index(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> AppResult<bool> {
    ensure_storage_runtime_current(&state)?;
    let scanner = state.scanner();
    let cost_estimation_policy = settings::get_cost_estimation_policy(&app)?;
    let mut context = BTreeMap::new();
    context.insert("sessionId".to_string(), session_id.clone());
    let result = run_blocking(move || {
        scanner.ensure_session_message_index(session_id.trim(), cost_estimation_policy)
    })
    .await;
    log_result(&state, "messages", "ensure_session_index", result, context)
}
