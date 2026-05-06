use super::*;

const SESSION_NAME_SQL: &str = "COALESCE(NULLIF(TRIM(s.title), ''), 'Untitled session')";
const SESSION_MODEL_SQL: &str = "COALESCE(s.model_last, s.model_first, 'unknown')";
const SESSION_LAST_UPDATED_SQL: &str = "COALESCE(s.source_updated_at, s.discovered_last_at)";
const SESSION_SOURCE_STATE_SQL: &str =
    "CASE s.source_state WHEN 'active' THEN 'synced' WHEN 'deleted_by_user' THEN 'deleted' ELSE s.source_state END";
const SESSION_ESTIMATED_COST_SQL: &str =
    "(SELECT SUM(r.estimated_cost_usd) FROM session_requests r WHERE r.session_id = s.id)";
const SESSION_ESTIMATED_COST_WITHOUT_FALLBACK_SQL: &str =
    "(SELECT SUM(r.estimated_cost_usd) FROM session_requests r WHERE r.session_id = s.id AND COALESCE(r.estimated_cost_source, 'catalog') != 'fallback')";
const SESSION_LIST_INPUT_TOKENS_SQL: &str = "CASE WHEN COALESCE(st.total_tokens_max, 0) > 0 THEN COALESCE(st.input_tokens_max, 0) ELSE (SELECT COALESCE(SUM(COALESCE(r.input_tokens, 0)), 0) FROM session_requests r WHERE r.session_id = s.id) END";
const SESSION_LIST_OUTPUT_TOKENS_SQL: &str = "CASE WHEN COALESCE(st.total_tokens_max, 0) > 0 THEN COALESCE(st.output_tokens_max, 0) ELSE (SELECT COALESCE(SUM(COALESCE(r.output_tokens, 0)), 0) FROM session_requests r WHERE r.session_id = s.id) END";
const SESSION_LIST_TOTAL_TOKENS_SQL: &str = "CASE WHEN COALESCE(st.total_tokens_max, 0) > 0 THEN COALESCE(st.total_tokens_max, 0) ELSE (SELECT COALESCE(SUM(COALESCE(r.total_tokens, COALESCE(r.input_tokens, 0) + COALESCE(r.output_tokens, 0) + COALESCE(r.cache_read_input_tokens, 0) + COALESCE(r.cache_write_input_tokens, 0))), 0) FROM session_requests r WHERE r.session_id = s.id) END";
const HIDE_CURSOR_WORKSPACE_EMPTY_SHELL_SQL: &str = "
    NOT (
        s.source_app = 'cursor'
        AND NULLIF(TRIM(COALESCE(s.title, '')), '') IS NULL
        AND EXISTS (
            SELECT 1 FROM session_source_refs ref_workspace
            WHERE ref_workspace.session_id = s.id
              AND ref_workspace.source_path LIKE '%workspaceStorage%'
        )
        AND NOT EXISTS (
            SELECT 1 FROM session_source_refs ref_global
            WHERE ref_global.session_id = s.id
              AND ref_global.source_path LIKE '%globalStorage%'
        )
        AND NOT EXISTS (
            SELECT 1 FROM session_requests request_rows
            WHERE request_rows.session_id = s.id
        )
        AND NOT EXISTS (
            SELECT 1 FROM session_token_totals totals
            WHERE totals.session_id = s.id
              AND (
                COALESCE(totals.total_tokens_max, 0) > 0
                OR EXISTS (
                    SELECT 1 FROM session_requests request_totals
                    WHERE request_totals.session_id = s.id
                      AND (
                        COALESCE(request_totals.total_tokens, 0) > 0
                        OR COALESCE(request_totals.input_tokens, 0) > 0
                        OR COALESCE(request_totals.output_tokens, 0) > 0
                      )
                )
              )
        )
    )";

impl Repository {
    pub fn sessions_list(&self, query: Option<SessionListQuery>) -> AppResult<SessionListResponse> {
        let query = normalize_session_list_query(query)?;
        let conn = self.pool.get()?;
        let facets = load_session_facets(&conn, &query)?;
        let total_items = count_filtered_sessions(&conn, &query)?;
        let total_pages = if total_items == 0 {
            1
        } else {
            total_items.div_ceil(query.page_size)
        };
        let page = query.page.min(total_pages.max(1));
        let offset = (page - 1) * query.page_size;
        let items = load_session_page(
            &conn,
            &query,
            self.cost_estimation_policy,
            query.page_size,
            offset,
        )?;

        Ok(SessionListResponse {
            items,
            pagination: SessionListPagination {
                page,
                page_size: query.page_size,
                total_items,
                total_pages,
            },
            summary: SessionListSummary {
                total_sessions: total_items,
                selected_source_apps: query.source_apps,
                selected_source_states: query.source_states,
            },
            facets,
        })
    }
}

