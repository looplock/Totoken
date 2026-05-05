use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::storage;

const CONFIG_DIR_NAME: &str = "config";
const SOURCE_SETTINGS_FILE_NAME: &str = "sources.json";
const SUPPORTED_SOURCE_APPS: &[&str] = &[
    "claude_code",
    "codex",
    "cursor",
    "opencode",
    "kilocode",
    "kiro",
];
const SCAN_ENABLED_SOURCE_APPS: &[&str] = &[
    "claude_code",
    "codex",
    "cursor",
    "opencode",
    "kilocode",
    "kiro",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSettingsState {
    pub items: Vec<SourceSettingsItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSettingsItem {
    pub id: String,
    pub app: String,
    pub root_path: String,
    pub scan_paths: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSettingsStateView {
    pub items: Vec<SourceSettingsItemView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSettingsItemView {
    pub id: String,
    pub app: String,
    pub root_path: String,
    pub enabled: bool,
    pub root_path_exists: bool,
    pub scan_paths: Vec<SourceScanPathView>,
    pub scan_supported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceScanPathView {
    pub path: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSettingsPatch {
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySourceSettingsState {
    items: Vec<LegacySourceSettingsItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySourceSettingsItem {
    id: String,
    app: String,
    path: String,
    enabled: bool,
    #[serde(default)]
    include_archived: bool,
}

#[derive(Debug, Clone)]
pub struct ScannableSourceTarget {
    pub app: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct SourceSettingsStore {
    lock: Arc<Mutex<()>>,
}

impl SourceSettingsStore {
    fn lock(&self) -> AppResult<std::sync::MutexGuard<'_, ()>> {
        self.lock
            .lock()
            .map_err(|_| AppError::internal("source settings lock was poisoned"))
    }
}

pub fn list_source_settings(
    app: &AppHandle,
    store: &SourceSettingsStore,
) -> AppResult<SourceSettingsStateView> {
    let _guard = store
        .lock()
        .map_err(|_| AppError::internal("source settings lock was poisoned"))?;
    let state = load_source_settings(app)?;
    Ok(to_view(&state))
}

pub fn get_source_settings(
    app: &AppHandle,
    store: &SourceSettingsStore,
) -> AppResult<SourceSettingsState> {
    let _guard = store
        .lock()
        .map_err(|_| AppError::internal("source settings lock was poisoned"))?;
    load_source_settings(app)
}

fn load_source_settings(app: &AppHandle) -> AppResult<SourceSettingsState> {
    let settings_path = resolve_source_settings_path(app)?;

    if !settings_path.exists() {
        let defaults = default_source_settings(app)?;
        write_source_settings_file(&settings_path, &defaults)?;
        return Ok(defaults);
    }

    let content = fs::read_to_string(&settings_path)?;
    if content.trim().is_empty() {
        let defaults = default_source_settings(app)?;
        write_source_settings_file(&settings_path, &defaults)?;
        return Ok(defaults);
    }

    let (settings, migrated) = deserialize_source_settings(app, &content)?;
    let normalized = normalize_source_settings(app, settings.clone())?;
    if migrated || normalized != settings {
        write_source_settings_file(&settings_path, &normalized)?;
    }

    Ok(normalized)
}

pub fn update_source_setting(
    app: &AppHandle,
    store: &SourceSettingsStore,
    id: &str,
    patch: SourceSettingsPatch,
) -> AppResult<SourceSettingsStateView> {
    let _guard = store
        .lock()
        .map_err(|_| AppError::internal("source settings lock was poisoned"))?;
    let mut settings = load_source_settings(app)?;
    let Some(item) = settings.items.iter_mut().find(|item| item.id == id) else {
        return Err(AppError::not_found(format!("source {id} was not found")));
    };

    if let Some(enabled) = patch.enabled {
        item.enabled = enabled;
    }

    validate_source_item(item)?;
    let normalized = normalize_source_settings(app, settings)?;
    save_source_settings_unlocked(app, &normalized)?;
    Ok(to_view(&normalized))
}

pub fn list_enabled_scannable_sources(
    app: &AppHandle,
    store: &SourceSettingsStore,
) -> AppResult<Vec<ScannableSourceTarget>> {
    let settings = get_source_settings(app, store)?;
    Ok(settings
        .items
        .iter()
        .filter(|item| item.enabled && supports_scan_toggle(&item.app))
        .flat_map(scannable_targets_for_item)
        .collect())
}

pub fn list_scannable_sources_for_app(
    app: &AppHandle,
    store: &SourceSettingsStore,
    source_app: &str,
) -> AppResult<Vec<ScannableSourceTarget>> {
    validate_source_app(source_app)?;

    let settings = get_source_settings(app, store)?;
    if let Some(item) = settings.items.iter().find(|item| item.app == source_app) {
        return Ok(scannable_targets_for_item(item));
    }

    Ok(vec![])
}

fn save_source_settings_unlocked(app: &AppHandle, settings: &SourceSettingsState) -> AppResult<()> {
    let settings_path = resolve_source_settings_path(app)?;
    write_source_settings_file(&settings_path, settings)
}

pub fn default_source_settings(app: &AppHandle) -> AppResult<SourceSettingsState> {
    let mut items = Vec::with_capacity(SUPPORTED_SOURCE_APPS.len());
    for source_app in SUPPORTED_SOURCE_APPS {
        items.push(default_source_item(app, source_app)?);
    }

    Ok(SourceSettingsState { items })
}

fn normalize_source_settings(
    app: &AppHandle,
    settings: SourceSettingsState,
) -> AppResult<SourceSettingsState> {
    let mut items = Vec::new();

    for source_app in SUPPORTED_SOURCE_APPS {
        if let Some(existing) = settings.items.iter().find(|item| item.app == *source_app) {
            items.push(normalize_existing_item(app, source_app, existing)?);
        } else {
            items.push(default_source_item(app, source_app)?);
        }
    }

    Ok(SourceSettingsState { items })
}

fn normalize_existing_item(
    app: &AppHandle,
    source_app: &str,
    item: &SourceSettingsItem,
) -> AppResult<SourceSettingsItem> {
    let default_item = default_source_item(app, source_app)?;
    let root_path = normalize_root_path_input(app, source_app, &item.root_path)
        .ok()
        .unwrap_or_else(|| default_item.root_path.clone());
    let mut scan_paths = normalize_scan_path_list_input(app, &item.scan_paths)
        .unwrap_or_else(|_| build_default_scan_paths_for_app(source_app, Path::new(&root_path)));
    if source_app == "kiro" && scan_paths.len() == 1 && scan_paths[0] == root_path {
        scan_paths = build_default_scan_paths_for_app(source_app, Path::new(&root_path));
    }
    if source_app == "cursor" {
        scan_paths = ensure_cursor_workspace_scan_path(scan_paths, Path::new(&root_path));
    }
    let mut normalized = SourceSettingsItem {
        id: source_id_for_app(source_app),
        app: source_app.to_string(),
        root_path,
        scan_paths,
        enabled: item.enabled,
    };

    if !supports_scan_toggle(source_app) {
        normalized.enabled = false;
    }

    validate_source_item(&normalized)?;
    Ok(normalized)
}

fn default_source_item(app: &AppHandle, source_app: &str) -> AppResult<SourceSettingsItem> {
    validate_source_app(source_app)?;
    let root_paths = resolve_default_root_paths(app, source_app)?;
    let root_path = root_paths
        .first()
        .map(|path| path_to_string(path))
        .ok_or_else(|| AppError::internal("failed to resolve default source path"))?;
    let mut scan_paths = Vec::new();
    for root_path in &root_paths {
        for scan_path in build_default_scan_paths_for_app(source_app, root_path) {
            if !scan_paths.iter().any(|existing| existing == &scan_path) {
                scan_paths.push(scan_path);
            }
        }
    }

    Ok(SourceSettingsItem {
        id: source_id_for_app(source_app),
        app: source_app.to_string(),
        scan_paths,
        root_path,
        enabled: false,
    })
}

fn resolve_default_root_paths(app: &AppHandle, source_app: &str) -> AppResult<Vec<PathBuf>> {
    let home_dir = app.path().home_dir().map_err(|error| {
        AppError::internal(format!("Failed to resolve home directory: {error}"))
    })?;
    let paths = match source_app {
        "claude_code" => vec![home_dir.join(".claude")],
        "codex" => vec![home_dir.join(".codex")],
        "cursor" => vec![vscode_user_global_storage(&home_dir, "Cursor")],
        "opencode" => opencode_default_roots(&home_dir),
        "kilocode" => kilocode_default_roots(&home_dir),
        "kiro" => vec![vscode_user_global_storage(&home_dir, "Kiro").join("kiro.kiroagent")],
        _ => {
            return Err(AppError::validation(format!(
                "unsupported source app {source_app}"
            )));
        }
    };

    Ok(dedup_paths(paths))
}

fn vscode_user_global_storage(home_dir: &Path, app_dir_name: &str) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home_dir
            .join("Library")
            .join("Application Support")
            .join(app_dir_name)
            .join("User")
            .join("globalStorage")
    }
    #[cfg(target_os = "linux")]
    {
        home_dir
            .join(".config")
            .join(app_dir_name)
            .join("User")
            .join("globalStorage")
    }
    #[cfg(target_os = "windows")]
    {
        home_dir
            .join("AppData")
            .join("Roaming")
            .join(app_dir_name)
            .join("User")
            .join("globalStorage")
    }
}

fn opencode_default_roots(home_dir: &Path) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        dedup_paths(vec![
            windows_local_app_data(home_dir).join("opencode"),
            windows_roaming_app_data(home_dir).join("opencode"),
            linux_data_home(home_dir).join("opencode"),
        ])
    }

    #[cfg(target_os = "macos")]
    {
        dedup_paths(vec![
            macos_application_support(home_dir).join("opencode"),
            linux_data_home(home_dir).join("opencode"),
        ])
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        dedup_paths(vec![linux_data_home(home_dir).join("opencode")])
    }
}

fn kilocode_default_roots(home_dir: &Path) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        dedup_paths(vec![
            windows_local_app_data(home_dir).join("kilo"),
            windows_roaming_app_data(home_dir).join("kilo"),
            windows_local_app_data(home_dir).join("kilocode"),
            windows_roaming_app_data(home_dir).join("kilocode"),
            linux_data_home(home_dir).join("kilo"),
        ])
    }

    #[cfg(target_os = "macos")]
    {
        dedup_paths(vec![
            macos_application_support(home_dir).join("kilo"),
            macos_application_support(home_dir).join("Kilo Code"),
            linux_data_home(home_dir).join("kilo"),
        ])
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        dedup_paths(vec![
            linux_data_home(home_dir).join("kilo"),
            linux_data_home(home_dir).join("kilocode"),
        ])
    }
}

#[cfg(target_os = "macos")]
fn macos_application_support(home_dir: &Path) -> PathBuf {
    home_dir.join("Library").join("Application Support")
}

fn linux_data_home(home_dir: &Path) -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home_dir.join(".local").join("share"))
}

