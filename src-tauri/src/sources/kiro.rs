use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::error::{AppError, AppResult};
use crate::utils::cache::BoundedCache;
use crate::utils::{hash, time};

use super::message_stream::{
    MessageStreamAggregation, MessageStreamAggregator, MessageStreamItem, MessageTokenUsage,
};
use super::{NormalizedSession, SourceAdapter};

#[derive(Debug, Serialize, Deserialize)]
struct KiroLocator {
    history_index: usize,
    message_id: Option<String>,
    execution_id: Option<String>,
}

#[derive(Debug, Clone)]
struct KiroParsedTurn {
    source_message_id: Option<String>,
    execution_id: Option<String>,
    execution_status: Option<String>,
    role: String,
    model: Option<String>,
    source_created_at: Option<DateTime<Utc>>,
    source_updated_at: Option<DateTime<Utc>>,
    source_locator: String,
    estimated_message_tokens: Option<i64>,
    estimated_request_input_tokens: Option<i64>,
    estimated_request_output_tokens: Option<i64>,
}

#[derive(Debug)]
struct OpenKiroRequest {
    source_request_id: Option<String>,
    fallback_source_message_id: Option<String>,
    status: Option<String>,
    sequence_no: i64,
    message_indices: Vec<usize>,
    message_count: i64,
    model: Option<String>,
    input_tokens_estimate: Option<i64>,
    output_tokens_estimate: Option<i64>,
    source_created_at: Option<DateTime<Utc>>,
    source_updated_at: Option<DateTime<Utc>>,
    source_locator: String,
    saw_assistant: bool,
}

#[derive(Debug, Default)]
struct KiroSessionIndexEntry {
    title: Option<String>,
    source_created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
struct KiroExecutionChatSummary {
    chat_paths: Vec<PathBuf>,
    assistant_character_count: Option<i64>,
    assistant_tokens_estimate: Option<i64>,
    assistant_text_hash: Option<String>,
    model: Option<String>,
    end_time: Option<DateTime<Utc>>,
    status: Option<String>,
}

#[derive(Debug, Default)]
struct KiroChatIndex {
    executions: HashMap<String, KiroExecutionChatSummary>,
}

#[derive(Debug, Clone)]
struct KiroChatIndexCacheEntry {
    index: Arc<KiroChatIndex>,
    watch_states: Vec<KiroPathState>,
    snapshot_states: HashMap<PathBuf, KiroPathState>,
}

#[derive(Debug, Clone, Default)]
struct KiroExecutionSnapshot {
    assistant_text: Option<String>,
    model: Option<String>,
    end_time: Option<DateTime<Utc>>,
    status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KiroPathState {
    path: PathBuf,
    exists: bool,
    size_bytes: i64,
    mtime_ms: i64,
}

const KIRO_CHAT_INDEX_CACHE_CAPACITY: usize = 64;

#[derive(Clone)]
pub struct KiroAdapter {
    chat_index_cache: Arc<Mutex<BoundedCache<PathBuf, KiroChatIndexCacheEntry>>>,
}

impl Default for KiroAdapter {
    fn default() -> Self {
        Self {
            chat_index_cache: Arc::new(Mutex::new(BoundedCache::new(
                KIRO_CHAT_INDEX_CACHE_CAPACITY,
            ))),
        }
    }
}

impl SourceAdapter for KiroAdapter {
    fn name(&self) -> &str {
        "kiro"
    }

    fn parser_version(&self) -> i64 {
        7
    }

    fn can_handle(&self, path: &Path) -> bool {
        is_kiro_workspace_session_path(path)
    }

    fn discover_paths(&self, root_path: &Path) -> AppResult<Vec<PathBuf>> {
        if root_path.is_file() {
            return Ok((self.can_handle(root_path)
                && is_discoverable_kiro_workspace_session_file(root_path))
            .then(|| root_path.to_path_buf())
            .into_iter()
            .collect());
        }

        let mut paths = Vec::new();
        for sessions_root in resolve_kiro_workspace_sessions_roots(root_path) {
            for entry in walkdir::WalkDir::new(sessions_root)
                .into_iter()
                .filter_map(Result::ok)
            {
                if !entry.file_type().is_file() {
                    continue;
                }

                let path = entry.into_path();
                if self.can_handle(&path) && is_discoverable_kiro_workspace_session_file(&path) {
                    paths.push(path);
                }
            }
        }

        Ok(paths)
    }

    fn fingerprint_paths(&self, path: &Path) -> Vec<PathBuf> {
        let mut paths = vec![path.to_path_buf()];
        if let Some(index_path) = resolve_workspace_sessions_index_path(path) {
            if index_path.exists() {
                paths.push(index_path);
            }
        }
        if let (Some(storage_root), Some(execution_ids)) = (
            resolve_kiro_storage_root(path),
            collect_workspace_execution_ids(path),
        ) {
            if let Ok(chat_index) =
                self.load_kiro_chat_index_for_executions(&storage_root, &execution_ids)
            {
                for execution_id in execution_ids {
                    if let Some(summary) = chat_index.executions.get(&execution_id) {
                        paths.extend(summary.chat_paths.iter().cloned());
                    }
                }
            }
        }
        paths.sort();
        paths.dedup();
        paths
    }

