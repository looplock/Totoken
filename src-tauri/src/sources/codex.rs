use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::error::AppResult;
use crate::utils::cache::BoundedCache;
use crate::utils::{hash, time};

use super::message_stream::{MessageStreamAggregator, MessageStreamItem, MessageTokenUsage};
use super::{NormalizedSession, SourceAdapter};

const SESSION_INDEX_CACHE_CAPACITY: usize = 128;

#[derive(Clone)]
pub struct CodexAdapter {
    session_index_cache: Arc<Mutex<BoundedCache<String, CachedSessionIndex>>>,
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self {
            session_index_cache: Arc::new(Mutex::new(BoundedCache::new(
                SESSION_INDEX_CACHE_CAPACITY,
            ))),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CodexMessageLocator {
    line_number: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TokenSnapshot {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_input_tokens: i64,
    cache_write_input_tokens: i64,
}

#[derive(Debug, Clone)]
struct CachedSessionIndex {
    modified_at: Option<SystemTime>,
    titles: HashMap<String, String>,
}

#[derive(Debug, Default)]
struct ParsedCodexSession {
    external_session_id: Option<String>,
    title: Option<String>,
    model_first: Option<String>,
    model_last: Option<String>,
    source_created_at: Option<DateTime<Utc>>,
    source_updated_at: Option<DateTime<Utc>>,
    message_count: i64,
    previous_snapshot: Option<TokenSnapshot>,
    pending_turn_id: Option<String>,
    stream_items: Vec<MessageStreamItem>,
    current_request_id: Option<String>,
    next_request_sequence_no: i64,
}

impl SourceAdapter for CodexAdapter {
    fn name(&self) -> &str {
        "codex"
    }

    fn parser_version(&self) -> i64 {
        2
    }

    fn can_handle(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("jsonl"))
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.to_ascii_lowercase().starts_with("rollout-"))
    }

    fn fingerprint_paths(&self, path: &Path) -> Vec<std::path::PathBuf> {
        vec![path.to_path_buf()]
    }

    fn parse(&self, path: &Path) -> AppResult<Vec<NormalizedSession>> {
        let content = fs::read_to_string(path)?;
        let checksum = hash::sha256_text(&content);
        let session_index_titles = self.load_session_index_titles(path);
        let mut parsed = ParsedCodexSession {
            title: extract_session_index_title(&content, &session_index_titles),
            ..ParsedCodexSession::default()
        };

        for (line_index, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let record: Value = serde_json::from_str(trimmed)?;
            let timestamp = record
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_rfc3339_utc);

            parsed.consume_record(self.name(), &record, timestamp, line_index + 1);
        }

        let aggregate = parsed.build_message_stream_aggregation();

        let session_key = parsed
            .external_session_id
            .as_ref()
            .map(|value| format!("codex:{value}"))
            .unwrap_or_else(|| format!("codex:file:{checksum}"));

        Ok(vec![NormalizedSession {
            source_app: self.name().to_string(),
            external_session_id: parsed.external_session_id,
            session_key,
            title: parsed.title,
            model_first: parsed.model_first,
            model_last: parsed.model_last,
            source_created_at: parsed.source_created_at,
            source_updated_at: parsed.source_updated_at,
            total_input_tokens: aggregate.total_input_tokens,
            total_output_tokens: aggregate.total_output_tokens,
            message_count: parsed.message_count,
            conversation_checksum: checksum,
            requests: aggregate.requests,
            events: aggregate.events,
        }])
    }
}

impl CodexAdapter {
    fn load_session_index_titles(&self, path: &Path) -> HashMap<String, String> {
        load_session_index_titles(path, &self.session_index_cache)
    }
}

