use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::message_stream::{MessageStreamAggregator, MessageStreamItem, MessageTokenUsage};
use super::{NormalizedSession, SourceAdapter};
use crate::error::{AppError, AppResult};
use crate::utils::sqlite::SqliteSnapshot;
use crate::utils::{hash, time};

pub struct OpencodeAdapter;

#[derive(Debug, Serialize, Deserialize)]
struct OpencodeMessageLocator {
    message_id: String,
}

#[derive(Debug, Clone)]
struct SessionRow {
    id: String,
    title: Option<String>,
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

#[derive(Debug, Clone, Copy, Default)]
struct OpencodeTokenUsage {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_input_tokens: i64,
    cache_write_input_tokens: i64,
}

impl From<OpencodeTokenUsage> for MessageTokenUsage {
    fn from(value: OpencodeTokenUsage) -> Self {
        Self {
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            total_tokens: value.input_tokens + value.output_tokens,
            cache_read_input_tokens: value.cache_read_input_tokens,
            cache_write_input_tokens: value.cache_write_input_tokens,
        }
    }
}

impl SourceAdapter for OpencodeAdapter {
    fn name(&self) -> &str {
        "opencode"
    }

    fn parser_version(&self) -> i64 {
        2
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("opencode.db"))
    }

    fn discover_paths(&self, root_path: &Path) -> AppResult<Vec<PathBuf>> {
        if root_path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("opencode.db"))
        {
            return Ok(self
                .can_handle(root_path)
                .then(|| root_path.to_path_buf())
                .into_iter()
                .collect());
        }

        let candidate = root_path.join("opencode.db");
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

