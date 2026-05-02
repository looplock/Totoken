use serde::{Deserialize, Serialize};

use crate::storage::StorageConfigView;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataOverviewView {
    pub storage: StorageConfigView,
    pub summary: AppDataSummaryView,
    pub items: Vec<AppDataItemView>,
    pub default_selected_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataSummaryView {
    pub total_size_bytes: u64,
    pub file_count: u64,
    pub directory_count: u64,
    pub config_count: u64,
    pub cache_size_bytes: u64,
    pub backup_count: u64,
    pub last_backup_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataItemView {
    pub relative_path: String,
    pub name: String,
    pub full_path: String,
    pub item_type: String,
    pub category: String,
    pub health: String,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
    pub children: Vec<AppDataItemView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataItemDetailView {
    pub relative_path: String,
    pub name: String,
    pub full_path: String,
    pub item_type: String,
    pub category: String,
    pub health: String,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
    pub entry_count: Option<u64>,
    pub preview: Option<String>,
    pub preview_language: Option<String>,
    pub sqlite: Option<AppDataSqliteInfoView>,
    pub recommended_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataSqliteInfoView {
    pub table_count: u64,
    pub index_count: u64,
    pub page_count: u64,
    pub freelist_count: u64,
    pub page_size_bytes: u64,
    pub integrity: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataActionOutcomeView {
    pub overview: AppDataOverviewView,
    pub reclaimed_bytes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppDataMaintenanceAction {
    CreateBackup,
    VacuumDatabase,
    RebuildIndexes,
    ClearCaches,
}
