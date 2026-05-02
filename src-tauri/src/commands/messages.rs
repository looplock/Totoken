use tauri::State;

use super::{ensure_storage_runtime_current, log_result, run_blocking};
use crate::db::repo::Repository;
use crate::error::AppResult;
use crate::models::{MessageListQuery, MessageListResponse};
use crate::state::AppState;
use std::collections::BTreeMap;

#[tauri::command]
pub async fn messages_list(
    state: State<'_, AppState>,
    query: Option<MessageListQuery>,
) -> AppResult<MessageListResponse> {
    ensure_storage_runtime_current(&state)?;
    let pool = state.db_pool();
    let result = run_blocking(move || {
        let repo = Repository::new(pool);
        repo.messages_list(query)
    })
    .await;
    log_result(&state, "messages", "list", result, BTreeMap::new())
}

#[tauri::command]
pub async fn messages_ensure_session_index(
    state: State<'_, AppState>,
    session_id: String,
) -> AppResult<bool> {
    ensure_storage_runtime_current(&state)?;
    let scanner = state.scanner();
    let mut context = BTreeMap::new();
    context.insert("sessionId".to_string(), session_id.clone());
    let result =
        run_blocking(move || scanner.ensure_session_message_index(session_id.trim())).await;
    log_result(&state, "messages", "ensure_session_index", result, context)
}
