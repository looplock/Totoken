use chrono::{DateTime, Utc};
use rusqlite::{params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::message_stream::{MessageStreamAggregator, MessageStreamItem, MessageTokenUsage};
use super::{NormalizedRequest, NormalizedSession, NormalizedUsageEvent, SourceAdapter};
use crate::error::AppResult;
use crate::utils::sqlite::SqliteSnapshot;
use crate::utils::{hash, time};

pub struct CursorAdapter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CursorStateDbKind {
    Global,
    Workspace,
}

#[derive(Debug, Serialize, Deserialize)]
struct CursorLocator {
    composer_id: String,
    bubble_id: String,
}

#[derive(Debug, Clone, Default)]
struct CursorComposerHeader {
    title: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
struct CursorBubbleHeader {
    bubble_id: String,
    bubble_type: i64,
    capability_type: Option<i64>,
    is_renderable: bool,
}

#[derive(Debug, Clone)]
struct CursorParsedTurn {
    source_message_id: String,
    role: String,
    message_type: String,
    character_count: Option<i64>,
    model: Option<String>,
    source_created_at: Option<DateTime<Utc>>,
    source_updated_at: Option<DateTime<Utc>>,
    source_locator: String,
    high_input_tokens: i64,
    high_output_tokens: i64,
    estimated_input_tokens: Option<i64>,
    estimated_output_tokens: Option<i64>,
    estimated_tool_result_tokens: Option<i64>,
    usage_event_id: Option<String>,
}

impl SourceAdapter for CursorAdapter {
    fn name(&self) -> &str {
        "cursor"
    }

    fn parser_version(&self) -> i64 {
        5
    }

    fn can_handle(&self, path: &Path) -> bool {
        cursor_state_db_kind(path).is_some()
    }

    fn discover_paths(&self, root_path: &Path) -> AppResult<Vec<PathBuf>> {
        if root_path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("state.vscdb"))
        {
            return Ok(self
                .can_handle(root_path)
                .then(|| root_path.to_path_buf())
                .into_iter()
                .collect());
        }

        let mut paths = Vec::new();
        self.push_candidate_path(&mut paths, root_path.join("state.vscdb"));
        if !root_path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("globalStorage"))
        {
            self.push_candidate_path(
                &mut paths,
                root_path.join("globalStorage").join("state.vscdb"),
            );
        }

        let workspace_storage = if root_path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("workspaceStorage"))
        {
            root_path.to_path_buf()
        } else {
            root_path.join("workspaceStorage")
        };
        self.discover_workspace_state_dbs(&workspace_storage, &mut paths)?;

        Ok(paths)
    }

    fn parse(&self, path: &Path) -> AppResult<Vec<NormalizedSession>> {
        let snapshot = SqliteSnapshot::open(path, self.name())?;
        let conn = snapshot.connection();

        match cursor_state_db_kind(path).unwrap_or(CursorStateDbKind::Global) {
            CursorStateDbKind::Global => parse_global_state_db(conn, self.name()),
            CursorStateDbKind::Workspace => parse_workspace_state_db(conn, self.name()),
        }
    }

    fn fingerprint_paths(&self, path: &Path) -> Vec<PathBuf> {
        let mut paths = vec![path.to_path_buf()];
        let companion = PathBuf::from(format!("{}{}", path.to_string_lossy(), "-wal"));
        if companion
            .metadata()
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false)
        {
            paths.push(companion);
        }
        paths
    }
}

impl CursorAdapter {
    fn push_candidate_path(&self, paths: &mut Vec<PathBuf>, candidate: PathBuf) {
        if candidate.is_file()
            && self.can_handle(&candidate)
            && !paths.iter().any(|path| path == &candidate)
        {
            paths.push(candidate);
        }
    }

    fn discover_workspace_state_dbs(
        &self,
        workspace_storage: &Path,
        paths: &mut Vec<PathBuf>,
    ) -> AppResult<()> {
        if !workspace_storage.is_dir() {
            return Ok(());
        }

        for entry in fs::read_dir(workspace_storage)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }

            self.push_candidate_path(paths, entry.path().join("state.vscdb"));
        }

        Ok(())
    }
}

fn parse_global_state_db(conn: &Connection, source_app: &str) -> AppResult<Vec<NormalizedSession>> {
    let headers_by_id = load_composer_headers(conn)?;
    let composer_rows = load_composer_rows(conn)?;
    let bubble_keys = collect_referenced_bubble_keys(&composer_rows);
    let bubbles_by_key = load_bubbles(conn, &bubble_keys)?;

    Ok(build_sessions_from_composer_rows(
        source_app,
        &headers_by_id,
        &composer_rows,
        &bubbles_by_key,
        false,
    ))
}

fn parse_workspace_state_db(
    conn: &Connection,
    source_app: &str,
) -> AppResult<Vec<NormalizedSession>> {
    let mut headers_by_id = load_composer_headers(conn)?;
    headers_by_id.extend(load_workspace_composer_headers(conn)?);
    let composer_rows = load_composer_rows(conn)?;
    let bubble_keys = collect_referenced_bubble_keys(&composer_rows);
    let bubbles_by_key = load_bubbles(conn, &bubble_keys)?;

    Ok(build_sessions_from_composer_rows(
        source_app,
        &headers_by_id,
        &composer_rows,
        &bubbles_by_key,
        true,
    ))
}

fn build_sessions_from_composer_rows(
    source_app: &str,
    headers_by_id: &HashMap<String, CursorComposerHeader>,
    composer_rows: &HashMap<String, Value>,
    bubbles_by_key: &HashMap<(String, String), Value>,
    include_metadata_only: bool,
) -> Vec<NormalizedSession> {
    let mut composer_ids = headers_by_id.keys().cloned().collect::<Vec<_>>();
    for (composer_id, value) in composer_rows {
        if !composer_ids.iter().any(|existing| existing == composer_id)
            && !extract_conversation_headers(value).is_empty()
        {
            composer_ids.push(composer_id.clone());
        }
    }

    let mut sessions = Vec::new();
    for composer_id in composer_ids {
        let header = headers_by_id.get(&composer_id).cloned().unwrap_or_default();
        if let Some(payload) = composer_rows.get(&composer_id) {
            if let Some(session) =
                build_normalized_session(source_app, &composer_id, &header, payload, bubbles_by_key)
            {
                sessions.push(session);
                continue;
            }
        }

        if include_metadata_only {
            if let Some(session) =
                build_workspace_metadata_session(source_app, &composer_id, &header)
            {
                sessions.push(session);
            }
        }
    }

    sessions
}