    fn parse(&self, path: &Path) -> AppResult<Vec<NormalizedSession>> {
        if !is_kiro_workspace_session_path(path) {
            return Err(AppError::validation(
                "File is outside the Kiro workspace sessions directory",
            ));
        }

        let content = fs::read_to_string(path)?;
        let document: Value = serde_json::from_str(&content)?;
        let history = document
            .get("history")
            .and_then(Value::as_array)
            .ok_or_else(|| AppError::validation("Kiro session file is missing history[]"))?;
        if !kiro_document_has_conversation(&document) {
            return Ok(Vec::new());
        }
        let execution_ids = collect_workspace_execution_ids_from_history(history);
        let chat_index = resolve_kiro_storage_root(path)
            .map(|storage_root| {
                self.load_kiro_chat_index_for_executions(&storage_root, &execution_ids)
            })
            .transpose()?;

        let session_id = document
            .get("sessionId")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                path.file_stem()
                    .and_then(|value| value.to_str())
                    .map(ToString::to_string)
            });
        let default_model_title =
            normalize_optional_text(document.get("defaultModelTitle").and_then(Value::as_str));
        let session_model = derive_session_model(&document, default_model_title.clone());
        let session_index = session_id
            .as_deref()
            .and_then(|value| load_workspace_session_index_entry(path, value));
        let source_updated_at = fs::metadata(path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .map(DateTime::<Utc>::from);

        let mut title = document
            .get("title")
            .and_then(Value::as_str)
            .and_then(select_title_candidate)
            .or_else(|| {
                session_index
                    .as_ref()
                    .and_then(|entry| entry.title.as_deref())
                    .and_then(select_title_candidate)
            });
        let mut messages = Vec::<KiroParsedTurn>::new();
        let mut checksum_parts = vec![
            session_id.clone().unwrap_or_default(),
            session_model.clone().unwrap_or_default(),
            title.clone().unwrap_or_default(),
        ];

        for (history_index, entry) in history.iter().enumerate() {
            let message = entry.get("message").unwrap_or(&Value::Null);
            if !message.is_object() {
                continue;
            }

            let raw_role = message.get("role").and_then(Value::as_str);
            let text_content =
                extract_kiro_message_text(message.get("content").unwrap_or(&Value::Null));
            let borrowed_execution_id = resolve_embedded_kiro_assistant_execution_id(
                history,
                history_index,
                raw_role,
                text_content.as_deref(),
            );
            let role = borrowed_execution_id
                .as_ref()
                .map(|_| "assistant".to_string())
                .unwrap_or_else(|| normalize_role(raw_role));
            let source_message_id = message
                .get("id")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let execution_id = entry
                .get("executionId")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let execution_id = execution_id.or(borrowed_execution_id);
            let chat_summary = execution_id.as_deref().and_then(|value| {
                chat_index
                    .as_ref()
                    .and_then(|index| index.executions.get(value))
            });
            let entry_model = chat_summary
                .and_then(|summary| summary.model.clone())
                .or_else(|| extract_entry_model(entry, default_model_title.as_deref()));
            let estimated_message_tokens = if role == "assistant" {
                chat_summary
                    .and_then(|summary| summary.assistant_tokens_estimate)
                    .or_else(|| {
                        text_content
                            .as_deref()
                            .map(estimate_text_tokens)
                            .filter(|value| *value > 0)
                    })
            } else {
                text_content
                    .as_deref()
                    .map(estimate_text_tokens)
                    .filter(|value| *value > 0)
            };
            let estimated_request_input_tokens = extract_prompt_logs_prompt(entry)
                .as_deref()
                .map(estimate_text_tokens)
                .filter(|value| *value > 0);
            let estimated_request_output_tokens = extract_prompt_logs_completion(entry)
                .as_deref()
                .map(estimate_text_tokens)
                .filter(|value| *value > 0)
                .or_else(|| chat_summary.and_then(|summary| summary.assistant_tokens_estimate))
                .or_else(|| {
                    (role == "assistant")
                        .then_some(estimated_message_tokens)
                        .flatten()
                });
            let source_locator = serialize_locator(
                history_index + 1,
                source_message_id.clone(),
                execution_id.clone(),
            );
            let model = if role == "assistant" {
                prefer_specific_kiro_model(entry_model.clone(), session_model.clone())
            } else {
                entry_model
            };

            if title.is_none() && role == "user" {
                title = text_content.as_deref().and_then(select_title_candidate);
            }

            checksum_parts.push(format!("history:{}", history_index + 1));
            checksum_parts.push(role.clone());
            checksum_parts.push(source_message_id.clone().unwrap_or_default());
            checksum_parts.push(execution_id.clone().unwrap_or_default());
            checksum_parts.push(text_content.clone().unwrap_or_default());
            checksum_parts.push(
                chat_summary
                    .and_then(|summary| summary.assistant_text_hash.clone())
                    .unwrap_or_default(),
            );

            messages.push(KiroParsedTurn {
                source_message_id,
                execution_id,
                execution_status: chat_summary.and_then(|summary| summary.status.clone()),
                role,
                model,
                source_created_at: None,
                source_updated_at: chat_summary.and_then(|summary| summary.end_time),
                source_locator,
                estimated_message_tokens,
                estimated_request_input_tokens,
                estimated_request_output_tokens,
            });
        }

        if messages.is_empty() {
            return Ok(Vec::new());
        }

        let source_created_at = session_index.and_then(|entry| entry.source_created_at);
        let message_count = messages.len() as i64;
        let mut aggregate = build_message_stream_aggregation(
            messages,
            &session_model,
            source_updated_at.or(source_created_at),
        );
        aggregate.events.clear();
        aggregate.total_input_tokens = 0;
        aggregate.total_output_tokens = 0;
        let latest_request_updated_at = aggregate
            .requests
            .iter()
            .filter_map(|request| request.source_updated_at)
            .max();
        let source_updated_at = later_timestamp(source_updated_at, latest_request_updated_at);
        let conversation_checksum = hash::sha256_text(&checksum_parts.join("\n"));
        let session_key = session_id
            .as_ref()
            .map(|value| format!("kiro:{value}"))
            .unwrap_or_else(|| format!("kiro:file:{conversation_checksum}"));

        let (model_first, model_last) = derive_session_models(
            session_model.clone(),
            aggregate
                .requests
                .iter()
                .filter_map(|request| request.model.clone())
                .collect(),
        );

        Ok(vec![NormalizedSession {
            source_app: self.name().to_string(),
            external_session_id: session_id,
            session_key,
            title,
            model_first,
            model_last,
            source_created_at,
            source_updated_at,
            total_input_tokens: aggregate.total_input_tokens,
            total_output_tokens: aggregate.total_output_tokens,
            message_count,
            conversation_checksum,
            requests: aggregate.requests,
            events: aggregate.events,
        }])
    }
}

impl KiroAdapter {
    fn load_kiro_chat_index_for_executions(
        &self,
        root: &Path,
        execution_ids: &[String],
    ) -> AppResult<Arc<KiroChatIndex>> {
        load_kiro_chat_index_for_executions(root, execution_ids, &self.chat_index_cache)
    }
}

fn build_message_stream_aggregation(
    parsed_messages: Vec<KiroParsedTurn>,
    session_model: &Option<String>,
    fallback_event_time: Option<DateTime<Utc>>,
) -> MessageStreamAggregation {
    let mut stream_items = Vec::<MessageStreamItem>::new();
    let mut open_request: Option<OpenKiroRequest> = None;
    let mut request_count = 0_i64;

    for (index, message) in parsed_messages.iter().enumerate() {
        let should_rotate = match message.role.as_str() {
            "assistant" => open_request
                .as_ref()
                .map(|request| {
                    request.saw_assistant
                        && message.execution_id.is_some()
                        && request.source_request_id.as_deref() != message.execution_id.as_deref()
                })
                .unwrap_or(false),
            _ => open_request
                .as_ref()
                .map(|request| request.saw_assistant)
                .unwrap_or(false),
        };

        if should_rotate {
            finalize_open_request(
                &mut open_request,
                &parsed_messages,
                &mut stream_items,
                fallback_event_time,
            );
        }

        if open_request.is_none() {
            request_count += 1;
            open_request = Some(OpenKiroRequest {
                source_request_id: None,
                fallback_source_message_id: message.source_message_id.clone(),
                status: None,
                sequence_no: request_count,
                message_indices: Vec::new(),
                message_count: 0,
                model: message.model.clone().or_else(|| session_model.clone()),
                input_tokens_estimate: None,
                output_tokens_estimate: None,
                source_created_at: message.source_created_at,
                source_updated_at: message.source_updated_at,
                source_locator: message.source_locator.clone(),
                saw_assistant: false,
            });
        }

        let request = open_request.as_mut().expect("request is always available");
        if request.source_request_id.is_none() {
            request.source_request_id = message.execution_id.clone();
        }
        request.model = prefer_specific_kiro_model(
            request.model.clone(),
            message.model.clone().or_else(|| session_model.clone()),
        );
        request.source_created_at =
            earlier_timestamp(request.source_created_at, message.source_created_at);
        request.source_updated_at = later_timestamp(
            request.source_updated_at,
            message.source_updated_at.or(message.source_created_at),
        );
        request.input_tokens_estimate = merge_optional_max(
            request.input_tokens_estimate,
            message.estimated_request_input_tokens,
        );
        request.output_tokens_estimate = merge_optional_max(
            request.output_tokens_estimate,
            message.estimated_request_output_tokens,
        );
        request.message_indices.push(index);
        request.message_count += 1;
        if message.role == "assistant" {
            request.saw_assistant = true;
            if request.status.is_none() {
                request.status = message.execution_status.clone();
            }
            if request.source_request_id.is_none() {
                request.source_request_id = message.source_message_id.clone();
            }
        }
    }

    finalize_open_request(
        &mut open_request,
        &parsed_messages,
        &mut stream_items,
        fallback_event_time,
    );

    let mut aggregate =
        MessageStreamAggregator::new(stream_items).aggregate_explicit_request_groups();
    for request in &mut aggregate.requests {
        request.cache_read_input_tokens = None;
        request.cache_write_input_tokens = None;
        request.token_confidence = request.total_tokens.map(|_| "low".to_string());
    }
    aggregate
}

fn finalize_open_request(
    open_request: &mut Option<OpenKiroRequest>,
    parsed_messages: &[KiroParsedTurn],
    stream_items: &mut Vec<MessageStreamItem>,
    fallback_event_time: Option<DateTime<Utc>>,
) {
    let Some(request) = open_request.take() else {
        return;
    };

    let source_request_id = request
        .source_request_id
        .or(request.fallback_source_message_id)
        .unwrap_or_else(|| format!("kiro-request-{}", request.sequence_no));

    let fallback_input_tokens: i64 = request
        .message_indices
        .iter()
        .filter_map(|message_index| parsed_messages.get(*message_index))
        .filter(|message| message.role == "user")
        .filter_map(|message| message.estimated_message_tokens)
        .sum();
    let fallback_output_tokens: i64 = request
        .message_indices
        .iter()
        .filter_map(|message_index| parsed_messages.get(*message_index))
        .filter(|message| message.role == "assistant")
        .filter_map(|message| message.estimated_message_tokens)
        .sum();
    let input_tokens = merge_optional_max(
        request.input_tokens_estimate,
        (fallback_input_tokens > 0).then_some(fallback_input_tokens),
    );
    let output_tokens = merge_optional_max(
        request.output_tokens_estimate,
        (fallback_output_tokens > 0).then_some(fallback_output_tokens),
    );
    let total_tokens = match (input_tokens, output_tokens) {
        (Some(input_tokens), Some(output_tokens)) => Some(input_tokens + output_tokens),
        (Some(input_tokens), None) => Some(input_tokens),
        (None, Some(output_tokens)) => Some(output_tokens),
        (None, None) => None,
    };

    for message_index in &request.message_indices {
        let Some(message) = parsed_messages.get(*message_index) else {
            continue;
        };

        stream_items.push(MessageStreamItem {
            source_id: message
                .source_message_id
                .clone()
                .unwrap_or_else(|| format!("{source_request_id}:message:{}", message_index + 1)),
            role: message.role.clone(),
            request_id: Some(source_request_id.clone()),
            parent_id: None,
            status: None,
            model: message.model.clone(),
            usage: None,
            count_as_message: true,
            source_created_at: message.source_created_at,
            source_updated_at: message.source_updated_at.or(message.source_created_at),
            usage_event_time_utc: None,
            source_event_id: None,
            usage_event_granularity: None,
            usage_event_confidence: None,
            source_locator: message.source_locator.clone(),
            use_as_request_locator: false,
        });
    }

    let status = request
        .status
        .or_else(|| request.saw_assistant.then(|| "completed".to_string()));
    let usage = total_tokens.map(|total_tokens| MessageTokenUsage {
        input_tokens: input_tokens.unwrap_or(0),
        output_tokens: output_tokens.unwrap_or(0),
        total_tokens,
        cache_read_input_tokens: 0,
        cache_write_input_tokens: 0,
    });
    if usage.is_none() && status.is_none() && request.model.is_none() {
        return;
    }

    let event_time = request
        .source_updated_at
        .or(request.source_created_at)
        .or(fallback_event_time);
    stream_items.push(MessageStreamItem {
        source_id: format!("{source_request_id}:usage"),
        role: "assistant".to_string(),
        request_id: Some(source_request_id.clone()),
        parent_id: None,
        status,
        model: request.model,
        usage,
        count_as_message: false,
        source_created_at: request.source_created_at,
        source_updated_at: event_time,
        usage_event_time_utc: event_time,
        source_event_id: Some(source_request_id),
        usage_event_granularity: None,
        usage_event_confidence: Some("low".to_string()),
        source_locator: request.source_locator,
        use_as_request_locator: false,
    });
}

fn is_kiro_workspace_session_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("json"))
        && !path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("sessions.json"))
        && path_has_component(path, "workspace-sessions")
}

