use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::message_stream::{MessageStreamAggregator, MessageStreamItem, MessageTokenUsage};
use super::{NormalizedSession, NormalizedUsageEvent, SourceAdapter};
use crate::error::{AppError, AppResult};
use crate::utils::sqlite::SqliteSnapshot;
use crate::utils::{hash, time};

// Supports the current Kilo Code VS Code extension runtime after its built-in
// migration to Kilo/OpenCode SQLite storage.
pub struct KilocodeAdapter;

#[derive(Debug, Serialize, Deserialize)]
struct KilocodeLocator {
    message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    part_id: Option<String>,
}

#[derive(Debug, Clone)]
struct SessionRow {
    id: String,
    title: String,
    time_created: i64,
    time_updated: i64,
}

#[derive(Debug, Clone)]
struct MessageRow {
    id: String,
    session_id: String,
    time_created: i64,
    time_updated: i64,
    data: String,
}

#[derive(Debug, Clone)]
struct PartRow {
    id: String,
    message_id: String,
    time_created: i64,
    time_updated: i64,
    data: String,
}

#[derive(Debug, Clone, Copy, Default)]
struct KiloTokenUsage {
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    cache_read_input_tokens: i64,
    cache_write_input_tokens: i64,
}

impl From<KiloTokenUsage> for MessageTokenUsage {
    fn from(value: KiloTokenUsage) -> Self {
        Self {
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            total_tokens: value.total_tokens,
            cache_read_input_tokens: value.cache_read_input_tokens,
            cache_write_input_tokens: value.cache_write_input_tokens,
        }
    }
}

#[derive(Debug, Clone)]
struct PartMeta {
    id: String,
    kind: String,
    time_created: i64,
    time_updated: i64,
    character_count: i64,
    estimated_tokens: i64,
    status: Option<String>,
    usage: Option<KiloTokenUsage>,
}

#[derive(Debug, Clone)]
struct KiloParsedTurn {
    id: String,
    role: String,
    parent_id: Option<String>,
    model: Option<String>,
    estimated_tokens: Option<i64>,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    updated_at: Option<chrono::DateTime<chrono::Utc>>,
    status: Option<String>,
    usage: Option<KiloTokenUsage>,
    usage_part_id: Option<String>,
    usage_part_time_ms: Option<i64>,
    source_locator: String,
}

impl SourceAdapter for KilocodeAdapter {
    fn name(&self) -> &str {
        "kilocode"
    }

    fn parser_version(&self) -> i64 {
        4
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("kilo.db"))
    }

    fn discover_paths(&self, root_path: &Path) -> AppResult<Vec<PathBuf>> {
        if root_path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("kilo.db"))
        {
            return Ok(self
                .can_handle(root_path)
                .then(|| root_path.to_path_buf())
                .into_iter()
                .collect());
        }

        let candidate = root_path.join("kilo.db");
        Ok(candidate
            .exists()
            .then_some(candidate)
            .into_iter()
            .collect())
    }

    fn parse(&self, path: &Path) -> AppResult<Vec<NormalizedSession>> {
        let snapshot = SqliteSnapshot::open(path, self.name())?;
        let conn = snapshot.connection();

        let sessions = load_sessions(conn)?;
        let messages_by_session = load_messages(conn)?;
        let parts_by_message = load_parts(conn)?;

        sessions
            .into_iter()
            .map(|session| {
                build_normalized_session(
                    self.name(),
                    session,
                    &messages_by_session,
                    &parts_by_message,
                )
            })
            .collect()
    }

    fn fingerprint_paths(&self, path: &Path) -> Vec<PathBuf> {
        let mut paths = vec![path.to_path_buf()];

        for suffix in ["-wal", "-shm"] {
            let companion = PathBuf::from(format!("{}{}", path.to_string_lossy(), suffix));
            if companion.exists() {
                paths.push(companion);
            }
        }

        paths
    }
}

fn load_sessions(conn: &Connection) -> AppResult<Vec<SessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, time_created, time_updated
         FROM session
         WHERE time_archived IS NULL
         ORDER BY time_updated ASC, id ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(SessionRow {
            id: row.get(0)?,
            title: row.get(1)?,
            time_created: row.get(2)?,
            time_updated: row.get(3)?,
        })
    })?;

    let sessions = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(sessions)
}

fn load_messages(conn: &Connection) -> AppResult<HashMap<String, Vec<MessageRow>>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, time_created, time_updated, data
         FROM message
         ORDER BY session_id ASC, time_created ASC, id ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(MessageRow {
            id: row.get(0)?,
            session_id: row.get(1)?,
            time_created: row.get(2)?,
            time_updated: row.get(3)?,
            data: row.get(4)?,
        })
    })?;

    let mut grouped = HashMap::<String, Vec<MessageRow>>::new();
    for row in rows {
        let message = row?;
        grouped
            .entry(message.session_id.clone())
            .or_default()
            .push(message);
    }

    Ok(grouped)
}

