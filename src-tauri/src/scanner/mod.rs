use std::path::PathBuf;

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::db::cleanup_scan_run_history;
use crate::db::DbPool;
use crate::error::{AppError, AppResult};
use crate::pricing::{
    estimate_usage_cost_details, estimated_cost_source_sql, CostEstimationPolicy, ModelPricingCache,
};
use crate::sources::{
    claude_code::ClaudeCodeAdapter, codex::CodexAdapter, cursor::CursorAdapter,
    kilocode::KilocodeAdapter, kiro::KiroAdapter, opencode::OpencodeAdapter, NormalizedSession,
    SourceAdapter,
};
use crate::utils::{fs, ids};

pub mod fingerprint;
pub mod scheduler;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub trigger_type: String,
    pub root_path: String,
    pub started_at: DateTime<Utc>,
    pub files_seen: u64,
    pub files_parsed: u64,
    pub files_skipped: u64,
    pub files_failed: u64,
    pub sessions_changed: u64,
    pub error_count: u64,
}

#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub root_path: PathBuf,
    pub source_app: String,
    pub trigger_type: String,
    pub create_run: bool,
}

pub struct Scanner {
    pool: DbPool,
    adapters: Vec<Box<dyn SourceAdapter + Send + Sync>>,
}

#[derive(Debug, Clone)]
struct ExistingSessionRecord {
    id: String,
    external_session_id: Option<String>,
    title: Option<String>,
    model_first: Option<String>,
    model_last: Option<String>,
    source_created_at: Option<chrono::DateTime<chrono::Utc>>,
    source_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    source_state: String,
}

#[derive(Debug, Clone)]
struct SessionMergePreview {
    external_session_id: Option<String>,
    title: Option<String>,
    model_first: Option<String>,
    model_last: Option<String>,
    source_created_at: Option<chrono::DateTime<chrono::Utc>>,
    source_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    source_state: String,
}

impl Scanner {
    pub fn new(pool: DbPool) -> Self {
        Self {
            pool,
            adapters: vec![
                Box::new(ClaudeCodeAdapter),
                Box::new(CodexAdapter::default()),
                Box::new(CursorAdapter),
                Box::new(KiroAdapter::default()),
                Box::new(KilocodeAdapter),
                Box::new(OpencodeAdapter),
            ],
        }
    }

    pub fn create_scan_run(&self, trigger_type: &str) -> AppResult<String> {
        let conn = self.pool.get()?;
        let run_id = ids::new_uuid();
        conn.execute(
            "INSERT INTO scan_runs (id, trigger_type, status) VALUES (?, ?, ?)",
            params![run_id, trigger_type, "running"],
        )?;
        cleanup_scan_run_history(&conn)?;
        Ok(run_id)
    }