fn is_discoverable_kiro_workspace_session_file(path: &Path) -> bool {
    if !is_kiro_workspace_session_path(path) {
        return false;
    }

    let Ok(content) = fs::read_to_string(path) else {
        return true;
    };
    let Ok(document) = serde_json::from_str::<Value>(&content) else {
        return true;
    };

    kiro_document_has_conversation(&document)
}

fn kiro_document_has_conversation(document: &Value) -> bool {
    let Some(history) = document.get("history").and_then(Value::as_array) else {
        return false;
    };

    history.iter().any(|entry| {
        let message = entry.get("message").unwrap_or(&Value::Null);
        let role = message.get("role").and_then(Value::as_str);
        if !matches!(role, Some("user" | "assistant")) {
            return false;
        }

        let has_message_text =
            extract_kiro_message_text(message.get("content").unwrap_or(&Value::Null)).is_some();
        let has_prompt_log = extract_prompt_logs_prompt(entry).is_some()
            || extract_prompt_logs_completion(entry).is_some();
        let has_execution_id =
            normalize_optional_text(entry.get("executionId").and_then(Value::as_str)).is_some();

        has_message_text || has_prompt_log || has_execution_id
    })
}

fn path_has_component(path: &Path, expected: &str) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .map(|value| value.eq_ignore_ascii_case(expected))
            .unwrap_or(false)
    })
}

fn resolve_workspace_sessions_index_path(path: &Path) -> Option<PathBuf> {
    path.parent().map(|parent| parent.join("sessions.json"))
}

fn load_workspace_session_index_entry(
    path: &Path,
    session_id: &str,
) -> Option<KiroSessionIndexEntry> {
    let index_path = resolve_workspace_sessions_index_path(path)?;
    let content = fs::read_to_string(index_path).ok()?;
    let entries: Value = serde_json::from_str(&content).ok()?;

    entries.as_array()?.iter().find_map(|entry| {
        (entry.get("sessionId").and_then(Value::as_str) == Some(session_id)).then(|| {
            KiroSessionIndexEntry {
                title: normalize_optional_text(entry.get("title").and_then(Value::as_str)),
                source_created_at: entry.get("dateCreated").and_then(parse_kiro_epoch_ms_value),
            }
        })
    })
}

fn resolve_kiro_storage_root(path: &Path) -> Option<PathBuf> {
    let mut current = path.parent();
    while let Some(candidate) = current {
        if candidate
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("workspace-sessions"))
            .unwrap_or(false)
        {
            return candidate.parent().map(Path::to_path_buf);
        }
        current = candidate.parent();
    }
    None
}

fn resolve_kiro_workspace_sessions_roots(root_path: &Path) -> Vec<PathBuf> {
    if path_has_component(root_path, "workspace-sessions") {
        return vec![root_path.to_path_buf()];
    }

    let direct_root = root_path.join("workspace-sessions");
    let nested_root = root_path.join("kiro.kiroagent").join("workspace-sessions");
    let mut roots = Vec::new();

    if direct_root.exists() {
        roots.push(direct_root);
    }

    if nested_root.exists() && !roots.iter().any(|root| root == &nested_root) {
        roots.push(nested_root);
    }

    roots
}

fn collect_workspace_execution_ids(path: &Path) -> Option<Vec<String>> {
    let content = fs::read_to_string(path).ok()?;
    let document: Value = serde_json::from_str(&content).ok()?;
    let history = document.get("history").and_then(Value::as_array)?;
    Some(collect_workspace_execution_ids_from_history(history))
}

fn collect_workspace_execution_ids_from_history(history: &[Value]) -> Vec<String> {
    let mut execution_ids = history
        .iter()
        .filter_map(|entry| {
            normalize_optional_text(entry.get("executionId").and_then(Value::as_str))
        })
        .collect::<Vec<_>>();
    execution_ids.sort();
    execution_ids.dedup();
    execution_ids
}

fn load_kiro_chat_index_uncached(
    root: &Path,
    cache: &Mutex<BoundedCache<PathBuf, KiroChatIndexCacheEntry>>,
) -> AppResult<Arc<KiroChatIndex>> {
    let entry = build_kiro_chat_index_cache_entry(root)?;
    let mut guard = cache
        .lock()
        .map_err(|_| AppError::internal("Kiro chat index cache lock poisoned"))?;
    guard.insert(root.to_path_buf(), entry);
    let cached = guard
        .get_cloned(&root.to_path_buf())
        .map(|cached| cached.index)
        .expect("kiro chat index cache entry inserted");
    Ok(cached)
}

fn load_kiro_chat_index_for_executions(
    root: &Path,
    execution_ids: &[String],
    cache: &Mutex<BoundedCache<PathBuf, KiroChatIndexCacheEntry>>,
) -> AppResult<Arc<KiroChatIndex>> {
    {
        let mut guard = cache
            .lock()
            .map_err(|_| AppError::internal("Kiro chat index cache lock poisoned"))?;
        if let Some(entry) = guard.get_cloned(&root.to_path_buf()) {
            if kiro_chat_index_cache_is_fresh_for_executions(&entry, execution_ids) {
                return Ok(entry.index.clone());
            }
        }
    }

    load_kiro_chat_index_uncached(root, cache)
}

