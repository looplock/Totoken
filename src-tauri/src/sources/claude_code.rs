use super::message_stream::{MessageStreamAggregator, MessageStreamItem, MessageTokenUsage};
use super::{NormalizedSession, SourceAdapter};
use crate::error::{AppError, AppResult};
use crate::utils::{hash, time};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ClaudeCodeAdapter;

#[derive(Debug, Serialize, Deserialize)]
struct ClaudeMessageLocator {
    line_number: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct ClaudeUsage {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_input_tokens: i64,
    cache_write_input_tokens: i64,
}

impl ClaudeUsage {
    fn effective_input_tokens(self) -> i64 {
        self.input_tokens + self.cache_read_input_tokens + self.cache_write_input_tokens
    }

    fn total_tokens(self) -> i64 {
        self.effective_input_tokens() + self.output_tokens
    }
}

#[derive(Debug)]
struct OpenAssistantMessage {
    logical_message_id: String,
    request_id: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    source_line_number: usize,
    model: Option<String>,
    usage: Option<ClaudeUsage>,
    stop_reason: Option<String>,
    text_parts: Vec<String>,
    tool_names: Vec<String>,
}

#[derive(Debug, Default)]
struct ParsedClaudeSession {
    external_session_id: Option<String>,
    title: Option<String>,
    model_first: Option<String>,
    model_last: Option<String>,
    source_created_at: Option<DateTime<Utc>>,
    source_updated_at: Option<DateTime<Utc>>,
    message_count: i64,
    stream_items: Vec<MessageStreamItem>,
    current_request_id: Option<String>,
    next_request_sequence_no: i64,
    open_assistant_message: Option<OpenAssistantMessage>,
}

impl SourceAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &str {
        "claude_code"
    }

    fn parser_version(&self) -> i64 {
        3
    }

    fn can_handle(&self, path: &Path) -> bool {
        is_claude_session_path(path)
    }

    fn discover_paths(&self, root_path: &Path) -> AppResult<Vec<PathBuf>> {
        let mut paths = Vec::new();
        if !root_path.is_dir() {
            return Ok(paths);
        }

        for project_dir in fs::read_dir(root_path)? {
            let project_dir = project_dir?;
            if !project_dir.file_type()?.is_dir() {
                continue;
            }

            for entry in fs::read_dir(project_dir.path())? {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }

                let path = entry.path();
                if self.can_handle(&path) {
                    paths.push(path);
                }
            }
        }

