use std::fs;
use std::path::PathBuf;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::scanner::scheduler::{self, SchedulerPreviewView, AUTO_SCAN_SOURCE_APP};
use crate::source_settings;
use crate::state::{AppState, AutoScanStatusView};
use crate::storage;

const CONFIG_DIR_NAME: &str = "config";
const SETTINGS_FILE_NAME: &str = "settings.json";
const DEFAULT_THEME: &str = "blue-light";
const SCAN_MODE_AUTO: &str = "auto";
const SCAN_MODE_MANUAL: &str = "manual";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerSettings {
    #[serde(default = "default_scan_mode")]
    pub scan_mode: String,
    pub base_interval: u64,
    pub min_interval: u64,
    pub max_interval: u64,
    pub adaptive_scanning: bool,
    pub ewma_alpha: f64,
    pub change_rate_threshold: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPreferencesSettings {
    pub theme: String,
    pub language: String,
    pub notifications: bool,
    #[serde(default = "default_true")]
    pub localized_token_units: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsState {
    pub scheduler: SchedulerSettings,
    pub ui_preferences: UiPreferencesSettings,
}

pub fn get_settings(app: &AppHandle) -> AppResult<SettingsState> {
    let settings_path = resolve_settings_path(app)?;

    if !settings_path.exists() {
        let defaults = default_settings();
        write_settings_file(&settings_path, &defaults)?;
        return Ok(defaults);
    }

    let content = fs::read_to_string(&settings_path)?;
    if content.trim().is_empty() {
        let defaults = default_settings();
        write_settings_file(&settings_path, &defaults)?;
        return Ok(defaults);
    }

    let mut settings: SettingsState = serde_json::from_str(&content)?;
    let normalized_scan_mode = normalize_scan_mode(&settings.scheduler.scan_mode);
    let scan_mode_changed = settings.scheduler.scan_mode != normalized_scan_mode;
    settings.scheduler.scan_mode = normalized_scan_mode;
    let normalized_theme = normalize_theme(&settings.ui_preferences.theme);
    let theme_changed = settings.ui_preferences.theme != normalized_theme;
    settings.ui_preferences.theme = normalized_theme;
    validate_settings(&settings)?;

    if scan_mode_changed || theme_changed {
        write_settings_file(&settings_path, &settings)?;
    }

    Ok(settings)
}

pub fn update_settings(app: &AppHandle, mut settings: SettingsState) -> AppResult<SettingsState> {
    settings.scheduler.scan_mode = normalize_scan_mode(&settings.scheduler.scan_mode);
    settings.ui_preferences.theme = normalize_theme(&settings.ui_preferences.theme);
    validate_settings(&settings)?;
    let settings_path = resolve_settings_path(app)?;
    write_settings_file(&settings_path, &settings)?;
    Ok(settings)
}

pub fn reset_settings(app: &AppHandle) -> AppResult<SettingsState> {
    let defaults = default_settings();
    let settings_path = resolve_settings_path(app)?;
    write_settings_file(&settings_path, &defaults)?;
    Ok(defaults)
}

pub fn get_auto_scan_status(app: &AppHandle, state: &AppState) -> AppResult<AutoScanStatusView> {
    let enabled_sources =
        source_settings::list_enabled_scannable_sources(app, &state.source_settings_store())?;
    let root_path = enabled_sources
        .iter()
        .map(|source| format!("{}: {}", source.app, source.path.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" | ");
    Ok(state.auto_scan_status_view(AUTO_SCAN_SOURCE_APP, root_path))
}

pub fn preview_scheduler(
    state: &AppState,
    scheduler_settings: SchedulerSettings,
) -> AppResult<SchedulerPreviewView> {
    validate_scheduler_settings(&scheduler_settings)?;
    let raw_history = match load_scheduler_change_rate_history_percent(state) {
        Ok(history) => history,
        Err(_) => state.raw_change_rate_history_percent(),
    };
    Ok(scheduler::build_scheduler_preview(
        &scheduler_settings,
        &raw_history,
    ))
}

fn resolve_settings_path(app: &AppHandle) -> AppResult<PathBuf> {
    let storage_paths = storage::resolve_storage_paths(app)?;
    let config_dir = storage_paths.data_dir().join(CONFIG_DIR_NAME);
    fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join(SETTINGS_FILE_NAME))
}

fn write_settings_file(settings_path: &PathBuf, settings: &SettingsState) -> AppResult<()> {
    let content = serde_json::to_string_pretty(settings)?;
    fs::write(settings_path, content)?;
    Ok(())
}

fn normalize_theme(theme: &str) -> String {
    match theme {
        "bright" => "blue-light".to_string(),
        "dark" => "blue-dark".to_string(),
        "blue-light" | "blue-dark" | "green-light" | "green-dark" | "amber-light"
        | "amber-dark" => theme.to_string(),
        _ => DEFAULT_THEME.to_string(),
    }
}

fn validate_settings(settings: &SettingsState) -> AppResult<()> {
    validate_scheduler_settings(&settings.scheduler)?;
    Ok(())
}

fn validate_scheduler_settings(settings: &SchedulerSettings) -> AppResult<()> {
    if settings.scan_mode != SCAN_MODE_AUTO && settings.scan_mode != SCAN_MODE_MANUAL {
        return Err(AppError::validation(
            "scheduler.scanMode must be auto or manual",
        ));
    }
    if settings.min_interval < 1 {
        return Err(AppError::validation(
            "scheduler.minInterval must be at least 1",
        ));
    }
    if settings.base_interval < settings.min_interval {
        return Err(AppError::validation(
            "scheduler.baseInterval must be greater than or equal to scheduler.minInterval",
        ));
    }
    if settings.max_interval < settings.base_interval {
        return Err(AppError::validation(
            "scheduler.maxInterval must be greater than or equal to scheduler.baseInterval",
        ));
    }
    if !(0.0 < settings.ewma_alpha && settings.ewma_alpha <= 1.0) {
        return Err(AppError::validation(
            "scheduler.ewmaAlpha must be between 0 and 1",
        ));
    }
    if settings.change_rate_threshold < 1 {
        return Err(AppError::validation(
            "scheduler.changeRateThreshold must be greater than 0",
        ));
    }

    Ok(())
}

fn load_scheduler_change_rate_history_percent(state: &AppState) -> AppResult<Vec<f64>> {
    let conn = state.db_pool().get()?;
    let mut stmt = conn.prepare(
        "SELECT files_parsed, sessions_changed
         FROM scan_runs
         WHERE trigger_type = 'auto'
           AND status = 'completed'
         ORDER BY COALESCE(ended_at, started_at) DESC
         LIMIT 24",
    )?;

    let rows = stmt.query_map(params![], |row| {
        let files_parsed = row.get::<_, i64>(0)?;
        let sessions_changed = row.get::<_, i64>(1)?;
        let rate = if files_parsed <= 0 {
            0.0
        } else {
            (sessions_changed as f64 / files_parsed as f64) * 100.0
        };
        Ok(rate)
    })?;

    let mut history = rows.collect::<Result<Vec<_>, _>>()?;
    history.reverse();
    Ok(history)
}

pub fn default_settings() -> SettingsState {
    SettingsState {
        scheduler: SchedulerSettings {
            scan_mode: SCAN_MODE_AUTO.to_string(),
            base_interval: 60,
            min_interval: 15,
            max_interval: 300,
            adaptive_scanning: true,
            ewma_alpha: 0.3,
            change_rate_threshold: 5,
        },
        ui_preferences: UiPreferencesSettings {
            theme: DEFAULT_THEME.to_string(),
            language: "en-US".to_string(),
            notifications: true,
            localized_token_units: true,
        },
    }
}

fn default_scan_mode() -> String {
    SCAN_MODE_AUTO.to_string()
}

fn default_true() -> bool {
    true
}

fn normalize_scan_mode(value: &str) -> String {
    match value {
        SCAN_MODE_MANUAL => SCAN_MODE_MANUAL.to_string(),
        _ => SCAN_MODE_AUTO.to_string(),
    }
}