fn build_kiro_chat_index_cache_entry(root: &Path) -> AppResult<KiroChatIndexCacheEntry> {
    let mut index = KiroChatIndex::default();
    let mut snapshot_paths = Vec::new();
    if root.exists() {
        for snapshot_root in resolve_kiro_snapshot_roots(root) {
            for entry in walkdir::WalkDir::new(snapshot_root)
                .into_iter()
                .filter_map(Result::ok)
            {
                if !entry.file_type().is_file() {
                    continue;
                }

                let path = entry.into_path();
                if !is_kiro_execution_snapshot_candidate(&path) {
                    continue;
                }
                snapshot_paths.push(path.clone());

                let Ok(content) = fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(document) = serde_json::from_str::<Value>(&content) else {
                    continue;
                };
                let Some(execution_id) =
                    normalize_optional_text(document.get("executionId").and_then(Value::as_str))
                else {
                    continue;
                };
                let snapshot = match extract_kiro_execution_snapshot(&document) {
                    Some(snapshot) => snapshot,
                    None => continue,
                };
                let assistant_text = snapshot.assistant_text.clone();
                let assistant_character_count = assistant_text
                    .as_ref()
                    .map(|value| value.chars().count() as i64)
                    .filter(|value| *value > 0);
                let assistant_tokens_estimate = assistant_text
                    .as_deref()
                    .map(estimate_text_tokens)
                    .filter(|value| *value > 0);
                let assistant_text_hash = assistant_text
                    .as_deref()
                    .map(hash::sha256_text)
                    .filter(|value| !value.is_empty());

                let summary = index.executions.entry(execution_id).or_default();
                if !summary.chat_paths.iter().any(|existing| existing == &path) {
                    summary.chat_paths.push(path.clone());
                }

                if should_replace_kiro_chat_summary(
                    summary.assistant_character_count,
                    summary.end_time,
                    assistant_character_count,
                    snapshot.end_time,
                ) {
                    summary.assistant_character_count = assistant_character_count;
                    summary.assistant_tokens_estimate = assistant_tokens_estimate;
                    summary.assistant_text_hash = assistant_text_hash;
                    summary.end_time = snapshot.end_time;
                    summary.status = snapshot.status.clone();
                    if snapshot.model.is_some() {
                        summary.model = snapshot.model.clone();
                    }
                } else {
                    if summary.status.is_none() && snapshot.status.is_some() {
                        summary.status = snapshot.status.clone();
                    }
                    if summary.model.is_none() && snapshot.model.is_some() {
                        summary.model = snapshot.model.clone();
                    }
                }
            }
        }
    }

    snapshot_paths.sort();
    snapshot_paths.dedup();

    Ok(KiroChatIndexCacheEntry {
        index: Arc::new(index),
        watch_states: resolve_kiro_snapshot_watch_paths(root)
            .iter()
            .map(|path| capture_kiro_path_state(path))
            .collect(),
        snapshot_states: snapshot_paths
            .iter()
            .map(|path| capture_kiro_path_state(path))
            .map(|state| (state.path.clone(), state))
            .collect(),
    })
}

fn kiro_chat_index_cache_is_fresh_for_executions(
    entry: &KiroChatIndexCacheEntry,
    execution_ids: &[String],
) -> bool {
    if !kiro_chat_index_watch_paths_are_fresh(entry) {
        return false;
    }

    execution_ids.iter().all(|execution_id| {
        let Some(summary) = entry.index.executions.get(execution_id) else {
            return true;
        };

        summary.chat_paths.iter().all(|path| {
            entry
                .snapshot_states
                .get(path)
                .is_some_and(|state| *state == capture_kiro_path_state(path))
        })
    })
}

fn kiro_chat_index_watch_paths_are_fresh(entry: &KiroChatIndexCacheEntry) -> bool {
    entry
        .watch_states
        .iter()
        .all(|state| *state == capture_kiro_path_state(&state.path))
}

fn capture_kiro_path_state(path: &Path) -> KiroPathState {
    match fs::metadata(path) {
        Ok(metadata) => KiroPathState {
            path: path.to_path_buf(),
            exists: true,
            size_bytes: metadata.len() as i64,
            mtime_ms: crate::utils::fs::metadata_mtime_ms(&metadata).unwrap_or_default(),
        },
        Err(_) => KiroPathState {
            path: path.to_path_buf(),
            exists: false,
            size_bytes: 0,
            mtime_ms: 0,
        },
    }
}

fn resolve_kiro_snapshot_roots(root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for candidate in [root.join("cache"), root.join("session-cache")] {
        if candidate.exists() {
            roots.push(candidate);
        }
    }

    roots
}

fn resolve_kiro_snapshot_watch_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        root.join("cache"),
        root.join("session-cache"),
        root.to_path_buf(),
    ];
    paths.sort();
    paths.dedup();
    paths
}

fn should_replace_kiro_chat_summary(
    current_char_count: Option<i64>,
    current_end_time: Option<DateTime<Utc>>,
    next_char_count: Option<i64>,
    next_end_time: Option<DateTime<Utc>>,
) -> bool {
    let current_score = (
        current_char_count.unwrap_or_default(),
        current_end_time
            .map(|value| value.timestamp_millis())
            .unwrap_or_default(),
    );
    let next_score = (
        next_char_count.unwrap_or_default(),
        next_end_time
            .map(|value| value.timestamp_millis())
            .unwrap_or_default(),
    );
    next_score > current_score
}

fn is_kiro_execution_snapshot_candidate(path: &Path) -> bool {
    if path_has_component(path, "workspace-sessions") || path_has_component(path, "index") {
        return false;
    }
    match path.extension().and_then(|value| value.to_str()) {
        Some("chat") => true,
        Some("json") | Some("sqlite") | Some("wal") | Some("db") | Some("log") => false,
        Some(_) => false,
        None => true,
    }
}

fn extract_kiro_execution_snapshot(document: &Value) -> Option<KiroExecutionSnapshot> {
    if document.get("chat").and_then(Value::as_array).is_some() {
        return Some(KiroExecutionSnapshot {
            assistant_text: extract_kiro_chat_assistant_output(document),
            model: document
                .get("metadata")
                .and_then(|value| value.get("modelId"))
                .and_then(Value::as_str)
                .and_then(|value| normalize_optional_text(Some(value))),
            end_time: document
                .get("metadata")
                .and_then(|value| value.get("endTime"))
                .and_then(parse_kiro_epoch_ms_value),
            status: normalize_kiro_execution_status(
                document
                    .get("metadata")
                    .and_then(|value| value.get("status"))
                    .and_then(Value::as_str)
                    .or_else(|| document.get("status").and_then(Value::as_str)),
            ),
        });
    }

    (document.get("workflowType").and_then(Value::as_str) == Some("chat-agent")).then(|| {
        KiroExecutionSnapshot {
            assistant_text: extract_legacy_kiro_execution_output(document),
            model: None,
            end_time: document.get("endTime").and_then(parse_kiro_epoch_ms_value),
            status: normalize_kiro_execution_status(document.get("status").and_then(Value::as_str)),
        }
    })
}

fn extract_kiro_chat_assistant_output(document: &Value) -> Option<String> {
    let chat = document.get("chat").and_then(Value::as_array)?;
    let mut fallback = None::<String>;
    let mut substantive = None::<String>;

    for entry in chat {
        if entry.get("role").and_then(Value::as_str) != Some("bot") {
            continue;
        }

        let Some(text) = normalize_optional_text(entry.get("content").and_then(Value::as_str))
        else {
            continue;
        };
        if text.eq_ignore_ascii_case("I will follow these instructions.") {
            continue;
        }

        fallback = Some(text.clone());
        if is_substantive_kiro_chat_output(&text) {
            substantive = Some(text);
        }
    }

    substantive.or(fallback)
}

