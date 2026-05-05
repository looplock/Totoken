use super::*;

impl Repository {
    pub fn statistics_get(&self, query: Option<StatisticsQuery>) -> AppResult<StatisticsOverview> {
        let query = normalize_statistics_query(query)?;
        let conn = self.pool.get()?;

        let current_events = load_statistics_records(
            &conn,
            query.start_at,
            query.end_at,
            query.source_app.as_deref(),
        )?;
        let previous_events = load_statistics_records(
            &conn,
            query.previous_start_at,
            query.start_at,
            query.source_app.as_deref(),
        )?;

        let available_models = build_available_models(&current_events);
        let filtered_current = filter_statistics_events(&current_events, &query);
        let filtered_previous = filter_statistics_events(&previous_events, &query);
        let buckets = build_time_buckets(query.start_at, query.end_at, &query.granularity);
        let row_series_len = buckets.len();

        let summary = build_statistics_summary(&filtered_current, &filtered_previous);
        let trend = build_statistics_trend(&filtered_current, &buckets);
        let activity = build_statistics_activity(&filtered_current);
        let distribution = build_statistics_distribution(&filtered_current);
        let detail_rows = build_statistics_detail_rows(
            &filtered_current,
            &filtered_previous,
            &buckets,
            row_series_len,
        );

        Ok(StatisticsOverview {
            summary,
            trend,
            activity,
            distribution,
            detail_rows,
            available_models,
            range: StatisticsRange {
                period: query.period,
                granularity: query.granularity,
                start_at: query.start_at,
                end_at: query.end_at,
            },
        })
    }
}

fn normalize_statistics_query(
    query: Option<StatisticsQuery>,
) -> AppResult<NormalizedStatisticsQuery> {
    let query = query.unwrap_or(StatisticsQuery {
        q: None,
        source_app: None,
        model: None,
        period: None,
        granularity: None,
        start_date: None,
        end_date: None,
    });

    let period = query.period.unwrap_or_else(|| "1d".to_string());
    if !ALLOWED_STATISTICS_PERIODS
        .iter()
        .any(|allowed| *allowed == period)
    {
        return Err(AppError::validation(format!(
            "Invalid statistics period: {period}"
        )));
    }

    let granularity = query.granularity.unwrap_or_else(|| "day".to_string());
    if !ALLOWED_STATISTICS_GRANULARITIES
        .iter()
        .any(|allowed| *allowed == granularity)
    {
        return Err(AppError::validation(format!(
            "Invalid statistics granularity: {granularity}"
        )));
    }

    let source_app = query
        .source_app
        .map(|value| normalize_source_app(value.trim()))
        .filter(|value| !value.is_empty());
    if let Some(source_app) = source_app.as_deref() {
        if !ALLOWED_SOURCE_APPS.contains(&source_app) {
            return Err(AppError::validation(format!(
                "Invalid statistics source app: {source_app}"
            )));
        }
    }

    let model = query
        .model
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let q = query
        .q
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty());

    let today = Local::now().date_naive();
    let (start_date, end_date) = if period == "custom" {
        let start_date = query
            .start_date
            .as_deref()
            .map(parse_statistics_date)
            .transpose()?
            .unwrap_or(today - Duration::days(29));
        let end_date = query
            .end_date
            .as_deref()
            .map(parse_statistics_date)
            .transpose()?
            .unwrap_or(today);
        if end_date < start_date {
            return Err(AppError::validation(
                "Statistics end date must not be earlier than start date.",
            ));
        }
        (start_date, end_date)
    } else {
        let end_date = query
            .end_date
            .as_deref()
            .map(parse_statistics_date)
            .transpose()?
            .unwrap_or(today);
        let days = match period.as_str() {
            "1d" => 1,
            "7d" => 7,
            "30d" => 30,
            _ => 30,
        };
        (end_date - Duration::days(days - 1), end_date)
    };

    let start_at = local_date_start_to_utc(start_date)?;
    let end_at = local_date_start_to_utc(
        end_date
            .succ_opt()
            .ok_or_else(|| AppError::validation("Invalid statistics end date."))?,
    )?;
    let previous_start_at = start_at - (end_at - start_at);

    Ok(NormalizedStatisticsQuery {
        q,
        source_app,
        model,
        period,
        granularity,
        start_at,
        end_at,
        previous_start_at,
    })
}