fn load_composer_headers(conn: &Connection) -> AppResult<HashMap<String, CursorComposerHeader>> {
    let Some(raw_value) = load_item_table_value(conn, "composer.composerHeaders")? else {
        return Ok(HashMap::new());
    };

    let payload: Value = serde_json::from_str(&raw_value)?;
    Ok(extract_composer_headers(&payload))
}

fn load_workspace_composer_headers(
    conn: &Connection,
) -> AppResult<HashMap<String, CursorComposerHeader>> {
    let Some(raw_value) = load_item_table_value(conn, "composer.composerData")? else {
        return Ok(HashMap::new());
    };

    let payload: Value = serde_json::from_str(&raw_value)?;
    Ok(extract_composer_headers(&payload))
}

fn load_item_table_value(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    if !table_exists(conn, "ItemTable")? {
        return Ok(None);
    }

    let raw_value: Option<String> = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ? LIMIT 1",
            [key],
            |row| row.get(0),
        )
        .optional()?;

    Ok(raw_value)
}

fn extract_composer_headers(payload: &Value) -> HashMap<String, CursorComposerHeader> {
    let mut headers = HashMap::new();
    let all_composers = payload
        .get("allComposers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for composer in all_composers {
        let Some(composer_id) = composer
            .get("composerId")
            .and_then(Value::as_str)
            .and_then(|value| normalize_optional_text(Some(value)))
        else {
            continue;
        };

        let title = composer
            .get("name")
            .and_then(Value::as_str)
            .and_then(select_title_candidate)
            .or_else(|| {
                composer
                    .get("subtitle")
                    .and_then(Value::as_str)
                    .and_then(select_title_candidate)
            });
        let created_at = composer
            .get("createdAt")
            .and_then(Value::as_i64)
            .and_then(time::from_unix_ms);
        let updated_at = newer_timestamp(
            composer
                .get("lastUpdatedAt")
                .and_then(Value::as_i64)
                .and_then(time::from_unix_ms),
            composer
                .get("conversationCheckpointLastUpdatedAt")
                .and_then(Value::as_i64)
                .and_then(time::from_unix_ms),
        );

        headers.insert(
            composer_id.clone(),
            CursorComposerHeader {
                title,
                created_at,
                updated_at,
            },
        );
    }

    headers
}

fn load_composer_rows(conn: &Connection) -> AppResult<HashMap<String, Value>> {
    if !table_exists(conn, "cursorDiskKV")? {
        return Ok(HashMap::new());
    }

    let mut stmt =
        conn.prepare("SELECT key, value FROM cursorDiskKV WHERE key LIKE 'composerData:%'")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;

    let mut grouped = HashMap::new();
    for row in rows {
        let (key, value) = row?;
        let Some(value) = value else {
            continue;
        };
        let Some(composer_id) = key.strip_prefix("composerData:") else {
            continue;
        };
        let Ok(payload) = serde_json::from_str::<Value>(&value) else {
            continue;
        };
        grouped.insert(composer_id.to_string(), payload);
    }

    Ok(grouped)
}

fn collect_referenced_bubble_keys(composer_rows: &HashMap<String, Value>) -> HashSet<String> {
    let mut keys = HashSet::new();
    for (composer_id, payload) in composer_rows {
        for bubble_header in extract_conversation_headers(payload) {
            keys.insert(format!(
                "bubbleId:{composer_id}:{}",
                bubble_header.bubble_id
            ));
        }
    }
    keys
}

fn load_bubbles(
    conn: &Connection,
    bubble_keys: &HashSet<String>,
) -> AppResult<HashMap<(String, String), Value>> {
    if !table_exists(conn, "cursorDiskKV")? {
        return Ok(HashMap::new());
    }
    if bubble_keys.is_empty() {
        return Ok(HashMap::new());
    }

    let mut grouped = HashMap::new();
    let bubble_keys = bubble_keys.iter().collect::<Vec<_>>();
    for chunk in bubble_keys.chunks(500) {
        let placeholders = (0..chunk.len()).map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = format!("SELECT key, value FROM cursorDiskKV WHERE key IN ({placeholders})");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params_from_iter(chunk.iter().map(|value| value.as_str())),
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )?;

        for row in rows {
            let (key, value) = row?;
            let Some(value) = value else {
                continue;
            };
            let Some(rest) = key.strip_prefix("bubbleId:") else {
                continue;
            };
            let Some((composer_id, bubble_id)) = rest.split_once(':') else {
                continue;
            };
            let Ok(payload) = serde_json::from_str::<Value>(&value) else {
                continue;
            };
            grouped.insert((composer_id.to_string(), bubble_id.to_string()), payload);
        }
    }

    Ok(grouped)
}

