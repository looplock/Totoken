use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use chrono::{DateTime, TimeDelta, Utc};
use reqwest::Client;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::DbPool;
use crate::error::{AppError, AppResult};
use crate::pricing::{backfill_missing_estimated_costs, CostEstimationPolicy};
use crate::utils::{ids::new_uuid, time::now_utc};

const OPENROUTER_MODELS_URL: &str = "https://openrouter.ai/api/v1/models?output_modalities=all";
const OPENROUTER_SOURCE: &str = "openrouter";
const DEFAULT_SORT_BY: &str = "name";
static OPENROUTER_CLIENT: OnceLock<Client> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogListQuery {
    pub q: Option<String>,
    pub provider: Option<String>,
    pub capability: Option<String>,
    pub status: Option<String>,
    pub context_tier: Option<String>,
    pub pricing_tier: Option<String>,
    pub sort_by: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogListResponse {
    pub items: Vec<ModelCatalogListItem>,
    pub total_items: usize,
    pub sync_status: ModelCatalogSyncStatusView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogListItem {
    pub id: String,
    pub canonical_key: String,
    pub provider: String,
    pub api_family: String,
    pub model_id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub context_window: Option<i64>,
    pub max_output_tokens: Option<i64>,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub capabilities: Vec<String>,
    pub supported_parameters: Vec<String>,
    pub pricing_input_usd_per_mtok: Option<f64>,
    pub pricing_output_usd_per_mtok: Option<f64>,
    pub pricing_cache_read_usd_per_mtok: Option<f64>,
    pub pricing_cache_write_usd_per_mtok: Option<f64>,
    pub docs_url: Option<String>,
    pub status: String,
    pub raw_source: String,
    pub token_usage_total: i64,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub context_tier: String,
    pub pricing_tier: String,
    pub last_synced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogSyncStatusView {
    pub total_models: i64,
    pub latest_successful_sync_at: Option<DateTime<Utc>>,
    pub latest_run: Option<ModelCatalogSyncRunView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogSyncRunView {
    pub id: String,
    pub source: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: String,
    pub models_seen: i64,
    pub models_inserted: i64,
    pub models_updated: i64,
    pub error_count: i64,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
struct ModelCatalogRecord {
    id: String,
    canonical_key: String,
    provider: String,
    api_family: String,
    model_id: String,
    display_name: String,
    description: Option<String>,
    context_window: Option<i64>,
    max_output_tokens: Option<i64>,
    input_modalities_json: String,
    output_modalities_json: String,
    capabilities_json: String,
    supported_parameters_json: String,
    pricing_input_usd_per_mtok: Option<f64>,
    pricing_output_usd_per_mtok: Option<f64>,
    pricing_cache_read_usd_per_mtok: Option<f64>,
    pricing_cache_write_usd_per_mtok: Option<f64>,
    docs_url: Option<String>,
    status: String,
    raw_source: String,
    source_payload_json: String,
    last_synced_at: DateTime<Utc>,
    last_verified_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelsResponse {
    #[serde(default)]
    data: Vec<OpenRouterModel>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModel {
    id: String,
    canonical_slug: Option<String>,
    name: Option<String>,
    created: Option<i64>,
    description: Option<String>,
    context_length: Option<i64>,
    architecture: Option<OpenRouterArchitecture>,
    pricing: Option<OpenRouterPricing>,
    top_provider: Option<OpenRouterTopProvider>,
    links: Option<OpenRouterLinks>,
    #[serde(default)]
    supported_parameters: Vec<String>,
    expiration_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterArchitecture {
    #[serde(default)]
    input_modalities: Vec<String>,
    #[serde(default)]
    output_modalities: Vec<String>,
    modality: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterPricing {
    prompt: Option<Value>,
    completion: Option<Value>,
    input_cache_read: Option<Value>,
    input_cache_write: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterTopProvider {
    context_length: Option<i64>,
    max_completion_tokens: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterLinks {
    details: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct SyncCounters {
    models_seen: i64,
    models_inserted: i64,
    models_updated: i64,
}

pub fn list_models(
    pool: DbPool,
    query: Option<ModelCatalogListQuery>,
) -> AppResult<ModelCatalogListResponse> {
    let normalized_query = normalize_list_query(query);
    let conn = pool.get()?;

    let usage_by_model = load_usage_by_model(&conn)?;

    let mut stmt = conn.prepare(
        "SELECT
            id,
            canonical_key,
            provider,
            api_family,
            model_id,
            display_name,
            description,
            context_window,
            max_output_tokens,
            input_modalities_json,
            output_modalities_json,
            capabilities_json,
            supported_parameters_json,
            pricing_input_usd_per_mtok,
            pricing_output_usd_per_mtok,
            pricing_cache_read_usd_per_mtok,
            pricing_cache_write_usd_per_mtok,
            docs_url,
            status,
            raw_source,
            last_synced_at
         FROM model_catalog",
    )?;

    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let canonical_key: String = row.get(1)?;
        let model_id: String = row.get(4)?;
        let input_modalities = parse_json_vec(row.get::<_, String>(9)?);
        let output_modalities = parse_json_vec(row.get::<_, String>(10)?);
        let capabilities = parse_json_vec(row.get::<_, String>(11)?);
        let supported_parameters = parse_json_vec(row.get::<_, String>(12)?);
        let usage = usage_for_model(&usage_by_model, &canonical_key, &model_id);

        let pricing_input_usd_per_mtok = normalize_catalog_price(row.get(13)?);
        let pricing_output_usd_per_mtok = normalize_catalog_price(row.get(14)?);
        let pricing_cache_read_usd_per_mtok = normalize_catalog_price(row.get(15)?);
        let pricing_cache_write_usd_per_mtok = normalize_catalog_price(row.get(16)?);

        Ok(ModelCatalogListItem {
            id: id.clone(),
            canonical_key,
            provider: row.get(2)?,
            api_family: row.get(3)?,
            model_id,
            display_name: row.get(5)?,
            description: row.get(6)?,
            context_window: row.get(7)?,
            max_output_tokens: row.get(8)?,
            input_modalities,
            output_modalities,
            capabilities,
            supported_parameters,
            pricing_input_usd_per_mtok,
            pricing_output_usd_per_mtok,
            pricing_cache_read_usd_per_mtok,
            pricing_cache_write_usd_per_mtok,
            docs_url: row.get(17)?,
            status: row.get(18)?,
            raw_source: row.get(19)?,
            token_usage_total: usage.0,
            last_seen_at: usage.1,
            context_tier: context_tier_label(row.get(7)?),
            pricing_tier: pricing_tier_label(
                pricing_input_usd_per_mtok,
                pricing_output_usd_per_mtok,
            ),
            last_synced_at: row.get(20)?,
        })
    })?;

    let mut items = Vec::new();
    for row in rows {
        let item = row?;
        if matches_query(&item, &normalized_query) {
            items.push(item);
        }
    }

    sort_model_items(
        &mut items,
        normalized_query
            .sort_by
            .as_deref()
            .unwrap_or(DEFAULT_SORT_BY),
    );
    let sync_status = get_model_sync_status(pool)?;

    Ok(ModelCatalogListResponse {
        total_items: items.len(),
        items,
        sync_status,
    })
}

pub fn get_model_sync_status(pool: DbPool) -> AppResult<ModelCatalogSyncStatusView> {
    let conn = pool.get()?;
    let total_models =
        conn.query_row("SELECT COUNT(*) FROM model_catalog", [], |row| row.get(0))?;
    let latest_successful_sync_at = conn
        .query_row(
            "SELECT ended_at
         FROM model_sync_runs
         WHERE status = 'success'
         ORDER BY started_at DESC
         LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let latest_run = load_sync_run_view(&conn, None)?;

    Ok(ModelCatalogSyncStatusView {
        total_models,
        latest_successful_sync_at,
        latest_run,
    })
}

pub async fn refresh_model_catalog(
    pool: DbPool,
    cost_estimation_policy: CostEstimationPolicy,
) -> AppResult<ModelCatalogSyncRunView> {
    let run_id = new_uuid();
    let started_at = now_utc();

    {
        let conn = pool.get()?;
        conn.execute(
            "INSERT INTO model_sync_runs (
                id,
                source,
                started_at,
                status,
                models_seen,
                models_inserted,
                models_updated,
                error_count
             ) VALUES (?1, ?2, ?3, 'running', 0, 0, 0, 0)",
            params![&run_id, OPENROUTER_SOURCE, started_at],
        )?;
    }

    let fetched_models = fetch_openrouter_models().await.inspect_err(|error| {
        let _ = mark_sync_run_failed(&pool, &run_id, &error.message());
    })?;

    persist_openrouter_models(&pool, &run_id, fetched_models).inspect_err(|error| {
        let _ = mark_sync_run_failed(&pool, &run_id, &error.message());
    })?;

    {
        let mut conn = pool.get()?;
        let backfill_summary = backfill_missing_estimated_costs(&mut conn, cost_estimation_policy)?;
        log::info!(
            "Model catalog refresh backfilled estimated costs: session_requests_updated={}, token_usage_events_updated={}",
            backfill_summary.session_requests_updated,
            backfill_summary.token_usage_events_updated
        );
    }

    let conn = pool.get()?;
    load_sync_run_view(&conn, Some(&run_id))?
        .ok_or_else(|| AppError::internal("model sync run was not persisted"))
}

async fn fetch_openrouter_models() -> AppResult<Vec<OpenRouterModel>> {
    let response = openrouter_client()
        .get(OPENROUTER_MODELS_URL)
        .header("Accept", "application/json")
        .send()
        .await?
        .error_for_status()?;

    let payload: OpenRouterModelsResponse = response.json().await?;
    Ok(payload.data)
}

fn openrouter_client() -> &'static Client {
    OPENROUTER_CLIENT.get_or_init(Client::new)
}

fn persist_openrouter_models(
    pool: &DbPool,
    run_id: &str,
    models: Vec<OpenRouterModel>,
) -> AppResult<SyncCounters> {
    let mut conn = pool.get()?;
    let tx = conn.transaction()?;
    let mut counters = SyncCounters {
        models_seen: models.len() as i64,
        ..SyncCounters::default()
    };

    for model in models {
        let record = map_openrouter_model(model)?;
        let existing_id = tx
            .query_row(
                "SELECT id FROM model_catalog WHERE canonical_key = ?1",
                params![&record.canonical_key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        if let Some(model_catalog_id) = existing_id {
            tx.execute(
                "UPDATE model_catalog
                 SET provider = ?2,
                     api_family = ?3,
                     model_id = ?4,
                     display_name = ?5,
                     description = ?6,
                     context_window = ?7,
                     max_output_tokens = ?8,
                     input_modalities_json = ?9,
                     output_modalities_json = ?10,
                     capabilities_json = ?11,
                     supported_parameters_json = ?12,
                     pricing_input_usd_per_mtok = ?13,
                     pricing_output_usd_per_mtok = ?14,
                     pricing_cache_read_usd_per_mtok = ?15,
                     pricing_cache_write_usd_per_mtok = ?16,
                     docs_url = ?17,
                     status = ?18,
                     raw_source = ?19,
                     source_payload_json = ?20,
                     last_synced_at = ?21,
                     last_verified_at = ?22,
                     updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?1",
                params![
                    &model_catalog_id,
                    &record.provider,
                    &record.api_family,
                    &record.model_id,
                    &record.display_name,
                    record.description.as_deref(),
                    record.context_window,
                    record.max_output_tokens,
                    &record.input_modalities_json,
                    &record.output_modalities_json,
                    &record.capabilities_json,
                    &record.supported_parameters_json,
                    record.pricing_input_usd_per_mtok,
                    record.pricing_output_usd_per_mtok,
                    record.pricing_cache_read_usd_per_mtok,
                    record.pricing_cache_write_usd_per_mtok,
                    record.docs_url.as_deref(),
                    &record.status,
                    &record.raw_source,
                    &record.source_payload_json,
                    &record.last_synced_at,
                    &record.last_verified_at,
                ],
            )?;
            replace_aliases(&tx, &model_catalog_id, &record)?;
            counters.models_updated += 1;
        } else {
            tx.execute(
                "INSERT INTO model_catalog (
                    id,
                    canonical_key,
                    provider,
                    api_family,
                    model_id,
                    display_name,
                    description,
                    context_window,
                    max_output_tokens,
                    input_modalities_json,
                    output_modalities_json,
                    capabilities_json,
                    supported_parameters_json,
                    pricing_input_usd_per_mtok,
                    pricing_output_usd_per_mtok,
                    pricing_cache_read_usd_per_mtok,
                    pricing_cache_write_usd_per_mtok,
                    docs_url,
                    status,
                    raw_source,
                    source_payload_json,
                    last_synced_at,
                    last_verified_at
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23
                 )",
                params![
                    &record.id,
                    &record.canonical_key,
                    &record.provider,
                    &record.api_family,
                    &record.model_id,
                    &record.display_name,
                    record.description.as_deref(),
                    record.context_window,
                    record.max_output_tokens,
                    &record.input_modalities_json,
                    &record.output_modalities_json,
                    &record.capabilities_json,
                    &record.supported_parameters_json,
                    record.pricing_input_usd_per_mtok,
                    record.pricing_output_usd_per_mtok,
                    record.pricing_cache_read_usd_per_mtok,
                    record.pricing_cache_write_usd_per_mtok,
                    record.docs_url.as_deref(),
                    &record.status,
                    &record.raw_source,
                    &record.source_payload_json,
                    &record.last_synced_at,
                    &record.last_verified_at,
                ],
            )?;
            replace_aliases(&tx, &record.id, &record)?;
            counters.models_inserted += 1;
        }
    }

    tx.execute(
        "UPDATE model_sync_runs
         SET models_seen = ?2,
             models_inserted = ?3,
             models_updated = ?4,
             ended_at = ?5,
             status = 'success',
             error_count = 0,
             error_message = NULL
         WHERE id = ?1",
        params![
            run_id,
            counters.models_seen,
            counters.models_inserted,
            counters.models_updated,
            now_utc()
        ],
    )?;

    tx.commit()?;
    Ok(counters)
}

fn replace_aliases(
    conn: &rusqlite::Transaction<'_>,
    model_catalog_id: &str,
    record: &ModelCatalogRecord,
) -> AppResult<()> {
    conn.execute(
        "DELETE FROM model_aliases WHERE model_catalog_id = ?1 AND source = ?2",
        params![model_catalog_id, OPENROUTER_SOURCE],
    )?;

    let aliases = build_aliases(record);
    for alias in aliases {
        conn.execute(
            "INSERT INTO model_aliases (alias, model_catalog_id, provider, source)
             VALUES (?1, ?2, ?3, ?4)",
            params![alias, model_catalog_id, record.provider, OPENROUTER_SOURCE],
        )?;
    }

    Ok(())
}

fn build_aliases(record: &ModelCatalogRecord) -> Vec<String> {
    let mut aliases = HashSet::new();
    aliases.insert(record.canonical_key.clone());
    aliases.insert(record.model_id.clone());
    aliases.insert(record.model_id.to_lowercase());
    aliases.insert(record.display_name.clone());
    aliases.insert(record.display_name.to_lowercase());

    aliases.into_iter().collect()
}

fn load_sync_run_view(
    conn: &rusqlite::Connection,
    run_id: Option<&str>,
) -> AppResult<Option<ModelCatalogSyncRunView>> {
    let sql = if run_id.is_some() {
        "SELECT
            id,
            source,
            started_at,
            ended_at,
            status,
            models_seen,
            models_inserted,
            models_updated,
            error_count,
            error_message
         FROM model_sync_runs
         WHERE id = ?1
         LIMIT 1"
    } else {
        "SELECT
            id,
            source,
            started_at,
            ended_at,
            status,
            models_seen,
            models_inserted,
            models_updated,
            error_count,
            error_message
         FROM model_sync_runs
         ORDER BY started_at DESC
         LIMIT 1"
    };

    let mut stmt = conn.prepare(sql)?;
    let result = if let Some(run_id) = run_id {
        stmt.query_row(params![run_id], |row| {
            Ok(ModelCatalogSyncRunView {
                id: row.get(0)?,
                source: row.get(1)?,
                started_at: row.get(2)?,
                ended_at: row.get(3)?,
                status: row.get(4)?,
                models_seen: row.get(5)?,
                models_inserted: row.get(6)?,
                models_updated: row.get(7)?,
                error_count: row.get(8)?,
                error_message: row.get(9)?,
            })
        })
        .optional()?
    } else {
        stmt.query_row([], |row| {
            Ok(ModelCatalogSyncRunView {
                id: row.get(0)?,
                source: row.get(1)?,
                started_at: row.get(2)?,
                ended_at: row.get(3)?,
                status: row.get(4)?,
                models_seen: row.get(5)?,
                models_inserted: row.get(6)?,
                models_updated: row.get(7)?,
                error_count: row.get(8)?,
                error_message: row.get(9)?,
            })
        })
        .optional()?
    };

    Ok(result)
}

fn mark_sync_run_failed(pool: &DbPool, run_id: &str, error_message: &str) -> AppResult<()> {
    let conn = pool.get()?;
    conn.execute(
        "UPDATE model_sync_runs
         SET ended_at = ?2,
             status = 'failed',
             error_count = 1,
             error_message = ?3
         WHERE id = ?1",
        params![run_id, now_utc(), error_message],
    )?;
    Ok(())
}

fn map_openrouter_model(model: OpenRouterModel) -> AppResult<ModelCatalogRecord> {
    let now = now_utc();
    let canonical_key = model
        .canonical_slug
        .clone()
        .unwrap_or_else(|| model.id.clone());
    let provider = canonical_key
        .split_once('/')
        .map(|(provider, _)| provider.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let model_id = canonical_key
        .split_once('/')
        .map(|(_, value)| value.to_string())
        .unwrap_or_else(|| model.id.clone());
    let context_window = model.context_length.or_else(|| {
        model
            .top_provider
            .as_ref()
            .and_then(|provider| provider.context_length)
    });
    let max_output_tokens = model
        .top_provider
        .as_ref()
        .and_then(|provider| provider.max_completion_tokens);
    let input_modalities = model
        .architecture
        .as_ref()
        .map(|architecture| architecture.input_modalities.clone())
        .unwrap_or_default();
    let output_modalities = model
        .architecture
        .as_ref()
        .map(|architecture| architecture.output_modalities.clone())
        .unwrap_or_default();
    let capabilities = derive_capabilities(
        &model_id,
        model.name.as_deref(),
        context_window,
        model.architecture.as_ref(),
        &model.supported_parameters,
    );
    let display_name = model
        .name
        .clone()
        .unwrap_or_else(|| "Unnamed model".to_string());
    let description = model.description.clone().map(normalize_optional_string);
    let pricing_input_usd_per_mtok = model
        .pricing
        .as_ref()
        .and_then(|pricing| value_to_usd_per_mtok(pricing.prompt.as_ref()));
    let pricing_output_usd_per_mtok = model
        .pricing
        .as_ref()
        .and_then(|pricing| value_to_usd_per_mtok(pricing.completion.as_ref()));
    let pricing_cache_read_usd_per_mtok = model
        .pricing
        .as_ref()
        .and_then(|pricing| value_to_usd_per_mtok(pricing.input_cache_read.as_ref()));
    let pricing_cache_write_usd_per_mtok = model
        .pricing
        .as_ref()
        .and_then(|pricing| value_to_usd_per_mtok(pricing.input_cache_write.as_ref()));
    let docs_url = model
        .links
        .as_ref()
        .and_then(|links| links.details.as_deref())
        .map(resolve_openrouter_link)
        .or_else(|| Some(format!("https://openrouter.ai/{}", canonical_key)));
    let status = derive_status(model.created, model.expiration_date.as_deref(), &model_id);
    let source_payload_json = serde_json::to_string(&serde_json::json!({
        "id": &model.id,
        "canonical_slug": &model.canonical_slug,
        "name": &model.name,
        "created": model.created,
        "description": &model.description,
        "context_length": model.context_length,
        "architecture": model.architecture.as_ref().map(|architecture| serde_json::json!({
            "input_modalities": &architecture.input_modalities,
            "output_modalities": &architecture.output_modalities,
            "modality": &architecture.modality,
        })),
        "pricing": model.pricing.as_ref().map(|pricing| serde_json::json!({
            "prompt": &pricing.prompt,
            "completion": &pricing.completion,
            "input_cache_read": &pricing.input_cache_read,
            "input_cache_write": &pricing.input_cache_write,
        })),
        "supported_parameters": &model.supported_parameters,
        "expiration_date": &model.expiration_date,
    }))?;

    Ok(ModelCatalogRecord {
        id: new_uuid(),
        canonical_key,
        provider: provider.clone(),
        api_family: provider,
        model_id,
        display_name,
        description,
        context_window,
        max_output_tokens,
        input_modalities_json: serde_json::to_string(&input_modalities)?,
        output_modalities_json: serde_json::to_string(&output_modalities)?,
        capabilities_json: serde_json::to_string(&capabilities)?,
        supported_parameters_json: serde_json::to_string(&model.supported_parameters)?,
        pricing_input_usd_per_mtok,
        pricing_output_usd_per_mtok,
        pricing_cache_read_usd_per_mtok,
        pricing_cache_write_usd_per_mtok,
        docs_url,
        status,
        raw_source: OPENROUTER_SOURCE.to_string(),
        source_payload_json,
        last_synced_at: now,
        last_verified_at: now,
    })
}

fn normalize_list_query(query: Option<ModelCatalogListQuery>) -> ModelCatalogListQuery {
    let query = query.unwrap_or(ModelCatalogListQuery {
        q: None,
        provider: None,
        capability: None,
        status: None,
        context_tier: None,
        pricing_tier: None,
        sort_by: None,
    });

    ModelCatalogListQuery {
        q: query
            .q
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty()),
        provider: query
            .provider
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty() && value != "all"),
        capability: query
            .capability
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty() && value != "all"),
        status: query
            .status
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty() && value != "all"),
        context_tier: query
            .context_tier
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty() && value != "all"),
        pricing_tier: query
            .pricing_tier
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty() && value != "all"),
        sort_by: query
            .sort_by
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty()),
    }
}

fn matches_query(item: &ModelCatalogListItem, query: &ModelCatalogListQuery) -> bool {
    if let Some(q) = query.q.as_deref() {
        let haystack = format!(
            "{} {} {} {}",
            item.display_name, item.provider, item.model_id, item.canonical_key
        )
        .to_lowercase();
        if !haystack.contains(q)
            && !item
                .capabilities
                .iter()
                .any(|capability| capability.to_lowercase().contains(q))
        {
            return false;
        }
    }

    if let Some(provider) = query.provider.as_deref() {
        if item.provider.to_lowercase() != provider {
            return false;
        }
    }

    if let Some(capability) = query.capability.as_deref() {
        if !item
            .capabilities
            .iter()
            .any(|value| value.eq_ignore_ascii_case(capability))
        {
            return false;
        }
    }

    if let Some(status) = query.status.as_deref() {
        if !item.status.eq_ignore_ascii_case(status) {
            return false;
        }
    }

    if let Some(context_tier) = query.context_tier.as_deref() {
        if item.context_tier != context_tier {
            return false;
        }
    }

    if let Some(pricing_tier) = query.pricing_tier.as_deref() {
        if item.pricing_tier != pricing_tier {
            return false;
        }
    }

    true
}

fn sort_model_items(items: &mut [ModelCatalogListItem], sort_by: &str) {
    items.sort_by(|left, right| match sort_by {
        "provider" => left
            .provider
            .cmp(&right.provider)
            .then(left.display_name.cmp(&right.display_name)),
        "recent" => right
            .last_seen_at
            .cmp(&left.last_seen_at)
            .then(left.display_name.cmp(&right.display_name)),
        "usage" => right
            .token_usage_total
            .cmp(&left.token_usage_total)
            .then(left.display_name.cmp(&right.display_name)),
        "price" => average_price(left)
            .partial_cmp(&average_price(right))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.display_name.cmp(&right.display_name)),
        _ => left.display_name.cmp(&right.display_name),
    });
}

fn average_price(item: &ModelCatalogListItem) -> f64 {
    let input = item.pricing_input_usd_per_mtok.unwrap_or_default();
    let output = item.pricing_output_usd_per_mtok.unwrap_or_default();
    (input + output) / 2.0
}

type ModelUsageByModel = HashMap<String, (i64, Option<DateTime<Utc>>)>;

fn load_usage_by_model(conn: &rusqlite::Connection) -> AppResult<ModelUsageByModel> {
    let mut stmt = conn.prepare(
        "SELECT
            COALESCE(NULLIF(s.model_last, ''), NULLIF(s.model_first, '')) AS model_name,
            COALESCE(SUM(st.total_tokens_max), 0) AS token_usage_total,
            MAX(s.discovered_last_at) AS last_seen_at
         FROM sessions s
         LEFT JOIN session_token_totals st ON st.session_id = s.id
         WHERE COALESCE(NULLIF(s.model_last, ''), NULLIF(s.model_first, '')) IS NOT NULL
           AND s.source_app IN ('claude_code', 'codex', 'cursor', 'opencode', 'kilocode', 'kiro')
         GROUP BY COALESCE(NULLIF(s.model_last, ''), NULLIF(s.model_first, ''))",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?.to_lowercase(),
            (
                row.get::<_, i64>(1)?,
                row.get::<_, Option<DateTime<Utc>>>(2)?,
            ),
        ))
    })?;

    let mut usage_by_model = HashMap::new();
    for row in rows {
        let (key, value) = row?;
        usage_by_model.insert(key, value);
    }

    Ok(usage_by_model)
}

