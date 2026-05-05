use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::db::DbPool;
use crate::error::{AppError, AppResult};
use crate::models::{
    MessageListQuery, MessageListResponse, MessageRequestItem, MessageSessionSummary,
    MessageUsageEventItem, ScanRecordsListQuery, ScanRecordsListResponse, ScanRunListItem,
    SessionFacetItem, SessionFacets, SessionListItem, SessionListPagination, SessionListQuery,
    SessionListResponse, SessionListSummary, StatisticsActivity, StatisticsActivityMetric,
    StatisticsDetailRow, StatisticsDistributionRow, StatisticsMetricValue, StatisticsOverview,
    StatisticsQuery, StatisticsRange, StatisticsSummary, StatisticsTrend,
};
use crate::pricing::{estimate_usage_cost, ModelPricing};
use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, TimeZone, Timelike, Utc,
};
use rusqlite::{params, params_from_iter, OptionalExtension, Row};

const DEFAULT_PAGE_SIZE: u64 = 25;
const MAX_PAGE_SIZE: u64 = 100;
const DEFAULT_SCAN_RUN_LIMIT: u64 = 18;
const MAX_SCAN_RUN_LIMIT: u64 = 100;
const ALLOWED_SOURCE_STATES: &[&str] = &["synced", "archived", "deleted", "missing"];
const ALLOWED_SOURCE_APPS: &[&str] = &[
    "claude_code",
    "codex",
    "cursor",
    "opencode",
    "kilocode",
    "kiro",
];
const SUPPORTED_SOURCE_APP_SQL_FILTER: &str =
    "'claude_code', 'codex', 'cursor', 'opencode', 'kilocode', 'kiro'";
const ALLOWED_STATISTICS_PERIODS: &[&str] = &["1d", "7d", "30d", "custom"];
const ALLOWED_STATISTICS_GRANULARITIES: &[&str] = &["hour", "day", "week", "month"];
const ALLOWED_SORT_FIELDS: &[&str] = &[
    "name",
    "sourceApp",
    "model",
    "inputTokens",
    "outputTokens",
    "totalTokens",
    "estimatedCostUsd",
    "lastUpdated",
    "messages",
    "sourceState",
];
pub struct Repository {
    pool: DbPool,
}

#[derive(Debug, Clone)]
struct NormalizedSessionListQuery {
    page: u64,
    page_size: u64,
    q: Option<String>,
    source_apps: Vec<String>,
    source_states: Vec<String>,
    sort_by: String,
    sort_order: String,
}