fn extract_legacy_kiro_execution_output(document: &Value) -> Option<String> {
    let last_say = document
        .get("actions")
        .and_then(Value::as_array)
        .and_then(|actions| {
            actions.iter().rev().find_map(|action| {
                (action.get("actionType").and_then(Value::as_str) == Some("say"))
                    .then(|| {
                        action
                            .get("output")
                            .and_then(|value| value.get("message"))
                            .and_then(Value::as_str)
                            .and_then(|value| normalize_optional_text(Some(value)))
                    })
                    .flatten()
            })
        });

    last_say.or_else(|| extract_legacy_kiro_context_bot_output(document))
}

fn extract_legacy_kiro_context_bot_output(document: &Value) -> Option<String> {
    let messages = document
        .get("context")
        .and_then(|value| value.get("messages"))
        .and_then(Value::as_array)?;
    messages.iter().rev().find_map(|message| {
        if message.get("role").and_then(Value::as_str) != Some("bot") {
            return None;
        }

        message
            .get("entries")
            .and_then(Value::as_array)
            .and_then(|entries| {
                entries.iter().rev().find_map(|entry| {
                    if entry.get("type").and_then(Value::as_str) != Some("text") {
                        return None;
                    }

                    entry
                        .get("text")
                        .and_then(Value::as_str)
                        .and_then(|value| normalize_optional_text(Some(value)))
                })
            })
    })
}

fn normalize_kiro_execution_status(value: Option<&str>) -> Option<String> {
    match value? {
        "succeed" | "success" | "completed" => Some("completed".to_string()),
        "aborted" | "cancelled" | "canceled" => Some("aborted".to_string()),
        "failed" | "error" => Some("failed".to_string()),
        "running" | "in_progress" => Some("running".to_string()),
        other => normalize_optional_text(Some(other)),
    }
}

fn is_substantive_kiro_chat_output(value: &str) -> bool {
    value.contains('\n') || value.chars().count() >= 48 || value.split_whitespace().count() >= 12
}

fn derive_session_models(
    session_model: Option<String>,
    request_models: Vec<String>,
) -> (Option<String>, Option<String>) {
    let first_request_model = request_models.first().cloned();
    let last_request_model = request_models.last().cloned();

    if session_model
        .as_deref()
        .map(is_generic_kiro_model)
        .unwrap_or(true)
    {
        (
            first_request_model.or_else(|| session_model.clone()),
            last_request_model.or(session_model),
        )
    } else {
        (session_model.clone(), session_model)
    }
}

fn prefer_specific_kiro_model(primary: Option<String>, fallback: Option<String>) -> Option<String> {
    match (primary, fallback) {
        (Some(primary), Some(fallback)) => {
            if is_generic_kiro_model(&primary) && !is_generic_kiro_model(&fallback) {
                Some(fallback)
            } else {
                Some(primary)
            }
        }
        (Some(primary), None) => Some(primary),
        (None, Some(fallback)) => Some(fallback),
        (None, None) => None,
    }
}

fn is_generic_kiro_model(value: &str) -> bool {
    value.eq_ignore_ascii_case("agent") || value.eq_ignore_ascii_case("unknown")
}

fn parse_kiro_epoch_ms_value(value: &Value) -> Option<DateTime<Utc>> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()))
        .and_then(time::from_unix_ms)
}

fn normalize_role(value: Option<&str>) -> String {
    match value.unwrap_or("unknown") {
        "user" => "user",
        "assistant" => "assistant",
        "system" => "system",
        "tool" => "tool",
        _ => "unknown",
    }
    .to_string()
}

fn derive_session_model(document: &Value, default_model_title: Option<String>) -> Option<String> {
    normalize_optional_text(document.get("selectedModel").and_then(Value::as_str))
        .or_else(|| {
            document
                .get("history")
                .and_then(Value::as_array)
                .and_then(|history| {
                    history.iter().find_map(|entry| {
                        extract_entry_model(entry, default_model_title.as_deref())
                    })
                })
        })
        .or_else(|| default_model_title.map(|value| normalize_agent_model_name(&value)))
}

fn extract_entry_model(entry: &Value, default_model_title: Option<&str>) -> Option<String> {
    let model_title = entry
        .get("promptLogs")
        .and_then(Value::as_array)
        .and_then(|logs| {
            logs.iter().find_map(|log| {
                normalize_optional_text(log.get("modelTitle").and_then(Value::as_str))
            })
        });
    let completion_model = entry
        .get("promptLogs")
        .and_then(Value::as_array)
        .and_then(|logs| {
            logs.iter().find_map(|log| {
                normalize_optional_text(
                    log.get("completionOptions")
                        .and_then(|value| value.get("model"))
                        .and_then(Value::as_str),
                )
            })
        });

    completion_model
        .as_deref()
        .filter(|value| !value.eq_ignore_ascii_case("agent"))
        .map(ToString::to_string)
        .or(model_title)
        .or_else(|| completion_model.map(|value| normalize_agent_model_name(&value)))
        .or_else(|| default_model_title.map(normalize_agent_model_name))
}

fn extract_prompt_logs_prompt(entry: &Value) -> Option<String> {
    entry
        .get("promptLogs")
        .and_then(Value::as_array)
        .and_then(|logs| {
            logs.iter()
                .find_map(|log| normalize_optional_text(log.get("prompt").and_then(Value::as_str)))
        })
}

fn extract_prompt_logs_completion(entry: &Value) -> Option<String> {
    entry
        .get("promptLogs")
        .and_then(Value::as_array)
        .and_then(|logs| {
            logs.iter().find_map(|log| {
                normalize_optional_text(log.get("completion").and_then(Value::as_str))
            })
        })
}

fn resolve_embedded_kiro_assistant_execution_id(
    history: &[Value],
    history_index: usize,
    raw_role: Option<&str>,
    text_content: Option<&str>,
) -> Option<String> {
    if raw_role != Some("user")
        || !text_content
            .map(looks_like_kiro_assistant_transcript)
            .unwrap_or(false)
    {
        return None;
    }

    history
        .get(history_index + 1)
        .and_then(extract_kiro_placeholder_execution_id)
}

fn looks_like_kiro_assistant_transcript(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }

    trimmed.starts_with("Assistant message:")
        || trimmed.starts_with("Tool: ")
        || trimmed.starts_with("ToolResult:")
        || trimmed.contains("<Tool>")
        || trimmed.contains("</Tool>")
        || (trimmed.contains("Tool: ") && trimmed.contains("ToolResult:"))
        || (trimmed.contains("Assistant message:") && trimmed.contains("Tool: "))
}

fn extract_kiro_placeholder_execution_id(entry: &Value) -> Option<String> {
    let message = entry.get("message")?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    if message.get("content").and_then(Value::as_str) != Some("On it.") {
        return None;
    }

    normalize_optional_text(entry.get("executionId").and_then(Value::as_str))
}

fn normalize_agent_model_name(value: &str) -> String {
    if value.eq_ignore_ascii_case("agent") {
        "Agent".to_string()
    } else {
        value.to_string()
    }
}

fn extract_kiro_message_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => normalize_content_text(text),
        Value::Array(items) => {
            let mut parts = Vec::<String>::new();
            for item in items {
                match item {
                    Value::String(text) => {
                        if let Some(text) = normalize_content_text(text) {
                            parts.push(text);
                        }
                    }
                    Value::Object(_) => {
                        let item_type =
                            item.get("type").and_then(Value::as_str).unwrap_or_default();
                        if item_type.eq_ignore_ascii_case("imageUrl") {
                            continue;
                        }

                        if let Some(text) = item.get("text").and_then(Value::as_str) {
                            if let Some(text) = normalize_content_text(text) {
                                parts.push(text);
                            }
                        }
                    }
                    _ => {}
                }
            }

            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        _ => None,
    }
}

fn normalize_content_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || is_inline_image_data_url(trimmed) {
        return None;
    }

    Some(trimmed.to_string())
}

