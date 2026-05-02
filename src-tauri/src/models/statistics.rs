use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsQuery {
    pub q: Option<String>,
    pub source_app: Option<String>,
    pub model: Option<String>,
    pub period: Option<String>,
    pub granularity: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsMetricValue {
    pub value: i64,
    pub delta_percent: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsSummary {
    pub total_tokens: StatisticsMetricValue,
    pub input_tokens: StatisticsMetricValue,
    pub output_tokens: StatisticsMetricValue,
    pub total_sessions: StatisticsMetricValue,
    pub active_models: StatisticsMetricValue,
    pub avg_tokens_per_session: StatisticsMetricValue,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsTrend {
    pub bucket_starts: Vec<DateTime<Utc>>,
    pub input: Vec<i64>,
    pub output: Vec<i64>,
    pub total: Vec<i64>,
    pub cache_read_input: Vec<i64>,
    pub cache_write_input: Vec<i64>,
    pub cost_usd: Vec<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsActivityMetric {
    pub matrix: Vec<Vec<f64>>,
    pub max_value: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsActivity {
    pub sessions: StatisticsActivityMetric,
    pub tokens: StatisticsActivityMetric,
    pub cost: StatisticsActivityMetric,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsDetailRow {
    pub id: String,
    pub app: String,
    pub model: String,
    pub sessions: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost_usd: f64,
    pub avg_tokens_per_session: i64,
    pub last_active_at: Option<DateTime<Utc>>,
    pub trend_percent: f64,
    pub trend_direction: String,
    pub sparkline: Vec<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsDistributionRow {
    pub app: String,
    pub sessions: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsRange {
    pub period: String,
    pub granularity: String,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsOverview {
    pub summary: StatisticsSummary,
    pub trend: StatisticsTrend,
    pub activity: StatisticsActivity,
    pub distribution: Vec<StatisticsDistributionRow>,
    pub detail_rows: Vec<StatisticsDetailRow>,
    pub available_models: Vec<String>,
    pub range: StatisticsRange,
}
