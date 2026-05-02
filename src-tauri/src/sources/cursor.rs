use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::message_stream::{MessageStreamAggregator, MessageStreamItem, MessageTokenUsage};
use super::{NormalizedRequest, NormalizedSession, NormalizedUsageEvent, SourceAdapter};
use crate::error::AppResult;
use crate::utils::sqlite::SqliteSnapshot;
use crate::utils::{hash, time};

pub struct CursorAdapter;

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
    usage_event_id: Option<String>,
}

impl SourceAdapter for CursorAdapter {
    fn name(&self) -> &str {
        "cursor"
    }

    fn parser_version(&self) -> i64 {
        2
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("state.vscdb"))
            && path
                .parent()
                .and_then(|value| value.file_name())
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("globalStorage"))
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

        let candidate = root_path.join("state.vscdb");
        Ok(self
            .can_handle(&candidate)
            .then_some(candidate)
            .into_iter()
            .collect())
    }

    fn parse(&self, path: &Path) -> AppResult<Vec<NormalizedSession>> {
        let snapshot = SqliteSnapshot::open(path, self.name())?;
        let conn = snapshot.connection();

        let headers_by_id = load_composer_headers(conn)?;
        let composer_rows = load_composer_rows(conn)?;
        let bubbles_by_key = load_bubbles(conn)?;

        let mut composer_ids = headers_by_id.keys().cloned().collect::<Vec<_>>();
        for (composer_id, value) in &composer_rows {
            if !composer_ids.iter().any(|existing| existing == composer_id)
                && !extract_conversation_headers(value).is_empty()
            {
                composer_ids.push(composer_id.clone());
            }
        }

        let mut sessions = Vec::new();
        for composer_id in composer_ids {
            let header = headers_by_id.get(&composer_id).cloned().unwrap_or_default();
            let payload = match composer_rows.get(&composer_id) {
                Some(payload) => payload,
                None => continue,
            };

            if let Some(session) = build_normalized_session(
                self.name(),
                &composer_id,
                &header,
                payload,
                &bubbles_by_key,
            ) {
                sessions.push(session);
            }
        }

        Ok(sessions)
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

fn load_composer_headers(conn: &Connection) -> AppResult<HashMap<String, CursorComposerHeader>> {
    let raw_value: Option<String> = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = 'composer.composerHeaders' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw_value) = raw_value else {
        return Ok(HashMap::new());
    };

    let payload: Value = serde_json::from_str(&raw_value)?;
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

    Ok(headers)
}