#[cfg(target_os = "windows")]
fn windows_local_app_data(home_dir: &Path) -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home_dir.join("AppData").join("Local"))
}

#[cfg(target_os = "windows")]
fn windows_roaming_app_data(home_dir: &Path) -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home_dir.join("AppData").join("Roaming"))
}

fn dedup_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if !deduped.iter().any(|existing| existing == &path) {
            deduped.push(path);
        }
    }
    deduped
}

fn resolve_source_settings_path(app: &AppHandle) -> AppResult<PathBuf> {
    let storage_paths = storage::resolve_storage_paths(app)?;
    let config_dir = storage_paths.data_dir().join(CONFIG_DIR_NAME);
    fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join(SOURCE_SETTINGS_FILE_NAME))
}

fn write_source_settings_file(
    settings_path: &PathBuf,
    settings: &SourceSettingsState,
) -> AppResult<()> {
    let content = serde_json::to_string_pretty(settings)?;
    let temp_path = settings_path.with_extension("json.tmp");
    fs::write(&temp_path, content)?;
    if settings_path.exists() {
        fs::remove_file(settings_path)?;
    }
    fs::rename(temp_path, settings_path)?;
    Ok(())
}

fn validate_source_item(item: &SourceSettingsItem) -> AppResult<()> {
    validate_source_app(&item.app)?;

    if item.root_path.trim().is_empty() {
        return Err(AppError::validation("source root path cannot be empty"));
    }

    let root_path = Path::new(&item.root_path);
    if !root_path.is_absolute() {
        return Err(AppError::validation(
            "source root path must be absolute or start with ~/",
        ));
    }

    if item.scan_paths.is_empty() {
        return Err(AppError::validation("source scan paths cannot be empty"));
    }

    for scan_path in &item.scan_paths {
        if scan_path.trim().is_empty() {
            return Err(AppError::validation("source scan path cannot be empty"));
        }

        if !Path::new(scan_path).is_absolute() {
            return Err(AppError::validation(
                "source scan path must be absolute or start with ~/",
            ));
        }
    }

    if item.enabled && !supports_scan_toggle(&item.app) {
        return Err(AppError::validation(format!(
            "source app {} cannot be enabled yet",
            item.app
        )));
    }

    Ok(())
}

