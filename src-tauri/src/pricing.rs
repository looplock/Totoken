use std::collections::{BTreeSet, HashMap};

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::AppResult;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ModelPricing {
    pub input_usd_per_mtok: Option<f64>,
    pub output_usd_per_mtok: Option<f64>,
    pub cache_read_usd_per_mtok: Option<f64>,
    pub cache_write_usd_per_mtok: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EstimatedCostBackfillSummary {
    pub session_requests_updated: usize,
    pub token_usage_events_updated: usize,
}

#[derive(Debug)]
struct CostBackfillUsageRow {
    id: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: Option<i64>,
    cache_read_input_tokens: i64,
    cache_write_input_tokens: i64,
}

#[allow(clippy::too_many_arguments)]
pub fn estimate_usage_cost(
    conn: &Connection,
    pricing_by_model: &mut HashMap<String, Option<ModelPricing>>,
    model_name: Option<&str>,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: Option<i64>,
    cache_read_input_tokens: i64,
    cache_write_input_tokens: i64,
) -> AppResult<Option<f64>> {
    let Some(model_name) = sanitize_model_name(model_name) else {
        return Ok(None);
    };

    let pricing = resolve_model_pricing(conn, pricing_by_model, &model_name)?;
    let Some(pricing) = pricing else {
        return Ok(None);
    };
    if pricing.input_usd_per_mtok.is_none()
        && pricing.output_usd_per_mtok.is_none()
        && pricing.cache_read_usd_per_mtok.is_none()
        && pricing.cache_write_usd_per_mtok.is_none()
    {
        return Ok(None);
    }

    let billable_input_tokens = if cache_read_input_tokens > 0 || cache_write_input_tokens > 0 {
        match total_tokens {
            Some(total_tokens) if total_tokens > input_tokens.saturating_add(output_tokens) => {
                input_tokens
            }
            _ => input_tokens
                .saturating_sub(cache_read_input_tokens)
                .saturating_sub(cache_write_input_tokens),
        }
    } else {
        input_tokens
    };
    let total_cost = cost_component(billable_input_tokens, pricing.input_usd_per_mtok)
        + cost_component(output_tokens, pricing.output_usd_per_mtok)
        + cost_component(cache_read_input_tokens, pricing.cache_read_usd_per_mtok)
        + cost_component(cache_write_input_tokens, pricing.cache_write_usd_per_mtok);

    Ok((total_cost > 0.0).then_some(total_cost))
}

pub fn backfill_missing_estimated_costs(
    conn: &mut Connection,
) -> AppResult<EstimatedCostBackfillSummary> {
    let tx = conn.transaction()?;
    let request_rows = load_session_request_cost_backfill_rows(&tx)?;
    let event_rows = load_token_usage_event_cost_backfill_rows(&tx)?;
    let mut pricing_by_model = HashMap::<String, Option<ModelPricing>>::new();
    let mut summary = EstimatedCostBackfillSummary::default();

    for row in request_rows {
        let Some(estimated_cost_usd) = estimate_usage_cost(
            &tx,
            &mut pricing_by_model,
            Some(row.model.as_str()),
            row.input_tokens,
            row.output_tokens,
            row.total_tokens,
            row.cache_read_input_tokens,
            row.cache_write_input_tokens,
        )?
        else {
            continue;
        };

        let updated = tx.execute(
            "UPDATE session_requests
             SET estimated_cost_usd = ?2
             WHERE id = ?1
               AND estimated_cost_usd IS NULL",
            params![row.id, estimated_cost_usd],
        )?;
        summary.session_requests_updated += updated;
    }

    for row in event_rows {
        let Some(estimated_cost_usd) = estimate_usage_cost(
            &tx,
            &mut pricing_by_model,
            Some(row.model.as_str()),
            row.input_tokens,
            row.output_tokens,
            row.total_tokens,
            row.cache_read_input_tokens,
            row.cache_write_input_tokens,
        )?
        else {
            continue;
        };

        let updated = tx.execute(
            "UPDATE token_usage_events
             SET estimated_cost_usd = ?2
             WHERE id = ?1
               AND estimated_cost_usd IS NULL",
            params![row.id, estimated_cost_usd],
        )?;
        summary.token_usage_events_updated += updated;
    }

    tx.commit()?;
    Ok(summary)
}

pub fn resolve_model_pricing(
    conn: &Connection,
    pricing_by_model: &mut HashMap<String, Option<ModelPricing>>,
    model_name: &str,
) -> AppResult<Option<ModelPricing>> {
    let Some(model_name) = sanitize_model_name(Some(model_name)) else {
        return Ok(None);
    };

    let cache_key = model_name.to_ascii_lowercase();
    if let Some(cached) = pricing_by_model.get(&cache_key) {
        return Ok(*cached);
    }

    let mut pricing = None;
    for candidate in build_model_lookup_candidates(&model_name) {
        pricing = query_pricing_by_exact_match(conn, &candidate)?;
        if pricing.is_some() {
            break;
        }
    }

    if pricing.is_none() {
        for candidate in build_model_lookup_candidates(&model_name) {
            pricing = query_pricing_by_prefix_match(conn, &candidate)?;
            if pricing.is_some() {
                break;
            }
        }
    }

    pricing_by_model.insert(cache_key, pricing);
    Ok(pricing)
}

fn load_session_request_cost_backfill_rows(
    conn: &Connection,
) -> AppResult<Vec<CostBackfillUsageRow>> {
    let mut stmt = conn.prepare(
        "SELECT
            r.id,
            COALESCE(
                NULLIF(TRIM(r.model), ''),
                NULLIF(TRIM(s.model_last), ''),
                NULLIF(TRIM(s.model_first), '')
            ) AS model,
            COALESCE(r.input_tokens, 0),
            COALESCE(r.output_tokens, 0),
            r.total_tokens,
            COALESCE(r.cache_read_input_tokens, 0),
            COALESCE(r.cache_write_input_tokens, 0)
         FROM session_requests r
         INNER JOIN sessions s ON s.id = r.session_id
         WHERE r.estimated_cost_usd IS NULL
           AND COALESCE(
                NULLIF(TRIM(r.model), ''),
                NULLIF(TRIM(s.model_last), ''),
                NULLIF(TRIM(s.model_first), '')
           ) IS NOT NULL
           AND (
                COALESCE(r.input_tokens, 0) > 0
                OR COALESCE(r.output_tokens, 0) > 0
                OR COALESCE(r.cache_read_input_tokens, 0) > 0
                OR COALESCE(r.cache_write_input_tokens, 0) > 0
           )",
    )?;

    let rows = stmt.query_map([], map_cost_backfill_usage_row)?;
    collect_cost_backfill_usage_rows(rows)
}

fn load_token_usage_event_cost_backfill_rows(
    conn: &Connection,
) -> AppResult<Vec<CostBackfillUsageRow>> {
    let mut stmt = conn.prepare(
        "SELECT
            e.id,
            COALESCE(
                NULLIF(TRIM(e.model), ''),
                NULLIF(TRIM(s.model_last), ''),
                NULLIF(TRIM(s.model_first), '')
            ) AS model,
            COALESCE(e.delta_input, 0),
            COALESCE(e.delta_output, 0),
            e.delta_total,
            COALESCE(e.cache_read_input_tokens, 0),
            COALESCE(e.cache_write_input_tokens, 0)
         FROM token_usage_events e
         LEFT JOIN sessions s ON s.id = e.session_id
         WHERE e.estimated_cost_usd IS NULL
           AND COALESCE(
                NULLIF(TRIM(e.model), ''),
                NULLIF(TRIM(s.model_last), ''),
                NULLIF(TRIM(s.model_first), '')
           ) IS NOT NULL
           AND (
                COALESCE(e.delta_input, 0) > 0
                OR COALESCE(e.delta_output, 0) > 0
                OR COALESCE(e.cache_read_input_tokens, 0) > 0
                OR COALESCE(e.cache_write_input_tokens, 0) > 0
           )",
    )?;

    let rows = stmt.query_map([], map_cost_backfill_usage_row)?;
    collect_cost_backfill_usage_rows(rows)
}

fn map_cost_backfill_usage_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CostBackfillUsageRow> {
    Ok(CostBackfillUsageRow {
        id: row.get(0)?,
        model: row.get(1)?,
        input_tokens: row.get(2)?,
        output_tokens: row.get(3)?,
        total_tokens: row.get(4)?,
        cache_read_input_tokens: row.get(5)?,
        cache_write_input_tokens: row.get(6)?,
    })
}

fn collect_cost_backfill_usage_rows(
    rows: impl Iterator<Item = rusqlite::Result<CostBackfillUsageRow>>,
) -> AppResult<Vec<CostBackfillUsageRow>> {
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

fn query_pricing_by_exact_match(
    conn: &Connection,
    candidate: &str,
) -> AppResult<Option<ModelPricing>> {
    let pricing = conn
        .query_row(
            "SELECT
                mc.pricing_input_usd_per_mtok,
                mc.pricing_output_usd_per_mtok,
                mc.pricing_cache_read_usd_per_mtok,
                mc.pricing_cache_write_usd_per_mtok
             FROM model_aliases ma
             INNER JOIN model_catalog mc ON mc.id = ma.model_catalog_id
             WHERE LOWER(ma.alias) = LOWER(?1)
             ORDER BY CASE WHEN ma.alias = ?1 THEN 0 ELSE 1 END
             LIMIT 1",
            params![candidate],
            map_model_pricing_row,
        )
        .optional()?;

    if pricing.is_some() {
        return Ok(pricing);
    }

    conn.query_row(
        "SELECT
            pricing_input_usd_per_mtok,
            pricing_output_usd_per_mtok,
            pricing_cache_read_usd_per_mtok,
            pricing_cache_write_usd_per_mtok
         FROM model_catalog
         WHERE LOWER(canonical_key) = LOWER(?1) OR LOWER(model_id) = LOWER(?1)
         LIMIT 1",
        params![candidate],
        map_model_pricing_row,
    )
    .optional()
    .map_err(Into::into)
}

fn query_pricing_by_prefix_match(
    conn: &Connection,
    candidate: &str,
) -> AppResult<Option<ModelPricing>> {
    conn.query_row(
        "SELECT
            pricing_input_usd_per_mtok,
            pricing_output_usd_per_mtok,
            pricing_cache_read_usd_per_mtok,
            pricing_cache_write_usd_per_mtok
         FROM model_catalog
         WHERE LOWER(model_id) LIKE LOWER(?1 || '-%')
            OR LOWER(canonical_key) LIKE LOWER('%/' || ?1 || '-%')
         ORDER BY LENGTH(model_id) ASC, model_id ASC
         LIMIT 1",
        params![candidate],
        map_model_pricing_row,
    )
    .optional()
    .map_err(Into::into)
}

fn map_model_pricing_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ModelPricing> {
    Ok(ModelPricing {
        input_usd_per_mtok: sanitize_price_component(row.get(0)?),
        output_usd_per_mtok: sanitize_price_component(row.get(1)?),
        cache_read_usd_per_mtok: sanitize_price_component(row.get(2)?),
        cache_write_usd_per_mtok: sanitize_price_component(row.get(3)?),
    })
}

fn sanitize_price_component(value: Option<f64>) -> Option<f64> {
    value.filter(|price| price.is_finite() && *price > 0.0)
}

fn sanitize_model_name(model_name: Option<&str>) -> Option<String> {
    let model_name = model_name
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let lowered = model_name.to_ascii_lowercase();
    if is_non_billable_model_name(&lowered) {
        return None;
    }

    Some(model_name.to_string())
}

fn is_non_billable_model_name(model_name: &str) -> bool {
    matches!(
        model_name,
        "unknown"
            | "default"
            | "legacy"
            | "auto"
            | "agent"
            | "<synthetic>"
            | "<null>"
            | "null"
            | "none"
            | "router"
            | "openrouter/auto"
    )
}

fn build_model_lookup_candidates(model_name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();

    push_candidate(&mut candidates, &mut seen, model_name);

    if let Some((_, tail)) = model_name.split_once('/') {
        push_candidate(&mut candidates, &mut seen, tail);
    }

    if let Some((_, tail)) = model_name.split_once(':') {
        push_candidate(&mut candidates, &mut seen, tail.trim());
    }

    let normalized = normalize_candidate(model_name);
    push_candidate(&mut candidates, &mut seen, &normalized);

    if let Some((_, tail)) = normalized.split_once('/') {
        push_candidate(&mut candidates, &mut seen, tail);
    }

    if let Some(stripped) = strip_release_suffix(&normalized) {
        push_candidate(&mut candidates, &mut seen, &stripped);
    }

    for variant in build_claude_candidates(&normalized) {
        push_candidate(&mut candidates, &mut seen, &variant);
        if let Some(stripped) = strip_release_suffix(&variant) {
            push_candidate(&mut candidates, &mut seen, &stripped);
        }
    }

    candidates
}

fn push_candidate(candidates: &mut Vec<String>, seen: &mut BTreeSet<String>, value: &str) {
    let normalized = normalize_candidate(value);
    if normalized.is_empty() || !seen.insert(normalized.clone()) {
        return;
    }
    candidates.push(normalized);
}

fn normalize_candidate(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut previous_was_dash = false;

    for character in value.trim().chars() {
        let normalized_char = match character {
            'A'..='Z' => character.to_ascii_lowercase(),
            'a'..='z' | '0'..='9' | '.' | '/' => character,
            _ => '-',
        };

        if normalized_char == '-' {
            if previous_was_dash || normalized.is_empty() {
                continue;
            }
            previous_was_dash = true;
            normalized.push('-');
            continue;
        }

        previous_was_dash = false;
        normalized.push(normalized_char);
    }

    normalized.trim_matches('-').to_string()
}

fn strip_release_suffix(value: &str) -> Option<String> {
    let segments = value.split('-').collect::<Vec<_>>();
    if segments.len() < 3 {
        return None;
    }

    let last = segments.last().copied().unwrap_or_default();
    if !(is_release_segment(last) || is_two_digit_release_tail(&segments)) {
        return None;
    }

    let trimmed_len = if is_two_digit_release_tail(&segments) {
        segments.len().saturating_sub(2)
    } else {
        segments.len().saturating_sub(1)
    };
    let stripped = segments[..trimmed_len].join("-");
    (!stripped.is_empty()).then_some(stripped)
}

fn is_release_segment(segment: &str) -> bool {
    (segment.len() == 8 && segment.chars().all(|character| character.is_ascii_digit()))
        || (segment.len() == 6 && segment.chars().all(|character| character.is_ascii_digit()))
}

fn is_two_digit_release_tail(segments: &[&str]) -> bool {
    if segments.len() < 2 {
        return false;
    }

    let tail = &segments[segments.len().saturating_sub(2)..];
    tail.iter().all(|segment| {
        segment.len() == 2 && segment.chars().all(|character| character.is_ascii_digit())
    })
}

fn build_claude_candidates(value: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let parts = value.split('-').collect::<Vec<_>>();
    if parts.len() < 3 || parts.first().copied() != Some("claude") {
        return candidates;
    }

    let family = parts.get(1).copied().unwrap_or_default();
    if !matches!(family, "haiku" | "sonnet" | "opus") {
        return candidates;
    }

    let version = parts[2..].join("-");
    for normalized_version in normalize_version_variants(&version) {
        candidates.push(format!("claude-{normalized_version}-{family}"));
    }
    candidates
}

fn normalize_version_variants(version: &str) -> Vec<String> {
    let mut variants = Vec::new();
    let mut seen = BTreeSet::new();

    if seen.insert(version.to_string()) {
        variants.push(version.to_string());
    }

    if version.contains('-') {
        let dotted = version.replace('-', ".");
        if seen.insert(dotted.clone()) {
            variants.push(dotted);
        }
    }

    if version.contains('.') {
        let dashed = version.replace('.', "-");
        if seen.insert(dashed.clone()) {
            variants.push(dashed);
        }
    }

    variants
}

fn cost_component(tokens: i64, price_usd_per_mtok: Option<f64>) -> f64 {
    if tokens <= 0 {
        return 0.0;
    }

    let Some(price_usd_per_mtok) = price_usd_per_mtok else {
        return 0.0;
    };

    (tokens as f64 / 1_000_000.0) * price_usd_per_mtok
}

#[cfg(test)]
mod tests {
    use super::{
        backfill_missing_estimated_costs, build_model_lookup_candidates, estimate_usage_cost,
    };
    use crate::db::init_db_with_path;
    use chrono::Utc;
    use rusqlite::params;
    use std::collections::HashMap;

    #[test]
    fn lookup_candidates_cover_common_alias_forms() {
        let candidates = build_model_lookup_candidates("claude-opus-4-7");
        assert!(candidates.contains(&"claude-opus-4-7".to_string()));
        assert!(candidates.contains(&"claude-4.7-opus".to_string()));

        let gpt_candidates = build_model_lookup_candidates("google/gemini-3.1-flash-lite-preview");
        assert!(gpt_candidates.contains(&"google/gemini-3.1-flash-lite-preview".to_string()));
        assert!(gpt_candidates.contains(&"gemini-3.1-flash-lite-preview".to_string()));
    }

    #[test]
    fn estimate_usage_cost_ignores_non_billable_router_models() {
        let db_path = std::env::temp_dir().join(format!(
            "totoken-pricing-router-test-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let pool = init_db_with_path(&db_path).expect("init db");
        let conn = pool.get().expect("get conn");
        let mut cache = HashMap::new();

        let cost = estimate_usage_cost(&conn, &mut cache, Some("auto"), 35, 323, Some(358), 0, 0)
            .expect("estimate cost");
        assert_eq!(cost, None);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[test]
    fn estimate_usage_cost_matches_common_alias_variants() {
        let db_path = std::env::temp_dir().join(format!(
            "totoken-pricing-alias-test-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let pool = init_db_with_path(&db_path).expect("init db");
        let conn = pool.get().expect("get conn");

        insert_model(
            &conn,
            "model-gpt-54",
            "openai/gpt-5.4-20260305",
            "gpt-5.4-20260305",
            2.5,
            15.0,
            Some(0.25),
        );
        insert_model(
            &conn,
            "model-gpt-53-codex",
            "openai/gpt-5.3-codex-20260224",
            "gpt-5.3-codex-20260224",
            1.75,
            14.0,
            Some(0.175),
        );
        insert_model(
            &conn,
            "model-claude-45-sonnet",
            "anthropic/claude-4.5-sonnet-20250929",
            "claude-4.5-sonnet-20250929",
            3.0,
            15.0,
            Some(0.3),
        );
        insert_model(
            &conn,
            "model-claude-47-opus",
            "anthropic/claude-4.7-opus-20260416",
            "claude-4.7-opus-20260416",
            5.0,
            25.0,
            Some(0.5),
        );
        insert_model(
            &conn,
            "model-minimax-m27",
            "minimax/minimax-m2.7-20260318",
            "minimax-m2.7-20260318",
            0.3,
            1.2,
            Some(0.059),
        );
        insert_model(
            &conn,
            "model-glm-51",
            "z-ai/glm-5.1-20260406",
            "glm-5.1-20260406",
            1.05,
            3.5,
            Some(0.525),
        );

        let mut cache = HashMap::new();
        assert!(estimate_usage_cost(
            &conn,
            &mut cache,
            Some("gpt-5.4"),
            1_000_000,
            500_000,
            Some(1_500_000),
            0,
            0,
        )
        .expect("gpt-5.4 cost")
        .is_some());
        assert!(estimate_usage_cost(
            &conn,
            &mut cache,
            Some("gpt-5.3-codex"),
            1_000_000,
            500_000,
            Some(1_500_000),
            0,
            0,
        )
        .expect("gpt-5.3-codex cost")
        .is_some());
        assert!(estimate_usage_cost(
            &conn,
            &mut cache,
            Some("claude-sonnet-4.5"),
            1_000_000,
            500_000,
            Some(1_500_000),
            0,
            0,
        )
        .expect("claude-sonnet-4.5 cost")
        .is_some());
        assert!(estimate_usage_cost(
            &conn,
            &mut cache,
            Some("claude-opus-4-7"),
            1_000_000,
            500_000,
            Some(1_500_000),
            0,
            0,
        )
        .expect("claude-opus-4-7 cost")
        .is_some());
        assert!(estimate_usage_cost(
            &conn,
            &mut cache,
            Some("MiniMax-M2.7"),
            1_000_000,
            500_000,
            Some(1_500_000),
            0,
            0,
        )
        .expect("MiniMax-M2.7 cost")
        .is_some());
        assert!(estimate_usage_cost(
            &conn,
            &mut cache,
            Some("glm-5.1"),
            1_000_000,
            500_000,
            Some(1_500_000),
            0,
            0,
        )
        .expect("glm-5.1 cost")
        .is_some());

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[test]
    fn backfill_missing_estimated_costs_updates_stored_request_and_event_costs() {
        let db_path = std::env::temp_dir().join(format!(
            "totoken-pricing-backfill-test-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let pool = init_db_with_path(&db_path).expect("init db");
        let mut conn = pool.get().expect("get conn");

        insert_model(
            &conn,
            "model-claude-47-opus",
            "anthropic/claude-4.7-opus-20260416",
            "claude-4.7-opus-20260416",
            5.0,
            25.0,
            Some(0.5),
        );
        let now = Utc::now();
        conn.execute(
            "INSERT INTO sessions (
                id, source_app, external_session_id, session_key, title,
                model_first, model_last, source_state
             ) VALUES (
                'session-claude', 'claude_code', 'external-claude',
                'claude_code:external-claude', 'Claude test',
                'claude-opus-4-7', 'claude-opus-4-7', 'synced'
             )",
            [],
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO session_observations (
                id, session_id, input_tokens, output_tokens, total_tokens,
                conversation_checksum, message_count, source_model
             ) VALUES (
                'observation-claude', 'session-claude', 1000000, 500000,
                1500000, 'checksum-claude', 2, 'claude-opus-4-7'
             )",
            [],
        )
        .expect("insert observation");
        conn.execute(
            "INSERT INTO session_requests (
                id, session_id, observation_id, source_app, sequence_no,
                message_count, model, input_tokens, output_tokens, total_tokens,
                source_locator, estimated_cost_usd
             ) VALUES (
                'request-claude', 'session-claude', 'observation-claude',
                'claude_code', 1, 2, NULL, 1000000, 500000, 1500000,
                '{}', NULL
             )",
            [],
        )
        .expect("insert request");
        conn.execute(
            "INSERT INTO token_usage_events (
                id, session_id, observation_id, event_time_utc, delta_input,
                delta_output, delta_total, source_app, model, granularity,
                confidence, estimated_cost_usd
             ) VALUES (
                'event-claude', 'session-claude', 'observation-claude', ?1,
                200000, 100000, 300000, 'claude_code', NULL, 'request',
                'high', NULL
             )",
            params![now],
        )
        .expect("insert event");

        let summary = backfill_missing_estimated_costs(&mut conn).expect("backfill costs");
        assert_eq!(summary.session_requests_updated, 1);
        assert_eq!(summary.token_usage_events_updated, 1);

        let request_cost: f64 = conn
            .query_row(
                "SELECT estimated_cost_usd FROM session_requests WHERE id = 'request-claude'",
                [],
                |row| row.get(0),
            )
            .expect("load request cost");
        let event_cost: f64 = conn
            .query_row(
                "SELECT estimated_cost_usd FROM token_usage_events WHERE id = 'event-claude'",
                [],
                |row| row.get(0),
            )
            .expect("load event cost");

        assert!((request_cost - 17.5).abs() < 1e-9);
        assert!((event_cost - 3.5).abs() < 1e-9);

        drop(conn);
        drop(pool);
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    fn insert_model(
        conn: &rusqlite::Connection,
        id: &str,
        canonical_key: &str,
        model_id: &str,
        input_price: f64,
        output_price: f64,
        cache_read_price: Option<f64>,
    ) {
        conn.execute(
            "INSERT INTO model_catalog (
                id,
                canonical_key,
                provider,
                api_family,
                model_id,
                display_name,
                input_modalities_json,
                output_modalities_json,
                capabilities_json,
                supported_parameters_json,
                pricing_input_usd_per_mtok,
                pricing_output_usd_per_mtok,
                pricing_cache_read_usd_per_mtok,
                raw_source,
                status
             ) VALUES (
                ?1, ?2, 'test', 'test', ?3, ?3, '[]', '[]', '[]', '[]', ?4, ?5, ?6, 'test', 'active'
             )",
            params![
                id,
                canonical_key,
                model_id,
                input_price,
                output_price,
                cache_read_price
            ],
        )
        .expect("insert model");
    }
}
