use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanRecordsListQuery {
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanRunListItem {
    pub id: String,
    pub trigger_type: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i64>,
    pub files_seen: i64,
    pub files_parsed: i64,
    pub files_skipped: i64,
    pub files_failed: i64,
    pub sessions_changed: i64,
    pub error_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanRecordsListResponse {
    pub items: Vec<ScanRunListItem>,
}