fn validate_source_app(source_app: &str) -> AppResult<()> {
    if SUPPORTED_SOURCE_APPS.contains(&source_app) {
        return Ok(());
    }

    Err(AppError::validation(format!(
        "unsupported source app {source_app}"
    )))
}

fn normalize_source_path_input(app: &AppHandle, raw_value: &str) -> AppResult<String> {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation("source path cannot be empty"));
    }

    let home_dir = app.path().home_dir().map_err(|error| {
        AppError::internal(format!("Failed to resolve home directory: {error}"))
    })?;

    let candidate = if trimmed == "~" {
        home_dir
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        home_dir.join(rest)
    } else if let Some(rest) = trimmed.strip_prefix("~\\") {
        home_dir.join(rest)
    } else {
        PathBuf::from(trimmed)
    };

    if !candidate.is_absolute() {
        return Err(AppError::validation(
            "source path must be absolute or start with ~/",
        ));
    }

    Ok(path_to_string(&candidate))
}

fn normalize_root_path_input(
    app: &AppHandle,
    source_app: &str,
    raw_value: &str,
) -> AppResult<String> {
    let candidate = normalize_source_path_input(app, raw_value)?;
    Ok(path_to_string(&resolve_root_display_path(
        source_app,
        Path::new(&candidate),
    )))
}