fn table_exists(conn: &Connection, table_name: &str) -> AppResult<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
            [table_name],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn build_normalized_session(
    source_app: &str,
    composer_id: &str,
    header: &CursorComposerHeader,
    payload: &Value,
    bubbles_by_key: &HashMap<(String, String), Value>,
) -> Option<NormalizedSession> {
    let conversation_headers = extract_conversation_headers(payload);
    if conversation_headers.is_empty() && header.title.is_none() {
        return None;
    }

    let session_model = extract_model_name(payload.get("modelConfig"));
    let session_status = map_cursor_status(payload.get("status").and_then(Value::as_str));
    let payload_created_at = payload
        .get("createdAt")
        .and_then(Value::as_i64)
        .and_then(time::from_unix_ms);
    let payload_updated_at = payload
        .get("lastUpdatedAt")
        .and_then(Value::as_i64)
        .and_then(time::from_unix_ms);
    let source_created_at = older_timestamp(header.created_at, payload_created_at);
    let source_updated_at = newer_timestamp(header.updated_at, payload_updated_at);
    let mut title = header.title.clone().or_else(|| {
        payload
            .get("name")
            .and_then(Value::as_str)
            .and_then(select_title_candidate)
    });

    let mut checksum_parts = vec![
        "cursor-estimates-as-session-totals-v1".to_string(),
        composer_id.to_string(),
        session_model.clone().unwrap_or_default(),
        title.clone().unwrap_or_default(),
        payload
            .get("_v")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .to_string(),
    ];

    let mut parsed_messages = Vec::new();
    for bubble_header in conversation_headers {
        if !bubble_header.is_renderable {
            continue;
        }

        let Some(bubble_payload) =
            bubbles_by_key.get(&(composer_id.to_string(), bubble_header.bubble_id.clone()))
        else {
            continue;
        };

        let message = parse_bubble(
            composer_id,
            &bubble_header,
            bubble_payload,
            session_model.clone(),
        );

        if title.is_none() && message.role == "user" {
            title = bubble_payload
                .get("text")
                .and_then(Value::as_str)
                .and_then(select_title_candidate);
        }

        checksum_parts.push(message.source_message_id.clone());
        checksum_parts.push(message.role.clone());
        checksum_parts.push(message.message_type.clone());
        checksum_parts.push(message.character_count.unwrap_or(0).to_string());
        checksum_parts.push(message.high_input_tokens.to_string());
        checksum_parts.push(message.high_output_tokens.to_string());
        checksum_parts.push(message.estimated_input_tokens.unwrap_or(0).to_string());
        checksum_parts.push(message.estimated_output_tokens.unwrap_or(0).to_string());
        checksum_parts.push(
            message
                .estimated_tool_result_tokens
                .unwrap_or(0)
                .to_string(),
        );
        checksum_parts.push(
            bubble_payload
                .get("text")
                .and_then(Value::as_str)
                .and_then(|value| normalize_optional_text(Some(value)))
                .unwrap_or_default(),
        );

        parsed_messages.push(message);
    }

    if parsed_messages.is_empty() && title.is_none() {
        return None;
    }

    let message_count = parsed_messages.len() as i64;
    let fallback_event_time = source_updated_at.or(source_created_at);
    let (mut requests, events, total_input_tokens, total_output_tokens) =
        build_requests_messages_and_events(
            parsed_messages,
            fallback_event_time,
            session_model.clone(),
        );

    if let Some(last_request) = requests.last_mut() {
        if let Some(status) = session_status.clone() {
            last_request.status = Some(status);
        }
    }

    let model_first = session_model.clone();
    let model_last = requests
        .iter()
        .filter_map(|request| request.model.clone())
        .next_back()
        .or(session_model);
    let conversation_checksum = hash::sha256_text(&checksum_parts.join("\n"));

    Some(NormalizedSession {
        source_app: source_app.to_string(),
        external_session_id: Some(composer_id.to_string()),
        session_key: format!("cursor:{composer_id}"),
        title,
        model_first,
        model_last,
        source_created_at,
        source_updated_at,
        total_input_tokens,
        total_output_tokens,
        message_count,
        conversation_checksum,
        requests,
        events,
    })
}

fn build_workspace_metadata_session(
    source_app: &str,
    composer_id: &str,
    header: &CursorComposerHeader,
) -> Option<NormalizedSession> {
    header.title.as_ref()?;

    let checksum_parts = [
        "workspace-metadata",
        composer_id,
        header.title.as_deref().unwrap_or_default(),
        &header
            .created_at
            .map(|value| value.timestamp_millis().to_string())
            .unwrap_or_default(),
        &header
            .updated_at
            .map(|value| value.timestamp_millis().to_string())
            .unwrap_or_default(),
    ];

    Some(NormalizedSession {
        source_app: source_app.to_string(),
        external_session_id: Some(composer_id.to_string()),
        session_key: format!("cursor:{composer_id}"),
        title: header.title.clone(),
        model_first: None,
        model_last: None,
        source_created_at: header.created_at,
        source_updated_at: header.updated_at,
        total_input_tokens: 0,
        total_output_tokens: 0,
        message_count: 0,
        conversation_checksum: hash::sha256_text(&checksum_parts.join("\n")),
        requests: Vec::new(),
        events: Vec::new(),
    })
}

fn cursor_state_db_kind(path: &Path) -> Option<CursorStateDbKind> {
    if !path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("state.vscdb"))
    {
        return None;
    }

    let parent_name = path
        .parent()
        .and_then(|value| value.file_name())
        .and_then(|value| value.to_str())?;
    if parent_name.eq_ignore_ascii_case("globalStorage") {
        return Some(CursorStateDbKind::Global);
    }

    let grandparent_name = path
        .parent()
        .and_then(|value| value.parent())
        .and_then(|value| value.file_name())
        .and_then(|value| value.to_str());
    if grandparent_name.is_some_and(|value| value.eq_ignore_ascii_case("workspaceStorage")) {
        return Some(CursorStateDbKind::Workspace);
    }

    None
}

fn build_requests_messages_and_events(
    parsed_messages: Vec<CursorParsedTurn>,
    fallback_event_time: Option<DateTime<Utc>>,
    session_model: Option<String>,
) -> (Vec<NormalizedRequest>, Vec<NormalizedUsageEvent>, i64, i64) {
    let estimates_by_request_id = build_cursor_request_estimates(&parsed_messages);
    let stream_items = parsed_messages
        .iter()
        .map(|message| {
            let has_high_usage = message.high_input_tokens > 0 || message.high_output_tokens > 0;
            MessageStreamItem {
                source_id: message.source_message_id.clone(),
                role: message.role.clone(),
                request_id: None,
                parent_id: None,
                status: None,
                model: message.model.clone().or_else(|| session_model.clone()),
                usage: has_high_usage.then_some(MessageTokenUsage {
                    input_tokens: message.high_input_tokens,
                    output_tokens: message.high_output_tokens,
                    total_tokens: message.high_input_tokens + message.high_output_tokens,
                    cache_read_input_tokens: 0,
                    cache_write_input_tokens: 0,
                }),
                count_as_message: true,
                source_created_at: message.source_created_at,
                source_updated_at: message
                    .source_updated_at
                    .or(message.source_created_at)
                    .or(fallback_event_time),
                usage_event_time_utc: message
                    .source_updated_at
                    .or(message.source_created_at)
                    .or(fallback_event_time),
                source_event_id: message
                    .usage_event_id
                    .clone()
                    .or_else(|| Some(message.source_message_id.clone())),
                usage_event_granularity: None,
                usage_event_confidence: None,
                source_locator: message.source_locator.clone(),
                use_as_request_locator: false,
            }
        })
        .collect();
    let aggregate = MessageStreamAggregator::new(stream_items)
        .aggregate_sequential_user_requests("cursor-request");
    let mut requests = aggregate.requests;
    apply_cursor_estimates(&mut requests, &estimates_by_request_id);
    let (total_input_tokens, total_output_tokens) = cursor_request_token_totals(&requests)
        .unwrap_or((aggregate.total_input_tokens, aggregate.total_output_tokens));

    (
        requests,
        aggregate.events,
        total_input_tokens,
        total_output_tokens,
    )
}

#[derive(Debug, Clone, Copy, Default)]
struct CursorRequestEstimate {
    input_tokens: i64,
    output_tokens: i64,
}