impl ParsedCodexSession {
    fn consume_record(
        &mut self,
        source_app: &str,
        record: &Value,
        timestamp: Option<DateTime<Utc>>,
        line_number: usize,
    ) {
        self.track_timestamp(timestamp);

        match record.get("type").and_then(Value::as_str) {
            Some("session_meta") => self.consume_session_meta(record),
            Some("turn_context") => self.consume_turn_context(record),
            Some("event_msg") => {
                self.consume_event_message(source_app, record, timestamp, line_number)
            }
            Some("response_item") => self.consume_response_item(record),
            _ => {}
        }
    }

    fn consume_session_meta(&mut self, record: &Value) {
        let payload = record.get("payload").unwrap_or(&Value::Null);

        if self.external_session_id.is_none() {
            self.external_session_id = payload
                .get("id")
                .and_then(Value::as_str)
                .map(ToString::to_string);
        }

        if self.source_created_at.is_none() {
            self.source_created_at = payload
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_rfc3339_utc);
        }

        self.capture_model(payload.get("model").and_then(Value::as_str));
    }

    fn consume_turn_context(&mut self, record: &Value) {
        let payload = record.get("payload").unwrap_or(&Value::Null);
        if self.pending_turn_id.is_none() {
            self.pending_turn_id = payload
                .get("turn_id")
                .and_then(Value::as_str)
                .map(ToString::to_string);
        }
        self.capture_model(payload.get("model").and_then(Value::as_str));
    }

    fn consume_event_message(
        &mut self,
        source_app: &str,
        record: &Value,
        timestamp: Option<DateTime<Utc>>,
        line_number: usize,
    ) {
        let payload = record.get("payload").unwrap_or(&Value::Null);

        match payload.get("type").and_then(Value::as_str) {
            Some("task_started") => {
                self.pending_turn_id = payload
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
            }
            Some("user_message") => {
                self.start_request(timestamp, line_number);
                self.message_count += 1;
                self.push_message(
                    "user",
                    payload.get("message").and_then(Value::as_str),
                    timestamp,
                    line_number,
                );
                if self.title.is_none() {
                    self.title = payload
                        .get("message")
                        .and_then(Value::as_str)
                        .and_then(select_title_candidate);
                }
            }
            Some("agent_message") => {
                self.ensure_open_request(timestamp, line_number);
                self.message_count += 1;
                self.push_message(
                    "assistant",
                    payload.get("message").and_then(Value::as_str),
                    timestamp,
                    line_number,
                );
            }
            Some("token_count") => {
                self.consume_token_count(source_app, payload, timestamp, line_number);
            }
            Some("turn_aborted") => {
                let turn_id = payload.get("turn_id").and_then(Value::as_str);
                if self
                    .current_request_id
                    .as_ref()
                    .map(|request_id| Some(request_id.as_str()) == turn_id)
                    .unwrap_or(false)
                {
                    self.push_request_status_item("interrupted", timestamp, line_number);
                }
            }
            _ => {}
        }
    }

    fn consume_response_item(&mut self, record: &Value) {
        let payload = record.get("payload").unwrap_or(&Value::Null);
        let role = payload.get("role").and_then(Value::as_str);
        if payload.get("type").and_then(Value::as_str) != Some("message") {
            return;
        }

        if self.title.is_none() && role == Some("user") {
            self.title =
                extract_message_text(payload).and_then(|text| select_title_candidate(&text));
        }
    }

    fn consume_token_count(
        &mut self,
        source_app: &str,
        payload: &Value,
        timestamp: Option<DateTime<Utc>>,
        line_number: usize,
    ) {
        let info = payload.get("info").unwrap_or(&Value::Null);
        let current_snapshot = info
            .get("total_token_usage")
            .and_then(extract_token_snapshot);
        let last_usage_delta = info
            .get("last_token_usage")
            .and_then(extract_token_snapshot);

        self.capture_model(info.get("model").and_then(Value::as_str));

        let is_zero_snapshot = |snapshot: TokenSnapshot| {
            snapshot.input_tokens == 0
                && snapshot.output_tokens == 0
                && snapshot.cache_read_input_tokens == 0
                && snapshot.cache_write_input_tokens == 0
        };
        let usage = if let Some(current) = current_snapshot {
            let previous = self.previous_snapshot;
            self.previous_snapshot = Some(current);

            if let Some(previous) = previous {
                if current.input_tokens < previous.input_tokens
                    || current.output_tokens < previous.output_tokens
                    || current.cache_read_input_tokens < previous.cache_read_input_tokens
                    || current.cache_write_input_tokens < previous.cache_write_input_tokens
                {
                    if let Some(last_usage) = last_usage_delta {
                        Some((last_usage, true, false))
                    } else if is_zero_snapshot(current) {
                        None
                    } else {
                        Some((current, false, false))
                    }
                } else {
                    let snapshot_delta = TokenSnapshot {
                        input_tokens: current.input_tokens - previous.input_tokens,
                        output_tokens: current.output_tokens - previous.output_tokens,
                        cache_read_input_tokens: current.cache_read_input_tokens
                            - previous.cache_read_input_tokens,
                        cache_write_input_tokens: current.cache_write_input_tokens
                            - previous.cache_write_input_tokens,
                    };

                    if is_zero_snapshot(snapshot_delta) {
                        None
                    } else {
                        let matches_direct_delta = last_usage_delta
                            .map(|last_usage| last_usage == snapshot_delta)
                            .unwrap_or(false);
                        Some((snapshot_delta, matches_direct_delta, true))
                    }
                }
            } else if let Some(last_usage) = last_usage_delta {
                Some((last_usage, true, false))
            } else if is_zero_snapshot(current) {
                None
            } else {
                Some((current, false, true))
            }
        } else {
            last_usage_delta.map(|last_usage| (last_usage, true, false))
        };

        let Some((usage_delta, used_direct_delta, derived_from_snapshot)) = usage else {
            return;
        };

        if usage_delta.input_tokens == 0
            && usage_delta.output_tokens == 0
            && usage_delta.cache_read_input_tokens == 0
            && usage_delta.cache_write_input_tokens == 0
        {
            return;
        }

        let event_time_utc = timestamp
            .or(self.source_updated_at)
            .or(self.source_created_at)
            .unwrap_or_else(time::now_utc);
        let source_event_id = Some(format!(
            "{}:token_count:{}",
            self.external_session_id.as_deref().unwrap_or(source_app),
            line_number
        ));

        let request_id = self.current_request_id.clone();
        self.stream_items.push(MessageStreamItem {
            source_id: source_event_id
                .clone()
                .unwrap_or_else(|| format!("codex:token_count:{line_number}")),
            role: "assistant".to_string(),
            request_id,
            parent_id: None,
            status: None,
            model: self.model_last.clone().or_else(|| self.model_first.clone()),
            usage: Some(MessageTokenUsage {
                input_tokens: usage_delta.input_tokens,
                output_tokens: usage_delta.output_tokens,
                total_tokens: usage_delta.input_tokens + usage_delta.output_tokens,
                cache_read_input_tokens: usage_delta.cache_read_input_tokens,
                cache_write_input_tokens: usage_delta.cache_write_input_tokens,
            }),
            count_as_message: false,
            source_created_at: Some(event_time_utc),
            source_updated_at: Some(event_time_utc),
            usage_event_time_utc: Some(event_time_utc),
            source_event_id,
            usage_event_granularity: Some(
                if used_direct_delta {
                    "request"
                } else if derived_from_snapshot {
                    "snapshot_delta"
                } else {
                    "session_total"
                }
                .to_string(),
            ),
            usage_event_confidence: Some(
                if used_direct_delta { "high" } else { "medium" }.to_string(),
            ),
            source_locator: serialize_line_locator(line_number),
            use_as_request_locator: false,
        });
    }

    fn capture_model(&mut self, model: Option<&str>) {
        let Some(model) = normalize_optional_text(model) else {
            return;
        };

        if self.model_first.is_none() {
            self.model_first = Some(model.clone());
        }
        self.model_last = Some(model);
    }

    fn track_timestamp(&mut self, timestamp: Option<DateTime<Utc>>) {
        let Some(timestamp) = timestamp else {
            return;
        };

        if self.source_created_at.is_none() {
            self.source_created_at = Some(timestamp);
        }

        match self.source_updated_at {
            Some(current) if current >= timestamp => {}
            _ => self.source_updated_at = Some(timestamp),
        }
    }

    fn push_message(
        &mut self,
        role: &str,
        body: Option<&str>,
        timestamp: Option<DateTime<Utc>>,
        line_number: usize,
    ) {
        let normalized_body = normalize_optional_text(body);
        let request_id = self.current_request_id.clone();
        self.stream_items.push(MessageStreamItem {
            source_id: request_id
                .clone()
                .unwrap_or_else(|| format!("codex:message:{line_number}")),
            role: role.to_string(),
            request_id,
            parent_id: None,
            status: None,
            model: self.model_last.clone().or_else(|| self.model_first.clone()),
            usage: None,
            count_as_message: true,
            source_created_at: timestamp,
            source_updated_at: timestamp,
            usage_event_time_utc: None,
            source_event_id: None,
            usage_event_granularity: None,
            usage_event_confidence: None,
            source_locator: serialize_line_locator(line_number),
            use_as_request_locator: false,
        });

        let _ = normalized_body;
    }

    fn start_request(&mut self, timestamp: Option<DateTime<Utc>>, line_number: usize) {
        self.current_request_id = None;
        self.ensure_open_request(timestamp, line_number);
    }

    fn ensure_open_request(&mut self, timestamp: Option<DateTime<Utc>>, line_number: usize) {
        if self.current_request_id.is_some() {
            return;
        }

        self.next_request_sequence_no += 1;
        let source_request_id = self.pending_turn_id.clone().unwrap_or_else(|| {
            format!(
                "{}:request:{}",
                self.external_session_id.as_deref().unwrap_or("codex"),
                self.next_request_sequence_no
            )
        });

        self.current_request_id = Some(source_request_id);
        let _ = (timestamp, line_number);
        self.pending_turn_id = None;
    }

    fn push_request_status_item(
        &mut self,
        status: &str,
        timestamp: Option<DateTime<Utc>>,
        line_number: usize,
    ) {
        let Some(request_id) = self.current_request_id.clone() else {
            return;
        };

        self.stream_items.push(MessageStreamItem {
            source_id: format!("{request_id}:status:{line_number}"),
            role: "assistant".to_string(),
            request_id: Some(request_id),
            parent_id: None,
            status: Some(status.to_string()),
            model: self.model_last.clone().or_else(|| self.model_first.clone()),
            usage: None,
            count_as_message: false,
            source_created_at: timestamp,
            source_updated_at: timestamp,
            usage_event_time_utc: None,
            source_event_id: None,
            usage_event_granularity: None,
            usage_event_confidence: None,
            source_locator: serialize_line_locator(line_number),
            use_as_request_locator: false,
        });
    }

    fn build_message_stream_aggregation(&self) -> super::message_stream::MessageStreamAggregation {
        let mut aggregate = MessageStreamAggregator::new(self.stream_items.clone())
            .aggregate_sequential_user_requests_with_item_events("codex-request");
        for request in &mut aggregate.requests {
            request.token_confidence = Some("medium".to_string());
            request.input_tokens = Some(request.input_tokens.unwrap_or(0));
            request.output_tokens = Some(request.output_tokens.unwrap_or(0));
            request.total_tokens = Some(request.total_tokens.unwrap_or(0));
            request.cache_read_input_tokens = Some(request.cache_read_input_tokens.unwrap_or(0));
            request.cache_write_input_tokens = Some(request.cache_write_input_tokens.unwrap_or(0));
        }
        aggregate
    }
}