fn load_session_page(
    conn: &rusqlite::Connection,
    query: &NormalizedSessionListQuery,
    cost_estimation_policy: CostEstimationPolicy,
    limit: u64,
    offset: u64,
) -> AppResult<Vec<SessionListItem>> {
    let (where_sql, mut sql_params) = build_session_where_clause(query, true);
    let session_estimated_cost_sql = session_estimated_cost_sql(cost_estimation_policy);
    let order_sql = session_order_clause(query, session_estimated_cost_sql);
    let sql = format!(
        "SELECT
            s.id,
            s.source_app,
            {SESSION_NAME_SQL},
            {SESSION_MODEL_SQL},
            {SESSION_LAST_UPDATED_SQL},
            {SESSION_SOURCE_STATE_SQL},
            {SESSION_LIST_INPUT_TOKENS_SQL},
            {SESSION_LIST_OUTPUT_TOKENS_SQL},
            {SESSION_LIST_TOTAL_TOKENS_SQL},
            NULL,
            {session_estimated_cost_sql},
            COALESCE(obs.message_count, 0)
         FROM sessions s
         LEFT JOIN session_token_totals st ON st.session_id = s.id
         LEFT JOIN session_observations obs ON obs.id = st.last_observation_id
         WHERE {where_sql}
         ORDER BY {order_sql}
         LIMIT ? OFFSET ?"
    );
    sql_params.push(rusqlite::types::Value::Integer(limit as i64));
    sql_params.push(rusqlite::types::Value::Integer(offset as i64));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(sql_params.iter()), map_session_list_item)?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

fn count_filtered_sessions(
    conn: &rusqlite::Connection,
    query: &NormalizedSessionListQuery,
) -> AppResult<u64> {
    let (where_sql, sql_params) = build_session_where_clause(query, true);
    let sql = format!("SELECT COUNT(*) FROM sessions s WHERE {where_sql}");
    let count: i64 = conn.query_row(&sql, params_from_iter(sql_params.iter()), |row| row.get(0))?;
    Ok(count.max(0) as u64)
}

fn load_session_facets(
    conn: &rusqlite::Connection,
    query: &NormalizedSessionListQuery,
) -> AppResult<SessionFacets> {
    let (where_sql, sql_params) = build_session_where_clause(query, false);

    let source_app_sql = format!(
        "SELECT s.source_app, COUNT(*)
         FROM sessions s
         WHERE {where_sql}
         GROUP BY s.source_app
         ORDER BY s.source_app ASC"
    );
    let mut source_app_stmt = conn.prepare(&source_app_sql)?;
    let source_app_rows =
        source_app_stmt.query_map(params_from_iter(sql_params.iter()), |row| {
            Ok(SessionFacetItem {
                value: normalize_source_app(&row.get::<_, String>(0)?),
                count: row.get(1)?,
            })
        })?;
    let mut source_apps = Vec::new();
    for row in source_app_rows {
        source_apps.push(row?);
    }

    let source_state_sql = format!(
        "SELECT {SESSION_SOURCE_STATE_SQL}, COUNT(*)
         FROM sessions s
         WHERE {where_sql}
         GROUP BY {SESSION_SOURCE_STATE_SQL}
         ORDER BY {SESSION_SOURCE_STATE_SQL} ASC"
    );
    let mut source_state_stmt = conn.prepare(&source_state_sql)?;
    let source_state_rows =
        source_state_stmt.query_map(params_from_iter(sql_params.iter()), |row| {
            Ok(SessionFacetItem {
                value: normalize_source_state(row.get::<_, String>(0)?),
                count: row.get(1)?,
            })
        })?;
    let mut source_states = Vec::new();
    for row in source_state_rows {
        source_states.push(row?);
    }

    Ok(SessionFacets {
        source_apps,
        source_states,
    })
}

fn build_session_where_clause(
    query: &NormalizedSessionListQuery,
    include_source_filters: bool,
) -> (String, Vec<rusqlite::types::Value>) {
    let mut clauses = vec![format!(
        "s.source_app IN ({SUPPORTED_SOURCE_APP_SQL_FILTER})"
    )];
    clauses.push(HIDE_CURSOR_WORKSPACE_EMPTY_SHELL_SQL.to_string());
    let mut sql_params = Vec::new();

    if let Some(search) = query.q.as_deref() {
        let pattern = format!("%{search}%");
        clauses.push(format!(
            "(LOWER({SESSION_NAME_SQL}) LIKE ?
              OR LOWER(s.source_app) LIKE ?
              OR LOWER({SESSION_MODEL_SQL}) LIKE ?)"
        ));
        for _ in 0..3 {
            sql_params.push(rusqlite::types::Value::Text(pattern.clone()));
        }
    }

    if include_source_filters && !query.source_apps.is_empty() {
        clauses.push(format!(
            "s.source_app IN ({})",
            sql_placeholders(query.source_apps.len())
        ));
        sql_params.extend(
            query
                .source_apps
                .iter()
                .cloned()
                .map(rusqlite::types::Value::Text),
        );
    }

    if include_source_filters && !query.source_states.is_empty() {
        clauses.push(format!(
            "{SESSION_SOURCE_STATE_SQL} IN ({})",
            sql_placeholders(query.source_states.len())
        ));
        sql_params.extend(
            query
                .source_states
                .iter()
                .cloned()
                .map(rusqlite::types::Value::Text),
        );
    }

    (clauses.join(" AND "), sql_params)
}