fn build_cursor_request_estimates(
    parsed_messages: &[CursorParsedTurn],
) -> HashMap<String, CursorRequestEstimate> {
    let mut estimates = HashMap::<String, CursorRequestEstimate>::new();
    let mut current_request_id: Option<String> = None;
    let mut generated_sequence_no = 0_i64;
    let mut rolling_context_tokens = 0_i64;
    let mut pending_model_input = false;

    for message in parsed_messages {
        if message.role == "user" || current_request_id.is_none() {
            generated_sequence_no += 1;
            current_request_id = Some(if message.role == "user" {
                message.source_message_id.clone()
            } else {
                format!("cursor-request-{generated_sequence_no}")
            });
        }

        let Some(request_id) = current_request_id.as_ref() else {
            continue;
        };
        let estimate = estimates.entry(request_id.clone()).or_default();
        match message.role.as_str() {
            "user" => {
                rolling_context_tokens += message.estimated_input_tokens.unwrap_or(0);
                pending_model_input = true;
            }
            "assistant" => {
                if pending_model_input && rolling_context_tokens > 0 {
                    estimate.input_tokens += rolling_context_tokens;
                    pending_model_input = false;
                }
                let output_tokens = message.estimated_output_tokens.unwrap_or(0);
                estimate.output_tokens += output_tokens;
                rolling_context_tokens += output_tokens;
            }
            "tool" => {
                let tool_call_tokens = message.estimated_output_tokens.unwrap_or(0);
                let tool_result_tokens = message.estimated_tool_result_tokens.unwrap_or(0);
                estimate.output_tokens += tool_call_tokens;
                rolling_context_tokens += tool_call_tokens + tool_result_tokens;
                if tool_result_tokens > 0 {
                    pending_model_input = true;
                }
            }
            _ => {}
        }
    }

    estimates
}

fn apply_cursor_estimates(
    requests: &mut [NormalizedRequest],
    estimates_by_request_id: &HashMap<String, CursorRequestEstimate>,
) {
    for request in requests {
        if request.token_confidence.as_deref() == Some("high") {
            continue;
        }

        let Some(request_id) = request.source_request_id.as_ref() else {
            continue;
        };
        let Some(estimate) = estimates_by_request_id.get(request_id) else {
            continue;
        };
        if estimate.input_tokens <= 0 && estimate.output_tokens <= 0 {
            continue;
        }

        request.input_tokens = Some(estimate.input_tokens);
        request.output_tokens = Some(estimate.output_tokens);
        request.total_tokens = Some(estimate.input_tokens + estimate.output_tokens);
        request.cache_read_input_tokens = Some(0);
        request.cache_write_input_tokens = Some(0);
    }
}

fn cursor_request_token_totals(requests: &[NormalizedRequest]) -> Option<(i64, i64)> {
    let mut input_tokens = 0_i64;
    let mut output_tokens = 0_i64;

    for request in requests {
        input_tokens += request.input_tokens.unwrap_or(0);
        output_tokens += request.output_tokens.unwrap_or(0);
    }

    (input_tokens > 0 || output_tokens > 0).then_some((input_tokens, output_tokens))
}