    pub fn complete_scan_run(&self, run_id: &str, summary: &ScanSummary) -> AppResult<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE scan_runs SET status = 'completed', ended_at = CURRENT_TIMESTAMP,
             files_seen = ?, files_parsed = ?, files_skipped = ?, files_failed = ?,
             sessions_changed = ?, error_count = ? WHERE id = ?",
            params![
                summary.files_seen,
                summary.files_parsed,
                summary.files_skipped,
                summary.files_failed,
                summary.sessions_changed,
                summary.error_count,
                run_id
            ],
        )?;

        if should_drop_scan_run(summary, "completed") {
            conn.execute("DELETE FROM scan_runs WHERE id = ?", params![run_id])?;
        }

        cleanup_scan_run_history(&conn)?;
        Ok(())
    }

    pub fn fail_scan_run(&self, run_id: &str, summary: &ScanSummary) -> AppResult<()> {
        let conn = self.pool.get()?;
        let effective_error_count = summary.error_count.max(1);
        conn.execute(
            "UPDATE scan_runs SET status = 'failed', ended_at = CURRENT_TIMESTAMP,
             files_seen = ?, files_parsed = ?, files_skipped = ?, files_failed = ?,
             sessions_changed = ?, error_count = ? WHERE id = ?",
            params![
                summary.files_seen,
                summary.files_parsed,
                summary.files_skipped,
                summary.files_failed,
                summary.sessions_changed,
                effective_error_count,
                run_id
            ],
        )?;
        cleanup_scan_run_history(&conn)?;
        Ok(())
    }

    pub fn scan(
        &self,
        request: ScanRequest,
        cost_estimation_policy: CostEstimationPolicy,
    ) -> AppResult<ScanSummary> {
        let started_at = Utc::now();
        let root_path = request.root_path;
        let source_app = request.source_app;
        let trigger_type = request.trigger_type;
        let root_path_string = fs::canonicalize_to_string(&root_path)
            .unwrap_or_else(|_| root_path.to_string_lossy().to_string());
        let source_adapter = self
            .adapter_for_source_app(&source_app)
            .ok_or_else(|| AppError::validation("Unsupported source adapter for scan"))?;

        let run_id = if request.create_run {
            let run_id = ids::new_uuid();
            let conn = self.pool.get()?;
            conn.execute(
                "INSERT INTO scan_runs (id, trigger_type, status) VALUES (?, ?, ?)",
                params![run_id, &trigger_type, "running"],
            )?;
            cleanup_scan_run_history(&conn)?;
            Some(run_id)
        } else {
            None
        };

        let mut files_seen = 0_u64;
        let mut files_parsed = 0_u64;
        let mut files_skipped = 0_u64;
        let mut files_failed = 0_u64;
        let mut sessions_changed = 0_u64;
        let mut error_count = 0_u64;
        let result = (|| -> AppResult<ScanSummary> {
            for path in source_adapter.discover_paths(&root_path)? {
                files_seen += 1;
                let abs_path = match fs::canonicalize_to_string(&path) {
                    Ok(abs_path) => abs_path,
                    Err(error) => {
                        files_failed += 1;
                        error_count += 1;
                        let conn = self.pool.get()?;
                        self.record_source_file_failure(
                            &conn,
                            &path.to_string_lossy(),
                            source_adapter.parser_version(),
                            &error.to_string(),
                        )?;
                        continue;
                    }
                };
                self.process_source_file(
                    source_adapter,
                    &path,
                    &abs_path,
                    run_id.as_deref(),
                    cost_estimation_policy,
                    &mut files_parsed,
                    &mut files_skipped,
                    &mut files_failed,
                    &mut sessions_changed,
                    &mut error_count,
                )?;
            }

            let summary = ScanSummary {
                trigger_type: trigger_type.clone(),
                root_path: root_path_string.clone(),
                started_at,
                files_seen,
                files_parsed,
                files_skipped,
                files_failed,
                sessions_changed,
                error_count,
            };

            if let Some(run_id) = run_id.as_deref() {
                let conn = self.pool.get()?;
                conn.execute(
                    "UPDATE scan_runs SET status = 'completed', ended_at = CURRENT_TIMESTAMP,
                     files_seen = ?, files_parsed = ?, files_skipped = ?, files_failed = ?,
                     sessions_changed = ?, error_count = ? WHERE id = ?",
                    params![
                        summary.files_seen,
                        summary.files_parsed,
                        summary.files_skipped,
                        summary.files_failed,
                        summary.sessions_changed,
                        summary.error_count,
                        run_id
                    ],
                )?;

                if should_drop_scan_run(&summary, "completed") {
                    conn.execute("DELETE FROM scan_runs WHERE id = ?", params![run_id])?;
                }

                cleanup_scan_run_history(&conn)?;
            }

            Ok(summary)
        })();

        if let Err(_error) = &result {
            if let Some(run_id) = run_id.as_deref() {
                let conn = self.pool.get()?;
                let effective_error_count = error_count.max(1);
                let _ = conn.execute(
                    "UPDATE scan_runs SET status = 'failed', ended_at = CURRENT_TIMESTAMP,
                     files_seen = ?, files_parsed = ?, files_skipped = ?, files_failed = ?,
                     sessions_changed = ?, error_count = ? WHERE id = ?",
                    params![
                        files_seen,
                        files_parsed,
                        files_skipped,
                        files_failed,
                        sessions_changed,
                        effective_error_count,
                        run_id
                    ],
                );
                let _ = cleanup_scan_run_history(&conn);
            }
        }

        result
    }

    pub fn has_scannable_data(&self, root_path: PathBuf, source_app: &str) -> AppResult<bool> {
        let source_adapter = self
            .adapter_for_source_app(source_app)
            .ok_or_else(|| AppError::validation("Unsupported source adapter for scan"))?;
        source_adapter.has_scannable_data(&root_path)
    }

    #[allow(clippy::too_many_arguments)]
    fn process_source_file(
        &self,
        adapter: &(dyn SourceAdapter + Send + Sync),
        path: &std::path::Path,
        abs_path: &str,
        scan_run_id: Option<&str>,
        cost_estimation_policy: CostEstimationPolicy,
        files_parsed: &mut u64,
        files_skipped: &mut u64,
        files_failed: &mut u64,
        sessions_changed: &mut u64,
        error_count: &mut u64,
    ) -> AppResult<()> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        let fingerprint_paths = adapter.fingerprint_paths(path);
        let parser_version = adapter.parser_version();
        let fast_fingerprint = match fingerprint::fingerprint_files_fast(&fingerprint_paths) {
            Ok(fast_fingerprint) => fast_fingerprint,
            Err(error) => {
                *files_failed += 1;
                *error_count += 1;
                self.record_source_file_failure(&tx, abs_path, parser_version, &error.to_string())?;
                tx.commit()?;
                return Ok(());
            }
        };
        let existing_cache: Option<(String, String, i64)> = tx
            .query_row(
                "SELECT id, fingerprint_fast, COALESCE(parser_version, 1) FROM source_files_cache WHERE abs_path = ?",
                params![abs_path],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        let mut force_rebuild_message_index = false;
        let cache_id = if let Some((cache_id, cached_fast, cached_parser_version)) = existing_cache
        {
            if cached_fast == fast_fingerprint.fast && cached_parser_version == parser_version {
                *files_skipped += 1;
                return Ok(());
            }
            force_rebuild_message_index = true;

            tx.execute(
                "UPDATE source_files_cache
                 SET size_bytes = ?, mtime_ms = ?, fingerprint_fast = ?, fingerprint_strong = ?,
                     parser_version = ?,
                     last_scan_at = CURRENT_TIMESTAMP, last_parse_status = NULL, last_error = NULL
                 WHERE id = ?",
                params![
                    fast_fingerprint.size_bytes,
                    fast_fingerprint.mtime_ms,
                    fast_fingerprint.fast,
                    Option::<String>::None,
                    parser_version,
                    cache_id
                ],
            )?;
            cache_id
        } else {
            let cache_id = ids::new_uuid();
            tx.execute(
                "INSERT INTO source_files_cache (id, abs_path, size_bytes, mtime_ms, fingerprint_fast, fingerprint_strong, parser_version, last_scan_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
                params![
                    cache_id,
                    abs_path,
                    fast_fingerprint.size_bytes,
                    fast_fingerprint.mtime_ms,
                    fast_fingerprint.fast,
                    Option::<String>::None,
                    parser_version
                ],
            )?;
            cache_id
        };

        *files_parsed += 1;
        match adapter.parse(path) {
            Ok(sessions) => {
                let mut last_session_error: Option<String> = None;
                for session in sessions {
                    let session_key = session.session_key.clone();
                    match self.upsert_normalized_session_with_tx(
                        &tx,
                        session,
                        abs_path,
                        &cache_id,
                        scan_run_id,
                        force_rebuild_message_index,
                        cost_estimation_policy,
                    ) {
                        Ok(changed) => {
                            if changed {
                                *sessions_changed += 1;
                            }
                        }
                        Err(error) => {
                            *error_count += 1;
                            last_session_error = Some(format!(
                                "Failed to ingest session {session_key} from {abs_path}: {error}"
                            ));
                        }
                    }
                }

                tx.execute(
                    "UPDATE source_files_cache
                     SET last_parse_status = ?, last_error = ?, last_scan_at = CURRENT_TIMESTAMP
                     WHERE id = ?",
                    params!["parsed", last_session_error, cache_id],
                )?;
            }
            Err(error) => {
                *files_failed += 1;
                *error_count += 1;
                tx.execute(
                    "UPDATE source_files_cache
                     SET last_parse_status = ?, last_error = ?, last_scan_at = CURRENT_TIMESTAMP
                     WHERE id = ?",
                    params!["failed", error.to_string(), cache_id],
                )?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    fn record_source_file_failure<C>(
        &self,
        conn: &C,
        abs_path: &str,
        parser_version: i64,
        error_message: &str,
    ) -> AppResult<()>
    where
        C: std::ops::Deref<Target = rusqlite::Connection>,
    {
        let cache_id: Option<String> = conn
            .query_row(
                "SELECT id FROM source_files_cache WHERE abs_path = ?",
                params![abs_path],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(cache_id) = cache_id {
            conn.execute(
                "UPDATE source_files_cache
                 SET parser_version = ?, last_parse_status = ?, last_error = ?, last_scan_at = CURRENT_TIMESTAMP
                 WHERE id = ?",
                params![parser_version, "failed", error_message, cache_id],
            )?;
        } else {
            conn.execute(
                "INSERT INTO source_files_cache (
                    id, abs_path, parser_version, last_scan_at, last_parse_status, last_error
                 ) VALUES (?, ?, ?, CURRENT_TIMESTAMP, ?, ?)",
                params![
                    ids::new_uuid(),
                    abs_path,
                    parser_version,
                    "failed",
                    error_message
                ],
            )?;
        }

        Ok(())
    }

    fn adapter_for_source_app(
        &self,
        source_app: &str,
    ) -> Option<&(dyn SourceAdapter + Send + Sync)> {
        self.adapters
            .iter()
            .find(|adapter| adapter.name() == source_app)
            .map(|adapter| adapter.as_ref())
    }

    pub fn ensure_session_message_index(
        &self,
        session_id: &str,
        cost_estimation_policy: CostEstimationPolicy,
    ) -> AppResult<bool> {
        let conn = self.pool.get()?;
        let session_row = conn
            .query_row(
                "SELECT s.session_key, ref.source_path, ref.source_file_id
                 FROM sessions s
                 INNER JOIN session_source_refs ref ON ref.session_id = s.id
                 WHERE s.id = ?1
                 ORDER BY ref.last_linked_at DESC, ref.source_path ASC
                 LIMIT 1",
                params![session_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;

        let Some((session_key, source_path, source_file_id)) = session_row else {
            return Ok(false);
        };

        let path = PathBuf::from(&source_path);
        let adapter = match self
            .adapters
            .iter()
            .find(|adapter| adapter.can_handle(&path))
        {
            Some(adapter) => adapter,
            None => return Ok(false),
        };

        let parsed_session = adapter
            .parse(&path)?
            .into_iter()
            .find(|parsed| parsed.session_key == session_key);

        let Some(parsed_session) = parsed_session else {
            return Ok(false);
        };

        let source_file_id = source_file_id.unwrap_or_else(|| format!("manual:{source_path}"));
        self.upsert_normalized_session(
            parsed_session,
            &source_path,
            &source_file_id,
            true,
            cost_estimation_policy,
        )
    }

    fn upsert_normalized_session(
        &self,
        session: NormalizedSession,
        source_path: &str,
        source_file_id: &str,
        force_rebuild_message_index: bool,
        cost_estimation_policy: CostEstimationPolicy,
    ) -> AppResult<bool> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        let changed = self.upsert_normalized_session_with_tx(
            &tx,
            session,
            source_path,
            source_file_id,
            None,
            force_rebuild_message_index,
            cost_estimation_policy,
        )?;
        tx.commit()?;
        Ok(changed)
    }

    fn upsert_normalized_session_with_tx(
        &self,
        tx: &rusqlite::Transaction<'_>,
        session: NormalizedSession,
        source_path: &str,
        source_file_id: &str,
        scan_run_id: Option<&str>,
        force_rebuild_message_index: bool,
        cost_estimation_policy: CostEstimationPolicy,
    ) -> AppResult<bool> {
        let source_state = derive_source_state(&session.source_app, source_path);
        let existing_session = tx
            .query_row(
                "SELECT id, external_session_id, title, model_first, model_last, source_created_at, source_updated_at, source_state
                 FROM sessions
                 WHERE session_key = ?",
                params![&session.session_key],
                |row| {
                    Ok(ExistingSessionRecord {
                        id: row.get(0)?,
                        external_session_id: row.get(1)?,
                        title: row.get(2)?,
                        model_first: row.get(3)?,
                        model_last: row.get(4)?,
                        source_created_at: row.get(5)?,
                        source_updated_at: row.get(6)?,
                        source_state: row.get(7)?,
                    })
                },
            )
            .optional()?;
        let existing_source_file_id = match existing_session.as_ref() {
            Some(existing) => tx
                .query_row(
                    "SELECT source_file_id
                     FROM session_source_refs
                     WHERE session_id = ? AND source_path = ?",
                    params![existing.id, source_path],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten(),
            None => None,
        };
        let preserve_existing_model_last = should_preserve_existing_model_last(&session);
        let effective_model_last = if preserve_existing_model_last {
            existing_session
                .as_ref()
                .and_then(|existing| existing.model_last.clone())
                .or_else(|| session.model_last.clone())
        } else {
            session.model_last.clone()
        };
        let metadata_changed = match existing_session.as_ref() {
            Some(existing) => {
                let merged = preview_session_merge(
                    existing,
                    &session,
                    source_state,
                    effective_model_last.clone(),
                );
                session_metadata_changed(existing, &merged)
                    || existing_source_file_id.as_deref() != Some(source_file_id)
            }
            None => true,
        };

        let session_id_candidate = ids::new_uuid();
        let session_id: String = tx.query_row(
            "INSERT INTO sessions (id, source_app, external_session_id, session_key, title, model_first, model_last, source_created_at, source_updated_at, source_state)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(session_key) DO UPDATE SET
                external_session_id = COALESCE(sessions.external_session_id, excluded.external_session_id),
                title = COALESCE(excluded.title, sessions.title),
                model_first = COALESCE(sessions.model_first, excluded.model_first),
                model_last = excluded.model_last,
                source_created_at = CASE
                    WHEN sessions.source_created_at IS NULL THEN excluded.source_created_at
                    WHEN excluded.source_created_at IS NULL THEN sessions.source_created_at
                    WHEN excluded.source_created_at < sessions.source_created_at THEN excluded.source_created_at
                    ELSE sessions.source_created_at
                END,
                source_updated_at = CASE
                    WHEN sessions.source_updated_at IS NULL THEN excluded.source_updated_at
                    WHEN excluded.source_updated_at IS NULL THEN sessions.source_updated_at
                    WHEN excluded.source_updated_at > sessions.source_updated_at THEN excluded.source_updated_at
                    ELSE sessions.source_updated_at
                END,
                source_state = excluded.source_state,
                discovered_last_at = CURRENT_TIMESTAMP
             RETURNING id",
            params![
                session_id_candidate,
                session.source_app,
                session.external_session_id,
                session.session_key,
                session.title,
                session.model_first,
                effective_model_last,
                session.source_created_at,
                session.source_updated_at,
                source_state
            ],
            |row| row.get(0),
        )?;

        tx.execute(
            "INSERT INTO session_source_refs (session_id, source_path, source_file_id, last_linked_at)
             VALUES (?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(session_id, source_path) DO UPDATE SET
                source_file_id = excluded.source_file_id,
                last_linked_at = CURRENT_TIMESTAMP",
            params![session_id, source_path, source_file_id],
        )?;

        let existing_observation_id = tx
            .query_row(
                "SELECT id
                 FROM session_observations
                 WHERE session_id = ?1 AND conversation_checksum = ?2
                 LIMIT 1",
                params![session_id, session.conversation_checksum],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let inserted_observation = existing_observation_id.is_none();
        let observation_id = existing_observation_id.unwrap_or_else(ids::new_uuid);
        let session_token_totals = normalized_session_token_totals(&session);
        tx.execute(
            "INSERT INTO session_observations (
                id, session_id, input_tokens, output_tokens, total_tokens,
                conversation_checksum, message_count, source_model, scan_run_id
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                input_tokens = excluded.input_tokens,
                output_tokens = excluded.output_tokens,
                total_tokens = excluded.total_tokens,
                message_count = excluded.message_count,
                source_model = COALESCE(excluded.source_model, session_observations.source_model),
                scan_run_id = COALESCE(excluded.scan_run_id, session_observations.scan_run_id)",
            params![
                observation_id,
                session_id,
                session_token_totals.input_tokens,
                session_token_totals.output_tokens,
                session_token_totals.total_tokens,
                session.conversation_checksum,
                session.message_count,
                session.model_last,
                scan_run_id
            ],
        )?;

        let has_request_index_payload = !session.requests.is_empty() || !session.events.is_empty();
        let should_rebuild_request_index =
            has_request_index_payload && (inserted_observation || force_rebuild_message_index);
        let has_session_usage_payload = has_request_index_payload
            || session.total_input_tokens > 0
            || session.total_output_tokens > 0;
        let should_update_token_totals =
            has_session_usage_payload && (inserted_observation || force_rebuild_message_index);

        if should_rebuild_request_index {
            tx.execute(
                "DELETE FROM session_requests WHERE session_id = ?",
                params![session_id],
            )?;
            if force_rebuild_message_index {
                tx.execute(
                    "DELETE FROM token_usage_events WHERE session_id = ?",
                    params![session_id],
                )?;
            }
        } else if !should_update_token_totals {
            return Ok(metadata_changed);
        }

        let mut pricing_by_model = ModelPricingCache::new();
        for request in &session.requests {
            let request_id = ids::new_uuid();
            let stored_source_request_id = request
                .source_request_id
                .as_deref()
                .map(|value| scope_source_local_id(&session.session_key, value));
            let request_model = request
                .model
                .as_deref()
                .or(session.model_last.as_deref())
                .or(session.model_first.as_deref());
            let request_input_tokens = request.input_tokens.unwrap_or(0);
            let request_output_tokens = request.output_tokens.unwrap_or(0);
            let request_cache_read_input_tokens = request.cache_read_input_tokens.unwrap_or(0);
            let request_cache_write_input_tokens = request.cache_write_input_tokens.unwrap_or(0);
            let request_estimated_cost = estimate_usage_cost_details(
                tx,
                &mut pricing_by_model,
                cost_estimation_policy,
                request_model,
                request_input_tokens,
                request_output_tokens,
                request.total_tokens,
                request_cache_read_input_tokens,
                request_cache_write_input_tokens,
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO session_requests (
                    id, session_id, observation_id, source_app, source_request_id, sequence_no,
                    status, message_count, model, input_tokens, output_tokens, total_tokens,
                    cache_read_input_tokens, cache_write_input_tokens, estimated_cost_usd,
                    estimated_cost_source,
                    token_confidence, source_created_at, source_updated_at, source_locator
                )
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    request_id,
                    session_id,
                    observation_id,
                    session.source_app,
                    stored_source_request_id,
                    request.sequence_no,
                    request.status,
                    request.message_count,
                    request.model,
                    request.input_tokens,
                    request.output_tokens,
                    request.total_tokens,
                    request.cache_read_input_tokens,
                    request.cache_write_input_tokens,
                    request_estimated_cost.map(|cost| cost.amount_usd),
                    request_estimated_cost.map(|cost| estimated_cost_source_sql(cost.source)),
                    request.token_confidence,
                    request.source_created_at,
                    request.source_updated_at,
                    request.source_locator
                ],
            )?;
        }

        if should_rebuild_request_index {
            for event in session.events {
                let event_id = ids::new_uuid();
                let event_estimated_cost = estimate_usage_cost_details(
                    tx,
                    &mut pricing_by_model,
                    cost_estimation_policy,
                    event.model.as_deref(),
                    event.delta_input,
                    event.delta_output,
                    Some(event.delta_total),
                    event.cache_read_input_tokens,
                    event.cache_write_input_tokens,
                )?;
                tx.execute(
                    "INSERT OR IGNORE INTO token_usage_events (
                        id, session_id, observation_id, event_time_utc, delta_input, delta_output, delta_total,
                        cache_read_input_tokens, cache_write_input_tokens, estimated_cost_usd,
                        estimated_cost_source,
                        source_app, model, granularity, confidence, source_event_id
                    )
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        event_id,
                        session_id,
                        observation_id,
                        event.event_time_utc,
                        event.delta_input,
                        event.delta_output,
                        event.delta_total,
                        event.cache_read_input_tokens,
                        event.cache_write_input_tokens,
                        event_estimated_cost.map(|cost| cost.amount_usd),
                        event_estimated_cost.map(|cost| estimated_cost_source_sql(cost.source)),
                        session.source_app,
                        event.model,
                        event.granularity,
                        event.confidence,
                        event.source_event_id
                    ],
                )?;
            }
        }

        if should_update_token_totals {
            tx.execute(
                "INSERT INTO session_token_totals (session_id, input_tokens_max, output_tokens_max, total_tokens_max, last_observed_at, last_observation_id)
                 VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP, ?)
                 ON CONFLICT(session_id) DO UPDATE SET
                    input_tokens_max = excluded.input_tokens_max,
                    output_tokens_max = excluded.output_tokens_max,
                    total_tokens_max = excluded.total_tokens_max,
                    last_observed_at = CURRENT_TIMESTAMP,
                    last_observation_id = excluded.last_observation_id",
                params![
                    session_id,
                    session_token_totals.input_tokens,
                    session_token_totals.output_tokens,
                    session_token_totals.total_tokens,
                    observation_id
                ],
            )?;
        }

        Ok(inserted_observation || metadata_changed || force_rebuild_message_index)
    }
}

fn scope_source_local_id(session_key: &str, local_id: &str) -> String {
    format!("{session_key}:{local_id}")
}

fn should_drop_scan_run(summary: &ScanSummary, status: &str) -> bool {
    status == "completed"
        && summary.trigger_type == "auto"
        && summary.files_parsed == 0
        && summary.files_failed == 0
        && summary.sessions_changed == 0
        && summary.error_count == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NormalizedSessionTokenTotals {
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
}

fn normalized_session_token_totals(session: &NormalizedSession) -> NormalizedSessionTokenTotals {
    let mut request_input_tokens = 0_i64;
    let mut request_output_tokens = 0_i64;
    let request_total_tokens: i64 = session
        .requests
        .iter()
        .map(|request| {
            request_input_tokens += request.input_tokens.unwrap_or(0);
            request_output_tokens += request.output_tokens.unwrap_or(0);
            request.total_tokens.unwrap_or(
                request.input_tokens.unwrap_or(0)
                    + request.output_tokens.unwrap_or(0)
                    + request.cache_read_input_tokens.unwrap_or(0)
                    + request.cache_write_input_tokens.unwrap_or(0),
            )
        })
        .sum();

    if request_total_tokens > 0 {
        NormalizedSessionTokenTotals {
            input_tokens: request_input_tokens,
            output_tokens: request_output_tokens,
            total_tokens: request_total_tokens,
        }
    } else {
        NormalizedSessionTokenTotals {
            input_tokens: session.total_input_tokens,
            output_tokens: session.total_output_tokens,
            total_tokens: session.total_input_tokens + session.total_output_tokens,
        }
    }
}

fn preview_session_merge(
    existing: &ExistingSessionRecord,
    session: &NormalizedSession,
    source_state: &str,
    effective_model_last: Option<String>,
) -> SessionMergePreview {
    SessionMergePreview {
        external_session_id: existing
            .external_session_id
            .clone()
            .or_else(|| session.external_session_id.clone()),
        title: session.title.clone().or_else(|| existing.title.clone()),
        model_first: existing
            .model_first
            .clone()
            .or_else(|| session.model_first.clone()),
        model_last: effective_model_last,
        source_created_at: older_timestamp(existing.source_created_at, session.source_created_at),
        source_updated_at: newer_timestamp(existing.source_updated_at, session.source_updated_at),
        source_state: source_state.to_string(),
    }
}

fn should_preserve_existing_model_last(session: &NormalizedSession) -> bool {
    session.source_app == "cursor"
        && session.model_last.is_none()
        && session.requests.is_empty()
        && session.events.is_empty()
        && session.total_input_tokens == 0
        && session.total_output_tokens == 0
}

fn session_metadata_changed(
    existing: &ExistingSessionRecord,
    merged: &SessionMergePreview,
) -> bool {
    existing.external_session_id != merged.external_session_id
        || existing.title != merged.title
        || existing.model_first != merged.model_first
        || existing.model_last != merged.model_last
        || existing.source_created_at != merged.source_created_at
        || existing.source_updated_at != merged.source_updated_at
        || existing.source_state != merged.source_state
}

fn older_timestamp(
    left: Option<chrono::DateTime<chrono::Utc>>,
    right: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn newer_timestamp(
    left: Option<chrono::DateTime<chrono::Utc>>,
    right: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn derive_source_state(source_app: &str, source_path: &str) -> &'static str {
    if is_archived_source_path(source_app, source_path) {
        "archived"
    } else {
        "synced"
    }
}

fn is_archived_source_path(source_app: &str, source_path: &str) -> bool {
    if source_app != "codex" {
        return false;
    }

    let normalized = source_path.replace('\\', "/").to_ascii_lowercase();
    normalized.contains("/.codex/archived_sessions/")
}

#[cfg(test)]
mod tests {
    use super::Scanner;
    use crate::db::init_db_with_path;
    use crate::error::AppResult;
    use crate::pricing::CostEstimationPolicy;
    use crate::sources::{NormalizedRequest, NormalizedSession, SourceAdapter};
    use chrono::Utc;
    use std::path::Path;

    struct EmptyFileAdapter;

    impl SourceAdapter for EmptyFileAdapter {
        fn name(&self) -> &str {
            "test"
        }

        fn can_handle(&self, path: &Path) -> bool {
            path.is_file()
        }

        fn parse(&self, _path: &Path) -> AppResult<Vec<NormalizedSession>> {
            Ok(Vec::new())
        }
    }

    struct FixedSessionAdapter {
        session: NormalizedSession,
        parser_version: i64,
    }

    impl SourceAdapter for FixedSessionAdapter {
        fn name(&self) -> &str {
            "kiro"
        }

        fn parser_version(&self) -> i64 {
            self.parser_version
        }

        fn can_handle(&self, path: &Path) -> bool {
            path.is_file()
        }

        fn parse(&self, _path: &Path) -> AppResult<Vec<NormalizedSession>> {
            Ok(vec![self.session.clone()])
        }
    }

    fn make_session(
        session_key: &str,
        checksum: &str,
        input_tokens: i64,
        output_tokens: i64,
    ) -> NormalizedSession {
        let now = Utc::now();
        NormalizedSession {
            source_app: "kiro".to_string(),
            external_session_id: Some("external-session".to_string()),
            session_key: session_key.to_string(),
            title: Some("Kiro Session".to_string()),
            model_first: Some("claude-sonnet-4".to_string()),
            model_last: Some("claude-sonnet-4".to_string()),
            source_created_at: Some(now),
            source_updated_at: Some(now),
            total_input_tokens: input_tokens,
            total_output_tokens: output_tokens,
            message_count: 0,
            conversation_checksum: checksum.to_string(),
            requests: Vec::new(),
            events: Vec::new(),
        }
    }

    fn make_request(
        sequence_no: i64,
        input_tokens: i64,
        output_tokens: i64,
        total_tokens: i64,
        cache_read_input_tokens: i64,
    ) -> NormalizedRequest {
        NormalizedRequest {
            source_request_id: Some(format!("request-{sequence_no}")),
            sequence_no,
            status: Some("completed".to_string()),
            message_count: 2,
            model: Some("claude-sonnet-4".to_string()),
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            total_tokens: Some(total_tokens),
            cache_read_input_tokens: Some(cache_read_input_tokens),
            cache_write_input_tokens: Some(0),
            token_confidence: Some("high".to_string()),
            source_created_at: None,
            source_updated_at: None,
            source_locator: format!("request-{sequence_no}"),
        }
    }

    #[test]
    fn source_cache_uses_fast_fingerprint_without_strong_file_hash() {
        let db_path = std::env::temp_dir().join(format!(
            "totoken-scanner-fast-fingerprint-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let source_path = std::env::temp_dir().join(format!(
            "totoken-scanner-fast-source-{}.txt",
            crate::utils::ids::new_uuid()
        ));
        std::fs::write(&source_path, "source").expect("write source file");
        let pool = init_db_with_path(&db_path).expect("init test db");
        let scanner = Scanner::new(pool.clone());
        let adapter = EmptyFileAdapter;
        let abs_path = source_path.to_string_lossy().to_string();
        let mut files_parsed = 0;
        let mut files_skipped = 0;
        let mut files_failed = 0;
        let mut sessions_changed = 0;
        let mut error_count = 0;

        scanner
            .process_source_file(
                &adapter,
                &source_path,
                &abs_path,
                None,
                CostEstimationPolicy::default(),
                &mut files_parsed,
                &mut files_skipped,
                &mut files_failed,
                &mut sessions_changed,
                &mut error_count,
            )
            .expect("process source file");

        let conn = pool.get().expect("load db conn");
        let (fingerprint_fast, fingerprint_strong): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT fingerprint_fast, fingerprint_strong FROM source_files_cache WHERE abs_path = ?",
                rusqlite::params![abs_path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load source cache");
        assert!(fingerprint_fast.is_some());
        assert_eq!(fingerprint_strong, None);

        let _ = std::fs::remove_file(&source_path);
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[test]
    fn parser_version_reparse_rebuilds_existing_session_token_totals() {
        let db_path = std::env::temp_dir().join(format!(
            "totoken-scanner-parser-rebuild-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let source_path = std::env::temp_dir().join(format!(
            "totoken-scanner-parser-rebuild-source-{}.json",
            crate::utils::ids::new_uuid()
        ));
        std::fs::write(&source_path, r#"{"history":[]}"#).expect("write source file");
        let pool = init_db_with_path(&db_path).expect("init test db");
        let conn = pool.get().expect("load db conn");
        let now = Utc::now();
        let session_id = "session-parser-rebuild";
        let observation_id = "observation-parser-rebuild";
        let source_file_id = "source-file-parser-rebuild";
        let source_path_text = source_path.to_string_lossy().to_string();

        conn.execute(
            "INSERT INTO sessions (
                id, source_app, external_session_id, session_key, title,
                model_first, model_last, source_created_at, source_updated_at,
                discovered_first_at, discovered_last_at, source_state
             ) VALUES (?1, 'kiro', 'external-parser-rebuild', 'kiro:parser-rebuild',
                'Parser rebuild', 'claude-sonnet-4', 'claude-sonnet-4',
                ?2, ?2, ?2, ?2, 'synced')",
            rusqlite::params![session_id, now],
        )
        .expect("insert existing session");
        conn.execute(
            "INSERT INTO session_observations (
                id, session_id, observed_at, input_tokens, output_tokens, total_tokens,
                conversation_checksum, message_count, source_model, scan_run_id
             ) VALUES (?1, ?2, ?3, 0, 0, 0, 'checksum-parser-rebuild', 2,
                'claude-sonnet-4', 'scan-run-old')",
            rusqlite::params![observation_id, session_id, now],
        )
        .expect("insert existing observation");
        conn.execute(
            "INSERT INTO session_token_totals (
                session_id, input_tokens_max, output_tokens_max, total_tokens_max,
                last_observed_at, last_observation_id
             ) VALUES (?1, 0, 0, 0, ?2, ?3)",
            rusqlite::params![session_id, now, observation_id],
        )
        .expect("insert stale token totals");
        conn.execute(
            "INSERT INTO source_files_cache (
                id, abs_path, size_bytes, mtime_ms, fingerprint_fast, fingerprint_strong,
                parser_version, last_scan_at, last_parse_status
             ) VALUES (?1, ?2, 1, 1, 'stale-fast-fingerprint', NULL, 1, ?3, 'parsed')",
            rusqlite::params![source_file_id, source_path_text, now],
        )
        .expect("insert stale source cache");
        drop(conn);

        let mut session = make_session("kiro:parser-rebuild", "checksum-parser-rebuild", 0, 0);
        session.message_count = 2;
        session.requests = vec![make_request(1, 120, 340, 460, 0)];
        let adapter = FixedSessionAdapter {
            session,
            parser_version: 2,
        };
        let scanner = Scanner::new(pool.clone());
        let mut files_parsed = 0;
        let mut files_skipped = 0;
        let mut files_failed = 0;
        let mut sessions_changed = 0;
        let mut error_count = 0;

        scanner
            .process_source_file(
                &adapter,
                &source_path,
                &source_path_text,
                None,
                CostEstimationPolicy::default(),
                &mut files_parsed,
                &mut files_skipped,
                &mut files_failed,
                &mut sessions_changed,
                &mut error_count,
            )
            .expect("process source file");

        let conn = pool.get().expect("load db conn");
        let totals = conn
            .query_row(
                "SELECT input_tokens_max, output_tokens_max, total_tokens_max
                 FROM session_token_totals WHERE session_id = ?",
                rusqlite::params![session_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .expect("load rebuilt totals");
        assert_eq!(totals, (120, 340, 460));
        assert_eq!(files_parsed, 1);
        assert_eq!(files_skipped, 0);
        assert_eq!(files_failed, 0);
        assert_eq!(error_count, 0);

        let _ = std::fs::remove_file(&source_path);
        drop(conn);
        drop(pool);
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[test]
    fn reparsing_session_replaces_stale_token_totals() {
        let db_path = std::env::temp_dir().join(format!(
            "totoken-scanner-test-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let pool = init_db_with_path(&db_path).expect("init test db");
        let scanner = Scanner::new(pool.clone());

        scanner
            .upsert_normalized_session(
                make_session("kiro:test-session", "checksum-a", 22044, 772),
                "C:/kiro/session.json",
                "source-file-1",
                false,
                CostEstimationPolicy::default(),
            )
            .expect("insert first observation");
        scanner
            .upsert_normalized_session(
                make_session("kiro:test-session", "checksum-b", 16607, 793),
                "C:/kiro/session.json",
                "source-file-1",
                true,
                CostEstimationPolicy::default(),
            )
            .expect("replace with reparsed observation");

        let conn = pool.get().expect("load db conn");
        let totals = conn
            .query_row(
                "SELECT input_tokens_max, output_tokens_max, total_tokens_max
                 FROM session_token_totals",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .expect("load totals");
        assert_eq!(totals, (16607, 793, 17400));

        let observation_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_observations", [], |row| {
                row.get(0)
            })
            .expect("count observations");
        assert_eq!(observation_count, 2);

        let latest_observation = conn
            .query_row(
                "SELECT o.input_tokens, o.output_tokens
                 FROM session_token_totals st
                 INNER JOIN session_observations o ON o.id = st.last_observation_id",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .expect("load latest observation");
        assert_eq!(latest_observation, (16607, 793));

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[test]
    fn session_total_tokens_follow_request_totals_when_cache_is_separate() {
        let db_path = std::env::temp_dir().join(format!(
            "totoken-scanner-total-test-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let pool = init_db_with_path(&db_path).expect("init test db");
        let scanner = Scanner::new(pool.clone());

        let mut session = make_session("kilocode:test-session", "checksum-cache", 100, 30);
        session.source_app = "kilocode".to_string();
        session.requests = vec![make_request(1, 100, 30, 140, 10)];

        scanner
            .upsert_normalized_session(
                session,
                "C:/kilo/kilo.db",
                "source-file-1",
                false,
                CostEstimationPolicy::default(),
            )
            .expect("insert cache-aware observation");

        let conn = pool.get().expect("load db conn");
        let totals = conn
            .query_row(
                "SELECT input_tokens_max, output_tokens_max, total_tokens_max
                 FROM session_token_totals",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .expect("load totals");
        assert_eq!(totals, (100, 30, 140));

        let observation_totals = conn
            .query_row(
                "SELECT input_tokens, output_tokens, total_tokens
                 FROM session_observations",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .expect("load observation totals");
        assert_eq!(observation_totals, (100, 30, 140));

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[test]
    fn reparse_preserves_model_first_when_reparsed_model_is_missing() {
        let db_path = std::env::temp_dir().join(format!(
            "totoken-scanner-model-reset-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let pool = init_db_with_path(&db_path).expect("init test db");
        let scanner = Scanner::new(pool.clone());

        let mut original = make_session("claude_code:model-reset", "checksum-a", 10, 20);
        original.source_app = "claude_code".to_string();
        original.model_first = Some("claude-opus-4-7".to_string());
        original.model_last = Some("claude-opus-4-7".to_string());

        scanner
            .upsert_normalized_session(
                original,
                "C:/Users/test/.claude/projects/demo/session.jsonl",
                "source-file-1",
                false,
                CostEstimationPolicy::default(),
            )
            .expect("insert original session");

        let mut reparsed = make_session("claude_code:model-reset", "checksum-a", 10, 20);
        reparsed.source_app = "claude_code".to_string();
        reparsed.model_first = None;
        reparsed.model_last = None;

        scanner
            .upsert_normalized_session(
                reparsed,
                "C:/Users/test/.claude/projects/demo/session.jsonl",
                "source-file-1",
                true,
                CostEstimationPolicy::default(),
            )
            .expect("reparse session with missing models");

        let conn = pool.get().expect("load db conn");
        let models = conn
            .query_row(
                "SELECT model_first, model_last FROM sessions WHERE session_key = ?",
                rusqlite::params!["claude_code:model-reset"],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                },
            )
            .expect("load reparsed models");
        assert_eq!(models, (Some("claude-opus-4-7".to_string()), None));

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[test]
    fn cursor_estimated_requests_feed_session_totals() {
        let db_path = std::env::temp_dir().join(format!(
            "totoken-scanner-cursor-estimated-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let pool = init_db_with_path(&db_path).expect("init test db");
        let scanner = Scanner::new(pool.clone());

        let now = Utc::now();
        let session = NormalizedSession {
            source_app: "cursor".to_string(),
            external_session_id: Some("cursor-estimated".to_string()),
            session_key: "cursor:estimated".to_string(),
            title: Some("Cursor Estimated".to_string()),
            model_first: Some("gpt-4.1".to_string()),
            model_last: Some("gpt-4.1".to_string()),
            source_created_at: Some(now),
            source_updated_at: Some(now),
            total_input_tokens: 0,
            total_output_tokens: 0,
            message_count: 2,
            conversation_checksum: "cursor-estimated-checksum".to_string(),
            requests: vec![NormalizedRequest {
                source_request_id: Some("cursor:estimated:request-1".to_string()),
                sequence_no: 1,
                status: Some("completed".to_string()),
                message_count: 2,
                model: Some("gpt-4.1".to_string()),
                input_tokens: Some(120),
                output_tokens: Some(340),
                total_tokens: Some(460),
                cache_read_input_tokens: Some(0),
                cache_write_input_tokens: Some(0),
                token_confidence: None,
                source_created_at: Some(now),
                source_updated_at: Some(now),
                source_locator: "cursor-estimated-request".to_string(),
            }],
            events: Vec::new(),
        };

        scanner
            .upsert_normalized_session(
                session,
                "C:/Cursor/state.vscdb",
                "source-file-1",
                false,
                CostEstimationPolicy::default(),
            )
            .expect("insert estimated cursor session");

        let conn = pool.get().expect("load db conn");
        let totals = conn
            .query_row(
                "SELECT input_tokens_max, output_tokens_max, total_tokens_max
                 FROM session_token_totals",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .expect("load totals");
        assert_eq!(totals, (120, 340, 460));

        let observation_totals = conn
            .query_row(
                "SELECT input_tokens, output_tokens, total_tokens
                 FROM session_observations",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .expect("load observation totals");
        assert_eq!(observation_totals, (120, 340, 460));

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[test]
    fn kiro_estimated_requests_feed_session_totals() {
        let db_path = std::env::temp_dir().join(format!(
            "totoken-scanner-kiro-estimated-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let pool = init_db_with_path(&db_path).expect("init test db");
        let scanner = Scanner::new(pool.clone());

        let now = Utc::now();
        let mut session = make_session("kiro:estimated", "kiro-estimated-checksum", 0, 0);
        session.message_count = 2;
        session.requests = vec![NormalizedRequest {
            source_request_id: Some("kiro-estimated-request-1".to_string()),
            sequence_no: 1,
            status: Some("completed".to_string()),
            message_count: 2,
            model: Some("claude-sonnet-4.5".to_string()),
            input_tokens: Some(120),
            output_tokens: Some(340),
            total_tokens: Some(460),
            cache_read_input_tokens: Some(0),
            cache_write_input_tokens: Some(0),
            token_confidence: Some("low".to_string()),
            source_created_at: Some(now),
            source_updated_at: Some(now),
            source_locator: "kiro-low-request".to_string(),
        }];

        scanner
            .upsert_normalized_session(
                session,
                "C:/Kiro/User/globalStorage/kiro.kiroagent/workspace-sessions/demo/session.json",
                "source-file-1",
                false,
                CostEstimationPolicy::default(),
            )
            .expect("insert estimated kiro session");

        let conn = pool.get().expect("load db conn");
        let totals = conn
            .query_row(
                "SELECT input_tokens_max, output_tokens_max, total_tokens_max
                 FROM session_token_totals",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .expect("load totals");
        assert_eq!(totals, (120, 340, 460));

        let request_totals = conn
            .query_row(
                "SELECT input_tokens, output_tokens, total_tokens, token_confidence
                 FROM session_requests",
                [],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .expect("load request totals");
        assert_eq!(
            request_totals,
            (Some(120), Some(340), Some(460), Some("low".to_string()))
        );

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[test]
    fn cursor_workspace_metadata_does_not_clear_global_request_index() {
        let db_path = std::env::temp_dir().join(format!(
            "totoken-scanner-cursor-workspace-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let pool = init_db_with_path(&db_path).expect("init test db");
        let scanner = Scanner::new(pool.clone());

        let now = Utc::now();
        let mut global_session = make_session("cursor:shared", "global-checksum", 100, 30);
        global_session.source_app = "cursor".to_string();
        global_session.external_session_id = Some("shared".to_string());
        global_session.title = Some("Global Cursor Session".to_string());
        global_session.model_first = Some("gpt-4.1".to_string());
        global_session.model_last = Some("gpt-4.1".to_string());
        global_session.message_count = 2;
        global_session.requests = vec![NormalizedRequest {
            source_request_id: Some("request-1".to_string()),
            sequence_no: 1,
            status: Some("completed".to_string()),
            message_count: 2,
            model: Some("gpt-4.1".to_string()),
            input_tokens: Some(100),
            output_tokens: Some(30),
            total_tokens: Some(130),
            cache_read_input_tokens: Some(0),
            cache_write_input_tokens: Some(0),
            token_confidence: Some("high".to_string()),
            source_created_at: Some(now),
            source_updated_at: Some(now),
            source_locator: "global-request".to_string(),
        }];

        scanner
            .upsert_normalized_session(
                global_session,
                "C:/Cursor/User/globalStorage/state.vscdb",
                "global-source-file",
                false,
                CostEstimationPolicy::default(),
            )
            .expect("insert global cursor session");

        let mut workspace_session = make_session("cursor:shared", "workspace-checksum", 0, 0);
        workspace_session.source_app = "cursor".to_string();
        workspace_session.external_session_id = Some("shared".to_string());
        workspace_session.title = Some("Workspace Cursor Session".to_string());
        workspace_session.model_first = None;
        workspace_session.model_last = None;
        workspace_session.message_count = 0;
        workspace_session.requests = Vec::new();
        workspace_session.events = Vec::new();

        scanner
            .upsert_normalized_session(
                workspace_session,
                "C:/Cursor/User/workspaceStorage/hash/state.vscdb",
                "workspace-source-file",
                false,
                CostEstimationPolicy::default(),
            )
            .expect("insert workspace cursor metadata");

        let conn = pool.get().expect("load db conn");
        let request_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_requests", [], |row| {
                row.get(0)
            })
            .expect("count requests");
        assert_eq!(request_count, 1);

        let totals = conn
            .query_row(
                "SELECT input_tokens_max, output_tokens_max, total_tokens_max
                 FROM session_token_totals",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .expect("load totals");
        assert_eq!(totals, (100, 30, 130));

        let source_ref_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_source_refs", [], |row| {
                row.get(0)
            })
            .expect("count source refs");
        assert_eq!(source_ref_count, 2);

        let models = conn
            .query_row(
                "SELECT model_first, model_last FROM sessions WHERE session_key = ?",
                rusqlite::params!["cursor:shared"],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                },
            )
            .expect("load cursor models");
        assert_eq!(
            models,
            (Some("gpt-4.1".to_string()), Some("gpt-4.1".to_string()))
        );

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[test]
    fn identical_reparse_does_not_count_as_session_change() {
        let db_path = std::env::temp_dir().join(format!(
            "totoken-scanner-identical-reparse-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let pool = init_db_with_path(&db_path).expect("init test db");
        let scanner = Scanner::new(pool.clone());

        let session = make_session("kiro:stable-session", "stable-checksum", 120, 45);

        let first_changed = scanner
            .upsert_normalized_session(
                session.clone(),
                "C:/kiro/session.json",
                "source-file-1",
                false,
                CostEstimationPolicy::default(),
            )
            .expect("insert first observation");
        assert!(first_changed);

        let second_changed = scanner
            .upsert_normalized_session(
                session,
                "C:/kiro/session.json",
                "source-file-1",
                false,
                CostEstimationPolicy::default(),
            )
            .expect("reparse identical observation");
        assert!(!second_changed);

        let conn = pool.get().expect("load db conn");
        let observation_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_observations", [], |row| {
                row.get(0)
            })
            .expect("count observations");
        assert_eq!(observation_count, 1);

        let request_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_requests", [], |row| {
                row.get(0)
            })
            .expect("count requests");
        assert_eq!(request_count, 0);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }
}