fn normalize_scan_path_list_input(
    app: &AppHandle,
    raw_values: &[String],
) -> AppResult<Vec<String>> {
    let mut normalized = Vec::new();
    for raw_value in raw_values {
        let path = normalize_source_path_input(app, raw_value)?;
        if !normalized.iter().any(|existing| existing == &path) {
            normalized.push(path);
        }
    }

    if normalized.is_empty() {
        return Err(AppError::validation("source scan paths cannot be empty"));
    }

    Ok(normalized)
}

fn supports_scan_toggle(source_app: &str) -> bool {
    SCAN_ENABLED_SOURCE_APPS.contains(&source_app)
}

fn scannable_targets_for_item(item: &SourceSettingsItem) -> Vec<ScannableSourceTarget> {
    item.scan_paths
        .iter()
        .map(|scan_path| ScannableSourceTarget {
            app: item.app.clone(),
            path: PathBuf::from(scan_path),
        })
        .collect()
}

fn resolve_codex_archived_path(root_path: &Path) -> PathBuf {
    match root_path.file_name().and_then(|value| value.to_str()) {
        Some(value) if value.eq_ignore_ascii_case("sessions") => root_path
            .parent()
            .map(|parent| parent.join("archived_sessions"))
            .unwrap_or_else(|| root_path.join("archived_sessions")),
        Some(value) if value.eq_ignore_ascii_case("archived_sessions") => root_path.to_path_buf(),
        Some(value) if value.eq_ignore_ascii_case(".codex") => root_path.join("archived_sessions"),
        _ => root_path
            .parent()
            .map(|parent| parent.join("archived_sessions"))
            .unwrap_or_else(|| root_path.join("archived_sessions")),
    }
}

