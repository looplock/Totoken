use super::*;
use crate::pricing::estimate_usage_cost;

impl Repository {
    pub fn messages_list(&self, query: Option<MessageListQuery>) -> AppResult<MessageListResponse> {
        let query = normalize_message_list_query(query)?;
        let Some(session_id) = query.session_id.as_deref() else {
            return Ok(MessageListResponse {
                session: None,
                requests: Vec::new(),
                usage_events: Vec::new(),
            });
        };

        let conn = self.pool.get()?;
        let session_sql = format!(
            "SELECT
                    s.id,
                    s.title,
                    s.source_app,
                    COALESCE(obs.message_count, 0),
                    COALESCE(st.input_tokens_max, 0),
                    COALESCE(st.output_tokens_max, 0),
                    COALESCE(st.total_tokens_max, 0),
                    COALESCE((SELECT SUM(COALESCE(r.cache_read_input_tokens, 0)) FROM session_requests r WHERE r.session_id = s.id), 0),
                    COALESCE((SELECT SUM(COALESCE(r.cache_write_input_tokens, 0)) FROM session_requests r WHERE r.session_id = s.id), 0),
                    (SELECT SUM(r.estimated_cost_usd) FROM session_requests r WHERE r.session_id = s.id),
                    COALESCE(s.source_updated_at, s.discovered_last_at),
                    s.source_state
                 FROM sessions s
                 LEFT JOIN session_token_totals st ON st.session_id = s.id
                 LEFT JOIN session_observations obs ON obs.id = st.last_observation_id
                 WHERE s.id = ?1 AND s.source_app IN ({})
                 LIMIT 1",
            SUPPORTED_SOURCE_APP_SQL_FILTER
        );
        let session = conn
            .query_row(&session_sql, params![session_id], |row| {
                Ok(MessageSessionSummary {
                    session_id: row.get(0)?,
                    session_name: normalize_session_name(
                        row.get::<_, Option<String>>(1)?.as_deref(),
                    ),
                    source_app: normalize_source_app(&row.get::<_, String>(2)?),
                    session_total_messages: row.get(3)?,
                    session_input_tokens: row.get(4)?,
                    session_output_tokens: row.get(5)?,
                    session_total_tokens: row.get(6)?,
                    session_cache_read_input_tokens: row.get(7)?,
                    session_cache_write_input_tokens: row.get(8)?,
                    session_estimated_cost_usd: row.get(9)?,
                    session_last_updated: row.get(10)?,
                    session_source_state: normalize_source_state(row.get::<_, String>(11)?),
                })
            })
            .optional()?;

        let Some(mut session) = session else {
            return Ok(MessageListResponse {
                session: None,
                requests: Vec::new(),
                usage_events: Vec::new(),
            });
        };

        let mut request_stmt = conn.prepare(
            "SELECT
                r.id,
                r.session_id,
                s.title,
                r.source_app,
                r.sequence_no,
                r.status,
                r.message_count,
                COALESCE(NULLIF(TRIM(r.model), ''), NULLIF(TRIM(s.model_last), ''), NULLIF(TRIM(s.model_first), '')),
                r.input_tokens,
                r.output_tokens,
                r.total_tokens,
                r.cache_read_input_tokens,
                r.cache_write_input_tokens,
                r.estimated_cost_usd,
                r.token_confidence,
                r.source_created_at,
                r.source_updated_at,
                r.source_locator
             FROM session_requests r
             INNER JOIN sessions s ON s.id = r.session_id
             WHERE r.session_id = ?1
             ORDER BY r.sequence_no ASC, r.id ASC",
        )?;
        let mut request_rows = request_stmt
            .query_map(params![session_id], |row| {
                let source_app = normalize_source_app(&row.get::<_, String>(3)?);
                let sequence_no = row.get::<_, i64>(4)?;
                let source_locator = row.get::<_, String>(17)?;
                Ok(MessageRequestItem {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    session_name: normalize_session_name(
                        row.get::<_, Option<String>>(2)?.as_deref(),
                    ),
                    source_app: source_app.clone(),
                    sequence_no,
                    status: row.get(5)?,
                    message_count: row.get(6)?,
                    model: row.get(7)?,
                    input_tokens: row.get(8)?,
                    output_tokens: row.get(9)?,
                    total_tokens: row.get(10)?,
                    cache_read_input_tokens: row.get(11)?,
                    cache_write_input_tokens: row.get(12)?,
                    estimated_cost_usd: row.get(13)?,
                    token_confidence: row.get(14)?,
                    created_at: row.get(15)?,
                    updated_at: row.get(16)?,
                    source_locator_label: build_request_locator_label(
                        source_app.as_str(),
                        sequence_no,
                        source_locator.as_str(),
                    ),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        hydrate_request_estimated_costs(&conn, &mut request_rows, self.cost_estimation_policy)?;

        let mut event_stmt = conn.prepare(
            "SELECT
                e.id,
                e.session_id,
                e.event_time_utc,
                e.source_app,
                COALESCE(NULLIF(TRIM(e.model), ''), NULLIF(TRIM(s.model_last), ''), NULLIF(TRIM(s.model_first), '')),
                e.delta_input,
                e.delta_output,
                e.delta_total,
                COALESCE(e.cache_read_input_tokens, 0),
                COALESCE(e.cache_write_input_tokens, 0),
                e.estimated_cost_usd,
                COALESCE(e.granularity, 'unknown'),
                COALESCE(e.confidence, 'low'),
                e.source_event_id,
                e.epoch_no
             FROM token_usage_events e
             LEFT JOIN sessions s ON s.id = e.session_id
             WHERE e.session_id = ?1
             ORDER BY e.event_time_utc ASC, e.id ASC",
        )?;
        let mut usage_events = event_stmt
            .query_map(params![session_id], |row| {
                Ok(MessageUsageEventItem {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    event_time_utc: row.get(2)?,
                    source_app: normalize_source_app(&row.get::<_, String>(3)?),
                    model: row.get(4)?,
                    delta_input: row.get(5)?,
                    delta_output: row.get(6)?,
                    delta_total: row.get(7)?,
                    cache_read_input_tokens: row.get(8)?,
                    cache_write_input_tokens: row.get(9)?,
                    estimated_cost_usd: row.get(10)?,
                    granularity: row.get(11)?,
                    confidence: row.get(12)?,
                    source_event_id: row.get(13)?,
                    epoch_no: row.get(14)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        hydrate_usage_event_estimated_costs(&conn, &mut usage_events, self.cost_estimation_policy)?;
        session.session_estimated_cost_usd = sum_estimated_costs(
            request_rows
                .iter()
                .filter_map(|request| request.estimated_cost_usd),
        );

        Ok(MessageListResponse {
            session: Some(session),
            requests: request_rows,
            usage_events,
        })
    }
}

fn normalize_message_list_query(
    query: Option<MessageListQuery>,
) -> AppResult<NormalizedSessionActivityQuery> {
    let query = query.unwrap_or(MessageListQuery { session_id: None });

    Ok(NormalizedSessionActivityQuery {
        session_id: query.session_id.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }),
    })
}

fn build_request_locator_label(source_app: &str, sequence_no: i64, source_locator: &str) -> String {
    match source_app {
        "opencode" => format!("request #{sequence_no}"),
        "claude_code" => serde_json::from_str::<serde_json::Value>(source_locator)
            .ok()
            .and_then(|value| value.get("line_number").and_then(serde_json::Value::as_i64))
            .map(|line_number| format!("turn near line {line_number}"))
            .unwrap_or_else(|| format!("request #{sequence_no}")),
        "codex" => serde_json::from_str::<serde_json::Value>(source_locator)
            .ok()
            .and_then(|value| value.get("line_number").and_then(serde_json::Value::as_i64))
            .map(|line_number| format!("turn near line {line_number}"))
            .unwrap_or_else(|| format!("request #{sequence_no}")),
        _ => format!("request #{sequence_no}"),
    }
}

fn hydrate_request_estimated_costs(
    conn: &rusqlite::Connection,
    requests: &mut [MessageRequestItem],
    cost_estimation_policy: CostEstimationPolicy,
) -> AppResult<()> {
    let mut pricing_by_model = crate::pricing::ModelPricingCache::new();

    for request in requests {
        request.estimated_cost_usd = estimate_usage_cost(
            conn,
            &mut pricing_by_model,
            cost_estimation_policy,
            request.model.as_deref(),
            request.input_tokens.unwrap_or(0),
            request.output_tokens.unwrap_or(0),
            request.total_tokens,
            request.cache_read_input_tokens.unwrap_or(0),
            request.cache_write_input_tokens.unwrap_or(0),
        )?;
    }

    Ok(())
}

fn hydrate_usage_event_estimated_costs(
    conn: &rusqlite::Connection,
    events: &mut [MessageUsageEventItem],
    cost_estimation_policy: CostEstimationPolicy,
) -> AppResult<()> {
    let mut pricing_by_model = crate::pricing::ModelPricingCache::new();

    for event in events {
        event.estimated_cost_usd = estimate_usage_cost(
            conn,
            &mut pricing_by_model,
            cost_estimation_policy,
            event.model.as_deref(),
            event.delta_input,
            event.delta_output,
            Some(event.delta_total),
            event.cache_read_input_tokens,
            event.cache_write_input_tokens,
        )?;
    }

    Ok(())
}

fn sum_estimated_costs(costs: impl Iterator<Item = f64>) -> Option<f64> {
    let mut total = 0.0;
    let mut has_value = false;

    for cost in costs {
        total += cost;
        has_value = true;
    }

    has_value.then_some(total)
}