fn parse_rfc3339_utc(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn serialize_line_locator(line_number: usize) -> String {
    serde_json::to_string(&CodexMessageLocator { line_number })
        .unwrap_or_else(|_| format!("{{\"lineNumber\":{line_number}}}"))
}

fn extract_token_snapshot(value: &Value) -> Option<TokenSnapshot> {
    let has_any_usage_field = value.get("input_tokens").is_some()
        || value.get("output_tokens").is_some()
        || value.get("cached_input_tokens").is_some()
        || value.get("cache_read_input_tokens").is_some()
        || value.get("cache_creation_input_tokens").is_some()
        || value.get("cache_write_input_tokens").is_some();
    if !has_any_usage_field {
        return None;
    }

    let input_tokens = value
        .get("input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = value
        .get("output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cache_read_input_tokens = value
        .get("cached_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| value.get("cache_read_input_tokens").and_then(Value::as_i64))
        .unwrap_or(0);
    let cache_write_input_tokens = value
        .get("cache_creation_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            value
                .get("cache_write_input_tokens")
                .and_then(Value::as_i64)
        })
        .unwrap_or(0);

    Some(TokenSnapshot {
        input_tokens,
        output_tokens,
        cache_read_input_tokens,
        cache_write_input_tokens,
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
    if is_noise_title(&normalized) {
        return None;
    }

    Some(normalized)
}

fn is_noise_title(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }

    let lowercase = trimmed.to_ascii_lowercase();
    lowercase.starts_with("<environment_context>")
        || lowercase.starts_with("<turn_aborted>")
        || lowercase.starts_with("<image")
        || lowercase.starts_with("<permissions instructions>")
        || lowercase.starts_with("<agents")
        || lowercase.starts_with("# agents.md instructions for ")
}

fn extract_message_text(payload: &Value) -> Option<String> {
    let items = payload.get("content")?.as_array()?;
    let mut parts = Vec::new();

    for item in items {
        let text = item
            .get("text")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty());

        if let Some(text) = text {
            parts.push(text.trim().to_string());
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn resolve_session_index_path(path: &Path) -> Option<std::path::PathBuf> {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case(".codex"))
        {
            return Some(dir.join("session_index.jsonl"));
        }
        current = dir.parent();
    }

    None
}

fn load_session_index_titles(
    path: &Path,
    cache: &Mutex<BoundedCache<String, CachedSessionIndex>>,
) -> HashMap<String, String> {
    let Some(session_index_path) = resolve_session_index_path(path) else {
        return HashMap::new();
    };

    let metadata = fs::metadata(&session_index_path).ok();
    let modified_at = metadata.as_ref().and_then(|value| value.modified().ok());
    let cache_key = session_index_path.to_string_lossy().to_string();

    {
        let mut guard = cache.lock().ok();
        if let Some(guard) = guard.as_mut() {
            if let Some(entry) = guard.get_cloned(&cache_key) {
                if entry.modified_at == modified_at {
                    return entry.titles;
                }
            }
        }
    }

    let content = fs::read_to_string(&session_index_path).ok();
    let titles = content
        .as_deref()
        .map(parse_session_index_titles)
        .unwrap_or_default();

    if let Ok(mut guard) = cache.lock() {
        guard.insert(
            cache_key,
            CachedSessionIndex {
                modified_at,
                titles: titles.clone(),
            },
        );
    }

    titles
}

fn parse_session_index_titles(content: &str) -> HashMap<String, String> {
    let mut titles = HashMap::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Ok(record) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        let Some(session_id) = record.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(title) = record
            .get("thread_name")
            .and_then(Value::as_str)
            .and_then(select_title_candidate)
        else {
            continue;
        };

        titles.insert(session_id.to_string(), title);
    }

    titles
}

fn extract_session_index_title(content: &str, titles: &HashMap<String, String>) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Ok(record) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        if record.get("type").and_then(Value::as_str) != Some("session_meta") {
            continue;
        }

        let Some(session_id) = record
            .get("payload")
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str)
        else {
            continue;
        };

        return titles.get(session_id).cloned();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::cache::BoundedCache;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn write_test_rollout(content: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let unique = format!(
            "rollout-codex-test-{}.jsonl",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        path.push(unique);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn ignores_replayed_turn_token_count_when_snapshot_does_not_advance() {
        let content = r#"{"timestamp":"2026-04-07T13:29:50.000Z","type":"session_meta","payload":{"id":"session-1","timestamp":"2026-04-07T13:29:50.000Z","model":"gpt-5"}}
{"timestamp":"2026-04-07T13:29:51.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}
{"timestamp":"2026-04-07T13:29:52.000Z","type":"event_msg","payload":{"type":"user_message","message":"first request","images":[],"local_images":[],"text_elements":[]}}
{"timestamp":"2026-04-07T13:29:53.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":12039,"cached_input_tokens":10624,"output_tokens":318},"last_token_usage":{"input_tokens":12039,"cached_input_tokens":10624,"output_tokens":318}}}}
{"timestamp":"2026-04-07T13:30:05.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":24600,"cached_input_tokens":22912,"output_tokens":426},"last_token_usage":{"input_tokens":12561,"cached_input_tokens":12288,"output_tokens":108}}}}
{"timestamp":"2026-04-07T13:30:18.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":37327,"cached_input_tokens":35456,"output_tokens":729},"last_token_usage":{"input_tokens":12727,"cached_input_tokens":12544,"output_tokens":303}}}}
{"timestamp":"2026-04-07T13:37:19.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-2"}}
{"timestamp":"2026-04-07T13:37:22.000Z","type":"event_msg","payload":{"type":"user_message","message":"second request","images":[],"local_images":[],"text_elements":[]}}
{"timestamp":"2026-04-07T13:37:23.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":37327,"cached_input_tokens":35456,"output_tokens":729},"last_token_usage":{"input_tokens":12727,"cached_input_tokens":12544,"output_tokens":303}}}}
{"timestamp":"2026-04-07T13:37:29.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":50190,"cached_input_tokens":46080,"output_tokens":986},"last_token_usage":{"input_tokens":12863,"cached_input_tokens":10624,"output_tokens":257}}}}
{"timestamp":"2026-04-07T13:38:17.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":63480,"cached_input_tokens":56704,"output_tokens":3127},"last_token_usage":{"input_tokens":13290,"cached_input_tokens":10624,"output_tokens":2141}}}}"#;
        let path = write_test_rollout(content);

        let parsed = CodexAdapter::default().parse(&path).unwrap();
        fs::remove_file(&path).unwrap();

        let session = &parsed[0];
        assert_eq!(session.requests.len(), 2);
        assert_eq!(session.total_input_tokens, 63_480);
        assert_eq!(session.total_output_tokens, 3_127);

        let first_request = &session.requests[0];
        assert_eq!(first_request.input_tokens, Some(37_327));
        assert_eq!(first_request.output_tokens, Some(729));
        assert_eq!(first_request.cache_read_input_tokens, Some(35_456));

        let second_request = &session.requests[1];
        assert_eq!(second_request.input_tokens, Some(26_153));
        assert_eq!(second_request.output_tokens, Some(2_398));
        assert_eq!(second_request.cache_read_input_tokens, Some(21_248));
    }

    #[test]
    fn session_index_cache_is_bounded() {
        let mut cache = BoundedCache::new(SESSION_INDEX_CACHE_CAPACITY);

        for index in 0..(SESSION_INDEX_CACHE_CAPACITY + 1) {
            cache.insert(
                format!("session-index-{index}"),
                CachedSessionIndex {
                    modified_at: None,
                    titles: HashMap::new(),
                },
            );
        }

        assert_eq!(cache.len(), SESSION_INDEX_CACHE_CAPACITY);
    }
}