fn source_id_for_app(source_app: &str) -> String {
    format!("source-{}", source_app.replace('_', "-"))
}

fn to_view(state: &SourceSettingsState) -> SourceSettingsStateView {
    SourceSettingsStateView {
        items: state
            .items
            .iter()
            .map(|item| {
                let root_path = Path::new(&item.root_path);

                SourceSettingsItemView {
                    id: item.id.clone(),
                    app: item.app.clone(),
                    root_path: item.root_path.clone(),
                    enabled: item.enabled,
                    root_path_exists: root_path.exists(),
                    scan_paths: item
                        .scan_paths
                        .iter()
                        .map(|scan_path| SourceScanPathView {
                            path: scan_path.clone(),
                            exists: Path::new(scan_path).exists(),
                        })
                        .collect(),
                    scan_supported: supports_scan_toggle(&item.app),
                }
            })
            .collect(),
    }
}

fn deserialize_source_settings(
    app: &AppHandle,
    content: &str,
) -> AppResult<(SourceSettingsState, bool)> {
    match serde_json::from_str::<SourceSettingsState>(content) {
        Ok(state) => Ok((state, false)),
        Err(_) => {
            let legacy_state: LegacySourceSettingsState = serde_json::from_str(content)?;
            Ok((migrate_legacy_source_settings(app, legacy_state)?, true))
        }
    }
}

fn migrate_legacy_source_settings(
    app: &AppHandle,
    legacy_state: LegacySourceSettingsState,
) -> AppResult<SourceSettingsState> {
    let items = legacy_state
        .items
        .into_iter()
        .map(|legacy_item| migrate_legacy_source_item(app, legacy_item))
        .collect::<AppResult<Vec<_>>>()?;

    Ok(SourceSettingsState { items })
}

fn migrate_legacy_source_item(
    app: &AppHandle,
    legacy_item: LegacySourceSettingsItem,
) -> AppResult<SourceSettingsItem> {
    let migrated_scan_path = normalize_source_path_input(app, &legacy_item.path)?;
    let root_path = normalize_root_path_input(app, &legacy_item.app, &migrated_scan_path)?;
    let mut scan_paths = if migrated_scan_path == root_path {
        build_default_scan_paths_for_app(&legacy_item.app, Path::new(&root_path))
    } else {
        vec![migrated_scan_path.clone()]
    };

    if legacy_item.app == "codex" && legacy_item.include_archived {
        let archived_path =
            path_to_string(&resolve_codex_archived_path(Path::new(&migrated_scan_path)));
        if !scan_paths.iter().any(|path| path == &archived_path) {
            scan_paths.push(archived_path);
        }
    }

    Ok(SourceSettingsItem {
        id: legacy_item.id,
        app: legacy_item.app,
        root_path,
        scan_paths,
        enabled: legacy_item.enabled,
    })
}

fn build_default_scan_paths_for_app(source_app: &str, root_path: &Path) -> Vec<String> {
    match source_app {
        "claude_code" => vec![path_to_string(&root_path.join("projects"))],
        "codex" => vec![
            path_to_string(&root_path.join("sessions")),
            path_to_string(&root_path.join("archived_sessions")),
        ],
        "cursor" => ensure_cursor_workspace_scan_path(
            vec![path_to_string(&database_scan_path(
                root_path,
                "state.vscdb",
            ))],
            root_path,
        ),
        "kilocode" => vec![path_to_string(&database_scan_path(root_path, "kilo.db"))],
        "kiro" => vec![path_to_string(&root_path.join("workspace-sessions"))],
        "opencode" => vec![path_to_string(root_path)],
        _ => vec![path_to_string(root_path)],
    }
}