#[derive(Debug, Clone)]
struct NormalizedSessionActivityQuery {
    session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct NormalizedStatisticsQuery {
    q: Option<String>,
    source_app: Option<String>,
    model: Option<String>,
    period: String,
    granularity: String,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    previous_start_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct StatisticsEventRecord {
    session_id: String,
    event_time_utc: DateTime<Utc>,
    source_app: String,
    model: String,
    delta_input: i64,
    delta_output: i64,
    delta_total: i64,
    cache_read_input_tokens: i64,
    cache_write_input_tokens: i64,
    estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
struct StatisticsSummaryAccumulator {
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    sessions: BTreeSet<String>,
    models: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct StatisticsRowAccumulator {
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    estimated_cost_usd: f64,
    sessions: BTreeSet<String>,
    last_active_at: Option<DateTime<Utc>>,
    sparkline: Vec<i64>,
}

#[derive(Debug, Clone)]
struct TimeBucket {
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
}

mod messages;
mod scan_records;
mod sessions;
mod statistics;

impl Repository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

fn normalize_string_filters(
    values: Option<Vec<String>>,
    allowed_values: &[&str],
    label: &str,
) -> AppResult<Vec<String>> {
    let mut normalized = Vec::new();

    for value in values.unwrap_or_default() {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }

        if !allowed_values.contains(&trimmed) {
            return Err(AppError::validation(format!("Invalid {label}: {trimmed}")));
        }

        if normalized.iter().any(|existing| existing == trimmed) {
            continue;
        }

        normalized.push(trimmed.to_string());
    }

    Ok(normalized)
}

fn normalize_session_name(title: Option<&str>) -> String {
    match title.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => value.to_string(),
        None => "Untitled session".to_string(),
    }
}

fn normalize_source_app(value: &str) -> String {
    value.to_string()
}

fn normalize_source_state(value: String) -> String {
    match value.as_str() {
        "active" => "synced".to_string(),
        "deleted_by_user" => "deleted".to_string(),
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use super::statistics::{build_statistics_activity, build_statistics_distribution};
    use super::*;
    use crate::db::init_db_with_path;
    use chrono::{Duration, Utc};
    use rusqlite::params;
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    #[test]
    fn cursor_session_list_reads_estimates_from_session_token_totals() -> AppResult<()> {
        let db_path = temp_db_path("cursor-estimated-session-list");
        let pool = init_db_with_path(&db_path)?;
        let conn = pool.get()?;

        let session_id = "session-cursor-estimated";
        let observation_id = "observation-cursor-estimated";
        let now = Utc::now();

        conn.execute(
            "INSERT INTO sessions (
                id,
                source_app,
                external_session_id,
                session_key,
                title,
                model_first,
                model_last,
                source_created_at,
                source_updated_at,
                discovered_first_at,
                discovered_last_at,
                source_state
             ) VALUES (?1, 'cursor', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'synced')",
            params![
                session_id,
                "cursor-external",
                "cursor::session-cursor-estimated",
                "Greeting in Chinese",
                "gpt-5.5",
                "gpt-5.5",
                now,
                now,
                now,
                now,
            ],
        )?;

        conn.execute(
            "INSERT INTO session_observations (
                id,
                session_id,
                observed_at,
                input_tokens,
                output_tokens,
                total_tokens,
                conversation_checksum,
                message_count,
                source_model,
                scan_run_id
             ) VALUES (?1, ?2, ?3, 53, 3719, 3772, ?4, 5, 'gpt-5.5', 'scan-run-test')",
            params![observation_id, session_id, now, "checksum-cursor-estimated"],
        )?;

        conn.execute(
            "INSERT INTO session_token_totals (
                session_id,
                input_tokens_max,
                output_tokens_max,
                total_tokens_max,
                last_observed_at,
                last_observation_id
             ) VALUES (?1, 53, 3719, 3772, ?2, ?3)",
            params![session_id, now, observation_id],
        )?;

        conn.execute(
            "INSERT INTO session_requests (
                id,
                session_id,
                observation_id,
                source_app,
                source_request_id,
                sequence_no,
                status,
                message_count,
                model,
                input_tokens,
                output_tokens,
                total_tokens,
                token_confidence,
                source_created_at,
                source_updated_at,
                source_locator,
                cache_read_input_tokens,
                cache_write_input_tokens,
                estimated_cost_usd
             ) VALUES (
                ?1, ?2, ?3, 'cursor', ?4, 1, 'completed', 2, 'gpt-5.5',
                53, 3719, 3772, NULL, ?5, ?6, '{}', 0, 0, NULL
             )",
            params![
                "request-cursor-estimated",
                session_id,
                observation_id,
                "request-source-id",
                now,
                now,
            ],
        )?;

        let repository = Repository::new(pool.clone());
        let response = repository.sessions_list(None)?;
        let item = response
            .items
            .into_iter()
            .find(|item| item.id == session_id)
            .expect("session should be returned");

        assert_eq!(item.input_tokens, 53);
        assert_eq!(item.output_tokens, 3719);
        assert_eq!(item.total_tokens, 3772);
        assert_eq!(item.token_confidence.as_deref(), Some("high"));
        assert_eq!(item.messages, 5);

        drop(conn);
        drop(pool);
        cleanup_temp_db(&db_path);