fn usage_for_model(
    usage_by_model: &HashMap<String, (i64, Option<DateTime<Utc>>)>,
    canonical_key: &str,
    model_id: &str,
) -> (i64, Option<DateTime<Utc>>) {
    usage_by_model
        .get(&canonical_key.to_lowercase())
        .cloned()
        .or_else(|| usage_by_model.get(&model_id.to_lowercase()).cloned())
        .unwrap_or((0, None))
}

fn parse_json_vec(raw_json: String) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&raw_json).unwrap_or_default()
}

fn context_tier_label(context_window: Option<i64>) -> String {
    match context_window.unwrap_or_default() {
        value if value <= 64_000 => "compact".to_string(),
        value if value <= 128_000 => "standard".to_string(),
        value if value <= 200_000 => "extended".to_string(),
        _ => "ultra".to_string(),
    }
}

fn pricing_tier_label(input_price: Option<f64>, output_price: Option<f64>) -> String {
    let price = (input_price.unwrap_or_default() + output_price.unwrap_or_default()) / 2.0;
    if price <= 1.0 {
        "economy".to_string()
    } else if price <= 10.0 {
        "balanced".to_string()
    } else {
        "premium".to_string()
    }
}

fn derive_capabilities(
    model_id: &str,
    display_name: Option<&str>,
    context_window: Option<i64>,
    architecture: Option<&OpenRouterArchitecture>,
    supported_parameters: &[String],
) -> Vec<String> {
    let combined = format!(
        "{} {} {}",
        model_id,
        display_name.unwrap_or_default(),
        architecture
            .and_then(|item| item.modality.as_deref())
            .unwrap_or_default()
    )
    .to_lowercase();
    let parameters = supported_parameters
        .iter()
        .map(|value| value.to_lowercase())
        .collect::<HashSet<_>>();
    let input_modalities = architecture
        .map(|item| {
            item.input_modalities
                .iter()
                .map(|value| value.to_lowercase())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    let mut capabilities = Vec::new();

    if combined.contains("code") || combined.contains("coder") {
        capabilities.push("coding".to_string());
    }

    if combined.contains("reason")
        || combined.contains("think")
        || combined.contains("/o1")
        || combined.contains("/o3")
        || combined.contains("/o4")
        || parameters.contains("reasoning")
        || parameters.contains("include_reasoning")
    {
        capabilities.push("reasoning".to_string());
    }

    if parameters.contains("tools") || parameters.contains("tool_choice") {
        capabilities.push("tool_use".to_string());
    }

    if input_modalities.contains("image")
        || input_modalities.contains("file")
        || input_modalities.contains("audio")
    {
        capabilities.push("vision".to_string());
    }

    if context_window.unwrap_or_default() >= 200_000 {
        capabilities.push("long_context".to_string());
    }

    capabilities.sort();
    capabilities.dedup();
    capabilities
}

fn derive_status(
    created_unix: Option<i64>,
    expiration_date: Option<&str>,
    model_id: &str,
) -> String {
    let model_id = model_id.to_lowercase();
    if expiration_date.is_some() {
        return "deprecated".to_string();
    }
    if model_id.contains("experimental") || model_id.contains("exp") {
        return "experimental".to_string();
    }
    if model_id.contains("preview") || model_id.contains("beta") || model_id.contains("alpha") {
        return "preview".to_string();
    }
    if let Some(created_unix) = created_unix {
        if let Some(created_at) = DateTime::<Utc>::from_timestamp(created_unix, 0) {
            if now_utc() - created_at <= TimeDelta::days(45) {
                return "new".to_string();
            }
        }
    }
    "active".to_string()
}

fn value_to_usd_per_mtok(value: Option<&Value>) -> Option<f64> {
    let per_token = match value {
        Some(Value::String(raw)) => raw.parse::<f64>().ok(),
        Some(Value::Number(raw)) => raw.as_f64(),
        _ => None,
    }?;

    if !per_token.is_finite() || per_token < 0.0 {
        return None;
    }

    Some(per_token * 1_000_000.0)
}

fn normalize_catalog_price(value: Option<f64>) -> Option<f64> {
    value.filter(|price| price.is_finite() && *price >= 0.0)
}

fn resolve_openrouter_link(link: &str) -> String {
    if link.starts_with("http://") || link.starts_with("https://") {
        link.to_string()
    } else {
        format!("https://openrouter.ai{link}")
    }
}

fn normalize_optional_string(value: String) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        value
    } else {
        trimmed.to_string()
    }
}