fn resolve_root_display_path(source_app: &str, scan_path: &Path) -> PathBuf {
    match source_app {
        "codex" => match scan_path.file_name().and_then(|value| value.to_str()) {
            Some(value)
                if value.eq_ignore_ascii_case("sessions")
                    || value.eq_ignore_ascii_case("archived_sessions") =>
            {
                scan_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| scan_path.to_path_buf())
            }
            Some(value) if value.eq_ignore_ascii_case(".codex") => scan_path.to_path_buf(),
            _ => scan_path.to_path_buf(),
        },
        "claude_code" => match scan_path.file_name().and_then(|value| value.to_str()) {
            Some(value) if value.eq_ignore_ascii_case("projects") => scan_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| scan_path.to_path_buf()),
            Some(value) if value.eq_ignore_ascii_case(".claude") => scan_path.to_path_buf(),
            _ => scan_path.to_path_buf(),
        },
        "kilocode" => match scan_path.file_name().and_then(|value| value.to_str()) {
            Some(value) if value.eq_ignore_ascii_case("kilo.db") => scan_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| scan_path.to_path_buf()),
            _ => scan_path.to_path_buf(),
        },
        "cursor" => match scan_path.file_name().and_then(|value| value.to_str()) {
            Some(value) if value.eq_ignore_ascii_case("state.vscdb") => scan_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| scan_path.to_path_buf()),
            _ => scan_path.to_path_buf(),
        },
        "kiro" => match scan_path.file_name().and_then(|value| value.to_str()) {
            Some(value) if value.eq_ignore_ascii_case("workspace-sessions") => scan_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| scan_path.to_path_buf()),
            _ => scan_path.to_path_buf(),
        },
        _ => scan_path.to_path_buf(),
    }
}

fn database_scan_path(root_path: &Path, file_name: &str) -> PathBuf {
    match root_path.file_name().and_then(|value| value.to_str()) {
        Some(value) if value.eq_ignore_ascii_case(file_name) => root_path.to_path_buf(),
        _ => root_path.join(file_name),
    }
}

fn ensure_cursor_workspace_scan_path(mut scan_paths: Vec<String>, root_path: &Path) -> Vec<String> {
    if let Some(workspace_path) = cursor_workspace_storage_path(root_path) {
        let workspace_path = path_to_string(&workspace_path);
        if !scan_paths.iter().any(|path| path == &workspace_path) {
            scan_paths.push(workspace_path);
        }
    }

    scan_paths
}

