use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionListQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub q: Option<String>,
    pub source_apps: Option<Vec<String>>,
    pub source_states: Option<Vec<String>>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionListItem {
    pub id: String,
    pub name: String,
    pub source_app: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
    pub token_confidence: Option<String>,
    pub last_updated: DateTime<Utc>,
    pub messages: i64,
    pub source_state: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionFacetItem {
    pub value: String,
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionFacets {
    pub source_apps: Vec<SessionFacetItem>,
    pub source_states: Vec<SessionFacetItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionListPagination {
    pub page: u64,
    pub page_size: u64,
    pub total_items: u64,
    pub total_pages: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionListSummary {
    pub total_sessions: u64,
    pub selected_source_apps: Vec<String>,
    pub selected_source_states: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionListResponse {
    pub items: Vec<SessionListItem>,
    pub pagination: SessionListPagination,
    pub summary: SessionListSummary,
    pub facets: SessionFacets,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub source_app: String,
    pub external_session_id: Option<String>,
    pub session_key: String,
    pub title: Option<String>,
    pub model_first: Option<String>,
    pub model_last: Option<String>,
    pub source_created_at: Option<DateTime<Utc>>,
    pub source_updated_at: Option<DateTime<Utc>>,
    pub discovered_first_at: DateTime<Utc>,
    pub discovered_last_at: DateTime<Utc>,
    pub source_state: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageEvent {
    pub id: String,
    pub session_id: String,
    pub observation_id: Option<String>,
    pub event_time_utc: DateTime<Utc>,
    pub event_timezone: Option<String>,
    pub delta_input: i64,
    pub delta_output: i64,
    pub delta_total: i64,
    pub source_app: String,
    pub model: Option<String>,
    pub granularity: String,
    pub confidence: String,
    pub source_event_id: Option<String>,
    pub epoch_no: i64,
}