fn parse_statistics_date(value: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| AppError::validation(format!("Invalid statistics date: {value}")))
}

fn local_date_start_to_utc(date: NaiveDate) -> AppResult<DateTime<Utc>> {
    let local_naive = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| AppError::validation("Invalid statistics date."))?;
    let local_time = match Local.from_local_datetime(&local_naive) {
        LocalResult::Single(value) => value,
        LocalResult::Ambiguous(value, _) => value,
        LocalResult::None => {
            return Err(AppError::validation(
                "Invalid statistics date in local timezone.",
            ));
        }
    };

    Ok(local_time.with_timezone(&Utc))
}

fn load_statistics_records(
    conn: &rusqlite::Connection,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    source_app: Option<&str>,
) -> AppResult<Vec<StatisticsEventRecord>> {
    let mut events = load_statistics_events(conn, start_at, end_at, source_app)?;
    events.extend(load_statistics_legacy_totals(
        conn, start_at, end_at, source_app,
    )?);
    events.sort_by(|left, right| {
        left.event_time_utc
            .cmp(&right.event_time_utc)
            .then_with(|| left.session_id.cmp(&right.session_id))
            .then_with(|| left.model.cmp(&right.model))
    });
    Ok(events)
}

fn load_statistics_events(
    conn: &rusqlite::Connection,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    source_app: Option<&str>,
) -> AppResult<Vec<StatisticsEventRecord>> {
    let sql = if source_app.is_some() {
        format!(
            "SELECT
            e.session_id,
            e.event_time_utc,
            e.source_app,
            COALESCE(NULLIF(TRIM(e.model), ''), NULLIF(TRIM(s.model_last), ''), NULLIF(TRIM(s.model_first), ''), 'Unknown'),
            COALESCE(e.delta_input, 0),
            COALESCE(e.delta_output, 0),
            COALESCE(e.delta_total, COALESCE(e.delta_input, 0) + COALESCE(e.delta_output, 0)),
            COALESCE(e.cache_read_input_tokens, 0),
            COALESCE(e.cache_write_input_tokens, 0)
         FROM token_usage_events e
         LEFT JOIN sessions s ON s.id = e.session_id
         WHERE e.event_time_utc >= ?1 AND e.event_time_utc < ?2
           AND e.source_app = ?3
           AND e.source_app IN ({})
         ORDER BY e.event_time_utc ASC, e.id ASC",
            SUPPORTED_SOURCE_APP_SQL_FILTER
        )
    } else {
        format!(
            "SELECT
            e.session_id,
            e.event_time_utc,
            e.source_app,
            COALESCE(NULLIF(TRIM(e.model), ''), NULLIF(TRIM(s.model_last), ''), NULLIF(TRIM(s.model_first), ''), 'Unknown'),
            COALESCE(e.delta_input, 0),
            COALESCE(e.delta_output, 0),
            COALESCE(e.delta_total, COALESCE(e.delta_input, 0) + COALESCE(e.delta_output, 0)),
            COALESCE(e.cache_read_input_tokens, 0),
            COALESCE(e.cache_write_input_tokens, 0)
         FROM token_usage_events e
         LEFT JOIN sessions s ON s.id = e.session_id
         WHERE e.event_time_utc >= ?1 AND e.event_time_utc < ?2
           AND e.source_app IN ({})
         ORDER BY e.event_time_utc ASC, e.id ASC",
            SUPPORTED_SOURCE_APP_SQL_FILTER
        )
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(source_app) = source_app {
        stmt.query_map(
            params![start_at, end_at, source_app],
            map_statistics_event_row,
        )?
    } else {
        stmt.query_map(params![start_at, end_at], map_statistics_event_row)?
    };

    let mut events = Vec::new();
    for row in rows {
        events.push(row?);
    }
    hydrate_statistics_event_estimated_costs(conn, &mut events)?;
    Ok(events)
}

fn load_statistics_legacy_totals(
    conn: &rusqlite::Connection,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    source_app: Option<&str>,
) -> AppResult<Vec<StatisticsEventRecord>> {
    let sql = if source_app.is_some() {
        format!(
            "SELECT
            s.id,
            COALESCE(s.source_updated_at, s.source_created_at, s.discovered_last_at, s.discovered_first_at),
            s.source_app,
            COALESCE(NULLIF(TRIM(s.model_last), ''), NULLIF(TRIM(s.model_first), ''), 'Unknown'),
            COALESCE(st.input_tokens_max, 0),
            COALESCE(st.output_tokens_max, 0),
            COALESCE(st.total_tokens_max, COALESCE(st.input_tokens_max, 0) + COALESCE(st.output_tokens_max, 0)),
            COALESCE((SELECT SUM(COALESCE(r.cache_read_input_tokens, 0)) FROM session_requests r WHERE r.session_id = s.id), 0),
            COALESCE((SELECT SUM(COALESCE(r.cache_write_input_tokens, 0)) FROM session_requests r WHERE r.session_id = s.id), 0)
         FROM sessions s
         INNER JOIN session_token_totals st ON st.session_id = s.id
         WHERE COALESCE(s.source_updated_at, s.source_created_at, s.discovered_last_at, s.discovered_first_at) >= ?1
           AND COALESCE(s.source_updated_at, s.source_created_at, s.discovered_last_at, s.discovered_first_at) < ?2
           AND COALESCE(st.total_tokens_max, COALESCE(st.input_tokens_max, 0) + COALESCE(st.output_tokens_max, 0)) > 0
           AND s.source_app = ?3
           AND s.source_app IN ({})
           AND NOT EXISTS (
                SELECT 1
                FROM token_usage_events e
                WHERE e.session_id = s.id
           )
         ORDER BY 2 ASC, s.id ASC",
            SUPPORTED_SOURCE_APP_SQL_FILTER
        )
    } else {
        format!(
            "SELECT
            s.id,
            COALESCE(s.source_updated_at, s.source_created_at, s.discovered_last_at, s.discovered_first_at),
            s.source_app,
            COALESCE(NULLIF(TRIM(s.model_last), ''), NULLIF(TRIM(s.model_first), ''), 'Unknown'),
            COALESCE(st.input_tokens_max, 0),
            COALESCE(st.output_tokens_max, 0),
            COALESCE(st.total_tokens_max, COALESCE(st.input_tokens_max, 0) + COALESCE(st.output_tokens_max, 0)),
            COALESCE((SELECT SUM(COALESCE(r.cache_read_input_tokens, 0)) FROM session_requests r WHERE r.session_id = s.id), 0),
            COALESCE((SELECT SUM(COALESCE(r.cache_write_input_tokens, 0)) FROM session_requests r WHERE r.session_id = s.id), 0)
         FROM sessions s
         INNER JOIN session_token_totals st ON st.session_id = s.id
         WHERE COALESCE(s.source_updated_at, s.source_created_at, s.discovered_last_at, s.discovered_first_at) >= ?1
           AND COALESCE(s.source_updated_at, s.source_created_at, s.discovered_last_at, s.discovered_first_at) < ?2
           AND COALESCE(st.total_tokens_max, COALESCE(st.input_tokens_max, 0) + COALESCE(st.output_tokens_max, 0)) > 0
           AND s.source_app IN ({})
           AND NOT EXISTS (
                SELECT 1
                FROM token_usage_events e
                WHERE e.session_id = s.id
           )
         ORDER BY 2 ASC, s.id ASC",
            SUPPORTED_SOURCE_APP_SQL_FILTER
        )
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(source_app) = source_app {
        stmt.query_map(
            params![start_at, end_at, source_app],
            map_statistics_event_row,
        )?
    } else {
        stmt.query_map(params![start_at, end_at], map_statistics_event_row)?
    };

    let mut events = Vec::new();
    for row in rows {
        events.push(row?);
    }
    hydrate_statistics_event_estimated_costs(conn, &mut events)?;
    Ok(events)
}

fn map_statistics_event_row(row: &Row) -> rusqlite::Result<StatisticsEventRecord> {
    Ok(StatisticsEventRecord {
        session_id: row.get(0)?,
        event_time_utc: row.get(1)?,
        source_app: normalize_source_app(&row.get::<_, String>(2)?),
        model: row.get(3)?,
        delta_input: row.get(4)?,
        delta_output: row.get(5)?,
        delta_total: row.get(6)?,
        cache_read_input_tokens: row.get(7)?,
        cache_write_input_tokens: row.get(8)?,
        estimated_cost_usd: None,
    })
}

fn build_available_models(events: &[StatisticsEventRecord]) -> Vec<String> {
    let mut models = BTreeSet::new();
    for event in events {
        models.insert(event.model.clone());
    }
    models.into_iter().collect()
}

fn filter_statistics_events(
    events: &[StatisticsEventRecord],
    query: &NormalizedStatisticsQuery,
) -> Vec<StatisticsEventRecord> {
    events
        .iter()
        .filter(|event| statistics_event_matches(event, query))
        .cloned()
        .collect()
}

fn statistics_event_matches(
    event: &StatisticsEventRecord,
    query: &NormalizedStatisticsQuery,
) -> bool {
    if let Some(model) = query.model.as_deref() {
        if !event.model.eq_ignore_ascii_case(model) {
            return false;
        }
    }

    if let Some(q) = query.q.as_deref() {
        let haystack = format!("{} {}", event.source_app, event.model).to_lowercase();
        if !haystack.contains(q) {
            return false;
        }
    }

    true
}

pub(super) fn build_statistics_summary(
    current_events: &[StatisticsEventRecord],
    previous_events: &[StatisticsEventRecord],
) -> StatisticsSummary {
    let current = accumulate_statistics_summary(current_events);
    let previous = accumulate_statistics_summary(previous_events);

    StatisticsSummary {
        total_tokens: build_statistics_metric(current.total_tokens, previous.total_tokens),
        input_tokens: build_statistics_metric(current.input_tokens, previous.input_tokens),
        output_tokens: build_statistics_metric(current.output_tokens, previous.output_tokens),
        estimated_cost_usd: build_statistics_cost_metric(
            current.estimated_cost_usd,
            previous.estimated_cost_usd,
        ),
        total_sessions: build_statistics_metric(
            current.sessions.len() as i64,
            previous.sessions.len() as i64,
        ),
        active_models: build_statistics_metric(
            current.models.len() as i64,
            previous.models.len() as i64,
        ),
        avg_tokens_per_session: build_statistics_metric(
            average_tokens_per_session(current.total_tokens, current.sessions.len()),
            average_tokens_per_session(previous.total_tokens, previous.sessions.len()),
        ),
    }
}

fn accumulate_statistics_summary(events: &[StatisticsEventRecord]) -> StatisticsSummaryAccumulator {
    let mut summary = StatisticsSummaryAccumulator {
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
        sessions: BTreeSet::new(),
        models: BTreeSet::new(),
    };

    for event in events {
        summary.input_tokens += event.delta_input;
        summary.output_tokens += event.delta_output;
        summary.total_tokens += event.delta_total;
        summary.estimated_cost_usd += event.estimated_cost_usd.unwrap_or(0.0);
        summary.sessions.insert(event.session_id.clone());
        summary.models.insert(event.model.clone());
    }

    summary
}

fn build_statistics_metric(current: i64, previous: i64) -> StatisticsMetricValue {
    StatisticsMetricValue {
        value: current,
        delta_percent: compute_delta_percent(current, previous),
    }
}

fn build_statistics_cost_metric(current: f64, previous: f64) -> StatisticsCostMetricValue {
    StatisticsCostMetricValue {
        value: current,
        delta_percent: compute_delta_percent_f64(current, previous),
    }
}

fn build_statistics_trend(
    events: &[StatisticsEventRecord],
    buckets: &[TimeBucket],
) -> StatisticsTrend {
    let mut input = vec![0_i64; buckets.len()];
    let mut output = vec![0_i64; buckets.len()];
    let mut total = vec![0_i64; buckets.len()];
    let mut cache_read_input = vec![0_i64; buckets.len()];
    let mut cache_write_input = vec![0_i64; buckets.len()];
    let mut cost_usd = vec![0.0_f64; buckets.len()];

    for event in events {
        if let Some(index) = statistics_bucket_index(event.event_time_utc, buckets) {
            input[index] += event.delta_input;
            output[index] += event.delta_output;
            total[index] += event.delta_total;
            cache_read_input[index] += event.cache_read_input_tokens;
            cache_write_input[index] += event.cache_write_input_tokens;
            cost_usd[index] += event.estimated_cost_usd.unwrap_or(0.0);
        }
    }

    StatisticsTrend {
        bucket_starts: buckets.iter().map(|bucket| bucket.start_at).collect(),
        input,
        output,
        total,
        cache_read_input,
        cache_write_input,
        cost_usd,
    }
}

pub(super) fn build_statistics_activity(events: &[StatisticsEventRecord]) -> StatisticsActivity {
    let mut session_matrix = vec![vec![BTreeSet::<String>::new(); 24]; 7];
    let mut token_matrix = vec![vec![0.0_f64; 24]; 7];
    let mut cost_matrix = vec![vec![0.0_f64; 24]; 7];
    let mut session_max_value = 0.0_f64;
    let mut token_max_value = 0.0_f64;
    let mut cost_max_value = 0.0_f64;

    for event in events {
        let local_time = event.event_time_utc.with_timezone(&Local);
        let day_index = local_time.weekday().num_days_from_monday() as usize;
        let hour_index = local_time.hour() as usize;
        session_matrix[day_index][hour_index].insert(event.session_id.clone());
        session_max_value =
            session_max_value.max(session_matrix[day_index][hour_index].len() as f64);

        token_matrix[day_index][hour_index] += event.delta_total as f64;
        token_max_value = token_max_value.max(token_matrix[day_index][hour_index]);

        cost_matrix[day_index][hour_index] += event.estimated_cost_usd.unwrap_or(0.0);
        cost_max_value = cost_max_value.max(cost_matrix[day_index][hour_index]);
    }

    let session_count_matrix = session_matrix
        .into_iter()
        .map(|row| row.into_iter().map(|bucket| bucket.len() as f64).collect())
        .collect();

    StatisticsActivity {
        sessions: StatisticsActivityMetric {
            matrix: session_count_matrix,
            max_value: session_max_value,
        },
        tokens: StatisticsActivityMetric {
            matrix: token_matrix,
            max_value: token_max_value,
        },
        cost: StatisticsActivityMetric {
            matrix: cost_matrix,
            max_value: cost_max_value,
        },
    }
}

pub(super) fn build_statistics_distribution(
    events: &[StatisticsEventRecord],
) -> Vec<StatisticsDistributionRow> {
    let mut grouped = BTreeMap::<String, (BTreeSet<String>, i64, f64)>::new();

    for event in events {
        let entry = grouped
            .entry(event.source_app.clone())
            .or_insert_with(|| (BTreeSet::new(), 0, 0.0));
        entry.0.insert(event.session_id.clone());
        entry.1 += event.delta_total;
        entry.2 += event.estimated_cost_usd.unwrap_or(0.0);
    }

    grouped
        .into_iter()
        .map(
            |(app, (sessions, total_tokens, estimated_cost_usd))| StatisticsDistributionRow {
                app,
                sessions: sessions.len() as i64,
                total_tokens,
                estimated_cost_usd,
            },
        )
        .collect()
}

fn build_statistics_detail_rows(
    current_events: &[StatisticsEventRecord],
    previous_events: &[StatisticsEventRecord],
    buckets: &[TimeBucket],
    row_series_len: usize,
) -> Vec<StatisticsDetailRow> {
    let mut current_by_key = BTreeMap::<(String, String), StatisticsRowAccumulator>::new();
    let mut previous_totals = BTreeMap::<(String, String), i64>::new();

    for event in current_events {
        let key = (event.source_app.clone(), event.model.clone());
        let entry = current_by_key
            .entry(key)
            .or_insert_with(|| StatisticsRowAccumulator {
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                estimated_cost_usd: 0.0,
                sessions: BTreeSet::new(),
                last_active_at: None,
                sparkline: vec![0; row_series_len],
            });
        entry.input_tokens += event.delta_input;
        entry.output_tokens += event.delta_output;
        entry.total_tokens += event.delta_total;
        entry.estimated_cost_usd += event.estimated_cost_usd.unwrap_or(0.0);
        entry.sessions.insert(event.session_id.clone());
        entry.last_active_at = Some(
            entry
                .last_active_at
                .map(|value| value.max(event.event_time_utc))
                .unwrap_or(event.event_time_utc),
        );
        if let Some(index) = statistics_bucket_index(event.event_time_utc, buckets) {
            entry.sparkline[index] += event.delta_total;
        }
    }

    for event in previous_events {
        let key = (event.source_app.clone(), event.model.clone());
        *previous_totals.entry(key).or_default() += event.delta_total;
    }

    let mut rows = current_by_key
        .into_iter()
        .map(|((app, model), value)| {
            let previous_total = previous_totals
                .get(&(app.clone(), model.clone()))
                .copied()
                .unwrap_or(0);
            let trend_percent = compute_delta_percent(value.total_tokens, previous_total);
            StatisticsDetailRow {
                id: format!("{app}:{model}"),
                app,
                model,
                sessions: value.sessions.len() as i64,
                input_tokens: value.input_tokens,
                output_tokens: value.output_tokens,
                estimated_cost_usd: value.estimated_cost_usd,
                avg_tokens_per_session: average_tokens_per_session(
                    value.total_tokens,
                    value.sessions.len(),
                ),
                last_active_at: value.last_active_at,
                trend_percent,
                trend_direction: if trend_percent < 0.0 {
                    "down".to_string()
                } else {
                    "up".to_string()
                },
                sparkline: value.sparkline,
            }
        })
        .collect::<Vec<_>>();

    rows.sort_by(|left, right| {
        right
            .input_tokens
            .saturating_add(right.output_tokens)
            .cmp(&left.input_tokens.saturating_add(left.output_tokens))
            .then_with(|| left.app.cmp(&right.app))
            .then_with(|| left.model.cmp(&right.model))
    });
    rows
}

fn build_time_buckets(
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    granularity: &str,
) -> Vec<TimeBucket> {
    let step = match granularity {
        "hour" => Duration::hours(1),
        "week" => Duration::days(7),
        "month" => Duration::days(30),
        _ => Duration::days(1),
    };

    let mut buckets = Vec::new();
    let mut cursor = start_at;
    while cursor < end_at {
        let next = (cursor + step).min(end_at);
        buckets.push(TimeBucket {
            start_at: cursor,
            end_at: next,
        });
        cursor = next;
    }

    if buckets.is_empty() {
        buckets.push(TimeBucket { start_at, end_at });
    }

    buckets
}

fn statistics_bucket_index(event_time: DateTime<Utc>, buckets: &[TimeBucket]) -> Option<usize> {
    buckets
        .iter()
        .position(|bucket| event_time >= bucket.start_at && event_time < bucket.end_at)
}

fn average_tokens_per_session(total_tokens: i64, session_count: usize) -> i64 {
    if session_count == 0 {
        0
    } else {
        (total_tokens as f64 / session_count as f64).round() as i64
    }
}

fn compute_delta_percent(current: i64, previous: i64) -> f64 {
    compute_delta_percent_f64(current as f64, previous as f64)
}

fn compute_delta_percent_f64(current: f64, previous: f64) -> f64 {
    let delta = if !current.is_finite() || !previous.is_finite() {
        0.0
    } else if previous <= 0.0 {
        if current <= 0.0 {
            0.0
        } else {
            100.0
        }
    } else {
        ((current - previous) / previous) * 100.0
    };

    (delta * 10.0).round() / 10.0
}

fn hydrate_statistics_event_estimated_costs(
    conn: &rusqlite::Connection,
    events: &mut [StatisticsEventRecord],
) -> AppResult<()> {
    let mut pricing_by_model = HashMap::<String, Option<ModelPricing>>::new();

    for event in events {
        event.estimated_cost_usd = estimate_usage_cost(
            conn,
            &mut pricing_by_model,
            Some(event.model.as_str()),
            event.delta_input,
            event.delta_output,
            Some(event.delta_total),
            event.cache_read_input_tokens,
            event.cache_write_input_tokens,
        )?;
    }

    Ok(())
}