fn load_parts(conn: &Connection) -> AppResult<HashMap<String, Vec<PartRow>>> {
    let mut stmt = conn.prepare(
        "SELECT id, message_id, session_id, time_created, time_updated, data
         FROM part
         ORDER BY session_id ASC, message_id ASC, time_created ASC, id ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(PartRow {
            id: row.get(0)?,
            message_id: row.get(1)?,
            time_created: row.get(3)?,
            time_updated: row.get(4)?,
            data: row.get(5)?,
        })
    })?;

    let mut grouped = HashMap::<String, Vec<PartRow>>::new();
    for row in rows {
        let part = row?;
        grouped
            .entry(part.message_id.clone())
            .or_default()
            .push(part);
    }

    Ok(grouped)
}

fn build_normalized_session(
    source_app: &str,
    session: SessionRow,
    messages_by_session: &HashMap<String, Vec<MessageRow>>,
    parts_by_message: &HashMap<String, Vec<PartRow>>,
) -> AppResult<NormalizedSession> {
    let messages = messages_by_session
        .get(&session.id)
        .cloned()
        .unwrap_or_default();

    let mut parsed_messages = Vec::<KiloParsedTurn>::new();
    let mut parsed_message_index_by_id = HashMap::<String, usize>::new();
    let mut user_message_ids = Vec::<String>::new();
    let mut model_first = None;
    let mut model_last = None;

    let mut checksum_parts = vec![
        session.id.clone(),
        session.title.clone(),
        session.time_created.to_string(),
        session.time_updated.to_string(),
    ];

    for (index, message) in messages.iter().enumerate() {
        let payload: Value = serde_json::from_str(&message.data)?;
        let role = payload
            .get("role")
            .and_then(Value::as_str)
            .map(normalize_role)
            .unwrap_or_else(|| "unknown".to_string());
        let parent_id = payload
            .get("parentID")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let model = extract_model_name(&payload);
        let parts = parts_by_message
            .get(&message.id)
            .cloned()
            .unwrap_or_default();
        let part_metas = parts
            .iter()
            .map(parse_part_meta)
            .collect::<AppResult<Vec<_>>>()?;
        let character_count = sum_visible_character_count(&part_metas);
        let estimated_tokens = sum_visible_token_estimate(&part_metas);
        let usage_part = part_metas.iter().rev().find(|part| part.usage.is_some());
        let usage = usage_part
            .and_then(|part| part.usage)
            .or_else(|| extract_token_usage(&payload));
        let status = usage_part
            .and_then(|part| part.status.clone())
            .or_else(|| extract_status(&payload));
        let created_at = extract_message_created_at(&payload, message.time_created);
        let updated_at = extract_message_updated_at(&payload, message.time_updated);
        let source_locator = serialize_locator(&message.id, None)?;

        if model_first.is_none() {
            model_first = model.clone();
        }
        if model.is_some() {
            model_last = model.clone();
        }

        checksum_parts.push(message.id.clone());
        checksum_parts.push(role.clone());
        checksum_parts.push(parent_id.clone().unwrap_or_default());
        checksum_parts.push(model.clone().unwrap_or_default());
        checksum_parts.push(message.time_created.to_string());
        checksum_parts.push(message.time_updated.to_string());
        checksum_parts.push(character_count.unwrap_or(0).to_string());
        for part in &part_metas {
            checksum_parts.push(part.id.clone());
            checksum_parts.push(part.kind.clone());
            checksum_parts.push(part.time_created.to_string());
            checksum_parts.push(part.time_updated.to_string());
            checksum_parts.push(part.character_count.to_string());
            if let Some(status) = part.status.as_ref() {
                checksum_parts.push(status.clone());
            }
            if let Some(usage) = part.usage {
                checksum_parts.push(usage.input_tokens.to_string());
                checksum_parts.push(usage.output_tokens.to_string());
                checksum_parts.push(usage.total_tokens.to_string());
                checksum_parts.push(usage.cache_read_input_tokens.to_string());
                checksum_parts.push(usage.cache_write_input_tokens.to_string());
            }
        }

        if role == "user" {
            user_message_ids.push(message.id.clone());
        }

        parsed_message_index_by_id.insert(message.id.clone(), parsed_messages.len());
        parsed_messages.push(KiloParsedTurn {
            id: message.id.clone(),
            role: role.clone(),
            parent_id,
            model: model.clone(),
            estimated_tokens,
            created_at,
            updated_at,
            status,
            usage,
            usage_part_id: usage_part.map(|part| part.id.clone()),
            usage_part_time_ms: usage_part.map(|part| part.time_updated.max(part.time_created)),
            source_locator,
        });

        let _ = (index, character_count, created_at, updated_at);
    }

    let mut request_id_by_assistant_id = HashMap::<String, String>::new();
    let mut assistant_ids_by_request_id = HashMap::<String, Vec<String>>::new();
    for (index, parsed) in parsed_messages.iter().enumerate() {
        if parsed.role != "assistant" {
            continue;
        }

        let paired_user_id =
            resolve_request_user_id(parsed, &parsed_messages, &user_message_ids, index);
        let request_id = paired_user_id.unwrap_or_else(|| parsed.id.clone());
        request_id_by_assistant_id.insert(parsed.id.clone(), request_id.clone());
        assistant_ids_by_request_id
            .entry(request_id)
            .or_default()
            .push(parsed.id.clone());
    }

    let mut stream_items = Vec::<MessageStreamItem>::new();
    for parsed in &parsed_messages {
        let request_id = request_id_by_assistant_id.get(&parsed.id).cloned();
        let request_model = parsed
            .model
            .clone()
            .or_else(|| {
                request_id
                    .as_ref()
                    .and_then(|user_id| {
                        parsed_message_index_by_id
                            .get(user_id)
                            .and_then(|message_index| parsed_messages.get(*message_index))
                    })
                    .and_then(|item| item.model.clone())
            })
            .or_else(|| model_last.clone())
            .or_else(|| model_first.clone());
        let usage = parsed.usage.unwrap_or_default();
        let usage_event_time_utc = if has_usage(usage) {
            Some(
                parsed
                    .updated_at
                    .or_else(|| parsed.usage_part_time_ms.and_then(time::from_unix_ms))
                    .or(parsed.created_at)
                    .ok_or_else(|| {
                        AppError::validation(format!(
                            "Invalid kilocode message time: {}",
                            parsed.id
                        ))
                    })?,
            )
        } else {
            None
        };

        stream_items.push(MessageStreamItem {
            source_id: parsed.id.clone(),
            role: parsed.role.clone(),
            request_id,
            parent_id: parsed.parent_id.clone(),
            status: parsed.status.clone(),
            model: request_model,
            usage: parsed.usage.map(Into::into),
            count_as_message: true,
            source_created_at: parsed.created_at,
            source_updated_at: parsed.updated_at,
            usage_event_time_utc,
            source_event_id: parsed
                .usage_part_id
                .clone()
                .or_else(|| Some(parsed.id.clone())),
            usage_event_granularity: None,
            usage_event_confidence: None,
            source_locator: if parsed.role == "assistant" {
                serialize_locator(&parsed.id, parsed.usage_part_id.as_deref())?
            } else {
                parsed.source_locator.clone()
            },
            use_as_request_locator: parsed.usage_part_id.is_some(),
        });
    }

    let aggregate = MessageStreamAggregator::new(stream_items).aggregate_assistant_request_groups();
    let mut requests = aggregate.requests;
    let mut events = aggregate.events;
    let mut total_input_tokens = 0_i64;
    let mut total_output_tokens = 0_i64;

    for request in &mut requests {
        let request_id = request.source_request_id.clone().unwrap_or_default();
        let sequence_no = request.sequence_no;
        let has_group_usage = request.input_tokens.unwrap_or(0) > 0
            || request.output_tokens.unwrap_or(0) > 0
            || request.cache_read_input_tokens.unwrap_or(0) > 0
            || request.cache_write_input_tokens.unwrap_or(0) > 0;
        let mut effective_input_tokens = request.input_tokens.unwrap_or(0);
        let mut effective_output_tokens = request.output_tokens.unwrap_or(0);
        let mut effective_total_tokens = request.total_tokens.unwrap_or(0);
        let effective_cache_read_input_tokens = request.cache_read_input_tokens.unwrap_or(0);
        let effective_cache_write_input_tokens = request.cache_write_input_tokens.unwrap_or(0);
        let mut token_confidence = request.token_confidence.clone();

        if !has_group_usage && is_legacy_request(&session.id, request.model.as_deref()) {
            if let Some((estimated_input_tokens, estimated_output_tokens)) =
                estimate_legacy_request_usage(
                    &request_id,
                    assistant_ids_by_request_id
                        .get(&request_id)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                    &parsed_messages,
                    &parsed_message_index_by_id,
                )
            {
                effective_input_tokens = estimated_input_tokens;
                effective_output_tokens = estimated_output_tokens;
                effective_total_tokens = estimated_input_tokens + estimated_output_tokens;
                token_confidence = Some("low".to_string());
            }
        }

        let has_effective_usage = effective_input_tokens > 0
            || effective_output_tokens > 0
            || effective_cache_read_input_tokens > 0
            || effective_cache_write_input_tokens > 0;

        request.status = request
            .status
            .clone()
            .or_else(|| Some("completed".to_string()));
        request.input_tokens = has_effective_usage.then_some(effective_input_tokens);
        request.output_tokens = has_effective_usage.then_some(effective_output_tokens);
        request.total_tokens = has_effective_usage.then_some(effective_total_tokens);
        request.cache_read_input_tokens =
            has_effective_usage.then_some(effective_cache_read_input_tokens);
        request.cache_write_input_tokens =
            has_effective_usage.then_some(effective_cache_write_input_tokens);
        request.token_confidence = token_confidence.clone();

        checksum_parts.push(request_id.clone());
        checksum_parts.push(sequence_no.to_string());
        checksum_parts.push(effective_input_tokens.to_string());
        checksum_parts.push(effective_output_tokens.to_string());
        checksum_parts.push(effective_total_tokens.to_string());
        checksum_parts.push(effective_cache_read_input_tokens.to_string());
        checksum_parts.push(effective_cache_write_input_tokens.to_string());
        checksum_parts.push(token_confidence.clone().unwrap_or_default());

        total_input_tokens += effective_input_tokens;
        total_output_tokens += effective_output_tokens;

        if !has_group_usage && has_effective_usage {
            let event_time = request
                .source_updated_at
                .or(request.source_created_at)
                .ok_or_else(|| {
                    AppError::validation(format!(
                        "Invalid kilocode request group time: {request_id}"
                    ))
                })?;
            events.push(NormalizedUsageEvent {
                event_time_utc: event_time,
                model: request.model.clone(),
                delta_input: effective_input_tokens,
                delta_output: effective_output_tokens,
                delta_total: effective_total_tokens,
                cache_read_input_tokens: effective_cache_read_input_tokens,
                cache_write_input_tokens: effective_cache_write_input_tokens,
                source_event_id: Some(request_id.clone()),
                granularity: "request".to_string(),
                confidence: "low".to_string(),
            });
        }
    }

    Ok(NormalizedSession {
        source_app: source_app.to_string(),
        external_session_id: Some(session.id.clone()),
        session_key: format!("{source_app}:{}", session.id),
        title: normalize_optional_text(Some(session.title)),
        model_first,
        model_last,
        source_created_at: time::from_unix_ms(session.time_created),
        source_updated_at: time::from_unix_ms(session.time_updated),
        total_input_tokens,
        total_output_tokens,
        message_count: parsed_messages.len() as i64,
        conversation_checksum: hash::sha256_text(&checksum_parts.join("\n")),
        requests,
        events,
    })
}