fn is_inline_image_data_url(value: &str) -> bool {
    value.starts_with("data:image/")
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

fn merge_optional_max(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn earlier_timestamp(
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

fn later_timestamp(
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

fn serialize_locator(
    history_index: usize,
    message_id: Option<String>,
    execution_id: Option<String>,
) -> String {
    serde_json::to_string(&KiroLocator {
        history_index,
        message_id,
        execution_id,
    })
    .unwrap_or_else(|_| format!("{{\"history_index\":{history_index}}}"))
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
    lowercase == "new session"
        || lowercase.starts_with("<environment_context>")
        || lowercase.starts_with("<turn_aborted>")
        || lowercase.starts_with("<image")
        || lowercase.starts_with("<permissions instructions>")
        || lowercase.starts_with("<agents")
        || lowercase.starts_with("# agents.md instructions for ")
}

#[cfg(test)]
mod tests {
    use super::KiroAdapter;
    use super::*;
    use crate::sources::SourceAdapter;
    use crate::utils::cache::BoundedCache;
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn make_temp_root() -> PathBuf {
        let mut root = std::env::temp_dir();
        root.push(format!(
            "kiro-adapter-test-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        root
    }

    fn write_temp_file(relative: &str, content: &str) -> PathBuf {
        let root = make_temp_root();
        write_temp_file_in_root(&root, relative, content)
    }

    fn write_temp_file_in_root(root: &Path, relative: &str, content: &str) -> PathBuf {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn parses_workspace_session_without_persisting_inline_image_payloads() {
        let path = write_temp_file(
            "kiro/workspace-sessions/workspace-a/session-1.json",
            r#"{
  "title": "New Session",
  "sessionId": "session-1",
  "defaultModelTitle": "Agent",
  "selectedModel": "claude-sonnet-4.5",
  "history": [
    {
      "message": {
        "role": "user",
        "content": [
          { "type": "text", "text": "First prompt" }
        ],
        "id": "msg-user-1"
      },
      "promptLogs": [
        {
          "modelTitle": "Agent",
          "prompt": "<user>\nFirst prompt",
          "completion": "",
          "completionOptions": {
            "model": "agent"
          }
        }
      ]
    },
    {
      "message": {
        "role": "assistant",
        "content": "On it.",
        "id": "msg-assistant-1"
      },
      "executionId": "exec-1"
    },
    {
      "message": {
        "role": "user",
        "content": [
          {
            "type": "imageUrl",
            "imageUrl": { "url": "data:image/png;base64,AAAA" }
          },
          { "type": "text", "text": "Image follow-up" }
        ],
        "id": "msg-user-2"
      }
    },
    {
      "message": {
        "role": "assistant",
        "content": "Done.",
        "id": "msg-assistant-2"
      },
      "executionId": "exec-2"
    }
  ]
}"#,
        );
        fs::write(
            path.parent().unwrap().join("sessions.json"),
            r#"[
  {
    "sessionId": "session-1",
    "title": "New Session",
    "dateCreated": "1775460729225",
    "workspaceDirectory": "E:\\demo"
  }
]"#,
        )
        .unwrap();

        let adapter = KiroAdapter::default();
        let sessions = adapter.parse(&path).unwrap();

        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.source_app, "kiro");
        assert_eq!(session.external_session_id.as_deref(), Some("session-1"));
        assert_eq!(session.title.as_deref(), Some("First prompt"));
        assert_eq!(session.model_first.as_deref(), Some("claude-sonnet-4.5"));
        assert_eq!(session.message_count, 4);
        assert_eq!(session.total_input_tokens, 0);
        assert_eq!(session.total_output_tokens, 0);
        assert_eq!(session.requests.len(), 2);
        assert!(session.events.is_empty());
        assert_eq!(
            session.requests[0].source_request_id.as_deref(),
            Some("exec-1")
        );
        assert_eq!(
            session.requests[1].source_request_id.as_deref(),
            Some("exec-2")
        );
        assert_eq!(session.message_count, 4);
        assert!(session.requests[0].input_tokens.unwrap_or(0) > 0);
        assert!(session.requests[0].output_tokens.unwrap_or(0) > 0);
        assert!(session.conversation_checksum.len() > 20);
    }

    #[test]
    fn groups_multiturn_history_into_interleaved_requests() {
        let path = write_temp_file(
            "kiro/workspace-sessions/workspace-b/session-2.json",
            r#"{
  "title": "New Session",
  "sessionId": "session-2",
  "defaultModelTitle": "Agent",
  "selectedModel": "claude-sonnet-4.5",
  "history": [
    {
      "message": {
        "role": "user",
        "content": [{"type": "text", "text": "Turn one"}],
        "id": "user-1"
      },
      "promptLogs": [
        {
          "modelTitle": "Agent",
          "prompt": "<user>\nTurn one",
          "completion": "",
          "completionOptions": { "model": "agent" }
        }
      ]
    },
    {
      "message": {
        "role": "assistant",
        "content": "Reply one",
        "id": "assistant-1"
      },
      "executionId": "exec-1"
    },
    {
      "message": {
        "role": "user",
        "content": [{"type": "text", "text": "Turn two"}],
        "id": "user-2"
      }
    },
    {
      "message": {
        "role": "assistant",
        "content": "Reply two",
        "id": "assistant-2"
      },
      "executionId": "exec-2",
      "promptLogs": [
        {
          "modelTitle": "Agent",
          "prompt": "<user>\nTurn two",
          "completion": "Reply two",
          "completionOptions": { "model": "agent" }
        }
      ]
    },
    {
      "message": {
        "role": "user",
        "content": [{"type": "text", "text": "Turn three"}],
        "id": "user-3"
      },
      "promptLogs": [
        {
          "modelTitle": "Agent",
          "prompt": "<user>\nTurn three",
          "completion": "",
          "completionOptions": { "model": "agent" }
        }
      ]
    },
    {
      "message": {
        "role": "assistant",
        "content": "Reply three",
        "id": "assistant-3"
      },
      "executionId": "exec-3"
    }
  ]
}"#,
        );

        let adapter = KiroAdapter::default();
        let sessions = adapter.parse(&path).unwrap();
        let session = &sessions[0];

        assert_eq!(session.requests.len(), 3);
        assert_eq!(session.message_count, 6);
        assert_eq!(session.requests[0].message_count, 2);
        assert_eq!(session.requests[1].message_count, 2);
        assert_eq!(session.requests[2].message_count, 2);
        assert!(session.events.is_empty());
        assert!(session
            .requests
            .iter()
            .all(|request| request.input_tokens.unwrap_or(0) > 0));
        assert!(session
            .requests
            .iter()
            .all(|request| request.output_tokens.unwrap_or(0) > 0));
    }

    #[test]
    fn falls_back_to_default_model_title_when_selected_model_is_empty() {
        let path = write_temp_file(
            "kiro/workspace-sessions/workspace-c/session-3.json",
            r#"{
  "title": "New Session",
  "sessionId": "session-3",
  "defaultModelTitle": "Agent",
  "selectedModel": "",
  "history": [
    {
      "message": {
        "role": "user",
        "content": [{"type": "text", "text": "Question"}],
        "id": "user-1"
      }
    },
    {
      "message": {
        "role": "assistant",
        "content": "Answer",
        "id": "assistant-1"
      },
      "executionId": "exec-1"
    }
  ]
}"#,
        );

        let adapter = KiroAdapter::default();
        let sessions = adapter.parse(&path).unwrap();
        let session = &sessions[0];

        assert_eq!(session.model_first.as_deref(), Some("Agent"));
        assert_eq!(session.model_last.as_deref(), Some("Agent"));
        assert_eq!(session.requests[0].model.as_deref(), Some("Agent"));
    }

    #[test]
    fn supplements_output_tokens_and_model_from_chat_snapshots() {
        let root = make_temp_root();
        let path = write_temp_file_in_root(
            &root,
            "kiro/workspace-sessions/workspace-d/session-4.json",
            r#"{
  "title": "New Session",
  "sessionId": "session-4",
  "defaultModelTitle": "Agent",
  "selectedModel": "",
  "history": [
    {
      "message": {
        "role": "user",
        "content": [{"type": "text", "text": "Explain ThreadLocal"}],
        "id": "user-1"
      },
      "promptLogs": [
        {
          "modelTitle": "Agent",
          "prompt": "<user>\nExplain ThreadLocal",
          "completion": "",
          "completionOptions": { "model": "agent" }
        }
      ]
    },
    {
      "message": {
        "role": "assistant",
        "content": "On it.",
        "id": "assistant-1"
      },
      "executionId": "exec-chat-1"
    }
  ]
}"#,
        );
        write_temp_file_in_root(
            &root,
            "kiro/session-cache/run-a.chat",
            r#"{
  "executionId": "exec-chat-1",
  "chat": [
    { "role": "human", "content": "Explain ThreadLocal" },
    { "role": "bot", "content": "Let me inspect the code first." },
    { "role": "bot", "content": "ThreadLocal stores per-thread state so each request sees isolated context.\nUse remove() in finally blocks to avoid leaks." }
  ],
  "metadata": {
    "modelId": "claude-sonnet-4.5",
    "endTime": 1775460802816
  }
}"#,
        );
        write_temp_file_in_root(
            &root,
            "kiro/session-cache/run-b.chat",
            r#"{
  "executionId": "exec-chat-1",
  "chat": [
    { "role": "human", "content": "Explain ThreadLocal" },
    { "role": "bot", "content": "Short draft" }
  ],
  "metadata": {
    "modelId": "claude-sonnet-4.5",
    "endTime": 1775460700000
  }
}"#,
        );

        let adapter = KiroAdapter::default();
        let sessions = adapter.parse(&path).unwrap();
        let session = &sessions[0];

        assert_eq!(session.model_first.as_deref(), Some("claude-sonnet-4.5"));
        assert_eq!(session.model_last.as_deref(), Some("claude-sonnet-4.5"));
        assert_eq!(
            session.requests[0].model.as_deref(),
            Some("claude-sonnet-4.5")
        );
        assert_eq!(
            session.requests[0]
                .source_updated_at
                .map(|value| value.timestamp_millis()),
            Some(1775460802816)
        );
        assert!(session.events.is_empty());
        assert!(session.requests[0].output_tokens.unwrap_or(0) > 10);

        let fingerprints = adapter.fingerprint_paths(&path);
        assert!(fingerprints.iter().any(|candidate| {
            candidate.file_name().and_then(|value| value.to_str()) == Some("run-a.chat")
        }));
    }

    #[test]
    fn supplements_output_tokens_status_and_model_from_legacy_execution_snapshots() {
        let root = make_temp_root();
        let path = write_temp_file_in_root(
            &root,
            "kiro/workspace-sessions/workspace-e/session-5.json",
            r#"{
  "title": "New Session",
  "sessionId": "session-5",
  "defaultModelTitle": "Agent",
  "selectedModel": "claude-sonnet-4.5",
  "history": [
    {
      "message": {
        "role": "user",
        "content": [{ "type": "text", "text": "Fix the ffmpeg filter graph bug" }],
        "id": "user-1"
      },
      "promptLogs": [
        {
          "modelTitle": "Agent",
          "prompt": "<user>\nFix the ffmpeg filter graph bug",
          "completion": "",
          "completionOptions": { "model": "agent" }
        }
      ]
    },
    {
      "message": {
        "role": "assistant",
        "content": "On it.",
        "id": "assistant-1"
      },
      "executionId": "legacy-exec-1"
    }
  ]
}"#,
        );
        write_temp_file_in_root(
            &root,
            "kiro/cache/legacy-exec-1-snapshot",
            r#"{
  "executionId": "legacy-exec-1",
  "workflowType": "chat-agent",
  "status": "succeed",
  "endTime": 1775460902816,
  "actions": [
    {
      "actionType": "say",
      "output": {
        "message": "I found the FFmpeg filter chain bug. The temporary stream label is reused without a separator, so the second crop stage is parsed as trailing garbage. I split the chain into explicit stages and verified the command layout."
      }
    }
  ]
}"#,
        );

        let adapter = KiroAdapter::default();
        let sessions = adapter.parse(&path).unwrap();
        let session = &sessions[0];

        assert_eq!(session.model_first.as_deref(), Some("claude-sonnet-4.5"));
        assert_eq!(session.model_last.as_deref(), Some("claude-sonnet-4.5"));
        assert_eq!(
            session.requests[0].model.as_deref(),
            Some("claude-sonnet-4.5")
        );
        assert_eq!(session.requests[0].status.as_deref(), Some("completed"));
        assert!(session.requests[0].output_tokens.unwrap_or(0) > 20);

        let fingerprints = adapter.fingerprint_paths(&path);
        assert!(fingerprints.iter().any(|candidate| {
            candidate.file_name().and_then(|value| value.to_str()) == Some("legacy-exec-1-snapshot")
        }));
    }

    #[test]
    fn reclassifies_embedded_agent_transcripts_as_assistant_output() {
        let path = write_temp_file(
            "kiro/workspace-sessions/workspace-f/session-6.json",
            r#"{
  "title": "New Session",
  "sessionId": "session-6",
  "defaultModelTitle": "Agent",
  "selectedModel": "claude-sonnet-4.5",
  "history": [
    {
      "message": {
        "role": "user",
        "content": "Assistant message: I fixed the dashboard access rules.\nTool: strReplace - {\"path\":\"frontend/src/views/dashboard/index.vue\",\"newStr\":\"<template>\n  <div>updated dashboard</div>\n</template>\"}\nToolResult: SUCCESS - Replaced text in frontend/src/views/dashboard/index.vue",
        "id": "transcript-1"
      }
    },
    {
      "message": {
        "role": "assistant",
        "content": "On it.",
        "id": "assistant-placeholder-1"
      },
      "executionId": "exec-embedded-1",
      "promptLogs": [
        {
          "modelTitle": "Agent",
          "prompt": "<user>\nPlease tighten dashboard permissions",
          "completion": "Short summary",
          "completionOptions": { "model": "agent" }
        }
      ]
    },
    {
      "message": {
        "role": "user",
        "content": [{ "type": "text", "text": "Actual user follow-up" }],
        "id": "user-2"
      }
    },
    {
      "message": {
        "role": "assistant",
        "content": "On it.",
        "id": "assistant-2"
      },
      "executionId": "exec-2",
      "promptLogs": [
        {
          "modelTitle": "Agent",
          "prompt": "<user>\nActual user follow-up",
          "completion": "Done",
          "completionOptions": { "model": "agent" }
        }
      ]
    }
  ]
}"#,
        );

        let adapter = KiroAdapter::default();
        let sessions = adapter.parse(&path).unwrap();
        let session = &sessions[0];

        assert_eq!(session.title.as_deref(), Some("Actual user follow-up"));
        assert_eq!(session.requests.len(), 2);
        assert_eq!(
            session.requests[0].source_request_id.as_deref(),
            Some("exec-embedded-1")
        );
        assert_eq!(session.requests[0].input_tokens.unwrap_or(0), 10);
        assert!(session.requests[0].output_tokens.unwrap_or(0) > 40);
        assert_eq!(session.total_input_tokens, 0);
        assert_eq!(session.total_output_tokens, 0);
    }

    #[test]
    fn reclassifies_embedded_tool_tag_transcripts_as_assistant_output() {
        let path = write_temp_file(
            "kiro/workspace-sessions/workspace-g/session-7.json",
            r##"{
  "title": "Long Doc Session",
  "sessionId": "session-7",
  "defaultModelTitle": "Agent",
  "selectedModel": "claude-sonnet-4.5",
  "history": [
    {
      "message": {
        "role": "user",
        "content": "Please create the first document.\n<Tool>\nfsWrite\n</Tool>\n<Tool>\n{\"path\":\"docs/phase-2/aop.md\",\"text\":\"# Spring AOP\n\nA very long generated article body goes here.\nIt contains multiple sections and examples.\"}\n</Tool>",
        "id": "transcript-tool-1"
      }
    },
    {
      "message": {
        "role": "assistant",
        "content": "On it.",
        "id": "assistant-placeholder-1"
      },
      "executionId": "exec-tool-1",
      "promptLogs": [
        {
          "modelTitle": "Agent",
          "prompt": "<user>\nPlease create the first document.",
          "completion": "Created docs/phase-2/aop.md",
          "completionOptions": { "model": "agent" }
        }
      ]
    }
  ]
}"##,
        );

        let adapter = KiroAdapter::default();
        let sessions = adapter.parse(&path).unwrap();
        let session = &sessions[0];

        assert_eq!(session.requests.len(), 1);
        assert_eq!(
            session.requests[0].source_request_id.as_deref(),
            Some("exec-tool-1")
        );
        assert_eq!(session.requests[0].input_tokens.unwrap_or(0), 9);
        assert!(session.requests[0].output_tokens.unwrap_or(0) > 20);
    }

    #[test]
    fn uses_per_request_execution_end_times_for_kiro_usage_events() {
        let root = make_temp_root();
        let path = write_temp_file_in_root(
            &root,
            "kiro/workspace-sessions/workspace-h/session-8.json",
            r#"{
  "title": "New Session",
  "sessionId": "session-8",
  "defaultModelTitle": "Agent",
  "selectedModel": "claude-sonnet-4.5",
  "history": [
    {
      "message": {
        "role": "user",
        "content": [{"type": "text", "text": "First task"}],
        "id": "user-1"
      },
      "promptLogs": [
        {
          "modelTitle": "Agent",
          "prompt": "<user>\nFirst task",
          "completion": "",
          "completionOptions": { "model": "agent" }
        }
      ]
    },
    {
      "message": {
        "role": "assistant",
        "content": "On it.",
        "id": "assistant-1"
      },
      "executionId": "exec-1"
    },
    {
      "message": {
        "role": "user",
        "content": [{"type": "text", "text": "Second task"}],
        "id": "user-2"
      },
      "promptLogs": [
        {
          "modelTitle": "Agent",
          "prompt": "<user>\nSecond task",
          "completion": "",
          "completionOptions": { "model": "agent" }
        }
      ]
    },
    {
      "message": {
        "role": "assistant",
        "content": "On it.",
        "id": "assistant-2"
      },
      "executionId": "exec-2"
    }
  ]
}"#,
        );
        write_temp_file_in_root(
            &root,
            "kiro/cache/exec-1-snapshot",
            r#"{
  "executionId": "exec-1",
  "workflowType": "chat-agent",
  "status": "succeed",
  "endTime": 1775461001000,
  "actions": [
    {
      "actionType": "say",
      "output": { "message": "First reply with enough detail to be substantive." }
    }
  ]
}"#,
        );
        write_temp_file_in_root(
            &root,
            "kiro/cache/exec-2-snapshot",
            r#"{
  "executionId": "exec-2",
  "workflowType": "chat-agent",
  "status": "succeed",
  "endTime": 1775461009000,
  "actions": [
    {
      "actionType": "say",
      "output": { "message": "Second reply with even more detail to stay substantive." }
    }
  ]
}"#,
        );

        let adapter = KiroAdapter::default();
        let sessions = adapter.parse(&path).unwrap();
        let session = &sessions[0];

        assert_eq!(session.requests.len(), 2);
        assert_eq!(
            session.requests[0]
                .source_updated_at
                .map(|value| value.timestamp_millis()),
            Some(1775461001000)
        );
        assert_eq!(
            session.requests[1]
                .source_updated_at
                .map(|value| value.timestamp_millis()),
            Some(1775461009000)
        );
        assert!(session.events.is_empty());
    }

    #[test]
    fn cached_kiro_chat_index_refreshes_changed_snapshots_automatically() {
        let root = make_temp_root();
        let path = write_temp_file_in_root(
            &root,
            "kiro/workspace-sessions/workspace-i/session-9.json",
            r#"{
  "title": "New Session",
  "sessionId": "session-9",
  "defaultModelTitle": "Agent",
  "selectedModel": "claude-sonnet-4.5",
  "history": [
    {
      "message": {
        "role": "user",
        "content": [{"type": "text", "text": "Refresh snapshot"}],
        "id": "user-1"
      },
      "promptLogs": [
        {
          "modelTitle": "Agent",
          "prompt": "<user>\nRefresh snapshot",
          "completion": "",
          "completionOptions": { "model": "agent" }
        }
      ]
    },
    {
      "message": {
        "role": "assistant",
        "content": "On it.",
        "id": "assistant-1"
      },
      "executionId": "exec-refresh"
    }
  ]
}"#,
        );
        let snapshot_path = write_temp_file_in_root(
            &root,
            "kiro/cache/exec-refresh-snapshot",
            r#"{
  "executionId": "exec-refresh",
  "workflowType": "chat-agent",
  "status": "succeed",
  "endTime": 1775461101000,
  "actions": [
    {
      "actionType": "say",
      "output": { "message": "Short draft response." }
    }
  ]
}"#,
        );

        let adapter = KiroAdapter::default();
        let initial = adapter.parse(&path).unwrap();
        let initial_output_tokens = initial[0].requests[0].output_tokens.unwrap_or(0);

        fs::write(
            &snapshot_path,
            r#"{
  "executionId": "exec-refresh",
  "workflowType": "chat-agent",
  "status": "succeed",
  "endTime": 1775461109000,
  "actions": [
    {
      "actionType": "say",
      "output": {
        "message": "This refreshed response is much longer and should produce a larger token estimate than the cached draft."
      }
    }
  ]
}"#,
        )
        .unwrap();

        let refreshed = adapter.parse(&path).unwrap();
        assert!(refreshed[0].requests[0].output_tokens.unwrap_or(0) > initial_output_tokens);
        assert_eq!(
            refreshed[0].requests[0]
                .source_updated_at
                .map(|value| value.timestamp_millis()),
            Some(1775461109000)
        );
    }

    #[test]
    fn skips_empty_workspace_session_shells() {
        let root = make_temp_root();
        let path = write_temp_file_in_root(
            &root,
            "kiro/workspace-sessions/workspace-j/empty.json",
            r#"{
  "title": "Empty Session",
  "sessionId": "empty-session",
  "history": []
}"#,
        );

        let adapter = KiroAdapter::default();

        assert!(adapter
            .discover_paths(path.parent().unwrap())
            .unwrap()
            .is_empty());
        assert!(adapter.parse(&path).unwrap().is_empty());
    }

    #[test]
    fn chat_index_does_not_walk_storage_root_without_snapshot_dirs() {
        let root = make_temp_root();
        let path = write_temp_file_in_root(
            &root,
            "kiro/workspace-sessions/workspace-k/session-10.json",
            r#"{
  "title": "New Session",
  "sessionId": "session-10",
  "defaultModelTitle": "Agent",
  "selectedModel": "claude-sonnet-4.5",
  "history": [
    {
      "message": {
        "role": "user",
        "content": [{"type": "text", "text": "Check root fallback"}],
        "id": "user-1"
      }
    },
    {
      "message": {
        "role": "assistant",
        "content": "On it.",
        "id": "assistant-1"
      },
      "executionId": "exec-root-fallback"
    }
  ]
}"#,
        );
        write_temp_file_in_root(
            &root,
            "kiro/exec-root-fallback-snapshot",
            r#"{
  "executionId": "exec-root-fallback",
  "workflowType": "chat-agent",
  "status": "succeed",
  "endTime": 1775461201000,
  "actions": [
    {
      "actionType": "say",
      "output": { "message": "This root file should not be indexed without cache directories." }
    }
  ]
}"#,
        );

        let adapter = KiroAdapter::default();
        let sessions = adapter.parse(&path).unwrap();
        let session = &sessions[0];

        assert_ne!(
            session.requests[0]
                .source_updated_at
                .map(|value| value.timestamp_millis()),
            Some(1775461201000)
        );
        assert!(adapter
            .fingerprint_paths(&path)
            .iter()
            .all(
                |candidate| candidate.file_name().and_then(|value| value.to_str())
                    != Some("exec-root-fallback-snapshot")
            ));
    }

    #[test]
    fn kiro_chat_index_cache_is_bounded() {
        let mut cache = BoundedCache::new(KIRO_CHAT_INDEX_CACHE_CAPACITY);

        for index in 0..(KIRO_CHAT_INDEX_CACHE_CAPACITY + 1) {
            cache.insert(
                PathBuf::from(format!("E:/kiro-root-{index}")),
                KiroChatIndexCacheEntry {
                    index: Arc::new(KiroChatIndex::default()),
                    watch_states: Vec::new(),
                    snapshot_states: HashMap::new(),
                },
            );
        }

        assert_eq!(cache.len(), KIRO_CHAT_INDEX_CACHE_CAPACITY);
    }
}