fn cursor_workspace_storage_path(root_path: &Path) -> Option<PathBuf> {
    let file_name = root_path.file_name().and_then(|value| value.to_str())?;
    if file_name.eq_ignore_ascii_case("workspaceStorage") {
        return Some(root_path.to_path_buf());
    }

    if file_name.eq_ignore_ascii_case("globalStorage") {
        return root_path
            .parent()
            .map(|parent| parent.join("workspaceStorage"));
    }

    if file_name.eq_ignore_ascii_case("state.vscdb") {
        return root_path.parent().and_then(|parent| {
            let parent_name = parent.file_name().and_then(|value| value.to_str())?;
            if parent_name.eq_ignore_ascii_case("globalStorage") {
                return parent
                    .parent()
                    .map(|user_dir| user_dir.join("workspaceStorage"));
            }

            parent
                .parent()
                .filter(|workspace_storage| {
                    workspace_storage
                        .file_name()
                        .and_then(|value| value.to_str())
                        .is_some_and(|value| value.eq_ignore_ascii_case("workspaceStorage"))
                })
                .map(Path::to_path_buf)
        });
    }

    if root_path
        .parent()
        .and_then(|parent| {
            parent
                .file_name()
                .and_then(|value| value.to_str())
                .filter(|value| value.eq_ignore_ascii_case("workspaceStorage"))
        })
        .is_some()
    {
        return root_path.parent().map(Path::to_path_buf);
    }

    Some(root_path.join("workspaceStorage"))
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_home_dir() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\Users\\testuser")
        } else {
            PathBuf::from("/home/testuser")
        }
    }

    fn codex_item(scan_paths: &[PathBuf]) -> SourceSettingsItem {
        SourceSettingsItem {
            id: "source-codex".to_string(),
            app: "codex".to_string(),
            root_path: path_to_string(&test_home_dir().join(".codex")),
            scan_paths: scan_paths.iter().map(|p| path_to_string(p)).collect(),
            enabled: true,
        }
    }

    #[test]
    fn codex_scannable_targets_follow_scan_paths() {
        let sessions_path = test_home_dir().join(".codex").join("sessions");
        let item = codex_item(std::slice::from_ref(&sessions_path));

        let targets = scannable_targets_for_item(&item);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, sessions_path);
    }

    #[test]
    fn codex_scannable_targets_can_include_archived_sessions() {
        let sessions_path = test_home_dir().join(".codex").join("sessions");
        let archived_path = test_home_dir().join(".codex").join("archived_sessions");
        let item = codex_item(&[sessions_path.clone(), archived_path.clone()]);

        let targets = scannable_targets_for_item(&item);

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].path, sessions_path);
        assert_eq!(targets[1].path, archived_path);
    }

    #[test]
    fn codex_default_scan_paths_include_sessions_and_archived_sessions() {
        let root_path = test_home_dir().join(".codex");

        let paths = build_default_scan_paths_for_app("codex", &root_path);

        assert_eq!(
            paths,
            vec![
                path_to_string(&root_path.join("sessions")),
                path_to_string(&root_path.join("archived_sessions")),
            ]
        );
    }

    #[test]
    fn kilocode_default_scan_path_points_to_kilo_db() {
        let root_path = test_home_dir().join(".local").join("share").join("kilo");

        let paths = build_default_scan_paths_for_app("kilocode", &root_path);

        assert_eq!(paths, vec![path_to_string(&root_path.join("kilo.db"))]);
    }

    #[test]
    fn cursor_default_scan_path_points_to_state_db() {
        let root_path = vscode_user_global_storage(&test_home_dir(), "Cursor");

        let paths = build_default_scan_paths_for_app("cursor", &root_path);

        assert_eq!(
            paths,
            vec![
                path_to_string(&root_path.join("state.vscdb")),
                path_to_string(
                    &root_path
                        .parent()
                        .expect("cursor user dir")
                        .join("workspaceStorage")
                ),
            ]
        );
    }

    #[test]
    fn cursor_workspace_hash_root_does_not_add_nested_workspace_storage() {
        let workspace_storage = test_home_dir()
            .join("AppData")
            .join("Roaming")
            .join("Cursor")
            .join("User")
            .join("workspaceStorage");
        let workspace_hash = workspace_storage.join("abc123");

        let paths = build_default_scan_paths_for_app("cursor", &workspace_hash);

        assert_eq!(
            paths,
            vec![
                path_to_string(&workspace_hash.join("state.vscdb")),
                path_to_string(&workspace_storage),
            ]
        );
    }

    #[test]
    fn cursor_workspace_state_db_scan_path_maps_to_workspace_storage_root() {
        let workspace_storage = test_home_dir()
            .join("AppData")
            .join("Roaming")
            .join("Cursor")
            .join("User")
            .join("workspaceStorage");
        let workspace_db = workspace_storage.join("abc123").join("state.vscdb");

        let paths = build_default_scan_paths_for_app("cursor", &workspace_db);

        assert_eq!(
            paths,
            vec![
                path_to_string(&workspace_db),
                path_to_string(&workspace_storage)
            ]
        );
    }

    #[test]
    fn kiro_default_scan_path_points_to_workspace_sessions() {
        let root_path = vscode_user_global_storage(&test_home_dir(), "Kiro").join("kiro.kiroagent");

        let paths = build_default_scan_paths_for_app("kiro", &root_path);

        assert_eq!(
            paths,
            vec![path_to_string(&root_path.join("workspace-sessions"))]
        );
    }

    #[test]
    fn source_settings_json_preserves_enabled_flags() {
        let content = r#"{
            "items": [
                {
                    "id": "source-claude-code",
                    "app": "claude_code",
                    "rootPath": "C:\\Users\\testuser\\.claude",
                    "scanPaths": ["C:\\Users\\testuser\\.claude\\projects"],
                    "enabled": true
                },
                {
                    "id": "source-codex",
                    "app": "codex",
                    "rootPath": "C:\\Users\\testuser\\.codex",
                    "scanPaths": ["C:\\Users\\testuser\\.codex\\sessions"],
                    "enabled": false
                }
            ]
        }"#;

        let state: SourceSettingsState =
            serde_json::from_str(content).expect("deserialize source settings");

        assert!(state.items[0].enabled);
        assert!(!state.items[1].enabled);
    }
}