fn parse_part_meta(part: &PartRow) -> AppResult<PartMeta> {
    let payload: Value = serde_json::from_str(&part.data)?;
    let kind = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let estimate_content = extract_part_estimate_content(&kind, &payload);
    let character_count = estimate_content
        .as_ref()
        .map(|text| text.chars().count() as i64)
        .unwrap_or(0);
    let estimated_tokens = estimate_content
        .as_deref()
        .map(estimate_text_tokens)
        .unwrap_or(0);
    let status = if kind == "step-finish" {
        payload
            .get("reason")
            .and_then(Value::as_str)
            .map(ToString::to_string)
    } else {
        None
    };
    let usage = if kind == "step-finish" {
        extract_token_usage(&payload)
    } else {
        None
    };

    Ok(PartMeta {
        id: part.id.clone(),
        kind,
        time_created: part.time_created,
        time_updated: part.time_updated,
        character_count,
        estimated_tokens,
        status,
        usage,
    })
}

fn sum_visible_character_count(parts: &[PartMeta]) -> Option<i64> {
    let total = parts.iter().map(|part| part.character_count).sum::<i64>();
    (total > 0).then_some(total)
}

fn sum_visible_token_estimate(parts: &[PartMeta]) -> Option<i64> {
    let total = parts.iter().map(|part| part.estimated_tokens).sum::<i64>();
    (total > 0).then_some(total)
}