fn load_composer_rows(conn: &Connection) -> AppResult<HashMap<String, Value>> {
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

fn load_bubbles(conn: &Connection) -> AppResult<HashMap<(String, String), Value>> {
    let mut stmt =
        conn.prepare("SELECT key, value FROM cursorDiskKV WHERE key LIKE 'bubbleId:%'")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;

    let mut grouped = HashMap::new();
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

    Ok(grouped)
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
    let (mut requests, mut events, total_input_tokens, total_output_tokens) =
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

    if events.is_empty() && (total_input_tokens > 0 || total_output_tokens > 0) {
        if let Some(event_time_utc) = fallback_event_time {
            events.push(NormalizedUsageEvent {
                event_time_utc,
                model: model_last.clone(),
                delta_input: total_input_tokens,
                delta_output: total_output_tokens,
                delta_total: total_input_tokens + total_output_tokens,
                cache_read_input_tokens: 0,
                cache_write_input_tokens: 0,
                source_event_id: Some(composer_id.to_string()),
                granularity: "session_total".to_string(),
                confidence: "low".to_string(),
            });
        }
    }

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
    apply_cursor_low_confidence_estimates(&mut requests, &estimates_by_request_id);

    (
        requests,
        aggregate.events,
        aggregate.total_input_tokens,
        aggregate.total_output_tokens,
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
        estimate.input_tokens += message.estimated_input_tokens.unwrap_or(0);
        estimate.output_tokens += message.estimated_output_tokens.unwrap_or(0);
    }

    estimates
}

fn apply_cursor_low_confidence_estimates(
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
        request.token_confidence = Some("low".to_string());
    }
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

    let (role, message_type, estimated_input_tokens, estimated_output_tokens, character_count) =
        if bubble_header.bubble_type == 1 {
            let character_count = text.as_ref().map(|value| value.chars().count() as i64);
            (
                "user".to_string(),
                "message".to_string(),
                text.as_deref().map(estimate_text_tokens),
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
                character_count,
            )
        } else if capability_type == Some(15) || tool_name.is_some() {
            let summary = build_tool_summary(tool_name.as_deref(), tool_status.as_deref());
            let character_count = summary.as_ref().map(|value| value.chars().count() as i64);
            (
                "tool".to_string(),
                "tool".to_string(),
                None,
                None,
                character_count,
            )
        } else {
            let character_count = text.as_ref().map(|value| value.chars().count() as i64);
            (
                "assistant".to_string(),
                "message".to_string(),
                None,
                text.as_deref().map(estimate_text_tokens),
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

    fn write_low_confidence_fixture_db(path: &Path) {
        let conn = Connection::open(path).expect("open low confidence fixture db");
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
        .expect("create low confidence cursor schema");

        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composer.composerHeaders",
                json!({
                    "allComposers": [{
                        "composerId": "cmp_low_1",
                        "name": "Cursor Low Session",
                        "createdAt": 1_747_465_749_805_i64,
                        "lastUpdatedAt": 1_747_466_678_347_i64
                    }]
                })
                .to_string()
            ],
        )
        .expect("insert low confidence composer headers");

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composerData:cmp_low_1",
                json!({
                    "_v": 1,
                    "composerId": "cmp_low_1",
                    "status": "completed",
                    "fullConversationHeadersOnly": [
                        { "bubbleId": "bubble_user_1", "type": 1, "grouping": { "isRenderable": true } },
                        { "bubbleId": "bubble_assistant_1", "type": 2, "grouping": { "isRenderable": true } }
                    ]
                })
                .to_string()
            ],
        )
        .expect("insert low confidence composer");

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:cmp_low_1:bubble_user_1",
                json!({
                    "bubbleId": "bubble_user_1",
                    "type": 1,
                    "text": "Summarize this project",
                    "tokenCount": { "inputTokens": 0, "outputTokens": 0 }
                })
                .to_string()
            ],
        )
        .expect("insert low confidence user bubble");

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:cmp_low_1:bubble_assistant_1",
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
        .expect("insert low confidence assistant bubble");
    }

    #[test]
    fn cursor_discovers_state_db_from_directory_or_file_path() {
        let adapter = CursorAdapter;
        let root = std::env::temp_dir().join(format!(
            "totoken-cursor-discover-{}",
            crate::utils::ids::new_uuid()
        ));
        let global_storage = root.join("globalStorage");
        fs::create_dir_all(&global_storage).expect("create temp cursor dir");
        let db_path = global_storage.join("state.vscdb");
        fs::write(&db_path, "").expect("write state db");

        let from_dir = adapter
            .discover_paths(&global_storage)
            .expect("discover from globalStorage");
        let from_file = adapter
            .discover_paths(&db_path)
            .expect("discover from file");

        assert_eq!(from_dir, vec![db_path.clone()]);
        assert_eq!(from_file, vec![db_path.clone()]);

        let _ = fs::remove_file(&db_path);
        let _ = fs::remove_dir(&global_storage);
        let _ = fs::remove_dir(&root);
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
    fn cursor_low_confidence_estimates_stay_out_of_session_aggregates() {
        let db_path = unique_temp_db_path("low-confidence");
        write_low_confidence_fixture_db(&db_path);

        let sessions = CursorAdapter
            .parse(&db_path)
            .expect("parse low confidence cursor fixture");

        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.requests.len(), 1);
        assert_eq!(session.requests[0].token_confidence.as_deref(), Some("low"));
        assert!(session.requests[0].input_tokens.unwrap_or(0) > 0);
        assert!(session.requests[0].output_tokens.unwrap_or(0) > 0);
        assert_eq!(session.total_input_tokens, 0);
        assert_eq!(session.total_output_tokens, 0);
        assert!(session.events.is_empty());

        let _ = fs::remove_file(&db_path);
    }
}