        Ok(paths)
    }

    fn parse(&self, path: &Path) -> AppResult<Vec<NormalizedSession>> {
        if !is_claude_session_path(path) {
            return Err(AppError::validation(
                "File is outside the Claude Code projects session directory",
            ));
        }

        let content = fs::read_to_string(path)?;
        let checksum = hash::sha256_text(&content);
        let mut parsed = ParsedClaudeSession::default();

        for (line_index, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let record: Value = serde_json::from_str(trimmed)?;
            parsed.consume_record(&record, line_index + 1);
        }

        parsed.finalize_open_assistant_message();
        let aggregate = parsed.build_message_stream_aggregation();

        if parsed.external_session_id.is_none() {
            parsed.external_session_id = path
                .file_stem()
                .and_then(|value| value.to_str())
                .map(ToString::to_string);
        }

        if parsed.title.is_none() {
            parsed.title = path
                .file_stem()
                .and_then(|value| value.to_str())
                .and_then(select_title_candidate);
        }

        let session_key = parsed
            .external_session_id
            .as_ref()
            .map(|value| format!("claude_code:{value}"))
            .unwrap_or_else(|| format!("claude_code:file:{checksum}"));

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

impl ParsedClaudeSession {
    fn consume_record(&mut self, record: &Value, line_number: usize) {
        let timestamp = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_utc);
        self.track_timestamp(timestamp);

        if self.external_session_id.is_none() {
            self.external_session_id = record
                .get("sessionId")
                .and_then(Value::as_str)
                .map(ToString::to_string);
        }

        match record.get("type").and_then(Value::as_str) {
            Some("user") => self.consume_user_record(record, timestamp, line_number),
            Some("assistant") => self.consume_assistant_record(record, timestamp, line_number),
            _ => {
                self.finalize_open_assistant_message_if_inactive(record);
            }
        }
    }

    fn consume_user_record(
        &mut self,
        record: &Value,
        timestamp: Option<DateTime<Utc>>,
        line_number: usize,
    ) {
        self.finalize_open_assistant_message();

        let message = record.get("message").unwrap_or(&Value::Null);
        let prompt_id = record
            .get("promptId")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let is_tool_result = message_content_items(message)
            .iter()
            .any(|item| item.get("type").and_then(Value::as_str) == Some("tool_result"));

        if is_tool_result {
            self.ensure_open_request(prompt_id, timestamp, line_number);
            let content = extract_tool_result_text(record, message);
            self.push_message("tool", "tool_result", content, None, timestamp, line_number);
            return;
        }

        self.start_or_continue_request(prompt_id, timestamp, line_number);
        let content = extract_user_message_text(message);
        if self.title.is_none() {
            self.title = content.as_deref().and_then(select_title_candidate);
        }
        self.push_message("user", "prompt", content, None, timestamp, line_number);
    }

    fn consume_assistant_record(
        &mut self,
        record: &Value,
        timestamp: Option<DateTime<Utc>>,
        line_number: usize,
    ) {
        let message = record.get("message").unwrap_or(&Value::Null);
        let logical_message_id = message
            .get("id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| {
                format!(
                    "{}:assistant:{line_number}",
                    self.external_session_id.as_deref().unwrap_or("claude_code")
                )
            });

        let request_model = self.model_last.clone().or_else(|| self.model_first.clone());
        if self.current_request_id.is_none() {
            self.ensure_open_request(None, timestamp, line_number);
            return self.consume_assistant_record(record, timestamp, line_number);
        }

        if self
            .open_assistant_message
            .as_ref()
            .map(|item| item.logical_message_id.as_str())
            != Some(logical_message_id.as_str())
        {
            self.finalize_open_assistant_message();
            self.open_assistant_message = Some(OpenAssistantMessage {
                logical_message_id: logical_message_id.clone(),
                request_id: self.current_request_id.clone(),
                created_at: timestamp,
                updated_at: timestamp,
                source_line_number: line_number,
                model: request_model,
                usage: None,
                stop_reason: None,
                text_parts: Vec::new(),
                tool_names: Vec::new(),
            });
        }

        let usage = extract_assistant_usage(message);
        let model = normalize_model_name(message.get("model").and_then(Value::as_str));
        let stop_reason = normalize_stop_reason(message.get("stop_reason").and_then(Value::as_str));
        self.capture_model(model.as_deref());

        if let Some(open) = self.open_assistant_message.as_mut() {
            open.updated_at = timestamp.or(open.updated_at);
            if open.created_at.is_none() {
                open.created_at = timestamp;
            }
            if !has_real_model(&open.model) {
                open.model = model.clone();
            }
            if usage.is_some() {
                open.usage = usage;
            }
            if stop_reason.is_some() {
                open.stop_reason = stop_reason;
            }

            for item in message_content_items(message) {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) =
                            normalize_optional_text(item.get("text").and_then(Value::as_str))
                        {
                            push_unique_text(&mut open.text_parts, text);
                        }
                    }
                    Some("tool_use") => {
                        if let Some(name) =
                            normalize_optional_text(item.get("name").and_then(Value::as_str))
                        {
                            push_unique_text(&mut open.tool_names, name);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn finalize_open_assistant_message_if_inactive(&mut self, record: &Value) {
        match record.get("type").and_then(Value::as_str) {
            Some("assistant") => {}
            _ => self.finalize_open_assistant_message(),
        }
    }

    fn finalize_open_assistant_message(&mut self) {
        let Some(open) = self.open_assistant_message.take() else {
            return;
        };

        let message_type = if !open.text_parts.is_empty() {
            "message".to_string()
        } else if !open.tool_names.is_empty() {
            "tool_use".to_string()
        } else {
            "assistant".to_string()
        };
        let body = if !open.text_parts.is_empty() {
            Some(open.text_parts.join("\n\n"))
        } else if !open.tool_names.is_empty() {
            Some(open.tool_names.join(", "))
        } else {
            None
        };
        let character_count = body.as_ref().map(|text| text.chars().count() as i64);
        let usage = open.usage.unwrap_or_default();
        let effective_input_tokens = usage.effective_input_tokens();
        let total_tokens = usage.total_tokens();
        let has_usage = usage.input_tokens > 0
            || usage.output_tokens > 0
            || usage.cache_read_input_tokens > 0
            || usage.cache_write_input_tokens > 0;
        let resolved_model = open
            .model
            .clone()
            .or_else(|| self.model_last.clone())
            .or_else(|| self.model_first.clone());
        let event_time_utc = open
            .updated_at
            .or(open.created_at)
            .or(self.source_updated_at)
            .or(self.source_created_at)
            .unwrap_or_else(time::now_utc);

        let _ = (message_type, character_count, open.source_line_number);
        self.message_count += 1;

        self.stream_items.push(MessageStreamItem {
            source_id: open.logical_message_id.clone(),
            role: "assistant".to_string(),
            request_id: open.request_id.or_else(|| self.current_request_id.clone()),
            parent_id: None,
            status: open.stop_reason.or_else(|| Some("open".to_string())),
            model: resolved_model,
            usage: has_usage.then_some(MessageTokenUsage {
                input_tokens: effective_input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens,
                cache_read_input_tokens: usage.cache_read_input_tokens,
                cache_write_input_tokens: usage.cache_write_input_tokens,
            }),
            count_as_message: true,
            source_created_at: open.created_at,
            source_updated_at: Some(event_time_utc),
            usage_event_time_utc: Some(event_time_utc),
            source_event_id: Some(open.logical_message_id),
            usage_event_granularity: None,
            usage_event_confidence: None,
            source_locator: serialize_locator(open.source_line_number),
            use_as_request_locator: false,
        });
    }

    fn start_or_continue_request(
        &mut self,
        prompt_id: Option<String>,
        timestamp: Option<DateTime<Utc>>,
        line_number: usize,
    ) {
        if let Some(prompt_id) = prompt_id.clone() {
            if self.current_request_id.as_deref() == Some(prompt_id.as_str()) {
                return;
            }
        }

        self.current_request_id = None;
        self.ensure_open_request(prompt_id, timestamp, line_number);
    }

    fn ensure_open_request(
        &mut self,
        prompt_id: Option<String>,
        timestamp: Option<DateTime<Utc>>,
        line_number: usize,
    ) {
        if self.current_request_id.is_some() {
            return;
        }

        self.next_request_sequence_no += 1;
        let source_request_id = prompt_id.unwrap_or_else(|| {
            format!(
                "{}:prompt:{}",
                self.external_session_id.as_deref().unwrap_or("claude_code"),
                self.next_request_sequence_no
            )
        });

        self.current_request_id = Some(source_request_id);
        let _ = (timestamp, line_number);
    }

    fn push_message(
        &mut self,
        role: &str,
        message_type: &str,
        body: Option<String>,
        model: Option<String>,
        timestamp: Option<DateTime<Utc>>,
        line_number: usize,
    ) {
        let character_count = body.as_ref().map(|text| text.chars().count() as i64);
        let _ = (message_type, character_count, model, timestamp, line_number);
        self.message_count += 1;

        let request_id = self.current_request_id.clone();
        self.stream_items.push(MessageStreamItem {
            source_id: request_id
                .as_ref()
                .map(|value| format!("{value}:{role}:{line_number}"))
                .unwrap_or_else(|| format!("claude_code:{role}:{line_number}")),
            role: role.to_string(),
            request_id,
            parent_id: None,
            status: (role == "user").then(|| "open".to_string()),
            model: self.model_last.clone().or_else(|| self.model_first.clone()),
            usage: None,
            count_as_message: true,
            source_created_at: timestamp,
            source_updated_at: timestamp,
            usage_event_time_utc: None,
            source_event_id: None,
            usage_event_granularity: None,
            usage_event_confidence: None,
            source_locator: serialize_locator(line_number),
            use_as_request_locator: false,
        });
    }

    fn capture_model(&mut self, model: Option<&str>) {
        let Some(model) = normalize_model_name(model) else {
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

    fn build_message_stream_aggregation(&self) -> super::message_stream::MessageStreamAggregation {
        let mut aggregate = MessageStreamAggregator::new(self.stream_items.clone())
            .aggregate_explicit_request_groups_with_item_events();
        for request in &mut aggregate.requests {
            request.status = request.status.clone().or_else(|| Some("open".to_string()));
            request.model = sanitize_model(request.model.clone());
            request.input_tokens = Some(request.input_tokens.unwrap_or(0));
            request.output_tokens = Some(request.output_tokens.unwrap_or(0));
            request.total_tokens = Some(request.total_tokens.unwrap_or(0));
            request.cache_read_input_tokens = Some(request.cache_read_input_tokens.unwrap_or(0));
            request.cache_write_input_tokens = Some(request.cache_write_input_tokens.unwrap_or(0));
            request.token_confidence = Some("high".to_string());
        }
        aggregate
    }
}

fn is_claude_session_path(path: &Path) -> bool {
    if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
        return false;
    }

    let normalized = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if !normalized.contains("/.claude/projects/") || normalized.contains("/subagents/") {
        return false;
    }

    let parent = path.parent();
    let grandparent = parent.and_then(Path::parent);
    grandparent
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("projects"))
}

fn parse_rfc3339_utc(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn serialize_locator(line_number: usize) -> String {
    serde_json::to_string(&ClaudeMessageLocator { line_number })
        .unwrap_or_else(|_| format!("{{\"line_number\":{line_number}}}"))
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

fn sanitize_model(model: Option<String>) -> Option<String> {
    model.and_then(|value| normalize_model_name(Some(value.as_str())))
}

fn normalize_model_name(value: Option<&str>) -> Option<String> {
    normalize_optional_text(value).filter(|model| !is_placeholder_model(model))
}

fn is_placeholder_model(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "<synthetic>" | "unknown"
    )
}

fn has_real_model(model: &Option<String>) -> bool {
    model
        .as_deref()
        .is_some_and(|value| !is_placeholder_model(value))
}

fn normalize_stop_reason(value: Option<&str>) -> Option<String> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some("end_turn") => Some("completed".to_string()),
        Some(other) => Some(other.to_string()),
        None => None,
    }
}

fn message_content_items(message: &Value) -> Vec<&Value> {
    match message.get("content") {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(Value::String(_)) => vec![message.get("content").unwrap_or(&Value::Null)],
        _ => Vec::new(),
    }
}

fn extract_user_message_text(message: &Value) -> Option<String> {
    match message.get("content") {
        Some(Value::String(text)) => normalize_optional_text(Some(text)),
        Some(Value::Array(items)) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|item| {
                    normalize_optional_text(item.get("text").and_then(Value::as_str)).or_else(
                        || normalize_optional_text(item.get("content").and_then(Value::as_str)),
                    )
                })
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n\n"))
            }
        }
        _ => None,
    }
}