fn extract_part_estimate_content(kind: &str, payload: &Value) -> Option<String> {
    match kind {
        "text" => payload
            .get("text")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        "tool" => serialize_tool_part_for_estimate(payload),
        _ => None,
    }
}

fn serialize_tool_part_for_estimate(payload: &Value) -> Option<String> {
    let mut segments = Vec::<String>::new();

    if let Some(tool_name) = payload.get("tool").and_then(Value::as_str) {
        let trimmed = tool_name.trim();
        if !trimmed.is_empty() {
            segments.push(trimmed.to_string());
        }
    }

    let state = payload.get("state");
    if let Some(status) = state
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
    {
        let trimmed = status.trim();
        if !trimmed.is_empty() {
            segments.push(trimmed.to_string());
        }
    }

    for field in ["input", "output"] {
        let Some(value) = state.and_then(|item| item.get(field)) else {
            continue;
        };
        match value {
            Value::Null => {}
            Value::String(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    segments.push(trimmed.to_string());
                }
            }
            other => {
                let serialized = serde_json::to_string(other).ok()?;
                if !serialized.is_empty() {
                    segments.push(serialized);
                }
            }
        }
    }

    (!segments.is_empty()).then(|| segments.join("\n"))
}

fn resolve_request_user_id(
    parsed: &KiloParsedTurn,
    parsed_messages: &[KiloParsedTurn],
    user_message_ids: &[String],
    assistant_index: usize,
) -> Option<String> {
    if let Some(parent_id) = parsed.parent_id.as_ref() {
        if parsed_messages
            .iter()
            .any(|item| item.id == *parent_id && item.role == "user")
        {
            return Some(parent_id.clone());
        }
    }

    user_message_ids
        .iter()
        .rev()
        .find(|user_id| {
            parsed_messages
                .iter()
                .position(|item| item.id == ***user_id)
                .is_some_and(|position| position < assistant_index)
        })
        .cloned()
}

fn extract_message_created_at(
    payload: &Value,
    fallback_time_ms: i64,
) -> Option<chrono::DateTime<chrono::Utc>> {
    payload
        .get("time")
        .and_then(|value| value.get("created"))
        .and_then(Value::as_i64)
        .or(Some(fallback_time_ms))
        .and_then(time::from_unix_ms)
}