        Ok(())
    }

    #[test]
    fn unsupported_browser_plugin_sessions_are_hidden_from_session_lists() -> AppResult<()> {
        let db_path = temp_db_path("unsupported-browser-plugin-session-list");
        let pool = init_db_with_path(&db_path)?;
        let conn = pool.get()?;
        let now = Utc::now();

        for (session_id, source_app, session_key, title) in [
            (
                "session-supported",
                "codex",
                "codex:supported",
                "Supported Session",
            ),
            (
                "session-browser-plugin",
                "browser_plugin",
                "browser_plugin:legacy",
                "Legacy Browser Plugin Session",
            ),
        ] {
            conn.execute(
                "INSERT INTO sessions (
                    id,
                    source_app,
                    external_session_id,
                    session_key,
                    title,
                    model_first,
                    model_last,
                    source_created_at,
                    source_updated_at,
                    discovered_first_at,
                    discovered_last_at,
                    source_state
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 'gpt-5', 'gpt-5', ?6, ?7, ?8, ?9, 'synced')",
                params![
                    session_id,
                    source_app,
                    session_key,
                    session_key,
                    title,
                    now,
                    now,
                    now,
                    now,
                ],
            )?;
        }

        let repository = Repository::new(pool.clone());
        let response = repository.sessions_list(None)?;
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].id, "session-supported");
        assert_eq!(response.items[0].source_app, "codex");

        let unsupported_filter = repository.sessions_list(Some(SessionListQuery {
            page: None,
            page_size: None,
            q: None,
            source_apps: Some(vec!["browser_plugin".to_string()]),
            source_states: None,
            sort_by: None,
            sort_order: None,
        }));
        assert!(unsupported_filter.is_err());

        drop(conn);
        drop(pool);
        cleanup_temp_db(&db_path);

        Ok(())
    }

    #[test]
    fn cursor_workspace_empty_shell_sessions_are_hidden_from_session_lists() -> AppResult<()> {
        let db_path = temp_db_path("cursor-workspace-empty-shell-session-list");
        let pool = init_db_with_path(&db_path)?;
        let conn = pool.get()?;
        let now = Utc::now();

        for (session_id, title, session_key) in [
            ("session-empty-shell", None, "cursor:empty-shell"),
            (
                "session-real-global",
                Some("Real Cursor Session"),
                "cursor:real-global",
            ),
        ] {
            conn.execute(
                "INSERT INTO sessions (
                    id,
                    source_app,
                    external_session_id,
                    session_key,
                    title,
                    model_first,
                    model_last,
                    source_created_at,
                    source_updated_at,
                    discovered_first_at,
                    discovered_last_at,
                    source_state
                 ) VALUES (?1, 'cursor', ?2, ?3, ?4, NULL, NULL, ?5, ?6, ?7, ?8, 'synced')",
                params![
                    session_id,
                    session_key,
                    session_key,
                    title,
                    now,
                    now,
                    now,
                    now
                ],
            )?;
        }

        conn.execute(
            "INSERT INTO session_source_refs (
                session_id, source_path, source_file_id, last_linked_at
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                "session-empty-shell",
                "C:/Cursor/User/workspaceStorage/hash/state.vscdb",
                "workspace-source-file",
                now
            ],
        )?;
        conn.execute(
            "INSERT INTO session_source_refs (
                session_id, source_path, source_file_id, last_linked_at
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                "session-real-global",
                "C:/Cursor/User/globalStorage/state.vscdb",
                "global-source-file",
                now
            ],
        )?;

        let repository = Repository::new(pool.clone());
        let response = repository.sessions_list(None)?;

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].id, "session-real-global");
        assert_eq!(response.pagination.total_items, 1);
        assert_eq!(response.facets.source_apps.len(), 1);
        assert_eq!(response.facets.source_apps[0].value, "cursor");
        assert_eq!(response.facets.source_apps[0].count, 1);

        drop(conn);
        drop(pool);
        cleanup_temp_db(&db_path);

        Ok(())
    }

    #[test]
    fn session_list_search_filter_sort_and_pagination_match_query() -> AppResult<()> {
        let db_path = temp_db_path("session-list-query");
        let pool = init_db_with_path(&db_path)?;
        let conn = pool.get()?;
        let now = Utc::now();

        for (session_id, source_app, title, source_state, updated_at) in [
            (
                "session-alpha-old",
                "codex",
                "Alpha Planning",
                "active",
                now - Duration::minutes(10),
            ),
            (
                "session-alpha-new",
                "codex",
                "Alpha Build",
                "synced",
                now - Duration::minutes(1),
            ),
            ("session-beta", "cursor", "Beta Notes", "synced", now),
        ] {
            conn.execute(
                "INSERT INTO sessions (
                    id,
                    source_app,
                    external_session_id,
                    session_key,
                    title,
                    model_first,
                    model_last,
                    source_created_at,
                    source_updated_at,
                    discovered_first_at,
                    discovered_last_at,
                    source_state
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 'gpt-5', 'gpt-5', ?6, ?6, ?6, ?6, ?7)",
                params![
                    session_id,
                    source_app,
                    session_id,
                    format!("{source_app}:{session_id}"),
                    title,
                    updated_at,
                    source_state,
                ],
            )?;
        }

        let repository = Repository::new(pool.clone());
        let response = repository.sessions_list(Some(SessionListQuery {
            page: Some(1),
            page_size: Some(1),
            q: Some("alpha".to_string()),
            source_apps: Some(vec!["codex".to_string()]),
            source_states: Some(vec!["synced".to_string()]),
            sort_by: Some("lastUpdated".to_string()),
            sort_order: Some("asc".to_string()),
        }))?;

        assert_eq!(response.pagination.total_items, 2);
        assert_eq!(response.pagination.total_pages, 2);
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].id, "session-alpha-old");
        assert_eq!(response.items[0].source_state, "synced");
        assert_eq!(
            response
                .facets
                .source_apps
                .iter()
                .find(|item| item.value == "codex")
                .map(|item| item.count),
            Some(2),
        );
        assert_eq!(
            response
                .facets
                .source_states
                .iter()
                .find(|item| item.value == "synced")
                .map(|item| item.count),
            Some(2),
        );

        drop(conn);
        drop(pool);
        cleanup_temp_db(&db_path);

        Ok(())
    }

    #[test]
    fn statistics_activity_tracks_sessions_tokens_and_cost_separately() {
        let event_time = Utc::now();
        let local_time = event_time.with_timezone(&Local);
        let day_index = local_time.weekday().num_days_from_monday() as usize;
        let hour_index = local_time.hour() as usize;

        let activity = build_statistics_activity(&[
            StatisticsEventRecord {
                session_id: "session-a".to_string(),
                event_time_utc: event_time,
                source_app: "codex".to_string(),
                model: "gpt-5".to_string(),
                delta_input: 40,
                delta_output: 60,
                delta_total: 100,
                cache_read_input_tokens: 0,
                cache_write_input_tokens: 0,
                estimated_cost_usd: Some(1.25),
            },
            StatisticsEventRecord {
                session_id: "session-a".to_string(),
                event_time_utc: event_time,
                source_app: "codex".to_string(),
                model: "gpt-5".to_string(),
                delta_input: 20,
                delta_output: 30,
                delta_total: 50,
                cache_read_input_tokens: 0,
                cache_write_input_tokens: 0,
                estimated_cost_usd: Some(0.5),
            },
            StatisticsEventRecord {
                session_id: "session-b".to_string(),
                event_time_utc: event_time,
                source_app: "cursor".to_string(),
                model: "gpt-5.5".to_string(),
                delta_input: 10,
                delta_output: 40,
                delta_total: 50,
                cache_read_input_tokens: 0,
                cache_write_input_tokens: 0,
                estimated_cost_usd: Some(0.25),
            },
        ]);

        assert_eq!(activity.sessions.matrix[day_index][hour_index], 2.0);
        assert_eq!(activity.tokens.matrix[day_index][hour_index], 200.0);
        assert!((activity.cost.matrix[day_index][hour_index] - 2.0).abs() < 1e-9);
        assert_eq!(activity.sessions.max_value, 2.0);
        assert_eq!(activity.tokens.max_value, 200.0);
        assert!((activity.cost.max_value - 2.0).abs() < 1e-9);
    }

    #[test]
    fn statistics_distribution_counts_sessions_per_app_without_model_double_counting() {
        let event_time = Utc::now();
        let distribution = build_statistics_distribution(&[
            StatisticsEventRecord {
                session_id: "session-a".to_string(),
                event_time_utc: event_time,
                source_app: "codex".to_string(),
                model: "gpt-5".to_string(),
                delta_input: 20,
                delta_output: 30,
                delta_total: 50,
                cache_read_input_tokens: 0,
                cache_write_input_tokens: 0,
                estimated_cost_usd: Some(0.25),
            },
            StatisticsEventRecord {
                session_id: "session-a".to_string(),
                event_time_utc: event_time,
                source_app: "codex".to_string(),
                model: "gpt-5-codex".to_string(),
                delta_input: 40,
                delta_output: 10,
                delta_total: 50,
                cache_read_input_tokens: 0,
                cache_write_input_tokens: 0,
                estimated_cost_usd: Some(0.5),
            },
            StatisticsEventRecord {
                session_id: "session-b".to_string(),
                event_time_utc: event_time,
                source_app: "cursor".to_string(),
                model: "gpt-5.5".to_string(),
                delta_input: 10,
                delta_output: 20,
                delta_total: 30,
                cache_read_input_tokens: 0,
                cache_write_input_tokens: 0,
                estimated_cost_usd: Some(0.1),
            },
        ]);

        assert_eq!(distribution.len(), 2);

        let codex = distribution
            .iter()
            .find(|row| row.app == "codex")
            .expect("codex distribution row should exist");
        assert_eq!(codex.sessions, 1);
        assert_eq!(codex.total_tokens, 100);
        assert!((codex.estimated_cost_usd - 0.75).abs() < 1e-9);

        let cursor = distribution
            .iter()
            .find(|row| row.app == "cursor")
            .expect("cursor distribution row should exist");
        assert_eq!(cursor.sessions, 1);
        assert_eq!(cursor.total_tokens, 30);
        assert!((cursor.estimated_cost_usd - 0.1).abs() < 1e-9);
    }

    fn temp_db_path(prefix: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("{prefix}-{}.db", Uuid::new_v4()));
        path
    }

    fn cleanup_temp_db(db_path: &Path) {
        let _ = fs::remove_file(db_path);
        let wal = PathBuf::from(format!("{}-wal", db_path.display()));
        let shm = PathBuf::from(format!("{}-shm", db_path.display()));
        let _ = fs::remove_file(wal);
        let _ = fs::remove_file(shm);
    }
}
