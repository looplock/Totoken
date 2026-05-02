use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MessageListQuery {
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MessageRequestItem {
    pub id: String,
    pub session_id: String,
    pub session_name: String,
    pub source_app: String,
    pub sequence_no: i64,
    pub status: Option<String>,
    pub message_count: i64,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_input_tokens: Option<i64>,
    pub cache_write_input_tokens: Option<i64>,
    pub estimated_cost_usd: Option<f64>,
    pub token_confidence: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub source_locator_label: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MessageUsageEventItem {
    pub id: String,
    pub session_id: String,
    pub event_time_utc: DateTime<Utc>,
    pub source_app: String,
    pub model: Option<String>,
    pub delta_input: i64,
    pub delta_output: i64,
    pub delta_total: i64,
    pub cache_read_input_tokens: i64,
    pub cache_write_input_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
    pub granularity: String,
    pub confidence: String,
    pub source_event_id: Option<String>,
    pub epoch_no: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MessageSessionSummary {
    pub session_id: String,
    pub session_name: String,
    pub source_app: String,
    pub session_total_messages: i64,
    pub session_input_tokens: i64,
    pub session_output_tokens: i64,
    pub session_total_tokens: i64,
    pub session_cache_read_input_tokens: i64,
    pub session_cache_write_input_tokens: i64,
    pub session_estimated_cost_usd: Option<f64>,
    pub session_last_updated: DateTime<Utc>,
    pub session_source_state: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MessageListResponse {
    pub session: Option<MessageSessionSummary>,
    pub requests: Vec<MessageRequestItem>,
    pub usage_events: Vec<MessageUsageEventItem>,
}