fn sql_placeholders(count: usize) -> String {
    (0..count).map(|_| "?").collect::<Vec<_>>().join(", ")
}

fn session_order_clause(
    query: &NormalizedSessionListQuery,
    session_estimated_cost_sql: &str,
) -> String {
    let direction = if query.sort_order == "asc" {
        "ASC"
    } else {
        "DESC"
    };

    match query.sort_by.as_str() {
        "name" => format!("{SESSION_NAME_SQL} {direction}, s.id {direction}"),
        "sourceApp" => format!("s.source_app {direction}, s.id {direction}"),
        "model" => format!("{SESSION_MODEL_SQL} {direction}, s.id {direction}"),
        "inputTokens" => {
            format!("{SESSION_LIST_INPUT_TOKENS_SQL} {direction}, s.id {direction}")
        }
        "outputTokens" => {
            format!("{SESSION_LIST_OUTPUT_TOKENS_SQL} {direction}, s.id {direction}")
        }
        "totalTokens" => {
            format!("{SESSION_LIST_TOTAL_TOKENS_SQL} {direction}, s.id {direction}")
        }
        "estimatedCostUsd" if query.sort_order == "desc" => {
            format!(
                "({session_estimated_cost_sql}) IS NOT NULL ASC, {session_estimated_cost_sql} DESC, s.id DESC"
            )
        }
        "estimatedCostUsd" => format!("{session_estimated_cost_sql} ASC, s.id ASC"),
        "messages" => format!("COALESCE(obs.message_count, 0) {direction}, s.id {direction}"),
        "sourceState" => format!("{SESSION_SOURCE_STATE_SQL} {direction}, s.id {direction}"),
        "lastUpdated" => format!("{SESSION_LAST_UPDATED_SQL} {direction}, s.id {direction}"),
        _ => format!("{SESSION_LAST_UPDATED_SQL} DESC, s.id DESC"),
    }
}

fn session_estimated_cost_sql(cost_estimation_policy: CostEstimationPolicy) -> &'static str {
    if cost_estimation_policy.bill_unknown_models_with_default_pricing {
        SESSION_ESTIMATED_COST_SQL
    } else {
        SESSION_ESTIMATED_COST_WITHOUT_FALLBACK_SQL
    }
}

fn map_session_list_item(row: &Row) -> rusqlite::Result<SessionListItem> {
    Ok(SessionListItem {
        id: row.get(0)?,
        source_app: normalize_source_app(&row.get::<_, String>(1)?),
        name: row.get(2)?,
        model: row.get(3)?,
        last_updated: row.get(4)?,
        source_state: normalize_source_state(row.get::<_, String>(5)?),
        input_tokens: row.get(6)?,
        output_tokens: row.get(7)?,
        total_tokens: row.get(8)?,
        token_confidence: row.get(9)?,
        estimated_cost_usd: row.get(10)?,
        messages: row.get(11)?,
    })
}

fn normalize_session_list_query(
    query: Option<SessionListQuery>,
) -> AppResult<NormalizedSessionListQuery> {
    let query = query.unwrap_or(SessionListQuery {
        page: None,
        page_size: None,
        q: None,
        source_apps: None,
        source_states: None,
        sort_by: None,
        sort_order: None,
    });

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let sort_by = query.sort_by.unwrap_or_else(|| "lastUpdated".to_string());
    let sort_order = query
        .sort_order
        .unwrap_or_else(|| "desc".to_string())
        .to_ascii_lowercase();

    if !ALLOWED_SORT_FIELDS.iter().any(|field| *field == sort_by) {
        return Err(AppError::validation("Invalid session sort field"));
    }
    if sort_order != "asc" && sort_order != "desc" {
        return Err(AppError::validation("Invalid session sort order"));
    }

    let source_apps =
        normalize_string_filters(query.source_apps, ALLOWED_SOURCE_APPS, "source app")?;
    let source_states =
        normalize_string_filters(query.source_states, ALLOWED_SOURCE_STATES, "source state")?;

    Ok(NormalizedSessionListQuery {
        page,
        page_size,
        q: query.q.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_lowercase())
            }
        }),
        source_apps,
        source_states,
        sort_by,
        sort_order,
    })
}