fn extract_message_updated_at(
    payload: &Value,
    fallback_time_ms: i64,
) -> Option<chrono::DateTime<chrono::Utc>> {
    payload
        .get("time")
        .and_then(|value| value.get("completed"))
        .and_then(Value::as_i64)
        .or_else(|| {
            payload
                .get("time")
                .and_then(|value| value.get("created"))
                .and_then(Value::as_i64)
        })
        .or(Some(fallback_time_ms))
        .and_then(time::from_unix_ms)
}

fn extract_model_name(payload: &Value) -> Option<String> {
    payload
        .get("modelID")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .get("model")
                .and_then(|value| value.get("modelID"))
                .and_then(Value::as_str)
        })
        .map(ToString::to_string)
}

fn extract_status(payload: &Value) -> Option<String> {
    payload
        .get("finish")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn extract_token_usage(payload: &Value) -> Option<KiloTokenUsage> {
    let tokens = payload.get("tokens")?;
    let raw_input_tokens = tokens.get("input").and_then(Value::as_i64).unwrap_or(0);
    let raw_output_tokens = tokens.get("output").and_then(Value::as_i64).unwrap_or(0);
    let reasoning_tokens = tokens.get("reasoning").and_then(Value::as_i64).unwrap_or(0);
    let cache_read_input_tokens = tokens
        .get("cache")
        .and_then(|value| value.get("read"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cache_write_input_tokens = tokens
        .get("cache")
        .and_then(|value| value.get("write"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let has_any_usage = raw_input_tokens > 0
        || raw_output_tokens > 0
        || reasoning_tokens > 0
        || cache_read_input_tokens > 0
        || cache_write_input_tokens > 0;
    if !has_any_usage {
        return None;
    }

    Some(KiloTokenUsage {
        input_tokens: raw_input_tokens,
        output_tokens: raw_output_tokens + reasoning_tokens,
        total_tokens: tokens
            .get("total")
            .and_then(Value::as_i64)
            .filter(|value| *value > 0)
            .unwrap_or(
                raw_input_tokens
                    + raw_output_tokens
                    + reasoning_tokens
                    + cache_read_input_tokens
                    + cache_write_input_tokens,
            ),
        cache_read_input_tokens,
        cache_write_input_tokens,
    })
}

fn has_usage(usage: KiloTokenUsage) -> bool {
    usage.input_tokens > 0
        || usage.output_tokens > 0
        || usage.cache_read_input_tokens > 0
        || usage.cache_write_input_tokens > 0
}

fn is_legacy_request(session_id: &str, model: Option<&str>) -> bool {
    session_id.starts_with("ses_migrated_") || model == Some("legacy")
}

fn estimate_legacy_request_usage(
    request_id: &str,
    assistant_ids: &[String],
    parsed_messages: &[KiloParsedTurn],
    parsed_message_index_by_id: &HashMap<String, usize>,
) -> Option<(i64, i64)> {
    let input_tokens = parsed_message_index_by_id
        .get(request_id)
        .and_then(|index| parsed_messages.get(*index))
        .filter(|message| message.role == "user")
        .and_then(|message| message.estimated_tokens)
        .unwrap_or(0);
    let mut output_tokens = 0_i64;

    for assistant_id in assistant_ids {
        let estimated_tokens = parsed_message_index_by_id
            .get(assistant_id)
            .and_then(|index| parsed_messages.get(*index))
            .and_then(|message| message.estimated_tokens)
            .unwrap_or(0);
        if estimated_tokens > 0 {
            output_tokens += estimated_tokens;
        }
    }

    if assistant_ids.is_empty() {
        let estimated_tokens = parsed_message_index_by_id
            .get(request_id)
            .and_then(|index| parsed_messages.get(*index))
            .filter(|message| message.role == "assistant")
            .and_then(|message| message.estimated_tokens)
            .unwrap_or(0);
        if estimated_tokens > 0 {
            output_tokens += estimated_tokens;
        }
    }

    (input_tokens > 0 || output_tokens > 0).then_some((input_tokens, output_tokens))
}

fn normalize_role(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" | "assistant" | "system" | "tool" => value.trim().to_ascii_lowercase(),
        _ => "unknown".to_string(),
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
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

fn serialize_locator(message_id: &str, part_id: Option<&str>) -> AppResult<String> {
    Ok(serde_json::to_string(&KilocodeLocator {
        message_id: message_id.to_string(),
        part_id: part_id.map(ToString::to_string),
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::json;
    use std::fs;
    use std::time::SystemTime;

    fn unique_temp_db_path(label: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!("{label}-{unique}.db"));
        path
    }

    fn write_fixture_db(path: &Path) {
        let conn = Connection::open(path).expect("open fixture db");
        conn.execute_batch(
            "
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                parent_id TEXT,
                slug TEXT NOT NULL,
                directory TEXT NOT NULL,
                title TEXT NOT NULL,
                version TEXT NOT NULL,
                share_url TEXT,
                summary_additions INTEGER,
                summary_deletions INTEGER,
                summary_files INTEGER,
                summary_diffs TEXT,
                revert TEXT,
                permission TEXT,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                time_compacting INTEGER,
                time_archived INTEGER,
                workspace_id TEXT
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            ",
        )
        .expect("create fixture schema");

        let session_id = "ses_test_1";
        let user_message_id = "msg_user_1";
        let assistant_message_id = "msg_assistant_1";

        conn.execute(
            "INSERT INTO session (
                id, project_id, slug, directory, title, version, time_created, time_updated
             ) VALUES (?1, 'proj_1', 'fixture', 'E:/fixture', 'Fixture Session', '1', ?2, ?3)",
            rusqlite::params![session_id, 1_700_000_000_000_i64, 1_700_000_100_000_i64],
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                user_message_id,
                session_id,
                1_700_000_001_000_i64,
                1_700_000_001_000_i64,
                json!({
                    "role": "user",
                    "time": { "created": 1_700_000_001_000_i64 },
                    "model": { "providerID": "kilo", "modelID": "kilo-auto/free" }
                })
                .to_string()
            ],
        )
        .expect("insert user message");
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                assistant_message_id,
                session_id,
                1_700_000_002_000_i64,
                1_700_000_005_000_i64,
                json!({
                    "role": "assistant",
                    "parentID": user_message_id,
                    "time": {
                        "created": 1_700_000_002_000_i64,
                        "completed": 1_700_000_005_000_i64
                    },
                    "modelID": "kilo-auto/free",
                    "finish": "stop"
                })
                .to_string()
            ],
        )
        .expect("insert assistant message");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('prt_user_text', ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                user_message_id,
                session_id,
                1_700_000_001_100_i64,
                1_700_000_001_100_i64,
                json!({ "type": "text", "text": "hello kilo" }).to_string()
            ],
        )
        .expect("insert user part");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('prt_assistant_reasoning', ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                assistant_message_id,
                session_id,
                1_700_000_002_100_i64,
                1_700_000_002_200_i64,
                json!({ "type": "reasoning", "text": "internal only" }).to_string()
            ],
        )
        .expect("insert reasoning part");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('prt_assistant_text', ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                assistant_message_id,
                session_id,
                1_700_000_002_300_i64,
                1_700_000_002_300_i64,
                json!({ "type": "text", "text": "answer text" }).to_string()
            ],
        )
        .expect("insert assistant text part");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('prt_assistant_finish', ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                assistant_message_id,
                session_id,
                1_700_000_005_000_i64,
                1_700_000_005_000_i64,
                json!({
                    "type": "step-finish",
                    "reason": "stop",
                    "tokens": {
                        "total": 140,
                        "input": 100,
                        "output": 25,
                        "reasoning": 5,
                        "cache": {
                            "read": 10,
                            "write": 0
                        }
                    }
                })
                .to_string()
            ],
        )
        .expect("insert finish part");
    }

    #[test]
    fn kilocode_usage_includes_cache_and_reasoning_tokens() {
        let payload = json!({
            "tokens": {
                "input": 100,
                "output": 25,
                "reasoning": 5,
                "cache": {
                    "read": 10,
                    "write": 2
                }
            }
        });

        let usage = extract_token_usage(&payload).expect("usage should be parsed");
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 30);
        assert_eq!(usage.total_tokens, 142);
        assert_eq!(usage.cache_read_input_tokens, 10);
        assert_eq!(usage.cache_write_input_tokens, 2);
    }

    #[test]
    fn kilocode_discovers_database_from_directory_or_file_path() {
        let adapter = KilocodeAdapter;
        let db_parent = unique_temp_db_path("kilocode-discover-dir");
        fs::create_dir_all(&db_parent).expect("create temp directory");
        let other_db = db_parent.join("other.db");
        let kilo_db = db_parent.join("kilo.db");
        fs::write(&other_db, "").expect("write non-kilo db file");
        fs::write(&kilo_db, "").expect("write kilo db file");

        let from_directory = adapter
            .discover_paths(&db_parent)
            .expect("discover from directory");
        let from_other_file = adapter
            .discover_paths(&other_db)
            .expect("discover from non-kilo file");
        let from_file = adapter
            .discover_paths(&kilo_db)
            .expect("discover from kilo file");

        assert!(from_other_file.is_empty());
        assert_eq!(from_file, vec![kilo_db.clone()]);
        let discovered = adapter
            .discover_paths(&db_parent)
            .expect("discover kilo db from directory");
        assert_eq!(from_directory, vec![kilo_db.clone()]);
        assert_eq!(discovered, vec![kilo_db.clone()]);

        let _ = fs::remove_file(&other_db);
        let _ = fs::remove_file(&kilo_db);
        let _ = fs::remove_dir(&db_parent);
    }

    #[test]
    fn kilocode_fingerprint_includes_wal_and_shm_files() {
        let adapter = KilocodeAdapter;
        let db_path = unique_temp_db_path("kilocode-fingerprint");
        let wal_path = PathBuf::from(format!("{}-wal", db_path.to_string_lossy()));
        let shm_path = PathBuf::from(format!("{}-shm", db_path.to_string_lossy()));

        fs::write(&db_path, "").expect("write db");
        fs::write(&wal_path, "").expect("write wal");
        fs::write(&shm_path, "").expect("write shm");

        let fingerprint_paths = adapter.fingerprint_paths(&db_path);
        assert_eq!(fingerprint_paths.len(), 3);
        assert!(fingerprint_paths.contains(&db_path));
        assert!(fingerprint_paths.contains(&wal_path));
        assert!(fingerprint_paths.contains(&shm_path));

        let _ = fs::remove_file(&db_path);
        let _ = fs::remove_file(&wal_path);
        let _ = fs::remove_file(&shm_path);
    }

    #[test]
    fn kilocode_parse_extracts_metadata_without_storing_raw_text() {
        let db_path = unique_temp_db_path("kilocode-parse");
        write_fixture_db(&db_path);

        let sessions = KilocodeAdapter.parse(&db_path).expect("parse fixture db");
        let serialized = serde_json::to_string(&sessions).expect("serialize sessions");

        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.source_app, "kilocode");
        assert_eq!(session.external_session_id.as_deref(), Some("ses_test_1"));
        assert_eq!(session.total_input_tokens, 100);
        assert_eq!(session.total_output_tokens, 30);
        assert_eq!(session.requests.len(), 1);
        assert_eq!(session.message_count, 2);
        assert_eq!(session.events.len(), 1);
        assert_eq!(session.requests[0].input_tokens, Some(100));
        assert_eq!(session.requests[0].output_tokens, Some(30));
        assert_eq!(session.requests[0].total_tokens, Some(140));
        assert_eq!(session.requests[0].cache_read_input_tokens, Some(10));
        assert_eq!(session.events[0].delta_total, 140);
        assert!(!serialized.contains("hello kilo"));
        assert!(!serialized.contains("answer text"));
        assert!(!serialized.contains("internal only"));

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn kilocode_groups_multiple_assistant_steps_under_one_user_request() {
        let db_path = unique_temp_db_path("kilocode-grouped-request");
        write_fixture_db(&db_path);
        let conn = Connection::open(&db_path).expect("open fixture db");

        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg_assistant_2",
                "ses_test_1",
                1_700_000_006_000_i64,
                1_700_000_008_000_i64,
                json!({
                    "role": "assistant",
                    "parentID": "msg_user_1",
                    "time": {
                        "created": 1_700_000_006_000_i64,
                        "completed": 1_700_000_008_000_i64
                    },
                    "modelID": "kilo-auto/free",
                    "finish": "stop"
                })
                .to_string()
            ],
        )
        .expect("insert follow-up assistant message");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('prt_assistant_2_text', ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg_assistant_2",
                "ses_test_1",
                1_700_000_006_100_i64,
                1_700_000_006_100_i64,
                json!({ "type": "text", "text": "second answer" }).to_string()
            ],
        )
        .expect("insert follow-up text part");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('prt_assistant_2_finish', ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg_assistant_2",
                "ses_test_1",
                1_700_000_008_000_i64,
                1_700_000_008_000_i64,
                json!({
                    "type": "step-finish",
                    "reason": "stop",
                    "tokens": {
                        "total": 32,
                        "input": 20,
                        "output": 7,
                        "reasoning": 2,
                        "cache": {
                            "read": 3,
                            "write": 0
                        }
                    }
                })
                .to_string()
            ],
        )
        .expect("insert follow-up finish part");
        drop(conn);

        let sessions = KilocodeAdapter
            .parse(&db_path)
            .expect("parse grouped request fixture");
        let session = &sessions[0];

        assert_eq!(session.requests.len(), 1);
        assert_eq!(session.message_count, 3);
        assert_eq!(session.events.len(), 2);
        assert_eq!(session.requests[0].message_count, 3);
        assert_eq!(session.requests[0].input_tokens, Some(120));
        assert_eq!(session.requests[0].output_tokens, Some(39));
        assert_eq!(session.requests[0].total_tokens, Some(172));
        assert_eq!(session.total_input_tokens, 120);
        assert_eq!(session.total_output_tokens, 39);

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn kilocode_estimates_tokens_for_legacy_migrated_sessions_without_usage_events() {
        let db_path = unique_temp_db_path("kilocode-legacy-migrated");
        let conn = Connection::open(&db_path).expect("open fixture db");
        conn.execute_batch(
            "
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                parent_id TEXT,
                slug TEXT NOT NULL,
                directory TEXT NOT NULL,
                title TEXT NOT NULL,
                version TEXT NOT NULL,
                share_url TEXT,
                summary_additions INTEGER,
                summary_deletions INTEGER,
                summary_files INTEGER,
                summary_diffs TEXT,
                revert TEXT,
                permission TEXT,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                time_compacting INTEGER,
                time_archived INTEGER,
                workspace_id TEXT
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            ",
        )
        .expect("create fixture schema");
        conn.execute(
            "INSERT INTO session (
                id, project_id, slug, directory, title, version, time_created, time_updated
             ) VALUES (?1, 'proj_legacy', 'fixture', 'E:/fixture', 'Legacy Session', '1', ?2, ?3)",
            rusqlite::params![
                "ses_migrated_fixture",
                1_700_100_000_000_i64,
                1_700_100_010_000_i64
            ],
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg_user_legacy",
                "ses_migrated_fixture",
                1_700_100_001_000_i64,
                1_700_100_001_000_i64,
                json!({
                    "role": "user",
                    "time": { "created": 1_700_100_001_000_i64 }
                })
                .to_string()
            ],
        )
        .expect("insert legacy user message");
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg_assistant_legacy",
                "ses_migrated_fixture",
                1_700_100_002_000_i64,
                1_700_100_004_000_i64,
                json!({
                    "role": "assistant",
                    "parentID": "msg_user_legacy",
                    "modelID": "legacy",
                    "providerID": "legacy",
                    "tokens": {
                        "input": 0,
                        "output": 0,
                        "reasoning": 0,
                        "cache": {
                            "read": 0,
                            "write": 0
                        }
                    },
                    "time": {
                        "created": 1_700_100_002_000_i64,
                        "completed": 1_700_100_004_000_i64
                    }
                })
                .to_string()
            ],
        )
        .expect("insert legacy assistant message");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('prt_user_legacy_text', ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg_user_legacy",
                "ses_migrated_fixture",
                1_700_100_001_100_i64,
                1_700_100_001_100_i64,
                json!({ "type": "text", "text": "查看当前项目" }).to_string()
            ],
        )
        .expect("insert legacy user text part");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('prt_assistant_legacy_text', ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg_assistant_legacy",
                "ses_migrated_fixture",
                1_700_100_002_100_i64,
                1_700_100_002_100_i64,
                json!({ "type": "text", "text": "当前项目概览如下" }).to_string()
            ],
        )
        .expect("insert legacy assistant text part");
        drop(conn);

        let sessions = KilocodeAdapter
            .parse(&db_path)
            .expect("parse legacy migrated fixture");
        let session = &sessions[0];

        assert_eq!(session.requests.len(), 1);
        assert_eq!(session.events.len(), 1);
        assert!(session.total_input_tokens > 0);
        assert!(session.total_output_tokens > 0);
        assert_eq!(session.requests[0].token_confidence.as_deref(), Some("low"));
        assert_eq!(session.events[0].confidence, "low");

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn kilocode_estimates_tokens_from_legacy_tool_parts_without_text() {
        let db_path = unique_temp_db_path("kilocode-legacy-tool-part");
        let conn = Connection::open(&db_path).expect("open fixture db");
        conn.execute_batch(
            "
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                parent_id TEXT,
                slug TEXT NOT NULL,
                directory TEXT NOT NULL,
                title TEXT NOT NULL,
                version TEXT NOT NULL,
                share_url TEXT,
                summary_additions INTEGER,
                summary_deletions INTEGER,
                summary_files INTEGER,
                summary_diffs TEXT,
                revert TEXT,
                permission TEXT,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                time_compacting INTEGER,
                time_archived INTEGER,
                workspace_id TEXT
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            ",
        )
        .expect("create fixture schema");
        conn.execute(
            "INSERT INTO session (
                id, project_id, slug, directory, title, version, time_created, time_updated
             ) VALUES (?1, 'proj_legacy', 'fixture', 'E:/fixture', 'Legacy Tool Session', '1', ?2, ?3)",
            rusqlite::params![
                "ses_migrated_tool_fixture",
                1_700_200_000_000_i64,
                1_700_200_010_000_i64
            ],
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg_user_tool_legacy",
                "ses_migrated_tool_fixture",
                1_700_200_001_000_i64,
                1_700_200_001_000_i64,
                json!({
                    "role": "user",
                    "time": { "created": 1_700_200_001_000_i64 }
                })
                .to_string()
            ],
        )
        .expect("insert legacy user message");
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg_assistant_tool_legacy",
                "ses_migrated_tool_fixture",
                1_700_200_002_000_i64,
                1_700_200_004_000_i64,
                json!({
                    "role": "assistant",
                    "parentID": "msg_user_tool_legacy",
                    "modelID": "legacy",
                    "providerID": "legacy",
                    "tokens": {
                        "input": 0,
                        "output": 0,
                        "reasoning": 0,
                        "cache": {
                            "read": 0,
                            "write": 0
                        }
                    },
                    "time": {
                        "created": 1_700_200_002_000_i64,
                        "completed": 1_700_200_004_000_i64
                    }
                })
                .to_string()
            ],
        )
        .expect("insert legacy assistant message");
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES ('prt_user_legacy_tool', ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                "msg_user_tool_legacy",
                "ses_migrated_tool_fixture",
                1_700_200_001_100_i64,
                1_700_200_001_100_i64,
                json!({
                    "type": "tool",
                    "tool": "read_file",
                    "state": {
                        "status": "completed",
                        "input": {
                            "path": "src/main.ts",
                            "lineRanges": [{ "start": 1, "end": 120 }]
                        },
                        "output": "export const main = () => console.log('hello');"
                    }
                })
                .to_string()
            ],
        )
        .expect("insert legacy user tool part");
        drop(conn);

        let sessions = KilocodeAdapter
            .parse(&db_path)
            .expect("parse legacy tool fixture");
        let session = &sessions[0];

        assert_eq!(session.requests.len(), 1);
        assert_eq!(session.events.len(), 1);
        assert!(session.total_input_tokens > 0);
        assert_eq!(session.total_output_tokens, 0);
        assert_eq!(session.requests[0].token_confidence.as_deref(), Some("low"));
        assert!(session.requests[0].input_tokens.unwrap_or(0) > 0);
        assert_eq!(session.events[0].confidence, "low");

        let _ = fs::remove_file(&db_path);
    }
}