fn extract_tool_result_text(record: &Value, message: &Value) -> Option<String> {
    let parts: Vec<String> = message_content_items(message)
        .into_iter()
        .filter_map(|item| {
            if item.get("type").and_then(Value::as_str) != Some("tool_result") {
                return None;
            }

            normalize_optional_text(item.get("content").and_then(Value::as_str))
                .or_else(|| normalize_optional_text(item.get("text").and_then(Value::as_str)))
        })
        .collect();
    if !parts.is_empty() {
        return Some(parts.join("\n\n"));
    }

    normalize_optional_text(record.get("toolUseResult").and_then(Value::as_str))
}

fn extract_assistant_usage(message: &Value) -> Option<ClaudeUsage> {
    let usage = message.get("usage")?;
    let has_any_field = usage.get("input_tokens").is_some()
        || usage.get("output_tokens").is_some()
        || usage.get("cache_read_input_tokens").is_some()
        || usage.get("cache_creation_input_tokens").is_some()
        || usage.get("cache_write_input_tokens").is_some();
    if !has_any_field {
        return None;
    }

    Some(ClaudeUsage {
        input_tokens: usage
            .get("input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        output_tokens: usage
            .get("output_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        cache_read_input_tokens: usage
            .get("cache_read_input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        cache_write_input_tokens: usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_i64)
            .or_else(|| {
                usage
                    .get("cache_write_input_tokens")
                    .and_then(Value::as_i64)
            })
            .unwrap_or(0),
    })
}

fn push_unique_text(parts: &mut Vec<String>, value: String) {
    if parts.last() == Some(&value) {
        return;
    }
    parts.push(value);
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

#[cfg(test)]
mod tests {
    use super::ClaudeCodeAdapter;
    use crate::sources::SourceAdapter;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;

    fn write_temp_file(relative: &str, content: &str) -> PathBuf {
        let mut root = std::env::temp_dir();
        root.push(format!(
            "claude-adapter-test-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        path
    }

    fn cleanup_temp_session(path: &Path) {
        let Some(project_dir) = path.parent() else {
            return;
        };
        let Some(projects_dir) = project_dir.parent() else {
            return;
        };
        let Some(root_dir) = projects_dir.parent() else {
            return;
        };
        let _ = fs::remove_dir_all(root_dir);
    }

    #[test]
    fn parses_claude_jsonl_session_with_requests_messages_and_usage() {
        let path = write_temp_file(
            ".claude/projects/demo/session-1.jsonl",
            r#"{"type":"user","promptId":"prompt-1","message":{"role":"user","content":"First question"},"uuid":"u1","timestamp":"2026-04-23T10:00:00Z","sessionId":"session-1"}
{"type":"assistant","message":{"id":"assistant-1","type":"message","role":"assistant","content":[{"type":"thinking","thinking":"thinking"},{"type":"tool_use","id":"tool-1","name":"Search","input":{"q":"test"}}],"model":"claude-sonnet","stop_reason":"tool_use","usage":{"input_tokens":100,"cache_read_input_tokens":20,"output_tokens":10}},"uuid":"a1","timestamp":"2026-04-23T10:00:05Z","sessionId":"session-1"}
{"type":"assistant","message":{"id":"assistant-1","type":"message","role":"assistant","content":[{"type":"tool_use","id":"tool-2","name":"Fetch","input":{"url":"https://example.com"}}],"model":"claude-sonnet","stop_reason":"tool_use","usage":{"input_tokens":100,"cache_read_input_tokens":20,"output_tokens":10}},"uuid":"a2","timestamp":"2026-04-23T10:00:06Z","sessionId":"session-1"}
{"type":"user","promptId":"prompt-1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-2","content":"tool output"}]},"uuid":"u2","timestamp":"2026-04-23T10:00:07Z","sessionId":"session-1","toolUseResult":"tool output"}
{"type":"assistant","message":{"id":"assistant-2","type":"message","role":"assistant","content":[{"type":"text","text":"Final answer"}],"model":"claude-sonnet","stop_reason":"end_turn","usage":{"input_tokens":30,"cache_creation_input_tokens":40,"output_tokens":50}},"uuid":"a3","timestamp":"2026-04-23T10:00:08Z","sessionId":"session-1"}"#,
        );

        let parsed = ClaudeCodeAdapter.parse(&path).unwrap();
        cleanup_temp_session(&path);

        let session = &parsed[0];
        assert_eq!(session.external_session_id.as_deref(), Some("session-1"));
        assert_eq!(session.title.as_deref(), Some("First question"));
        assert_eq!(session.model_first.as_deref(), Some("claude-sonnet"));
        assert_eq!(session.model_last.as_deref(), Some("claude-sonnet"));
        assert_eq!(session.requests.len(), 1);
        assert_eq!(session.message_count, 4);
        assert_eq!(session.events.len(), 2);
        assert_eq!(session.total_input_tokens, 190);
        assert_eq!(session.total_output_tokens, 60);

        let request = &session.requests[0];
        assert_eq!(request.source_request_id.as_deref(), Some("prompt-1"));
        assert_eq!(request.status.as_deref(), Some("completed"));
        assert_eq!(request.message_count, 4);
        assert_eq!(request.input_tokens, Some(190));
        assert_eq!(request.output_tokens, Some(60));
        assert_eq!(request.total_tokens, Some(250));
        assert_eq!(request.cache_read_input_tokens, Some(20));
        assert_eq!(request.cache_write_input_tokens, Some(40));
        assert_eq!(session.events[0].delta_input, 120);
        assert_eq!(session.events[0].delta_total, 130);
        assert_eq!(session.events[1].delta_input, 70);
        assert_eq!(session.events[1].delta_total, 120);
    }

    #[test]
    fn only_handles_main_claude_project_jsonl_files() {
        assert!(ClaudeCodeAdapter.can_handle(Path::new(
            "C:/Users/test/.claude/projects/example/session.jsonl"
        )));
        assert!(!ClaudeCodeAdapter.can_handle(Path::new(
            "C:/Users/test/.claude/projects/example/subagents/agent-1.jsonl"
        )));
        assert!(!ClaudeCodeAdapter.can_handle(Path::new(
            "C:/Users/test/.claude/projects/example/session.json"
        )));
        assert!(!ClaudeCodeAdapter.can_handle(Path::new(
            "C:/Users/test/.local/share/opencode/storage/message/msg.json"
        )));
    }

    #[test]
    fn synthetic_placeholder_does_not_override_later_real_model() {
        let path = write_temp_file(
            ".claude/projects/demo/session-synthetic-recovery.jsonl",
            r#"{"type":"assistant","message":{"id":"assistant-1","role":"assistant","content":[{"type":"text","text":"No response requested."}],"model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}},"timestamp":"2026-04-26T08:19:29Z","sessionId":"session-synthetic-recovery"}
{"type":"user","promptId":"prompt-1","message":{"role":"user","content":"Use a real model now"},"timestamp":"2026-04-26T08:20:18Z","sessionId":"session-synthetic-recovery"}
{"type":"assistant","message":{"id":"assistant-2","role":"assistant","content":[{"type":"text","text":"Done"}],"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":41353,"output_tokens":135}},"timestamp":"2026-04-26T08:20:46Z","sessionId":"session-synthetic-recovery"}"#,
        );

        let parsed = ClaudeCodeAdapter.parse(&path).unwrap();
        cleanup_temp_session(&path);

        let session = &parsed[0];
        assert_eq!(session.model_first.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(session.model_last.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(session.requests.len(), 2);
        assert_eq!(
            session.requests[1].model.as_deref(),
            Some("claude-sonnet-4-6")
        );
        assert_eq!(
            session.events[0].model.as_deref(),
            Some("claude-sonnet-4-6")
        );
    }

    #[test]
    fn sessions_with_only_synthetic_models_fall_back_to_unknown() {
        let path = write_temp_file(
            ".claude/projects/demo/session-synthetic-only.jsonl",
            r#"{"type":"user","promptId":"prompt-1","message":{"role":"user","content":"Hello"},"timestamp":"2026-04-26T08:20:18Z","sessionId":"session-synthetic-only"}
{"type":"assistant","message":{"id":"assistant-1","role":"assistant","content":[{"type":"text","text":"API Error: 400"}],"model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}},"timestamp":"2026-04-26T08:20:46Z","sessionId":"session-synthetic-only"}"#,
        );

        let parsed = ClaudeCodeAdapter.parse(&path).unwrap();
        cleanup_temp_session(&path);

        let session = &parsed[0];
        assert_eq!(session.model_first, None);
        assert_eq!(session.model_last, None);
        assert_eq!(session.requests.len(), 1);
        assert_eq!(session.requests[0].model, None);
    }
}