fn parse_bubble(
    composer_id: &str,
    bubble_header: &CursorBubbleHeader,
    bubble_payload: &Value,
    session_model: Option<String>,
) -> CursorParsedTurn {
    let text = bubble_payload
        .get("text")
        .and_then(Value::as_str)
        .and_then(|value| normalize_optional_text(Some(value)));
    let capability_type = bubble_payload
        .get("capabilityType")
        .and_then(Value::as_i64)
        .or(bubble_header.capability_type);
    let tool_name = bubble_payload
        .get("toolFormerData")
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .and_then(|value| normalize_optional_text(Some(value)));
    let tool_status = bubble_payload
        .get("toolFormerData")
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .and_then(|value| normalize_optional_text(Some(value)));
    let timing_info = bubble_payload.get("timingInfo").unwrap_or(&Value::Null);
    let source_created_at = timing_info
        .get("clientRpcSendTime")
        .and_then(Value::as_i64)
        .and_then(time::from_unix_ms);
    let source_updated_at = newer_timestamp(
        timing_info
            .get("clientEndTime")
            .and_then(Value::as_i64)
            .and_then(time::from_unix_ms),
        timing_info
            .get("clientSettleTime")
            .and_then(Value::as_i64)
            .and_then(time::from_unix_ms),
    )
    .or(source_created_at);
    let input_tokens = bubble_payload
        .get("tokenCount")
        .and_then(|value| value.get("inputTokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = bubble_payload
        .get("tokenCount")
        .and_then(|value| value.get("outputTokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let high_input_tokens = if input_tokens > 0 { input_tokens } else { 0 };
    let high_output_tokens = if output_tokens > 0 { output_tokens } else { 0 };

    let (
        role,
        message_type,
        estimated_input_tokens,
        estimated_output_tokens,
        estimated_tool_result_tokens,
        character_count,
    ) = if bubble_header.bubble_type == 1 {
        let character_count = text.as_ref().map(|value| value.chars().count() as i64);
        (
            "user".to_string(),
            "message".to_string(),
            text.as_deref().map(estimate_text_tokens),
            None,
            None,
            character_count,
        )
    } else if capability_type == Some(30) {
        let character_count = text.as_ref().map(|value| value.chars().count() as i64);
        (
            "assistant".to_string(),
            "thinking".to_string(),
            None,
            text.as_deref().map(estimate_text_tokens),
            None,
            character_count,
        )
    } else if capability_type == Some(15) || tool_name.is_some() {
        let summary = build_tool_summary(tool_name.as_deref(), tool_status.as_deref());
        let character_count = summary.as_ref().map(|value| value.chars().count() as i64);
        (
            "tool".to_string(),
            "tool".to_string(),
            None,
            estimate_tool_call_tokens(bubble_payload),
            estimate_tool_result_tokens(bubble_payload),
            character_count,
        )
    } else {
        let character_count = text.as_ref().map(|value| value.chars().count() as i64);
        (
            "assistant".to_string(),
            "message".to_string(),
            None,
            text.as_deref().map(estimate_text_tokens),
            None,
            character_count,
        )
    };

    CursorParsedTurn {
        source_message_id: bubble_header.bubble_id.clone(),
        role,
        message_type,
        character_count,
        model: session_model,
        source_created_at,
        source_updated_at,
        source_locator: serialize_locator(composer_id, &bubble_header.bubble_id),
        high_input_tokens,
        high_output_tokens,
        estimated_input_tokens,
        estimated_output_tokens,
        estimated_tool_result_tokens,
        usage_event_id: bubble_payload
            .get("usageUuid")
            .and_then(Value::as_str)
            .and_then(|value| normalize_optional_text(Some(value)))
            .or_else(|| {
                bubble_payload
                    .get("serverBubbleId")
                    .and_then(Value::as_str)
                    .and_then(|value| normalize_optional_text(Some(value)))
            }),
    }
}

fn estimate_tool_call_tokens(bubble_payload: &Value) -> Option<i64> {
    let tool_data = bubble_payload.get("toolFormerData")?;
    max_estimated_tokens([
        estimate_string_value_tokens(tool_data.get("rawArgs")),
        estimate_string_value_tokens(tool_data.get("params")),
    ])
}

fn estimate_tool_result_tokens(bubble_payload: &Value) -> Option<i64> {
    let tool_data = bubble_payload.get("toolFormerData")?;
    max_estimated_tokens([
        estimate_string_value_tokens(tool_data.get("result")),
        estimate_json_value_tokens(tool_data.get("additionalData")),
    ])
}

fn estimate_string_value_tokens(value: Option<&Value>) -> Option<i64> {
    value
        .and_then(Value::as_str)
        .and_then(|value| normalize_optional_text(Some(value)))
        .map(|value| estimate_text_tokens(&value))
}

fn estimate_json_value_tokens(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if value.is_null() {
        return None;
    }

    let serialized = serde_json::to_string(value).ok()?;
    normalize_optional_text(Some(&serialized)).map(|value| estimate_text_tokens(&value))
}

fn max_estimated_tokens(values: impl IntoIterator<Item = Option<i64>>) -> Option<i64> {
    values
        .into_iter()
        .flatten()
        .max()
        .filter(|value| *value > 0)
}

fn extract_conversation_headers(payload: &Value) -> Vec<CursorBubbleHeader> {
    let conversation = payload
        .get("fullConversationHeadersOnly")
        .and_then(Value::as_array)
        .or_else(|| payload.get("conversation").and_then(Value::as_array))
        .cloned()
        .unwrap_or_default();

    conversation
        .into_iter()
        .filter_map(|entry| {
            let bubble_id = entry
                .get("bubbleId")
                .and_then(Value::as_str)
                .and_then(|value| normalize_optional_text(Some(value)))?;
            let bubble_type = entry
                .get("type")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let grouping = entry.get("grouping").unwrap_or(&Value::Null);
            let capability_type = grouping
                .get("capabilityType")
                .and_then(Value::as_i64)
                .or_else(|| entry.get("capabilityType").and_then(Value::as_i64));
            let is_renderable = grouping
                .get("isRenderable")
                .and_then(Value::as_bool)
                .unwrap_or(true);

            Some(CursorBubbleHeader {
                bubble_id,
                bubble_type,
                capability_type,
                is_renderable,
            })
        })
        .collect()
}

fn extract_model_name(value: Option<&Value>) -> Option<String> {
    let value = value?;

    value
        .get("selectedModels")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find_map(|item| {
                item.get("modelName")
                    .and_then(Value::as_str)
                    .and_then(|value| normalize_optional_text(Some(value)))
                    .or_else(|| {
                        item.get("modelId")
                            .and_then(Value::as_str)
                            .and_then(|value| normalize_optional_text(Some(value)))
                    })
            })
        })
        .or_else(|| {
            value
                .get("modelName")
                .and_then(Value::as_str)
                .and_then(|value| normalize_optional_text(Some(value)))
        })
}

fn map_cursor_status(value: Option<&str>) -> Option<String> {
    match value.map(|status| status.trim().to_ascii_lowercase()) {
        Some(status) if status == "completed" => Some("completed".to_string()),
        Some(status) if status == "aborted" || status == "cancelled" => {
            Some("interrupted".to_string())
        }
        Some(status) if status == "running" || status == "generating" => {
            Some("in_progress".to_string())
        }
        Some(status) if status == "none" || status.is_empty() => None,
        Some(status) => Some(status),
        None => None,
    }
}

fn build_tool_summary(tool_name: Option<&str>, tool_status: Option<&str>) -> Option<String> {
    match (tool_name, tool_status) {
        (Some(tool_name), Some(tool_status)) => Some(format!("{tool_name} {tool_status}")),
        (Some(tool_name), None) => Some(tool_name.to_string()),
        (None, Some(tool_status)) => Some(tool_status.to_string()),
        (None, None) => None,
    }
}

fn serialize_locator(composer_id: &str, bubble_id: &str) -> String {
    serde_json::to_string(&CursorLocator {
        composer_id: composer_id.to_string(),
        bubble_id: bubble_id.to_string(),
    })
    .unwrap_or_else(|_| {
        format!(
            "{{\"composer_id\":\"{}\",\"bubble_id\":\"{}\"}}",
            composer_id, bubble_id
        )
    })
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn normalize_title(value: &str) -> Option<String> {
    let line = value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");

    if line.is_empty() {
        return None;
    }

    let truncated: String = line.chars().take(120).collect();
    Some(truncated)
}

fn select_title_candidate(value: &str) -> Option<String> {
    let normalized = normalize_title(value)?;
    let lowercase = normalized.to_ascii_lowercase();
    if lowercase == "new chat"
        || lowercase == "new composer"
        || lowercase == "new session"
        || lowercase.starts_with("<environment_context>")
        || lowercase.starts_with("<permissions instructions>")
    {
        return None;
    }

    Some(normalized)
}

fn estimate_text_tokens(value: &str) -> i64 {
    let mut cjk_chars = 0_i64;
    let mut other_chars = 0_i64;

    for ch in value.chars() {
        if ch.is_whitespace() {
            continue;
        }

        if is_cjk_character(ch) {
            cjk_chars += 1;
        } else {
            other_chars += 1;
        }
    }

    let estimate =
        ((other_chars as f64) / 4.0).ceil() as i64 + ((cjk_chars as f64) * 0.6).ceil() as i64;
    estimate.max(1)
}

fn is_cjk_character(value: char) -> bool {
    matches!(
        value as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EBEF
    )
}

fn older_timestamp(
    left: Option<DateTime<Utc>>,
    right: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn newer_timestamp(
    left: Option<DateTime<Utc>>,
    right: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::CursorAdapter;
    use crate::sources::SourceAdapter;
    use rusqlite::Connection;
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn unique_temp_db_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "totoken-cursor-{label}-{}.db",
            crate::utils::ids::new_uuid()
        ))
    }

    fn write_fixture_db(path: &Path) {
        let conn = Connection::open(path).expect("open cursor fixture db");
        conn.execute_batch(
            "
            CREATE TABLE ItemTable (
                key TEXT PRIMARY KEY,
                value BLOB
            );
            CREATE TABLE cursorDiskKV (
                key TEXT PRIMARY KEY,
                value BLOB
            );
            ",
        )
        .expect("create cursor fixture schema");

        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composer.composerHeaders",
                json!({
                    "allComposers": [{
                        "composerId": "cmp_fixture_1",
                        "name": "Cursor Fixture Session",
                        "createdAt": 1_747_465_749_805_i64,
                        "lastUpdatedAt": 1_747_466_678_347_i64,
                        "conversationCheckpointLastUpdatedAt": 1_747_466_682_095_i64
                    }]
                })
                .to_string()
            ],
        )
        .expect("insert composer headers");

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composerData:cmp_fixture_1",
                json!({
                    "_v": 16,
                    "composerId": "cmp_fixture_1",
                    "createdAt": 1_747_465_749_805_i64,
                    "lastUpdatedAt": 1_747_466_678_347_i64,
                    "status": "completed",
                    "modelConfig": {
                        "modelName": "claude-3.7-sonnet",
                        "selectedModels": [{ "modelName": "claude-3.7-sonnet" }]
                    },
                    "fullConversationHeadersOnly": [
                        { "bubbleId": "bubble_user_1", "type": 1, "grouping": { "isRenderable": true, "hasText": true } },
                        { "bubbleId": "bubble_thinking_1", "type": 2, "grouping": { "isRenderable": true, "capabilityType": 30 } },
                        { "bubbleId": "bubble_tool_1", "type": 2, "grouping": { "isRenderable": true, "capabilityType": 15 } },
                        { "bubbleId": "bubble_assistant_1", "type": 2, "grouping": { "isRenderable": true, "hasText": true } }
                    ]
                })
                .to_string()
            ],
        )
        .expect("insert composer data");

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:cmp_fixture_1:bubble_user_1",
                json!({
                    "bubbleId": "bubble_user_1",
                    "type": 1,
                    "text": "你好，帮我总结这个项目",
                    "tokenCount": { "inputTokens": 0, "outputTokens": 0 }
                })
                .to_string()
            ],
        )
        .expect("insert user bubble");

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:cmp_fixture_1:bubble_thinking_1",
                json!({
                    "bubbleId": "bubble_thinking_1",
                    "type": 2,
                    "capabilityType": 30,
                    "text": "",
                    "tokenCount": { "inputTokens": 0, "outputTokens": 0 }
                })
                .to_string()
            ],
        )
        .expect("insert thinking bubble");

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:cmp_fixture_1:bubble_tool_1",
                json!({
                    "bubbleId": "bubble_tool_1",
                    "type": 2,
                    "capabilityType": 15,
                    "toolFormerData": { "name": "read_file", "status": "completed" },
                    "tokenCount": { "inputTokens": 0, "outputTokens": 0 }
                })
                .to_string()
            ],
        )
        .expect("insert tool bubble");

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:cmp_fixture_1:bubble_assistant_1",
                json!({
                    "bubbleId": "bubble_assistant_1",
                    "type": 2,
                    "text": "这个项目主要负责会话统计和 token 聚合。",
                    "timingInfo": {
                        "clientRpcSendTime": 1_749_815_330_790_i64,
                        "clientEndTime": 1_749_815_335_364_i64,
                        "clientSettleTime": 1_749_815_335_364_i64
                    },
                    "tokenCount": { "inputTokens": 5155, "outputTokens": 30 },
                    "usageUuid": "usage_fixture_1"
                })
                .to_string()
            ],
        )
        .expect("insert assistant bubble");
    }

    fn write_estimated_fixture_db(path: &Path) {
        let conn = Connection::open(path).expect("open estimated fixture db");
        conn.execute_batch(
            "
            CREATE TABLE ItemTable (
                key TEXT PRIMARY KEY,
                value BLOB
            );
            CREATE TABLE cursorDiskKV (
                key TEXT PRIMARY KEY,
                value BLOB
            );
            ",
        )
        .expect("create estimated cursor schema");

        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composer.composerHeaders",
                json!({
                    "allComposers": [{
                        "composerId": "cmp_estimated_1",
                        "name": "Cursor Estimated Session",
                        "createdAt": 1_747_465_749_805_i64,
                        "lastUpdatedAt": 1_747_466_678_347_i64
                    }]
                })
                .to_string()
            ],
        )
        .expect("insert estimated composer headers");

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composerData:cmp_estimated_1",
                json!({
                    "_v": 1,
                    "composerId": "cmp_estimated_1",
                    "status": "completed",
                    "fullConversationHeadersOnly": [
                        { "bubbleId": "bubble_user_1", "type": 1, "grouping": { "isRenderable": true } },
                        { "bubbleId": "bubble_assistant_1", "type": 2, "grouping": { "isRenderable": true } }
                    ]
                })
                .to_string()
            ],
        )
        .expect("insert estimated composer");

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:cmp_estimated_1:bubble_user_1",
                json!({
                    "bubbleId": "bubble_user_1",
                    "type": 1,
                    "text": "Summarize this project",
                    "tokenCount": { "inputTokens": 0, "outputTokens": 0 }
                })
                .to_string()
            ],
        )
        .expect("insert estimated user bubble");

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:cmp_estimated_1:bubble_assistant_1",
                json!({
                    "bubbleId": "bubble_assistant_1",
                    "type": 2,
                    "text": "This project tracks AI token usage across tools.",
                    "timingInfo": {
                        "clientRpcSendTime": 1_749_815_330_790_i64,
                        "clientEndTime": 1_749_815_335_364_i64,
                        "clientSettleTime": 1_749_815_335_364_i64
                    },
                    "tokenCount": { "inputTokens": 0, "outputTokens": 0 }
                })
                .to_string()
            ],
        )
        .expect("insert estimated assistant bubble");
    }

    fn write_estimated_tool_fixture_db(path: &Path) {
        let conn = Connection::open(path).expect("open estimated tool fixture db");
        conn.execute_batch(
            "
            CREATE TABLE ItemTable (
                key TEXT PRIMARY KEY,
                value BLOB
            );
            CREATE TABLE cursorDiskKV (
                key TEXT PRIMARY KEY,
                value BLOB
            );
            ",
        )
        .expect("create estimated tool cursor schema");

        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composer.composerHeaders",
                json!({
                    "allComposers": [{
                        "composerId": "cmp_estimated_tool_1",
                        "name": "Cursor Tool Session",
                        "createdAt": 1_747_465_749_805_i64,
                        "lastUpdatedAt": 1_747_466_678_347_i64
                    }]
                })
                .to_string()
            ],
        )
        .expect("insert estimated tool composer headers");

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composerData:cmp_estimated_tool_1",
                json!({
                    "_v": 1,
                    "composerId": "cmp_estimated_tool_1",
                    "status": "completed",
                    "fullConversationHeadersOnly": [
                        { "bubbleId": "bubble_user_1", "type": 1, "grouping": { "isRenderable": true } },
                        { "bubbleId": "bubble_assistant_1", "type": 2, "grouping": { "isRenderable": true } },
                        { "bubbleId": "bubble_tool_1", "type": 2, "capabilityType": 15, "grouping": { "isRenderable": true } },
                        { "bubbleId": "bubble_assistant_2", "type": 2, "grouping": { "isRenderable": true } }
                    ]
                })
                .to_string()
            ],
        )
        .expect("insert estimated tool composer");

        for (bubble_id, payload) in [
            (
                "bubble_user_1",
                json!({
                    "bubbleId": "bubble_user_1",
                    "type": 1,
                    "text": "abcd",
                    "tokenCount": { "inputTokens": 0, "outputTokens": 0 }
                }),
            ),
            (
                "bubble_assistant_1",
                json!({
                    "bubbleId": "bubble_assistant_1",
                    "type": 2,
                    "text": "efgh",
                    "tokenCount": { "inputTokens": 0, "outputTokens": 0 }
                }),
            ),
            (
                "bubble_tool_1",
                json!({
                    "bubbleId": "bubble_tool_1",
                    "type": 2,
                    "capabilityType": 15,
                    "text": "",
                    "toolFormerData": {
                        "name": "read_file",
                        "status": "completed",
                        "rawArgs": "ijkl",
                        "result": "r".repeat(120)
                    },
                    "tokenCount": { "inputTokens": 0, "outputTokens": 0 }
                }),
            ),
            (
                "bubble_assistant_2",
                json!({
                    "bubbleId": "bubble_assistant_2",
                    "type": 2,
                    "text": "mnop",
                    "tokenCount": { "inputTokens": 0, "outputTokens": 0 }
                }),
            ),
        ] {
            conn.execute(
                "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
                rusqlite::params![
                    format!("bubbleId:cmp_estimated_tool_1:{bubble_id}"),
                    payload.to_string()
                ],
            )
            .expect("insert estimated tool bubble");
        }
    }

    fn write_workspace_fixture_db(path: &Path) {
        let conn = Connection::open(path).expect("open workspace cursor fixture db");
        conn.execute_batch(
            "
            CREATE TABLE ItemTable (
                key TEXT PRIMARY KEY,
                value BLOB
            );
            ",
        )
        .expect("create workspace cursor schema");

        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composer.composerData",
                json!({
                    "allComposers": [{
                        "composerId": "cmp_workspace_1",
                        "name": "Workspace Cursor Session",
                        "createdAt": 1_747_465_749_805_i64,
                        "lastUpdatedAt": 1_747_466_678_347_i64
                    }],
                    "selectedComposerIds": ["cmp_workspace_1"]
                })
                .to_string()
            ],
        )
        .expect("insert workspace composer data");
    }

    fn write_untitled_workspace_fixture_db(path: &Path) {
        let conn = Connection::open(path).expect("open untitled workspace cursor fixture db");
        conn.execute_batch(
            "
            CREATE TABLE ItemTable (
                key TEXT PRIMARY KEY,
                value BLOB
            );
            ",
        )
        .expect("create untitled workspace cursor schema");

        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composer.composerData",
                json!({
                    "allComposers": [{
                        "composerId": "cmp_workspace_untitled_1",
                        "createdAt": 1_747_465_749_805_i64,
                        "lastUpdatedAt": 1_747_466_678_347_i64
                    }]
                })
                .to_string()
            ],
        )
        .expect("insert untitled workspace composer data");
    }

    #[test]
    fn cursor_discovers_state_db_from_directory_or_file_path() {
        let adapter = CursorAdapter;
        let root = std::env::temp_dir().join(format!(
            "totoken-cursor-discover-{}",
            crate::utils::ids::new_uuid()
        ));
        let global_storage = root.join("globalStorage");
        let workspace_storage = root.join("workspaceStorage").join("workspace_hash");
        let stale_workspace_storage = root.join("workspaceStorage").join("stale_hash");
        let nested_global_storage = root.join("workspaceStorage").join("globalStorage");
        fs::create_dir_all(&global_storage).expect("create temp cursor dir");
        fs::create_dir_all(&workspace_storage).expect("create temp cursor workspace dir");
        fs::create_dir_all(&stale_workspace_storage).expect("create stale workspace dir");
        fs::create_dir_all(&nested_global_storage).expect("create nested globalStorage dir");
        let db_path = global_storage.join("state.vscdb");
        let workspace_db_path = workspace_storage.join("state.vscdb");
        fs::write(&db_path, "").expect("write state db");
        fs::write(&workspace_db_path, "").expect("write workspace state db");

        let from_dir = adapter
            .discover_paths(&global_storage)
            .expect("discover from globalStorage");
        let from_user_dir = adapter
            .discover_paths(&root)
            .expect("discover from cursor user dir");
        let from_file = adapter
            .discover_paths(&db_path)
            .expect("discover from file");

        assert_eq!(from_dir, vec![db_path.clone()]);
        assert_eq!(
            from_user_dir,
            vec![db_path.clone(), workspace_db_path.clone()]
        );
        assert_eq!(from_file, vec![db_path.clone()]);
        assert!(adapter.can_handle(&workspace_db_path));

        let _ = fs::remove_file(&db_path);
        let _ = fs::remove_file(&workspace_db_path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cursor_fingerprint_ignores_shm_and_empty_wal() {
        let adapter = CursorAdapter;
        let root = std::env::temp_dir().join(format!(
            "totoken-cursor-fingerprint-{}",
            crate::utils::ids::new_uuid()
        ));
        let global_storage = root.join("globalStorage");
        fs::create_dir_all(&global_storage).expect("create fingerprint dir");
        let db_path = global_storage.join("state.vscdb");
        fs::write(&db_path, "db").expect("write state db");
        fs::write(global_storage.join("state.vscdb-wal"), "").expect("write empty wal");
        fs::write(global_storage.join("state.vscdb-shm"), "shared-memory").expect("write shm");

        let fingerprint_paths = adapter.fingerprint_paths(&db_path);
        assert_eq!(fingerprint_paths, vec![db_path.clone()]);

        fs::write(global_storage.join("state.vscdb-wal"), "wal-data").expect("write wal");
        let fingerprint_paths = adapter.fingerprint_paths(&db_path);
        assert_eq!(
            fingerprint_paths,
            vec![db_path.clone(), global_storage.join("state.vscdb-wal")]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cursor_parse_extracts_metadata_without_storing_raw_text() {
        let db_path = unique_temp_db_path("parse");
        write_fixture_db(&db_path);

        let sessions = CursorAdapter.parse(&db_path).expect("parse cursor fixture");
        let serialized = serde_json::to_string(&sessions).expect("serialize cursor sessions");

        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.source_app, "cursor");
        assert_eq!(
            session.external_session_id.as_deref(),
            Some("cmp_fixture_1")
        );
        assert_eq!(session.requests.len(), 1);
        assert_eq!(session.message_count, 4);
        assert_eq!(session.events.len(), 1);
        assert_eq!(session.total_input_tokens, 5155);
        assert_eq!(session.total_output_tokens, 30);
        assert_eq!(
            session.requests[0].token_confidence.as_deref(),
            Some("high")
        );
        assert_eq!(session.requests[0].input_tokens, Some(5155));
        assert_eq!(session.requests[0].output_tokens, Some(30));
        assert_eq!(
            session.requests[0]
                .source_updated_at
                .map(|value| value.timestamp_millis()),
            Some(1_749_815_335_364_i64)
        );
        assert_eq!(
            session.events[0].event_time_utc.timestamp_millis(),
            1_749_815_335_364_i64
        );
        assert!(!serialized.contains("你好，帮我总结这个项目"));
        assert!(!serialized.contains("这个项目主要负责会话统计和 token 聚合。"));

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn cursor_estimates_feed_session_aggregates() {
        let db_path = unique_temp_db_path("estimated");
        write_estimated_fixture_db(&db_path);

        let sessions = CursorAdapter
            .parse(&db_path)
            .expect("parse estimated cursor fixture");

        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.requests.len(), 1);
        assert_eq!(session.requests[0].token_confidence, None);
        assert!(session.requests[0].input_tokens.unwrap_or(0) > 0);
        assert!(session.requests[0].output_tokens.unwrap_or(0) > 0);
        assert_eq!(
            session.total_input_tokens,
            session.requests[0].input_tokens.unwrap_or(0)
        );
        assert_eq!(
            session.total_output_tokens,
            session.requests[0].output_tokens.unwrap_or(0)
        );
        assert!(session.events.is_empty());

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn cursor_estimates_include_tool_results_as_follow_up_context() {
        let db_path = unique_temp_db_path("estimated-tool");
        write_estimated_tool_fixture_db(&db_path);

        let sessions = CursorAdapter
            .parse(&db_path)
            .expect("parse estimated cursor tool fixture");

        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.requests.len(), 1);
        let request = &session.requests[0];
        assert_eq!(request.token_confidence, None);
        assert_eq!(request.input_tokens, Some(34));
        assert_eq!(request.output_tokens, Some(3));
        assert_eq!(request.total_tokens, Some(37));
        assert_eq!(session.total_input_tokens, 34);
        assert_eq!(session.total_output_tokens, 3);
        assert!(session.events.is_empty());

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn cursor_estimate_checksum_changes_with_tool_result_estimates() {
        let db_path = unique_temp_db_path("estimated-tool-checksum");
        write_estimated_tool_fixture_db(&db_path);

        let initial = CursorAdapter
            .parse(&db_path)
            .expect("parse initial estimated cursor tool fixture");
        assert_eq!(initial[0].requests[0].input_tokens, Some(34));

        let conn = Connection::open(&db_path).expect("open estimated tool checksum db");
        conn.execute(
            "UPDATE cursorDiskKV SET value = ?1 WHERE key = ?2",
            rusqlite::params![
                json!({
                    "bubbleId": "bubble_tool_1",
                    "type": 2,
                    "capabilityType": 15,
                    "text": "",
                    "toolFormerData": {
                        "name": "read_file",
                        "status": "completed",
                        "rawArgs": "ijkl",
                        "result": "r".repeat(240)
                    },
                    "tokenCount": { "inputTokens": 0, "outputTokens": 0 }
                })
                .to_string(),
                "bubbleId:cmp_estimated_tool_1:bubble_tool_1"
            ],
        )
        .expect("update estimated tool result");

        let updated = CursorAdapter
            .parse(&db_path)
            .expect("parse updated estimated cursor tool fixture");
        assert_ne!(
            initial[0].conversation_checksum,
            updated[0].conversation_checksum
        );
        assert_eq!(updated[0].requests[0].input_tokens, Some(64));

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn cursor_parse_workspace_composer_metadata() {
        let root = std::env::temp_dir().join(format!(
            "totoken-cursor-workspace-{}",
            crate::utils::ids::new_uuid()
        ));
        let workspace_dir = root.join("workspaceStorage").join("workspace_hash");
        fs::create_dir_all(&workspace_dir).expect("create workspace fixture dir");
        let db_path = workspace_dir.join("state.vscdb");
        write_workspace_fixture_db(&db_path);

        let sessions = CursorAdapter
            .parse(&db_path)
            .expect("parse workspace cursor fixture");

        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.session_key, "cursor:cmp_workspace_1");
        assert_eq!(session.title.as_deref(), Some("Workspace Cursor Session"));
        assert_eq!(session.message_count, 0);
        assert!(session.requests.is_empty());
        assert!(session.events.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cursor_parse_ignores_untitled_workspace_metadata_only_composers() {
        let root = std::env::temp_dir().join(format!(
            "totoken-cursor-workspace-untitled-{}",
            crate::utils::ids::new_uuid()
        ));
        let workspace_dir = root.join("workspaceStorage").join("workspace_hash");
        fs::create_dir_all(&workspace_dir).expect("create untitled workspace fixture dir");
        let db_path = workspace_dir.join("state.vscdb");
        write_untitled_workspace_fixture_db(&db_path);

        let sessions = CursorAdapter
            .parse(&db_path)
            .expect("parse untitled workspace cursor fixture");

        assert!(sessions.is_empty());

        let _ = fs::remove_dir_all(&root);
    }
}