        sessions
            .into_iter()
            .map(|session| build_normalized_session(self.name(), session, &messages_by_session))
            .collect()
    }

    fn fingerprint_paths(&self, path: &Path) -> Vec<PathBuf> {
        let mut paths = vec![path.to_path_buf()];
        let wal_path = PathBuf::from(format!("{}-wal", path.to_string_lossy()));
        if wal_path
            .metadata()
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false)
        {
            paths.push(wal_path);
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

fn build_normalized_session(
    source_app: &str,
    session: SessionRow,
    messages_by_session: &HashMap<String, Vec<MessageRow>>,
) -> AppResult<NormalizedSession> {
    #[derive(Debug, Clone)]
    struct OpencodeParsedTurn {
        id: String,
        role: String,
        parent_id: Option<String>,
        finish: Option<String>,
        model: Option<String>,
        usage: Option<OpencodeTokenUsage>,
        created_at: Option<chrono::DateTime<chrono::Utc>>,
        updated_at: Option<chrono::DateTime<chrono::Utc>>,
        usage_event_time_utc: Option<chrono::DateTime<chrono::Utc>>,
        source_locator: String,
    }

    let messages = messages_by_session
        .get(&session.id)
        .cloned()
        .unwrap_or_default();

    let mut parsed_messages = Vec::<OpencodeParsedTurn>::new();
    let mut model_first = None;
    let mut model_last = None;

    let mut checksum_parts = vec![
        session.id.clone(),
        session.title.clone().unwrap_or_default(),
        session.time_created.to_string(),
        session.time_updated.to_string(),
    ];

    for (index, message) in messages.iter().enumerate() {
        checksum_parts.push(message.id.clone());
        checksum_parts.push(message.time_created.to_string());
        checksum_parts.push(message.time_updated.to_string());
        checksum_parts.push(message.data.clone());

        let payload: Value = serde_json::from_str(&message.data)?;
        let model = extract_model_name(&payload);
        let role = payload
            .get("role")
            .and_then(Value::as_str)
            .map(normalize_role)
            .unwrap_or_else(|| "unknown".to_string());
        let parent_id = payload
            .get("parentID")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let finish = payload
            .get("finish")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let usage = extract_token_usage(&payload);
        let source_locator = serde_json::to_string(&OpencodeMessageLocator {
            message_id: message.id.clone(),
        })?;
        let created_at = time::from_unix_ms(message.time_created);
        let updated_at = time::from_unix_ms(message.time_updated);

        if model_first.is_none() {
            model_first = model.clone();
        }
        if model.is_some() {
            model_last = model.clone();
        }

        let usage_event_time_utc = if role == "assistant" && usage.is_some() {
            Some(
                payload
                    .get("time")
                    .and_then(|time_node| time_node.get("completed"))
                    .and_then(Value::as_i64)
                    .or_else(|| {
                        payload
                            .get("time")
                            .and_then(|time_node| time_node.get("created"))
                            .and_then(Value::as_i64)
                    })
                    .or(Some(message.time_created))
                    .and_then(time::from_unix_ms)
                    .ok_or_else(|| {
                        AppError::validation(format!(
                            "Invalid opencode message time: {}",
                            message.id
                        ))
                    })?,
            )
        } else {
            None
        };

        let _ = index;

        parsed_messages.push(OpencodeParsedTurn {
            id: message.id.clone(),
            role,
            parent_id,
            finish,
            model,
            usage,
            created_at,
            updated_at,
            usage_event_time_utc,
            source_locator,
        });
    }

    let stream_items = parsed_messages
        .into_iter()
        .map(|message| MessageStreamItem {
            source_id: message.id,
            role: message.role,
            request_id: None,
            parent_id: message.parent_id,
            status: message.finish,
            model: message.model,
            usage: message.usage.map(Into::into),
            count_as_message: true,
            source_created_at: message.created_at,
            source_updated_at: message.updated_at,
            usage_event_time_utc: message.usage_event_time_utc,
            source_event_id: None,
            usage_event_granularity: None,
            usage_event_confidence: None,
            source_locator: message.source_locator,
            use_as_request_locator: false,
        })
        .collect();
    let aggregate = MessageStreamAggregator::new(stream_items).aggregate_parent_child_requests();

    Ok(NormalizedSession {
        source_app: source_app.to_string(),
        external_session_id: Some(session.id.clone()),
        session_key: format!("{source_app}:{}", session.id),
        title: normalize_optional_text(session.title),
        model_first,
        model_last,
        source_created_at: time::from_unix_ms(session.time_created),
        source_updated_at: time::from_unix_ms(session.time_updated),
        total_input_tokens: aggregate.total_input_tokens,
        total_output_tokens: aggregate.total_output_tokens,
        message_count: messages.len() as i64,
        conversation_checksum: hash::sha256_text(&checksum_parts.join("\n")),
        requests: aggregate.requests,
        events: aggregate.events,
    })
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

fn extract_token_usage(payload: &Value) -> Option<OpencodeTokenUsage> {
    let tokens = payload.get("tokens")?;
    let raw_input_tokens = tokens.get("input").and_then(Value::as_i64).unwrap_or(0);
    let output_tokens = tokens.get("output").and_then(Value::as_i64).unwrap_or(0);
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
        || output_tokens > 0
        || cache_read_input_tokens > 0
        || cache_write_input_tokens > 0;
    if !has_any_usage {
        return None;
    }

    Some(OpencodeTokenUsage {
        input_tokens: raw_input_tokens + cache_read_input_tokens + cache_write_input_tokens,
        output_tokens,
        cache_read_input_tokens,
        cache_write_input_tokens,
    })
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

#[cfg(test)]
mod tests {
    use super::{extract_token_usage, OpencodeAdapter};
    use crate::sources::SourceAdapter;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn opencode_usage_includes_cache_tokens_in_effective_input() {
        let payload = json!({
            "tokens": {
                "input": 8999,
                "output": 12,
                "cache": {
                    "read": 1821,
                    "write": 7
                }
            }
        });

        let usage = extract_token_usage(&payload).expect("usage should be parsed");
        assert_eq!(usage.input_tokens, 10_827);
        assert_eq!(usage.output_tokens, 12);
        assert_eq!(usage.cache_read_input_tokens, 1_821);
        assert_eq!(usage.cache_write_input_tokens, 7);
    }

    #[test]
    fn opencode_fingerprint_ignores_shm_and_empty_wal() {
        let temp_root = std::env::temp_dir().join(format!(
            "totoken-opencode-fingerprint-{}",
            crate::utils::ids::new_uuid()
        ));
        fs::create_dir_all(&temp_root).expect("create temp dir");

        let db_path = temp_root.join("opencode.db");
        let wal_path = PathBuf::from(format!("{}-wal", db_path.to_string_lossy()));
        let shm_path = PathBuf::from(format!("{}-shm", db_path.to_string_lossy()));

        fs::write(&db_path, b"db").expect("write db");
        fs::write(&wal_path, b"").expect("write empty wal");
        fs::write(&shm_path, b"shm").expect("write shm");

        let adapter = OpencodeAdapter;
        assert_eq!(adapter.fingerprint_paths(&db_path), vec![db_path.clone()]);

        fs::write(&wal_path, b"wal").expect("write wal");
        assert_eq!(
            adapter.fingerprint_paths(&db_path),
            vec![db_path.clone(), wal_path.clone()]
        );

        let _ = fs::remove_file(&db_path);
        let _ = fs::remove_file(&wal_path);
        let _ = fs::remove_file(&shm_path);
        let _ = fs::remove_dir(&temp_root);
    }
}
