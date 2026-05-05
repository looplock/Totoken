use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::error::AppResult;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NormalizedSession {
    pub source_app: String,
    pub external_session_id: Option<String>,
    pub session_key: String,
    pub title: Option<String>,
    pub model_first: Option<String>,
    pub model_last: Option<String>,
    pub source_created_at: Option<DateTime<Utc>>,
    pub source_updated_at: Option<DateTime<Utc>>,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub message_count: i64,
    pub conversation_checksum: String,
    pub requests: Vec<NormalizedRequest>,
    pub events: Vec<NormalizedUsageEvent>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NormalizedRequest {
    pub source_request_id: Option<String>,
    pub sequence_no: i64,
    pub status: Option<String>,
    pub message_count: i64,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_input_tokens: Option<i64>,
    pub cache_write_input_tokens: Option<i64>,
    pub token_confidence: Option<String>,
    pub source_created_at: Option<DateTime<Utc>>,
    pub source_updated_at: Option<DateTime<Utc>>,
    pub source_locator: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NormalizedUsageEvent {
    pub event_time_utc: DateTime<Utc>,
    pub model: Option<String>,
    pub delta_input: i64,
    pub delta_output: i64,
    pub delta_total: i64,
    pub cache_read_input_tokens: i64,
    pub cache_write_input_tokens: i64,
    pub source_event_id: Option<String>,
    pub granularity: String, // request, snapshot_delta, session_total
    pub confidence: String,  // high, medium, low
}

pub trait SourceAdapter {
    fn name(&self) -> &str;
    fn parser_version(&self) -> i64 {
        1
    }
    fn can_handle(&self, path: &Path) -> bool;
    fn parse(&self, path: &Path) -> AppResult<Vec<NormalizedSession>>;
    fn has_scannable_data(&self, root_path: &Path) -> AppResult<bool> {
        Ok(!self.discover_paths(root_path)?.is_empty())
    }
    fn discover_paths(&self, root_path: &Path) -> AppResult<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for entry in WalkDir::new(root_path).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.into_path();
            if self.can_handle(&path) {
                paths.push(path);
            }
        }

        Ok(paths)
    }
    fn fingerprint_paths(&self, path: &Path) -> Vec<PathBuf> {
        vec![path.to_path_buf()]
    }
}

pub mod claude_code;
pub mod codex;
pub mod cursor;
pub mod kilocode;
pub mod kiro;
pub mod message_stream;
pub mod opencode;
