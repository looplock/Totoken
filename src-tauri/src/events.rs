use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::scanner::ScanSummary;
use crate::utils::ids;

pub const SCAN_NOTIFICATION_EVENT: &str = "scan-notification";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanNotificationEvent {
    pub id: String,
    pub status: String,
    pub trigger_type: String,
    pub source_app: Option<String>,
    pub root_path: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub files_parsed: Option<u64>,
    pub sessions_changed: Option<u64>,
    pub error_count: Option<u64>,
    pub error_message: Option<String>,
}

pub fn emit_scan_started(
    app: &AppHandle,
    trigger_type: &str,
    source_app: Option<&str>,
    root_path: &str,
) {
    let payload = ScanNotificationEvent {
        id: build_event_id("started"),
        status: "started".to_string(),
        trigger_type: trigger_type.to_string(),
        source_app: source_app.map(ToString::to_string),
        root_path: root_path.to_string(),
        started_at: Utc::now(),
        ended_at: None,
        files_parsed: None,
        sessions_changed: None,
        error_count: None,
        error_message: None,
    };

    let _ = app.emit(SCAN_NOTIFICATION_EVENT, payload);
}

pub fn emit_scan_completed(app: &AppHandle, source_app: Option<&str>, summary: &ScanSummary) {
    let now = Utc::now();
    let payload = ScanNotificationEvent {
        id: build_event_id("completed"),
        status: "completed".to_string(),
        trigger_type: summary.trigger_type.clone(),
        source_app: source_app.map(ToString::to_string),
        root_path: summary.root_path.clone(),
        started_at: summary.started_at,
        ended_at: Some(now),
        files_parsed: Some(summary.files_parsed),
        sessions_changed: Some(summary.sessions_changed),
        error_count: Some(summary.error_count),
        error_message: None,
    };

    let _ = app.emit(SCAN_NOTIFICATION_EVENT, payload);
}

pub fn emit_scan_failed(
    app: &AppHandle,
    trigger_type: &str,
    source_app: Option<&str>,
    root_path: &str,
    started_at: DateTime<Utc>,
    error_message: &str,
) {
    let now = Utc::now();
    let payload = ScanNotificationEvent {
        id: build_event_id("failed"),
        status: "failed".to_string(),
        trigger_type: trigger_type.to_string(),
        source_app: source_app.map(ToString::to_string),
        root_path: root_path.to_string(),
        started_at,
        ended_at: Some(now),
        files_parsed: None,
        sessions_changed: None,
        error_count: None,
        error_message: Some(error_message.to_string()),
    };

    let _ = app.emit(SCAN_NOTIFICATION_EVENT, payload);
}

fn build_event_id(status: &str) -> String {
    format!("scan-{status}-{}", ids::new_uuid())
}
