use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use rusqlite::OptionalExtension;
use tauri::AppHandle;
use walkdir::WalkDir;

use crate::error::{AppError, AppResult};
use crate::models::{
    AppDataActionOutcomeView, AppDataItemDetailView, AppDataItemView, AppDataMaintenanceAction,
    AppDataOverviewView, AppDataSqliteInfoView, AppDataSummaryView,
};
use crate::state::AppState;
use crate::storage::{self, StoragePaths};

const MAX_TREE_DEPTH: usize = 2;
const MAX_PREVIEW_BYTES: usize = 256 * 1024;
const MAX_PREVIEW_LINES: usize = 5000;
const MAX_DIR_PREVIEW_ENTRIES: usize = 200;
const MAX_BACKUP_SNAPSHOTS: usize = 10;
const MAINTENANCE_BUSY_TIMEOUT: Duration = Duration::from_secs(30);

pub fn get_overview(app: &AppHandle, restart_required: bool) -> AppResult<AppDataOverviewView> {
    let storage_paths = storage::resolve_storage_paths(app)?;
    let root = storage_paths.data_dir();
    let items = collect_items(root, root, 0, MAX_TREE_DEPTH)?;
    let summary = build_summary(root, &items)?;
    let default_selected_path = find_default_selection(&items);

    Ok(AppDataOverviewView {
        storage: storage_paths.to_view(restart_required),
        summary,
        items,
        default_selected_path,
    })
}

pub fn get_item_detail(
    app: &AppHandle,
    relative_path: Option<String>,
) -> AppResult<AppDataItemDetailView> {
    let storage_paths = storage::resolve_storage_paths(app)?;
    let root = storage_paths.data_dir();
    let target = resolve_relative_path(root, relative_path.as_deref())?;
    let metadata = fs::metadata(&target)?;
    let is_dir = metadata.is_dir();
    let size_bytes = if is_dir {
        directory_size(&target)?
    } else {
        metadata.len()
    };
    let relative = normalize_relative_path(root, &target);
    let category = categorize_item(&relative, &target, is_dir);
    let health = determine_health(&target, &category, is_dir);
    let entry_count = if is_dir {
        Some(fs::read_dir(&target)?.count() as u64)
    } else {
        None
    };

    Ok(AppDataItemDetailView {
        relative_path: relative,
        name: file_name_or_root(&target),
        full_path: target.to_string_lossy().to_string(),
        item_type: item_type_label(is_dir),
        category: category.clone(),
        health,
        size_bytes,
        modified_at: modified_at(metadata.modified().ok()),
        entry_count,
        preview: build_preview(&target, is_dir, &category)?,
        preview_language: preview_language(&target, is_dir, &category),
        sqlite: if category == "database" && !is_dir {
            Some(read_sqlite_info(&target)?)
        } else {
            None
        },
        recommended_actions: recommended_actions(&category, is_dir),
    })
}

pub fn run_action(
    app: &AppHandle,
    state: &AppState,
    action: AppDataMaintenanceAction,
) -> AppResult<AppDataActionOutcomeView> {
    let storage_paths = storage::resolve_storage_paths(app)?;

    let reclaimed_bytes = match action {
        AppDataMaintenanceAction::CreateBackup => {
            create_backup_snapshot(&storage_paths, state)?;
            None
        }
        AppDataMaintenanceAction::VacuumDatabase => {
            let pool = state.db_pool();
            let conn = pool.get()?;
            conn.busy_timeout(MAINTENANCE_BUSY_TIMEOUT)?;
            conn.execute_batch("VACUUM; ANALYZE;")?;
            None
        }
        AppDataMaintenanceAction::RebuildIndexes => {
            let pool = state.db_pool();
            let conn = pool.get()?;
            conn.busy_timeout(MAINTENANCE_BUSY_TIMEOUT)?;
            conn.execute_batch("REINDEX; ANALYZE;")?;
            None
        }
        AppDataMaintenanceAction::ClearCaches => {
            Some(clear_cache_entries(storage_paths.data_dir())?)
        }
    };

    let overview = get_overview(app, false)?;
    Ok(AppDataActionOutcomeView {
        overview,
        reclaimed_bytes,
    })
}

fn build_summary(root: &Path, items: &[AppDataItemView]) -> AppResult<AppDataSummaryView> {
    let mut total_size_bytes = 0_u64;
    let mut file_count = 0_u64;
    let mut directory_count = 0_u64;
    let mut cache_size_bytes = 0_u64;

    for entry in WalkDir::new(root).min_depth(1) {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            directory_count += 1;
        } else if metadata.is_file() {
            file_count += 1;
            total_size_bytes += metadata.len();

            let relative = normalize_relative_path(root, entry.path());
            let category = categorize_item(&relative, entry.path(), false);
            if category == "cache" {
                cache_size_bytes += metadata.len();
            }
        }
    }

    let config_count = count_category(items, "config");
    let backup_dir = root.join("backups");
    let backup_count = if backup_dir.exists() {
        fs::read_dir(&backup_dir)?.filter_map(Result::ok).count() as u64
    } else {
        0
    };
    let last_backup_at = if backup_dir.exists() {
        fs::read_dir(&backup_dir)?
            .filter_map(Result::ok)
            .filter_map(|entry| entry.metadata().ok())
            .filter_map(|metadata| metadata.modified().ok())
            .max()
            .map(|value| DateTime::<Utc>::from(value).to_rfc3339())
    } else {
        None
    };

    Ok(AppDataSummaryView {
        total_size_bytes,
        file_count,
        directory_count,
        config_count,
        cache_size_bytes,
        backup_count,
        last_backup_at,
    })
}

fn count_category(items: &[AppDataItemView], category: &str) -> u64 {
    items
        .iter()
        .map(|item| {
            let self_count = u64::from(item.category == category);
            self_count + count_category(&item.children, category)
        })
        .sum()
}

fn collect_items(
    root: &Path,
    current: &Path,
    depth: usize,
    max_depth: usize,
) -> AppResult<Vec<AppDataItemView>> {
    let mut entries: Vec<(PathBuf, fs::Metadata)> = fs::read_dir(current)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            Some((path, metadata))
        })
        .collect();

    entries.sort_by(|(left_path, left_meta), (right_path, right_meta)| {
        right_meta.is_dir().cmp(&left_meta.is_dir()).then_with(|| {
            left_path
                .file_name()
                .unwrap_or_default()
                .cmp(right_path.file_name().unwrap_or_default())
        })
    });

    let mut items = Vec::with_capacity(entries.len());
    for (path, metadata) in entries {
        let is_dir = metadata.is_dir();
        let relative = normalize_relative_path(root, &path);
        let children = if is_dir && depth < max_depth {
            collect_items(root, &path, depth + 1, max_depth)?
        } else {
            Vec::new()
        };
        let size_bytes = if is_dir {
            directory_size(&path)?
        } else {
            metadata.len()
        };
        let category = categorize_item(&relative, &path, is_dir);
        let health = determine_health(&path, &category, is_dir);

        items.push(AppDataItemView {
            relative_path: relative,
            name: file_name_or_root(&path),
            full_path: path.to_string_lossy().to_string(),
            item_type: item_type_label(is_dir),
            category,
            health,
            size_bytes,
            modified_at: modified_at(metadata.modified().ok()),
            children,
        });
    }

    Ok(items)
}

fn resolve_relative_path(root: &Path, relative_path: Option<&str>) -> AppResult<PathBuf> {
    let Some(raw_relative) = relative_path else {
        return Ok(root.to_path_buf());
    };
    let trimmed = raw_relative.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Ok(root.to_path_buf());
    }

    let relative = Path::new(trimmed);
    if relative.is_absolute() {
        return Err(AppError::validation(
            "path must be relative to the storage root",
        ));
    }
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(AppError::validation("path traversal is not allowed"));
    }

    let candidate = root.join(relative);
    if !candidate.exists() {
        return Err(AppError::not_found(
            "the requested storage item does not exist",
        ));
    }

    let canonical_root = fs::canonicalize(root)?;
    let canonical_candidate = fs::canonicalize(candidate)?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(AppError::validation(
            "the requested path is outside of the storage root",
        ));
    }

    Ok(canonical_candidate)
}

fn normalize_relative_path(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/"),
        _ => ".".to_string(),
    }
}

fn item_type_label(is_dir: bool) -> String {
    if is_dir {
        "directory".to_string()
    } else {
        "file".to_string()
    }
}

fn file_name_or_root(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn modified_at(value: Option<SystemTime>) -> Option<String> {
    value.map(|inner| DateTime::<Utc>::from(inner).to_rfc3339())
}

fn directory_size(path: &Path) -> AppResult<u64> {
    let mut total = 0_u64;
    for entry in WalkDir::new(path).min_depth(1) {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            total += metadata.len();
        }
    }
    Ok(total)
}

fn categorize_item(relative: &str, path: &Path, is_dir: bool) -> String {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let relative_lower = relative.to_lowercase();

    if relative == "." {
        return "root".to_string();
    }
    if name == "totoken.db" || name.ends_with(".db") || name.ends_with(".sqlite") {
        return "database".to_string();
    }
    if name == "storage.json"
        || (name.ends_with(".json")
            && !relative_lower.contains('/')
            && !relative_lower.contains("cache")
            && !relative_lower.contains("backup"))
    {
        return "config".to_string();
    }
    if name.contains("cache")
        || relative_lower.starts_with("cache/")
        || relative_lower.contains("/cache/")
    {
        return "cache".to_string();
    }
    if name.contains("backup") || relative_lower.starts_with("backups") {
        return "backup".to_string();
    }
    if name.contains("export") || relative_lower.starts_with("exports") {
        return "export".to_string();
    }
    if name.contains("report") {
        return "report".to_string();
    }
    if is_dir {
        return "folder".to_string();
    }

    "other".to_string()
}

fn determine_health(path: &Path, category: &str, is_dir: bool) -> String {
    if category == "database" {
        return "healthy".to_string();
    }
    if category == "cache" {
        return if is_dir
            || path
                .metadata()
                .map(|metadata| metadata.len() > 0)
                .unwrap_or(false)
        {
            "clearable".to_string()
        } else {
            "empty".to_string()
        };
    }
    if category == "backup" {
        return "available".to_string();
    }
    if category == "config" && path.extension().and_then(|value| value.to_str()) == Some("json") {
        return match fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        {
            Some(_) => "valid".to_string(),
            None => "invalid".to_string(),
        };
    }

    "ready".to_string()
}

fn build_preview(path: &Path, is_dir: bool, category: &str) -> AppResult<Option<String>> {
    if is_dir {
        let mut names = Vec::new();
        for entry in fs::read_dir(path)?.take(MAX_DIR_PREVIEW_ENTRIES) {
            let entry = entry?;
            names.push(entry.file_name().to_string_lossy().to_string());
        }
        if names.is_empty() {
            return Ok(None);
        }
        return Ok(Some(names.join("\n")));
    }

    if category == "database" {
        return Ok(Some(
            "SQLite database file. Use the detail metrics below for integrity and page statistics."
                .to_string(),
        ));
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let is_previewable = matches!(extension.as_str(), "json" | "txt" | "log" | "sql" | "md");
    if !is_previewable {
        return Ok(None);
    }

    let bytes = fs::read(path)?;
    let preview_bytes = bytes.len().min(MAX_PREVIEW_BYTES);
    let content = String::from_utf8_lossy(&bytes[..preview_bytes]).to_string();
    let preview = content
        .lines()
        .take(MAX_PREVIEW_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    if preview.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(preview))
}

fn preview_language(path: &Path, is_dir: bool, category: &str) -> Option<String> {
    if is_dir {
        return Some("text".to_string());
    }
    if category == "database" {
        return Some("text".to_string());
    }
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_lowercase())
}

fn read_sqlite_info(path: &Path) -> AppResult<AppDataSqliteInfoView> {
    let conn = rusqlite::Connection::open(path)?;
    let table_count = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get::<_, u64>(0),
    )?;
    let index_count = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get::<_, u64>(0),
    )?;
    let page_count = conn.query_row("PRAGMA page_count", [], |row| row.get::<_, u64>(0))?;
    let freelist_count = conn.query_row("PRAGMA freelist_count", [], |row| row.get::<_, u64>(0))?;
    let page_size_bytes = conn.query_row("PRAGMA page_size", [], |row| row.get::<_, u64>(0))?;
    let integrity = conn
        .query_row("PRAGMA integrity_check(1)", [], |row| {
            row.get::<_, String>(0)
        })
        .optional()?
        .unwrap_or_else(|| "unknown".to_string());

    Ok(AppDataSqliteInfoView {
        table_count,
        index_count,
        page_count,
        freelist_count,
        page_size_bytes,
        integrity,
    })
}

fn recommended_actions(category: &str, is_dir: bool) -> Vec<String> {
    match category {
        "database" => vec!["vacuum_database".to_string(), "rebuild_indexes".to_string()],
        "cache" => vec!["clear_caches".to_string()],
        "backup" => vec!["create_backup".to_string()],
        "config" => vec!["create_backup".to_string()],
        _ if is_dir => vec!["create_backup".to_string()],
        _ => Vec::new(),
    }
}

fn find_default_selection(items: &[AppDataItemView]) -> Option<String> {
    let mut database_path = None;
    let mut config_path = None;
    let mut first_path = None;

    fn walk(
        items: &[AppDataItemView],
        database_path: &mut Option<String>,
        config_path: &mut Option<String>,
        first_path: &mut Option<String>,
    ) {
        for item in items {
            if first_path.is_none() {
                *first_path = Some(item.relative_path.clone());
            }
            if database_path.is_none() && item.category == "database" {
                *database_path = Some(item.relative_path.clone());
            }
            if config_path.is_none() && item.category == "config" {
                *config_path = Some(item.relative_path.clone());
            }
            walk(&item.children, database_path, config_path, first_path);
        }
    }

    walk(items, &mut database_path, &mut config_path, &mut first_path);
    database_path.or(config_path).or(first_path)
}

fn create_backup_snapshot(storage_paths: &StoragePaths, state: &AppState) -> AppResult<()> {
    let root = storage_paths.data_dir();
    let db_path = storage_paths.db_path();
    let db_file_name = db_path.file_name();
    let backups_dir = root.join("backups");
    fs::create_dir_all(&backups_dir)?;

    let timestamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let final_name = format!("snapshot-{timestamp}");
    let final_dir = backups_dir.join(&final_name);
    let staging_dir = backups_dir.join(format!("{final_name}.tmp"));

    // Clean up any prior staging dir from a crashed run.
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir)?;
    }
    fs::create_dir_all(&staging_dir)?;

    let outcome: AppResult<()> = (|| {
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let entry_path = entry.path();
            if entry_path == backups_dir {
                continue;
            }

            let entry_name = entry.file_name();
            // Refuse to follow symlinks/junctions: copying through them risks
            // either pulling in unrelated data or hitting cycles.
            let meta = fs::symlink_metadata(&entry_path)?;
            if meta.file_type().is_symlink() {
                log::warn!("skipping symlink during backup: {}", entry_path.display());
                continue;
            }

            // The active SQLite db: snapshot via Online Backup API to avoid
            // copying a file mid-transaction (corrupted backups otherwise).
            if Some(entry_name.as_os_str()) == db_file_name {
                let destination = staging_dir.join(&entry_name);
                backup_sqlite_database(state, &destination)?;
                continue;
            }

            // WAL/SHM/journal sidecars are ephemeral and unsafe to copy
            // independently — Online Backup produces a self-contained file.
            if is_wal_artifact(&entry_name, db_file_name) {
                continue;
            }

            let destination = staging_dir.join(&entry_name);
            if meta.is_dir() {
                copy_directory(&entry_path, &destination)?;
            } else {
                fs::copy(&entry_path, &destination)?;
            }
        }
        Ok(())
    })();

    match outcome {
        Ok(()) => {
            // Atomic publish. If a snapshot with the same name somehow
            // exists (clock collision within the same second), replace it.
            if final_dir.exists() {
                fs::remove_dir_all(&final_dir)?;
            }
            fs::rename(&staging_dir, &final_dir)?;
            prune_old_snapshots(&backups_dir, MAX_BACKUP_SNAPSHOTS);
            Ok(())
        }
        Err(error) => {
            // Best-effort cleanup; do not shadow the original error.
            let _ = fs::remove_dir_all(&staging_dir);
            Err(error)
        }
    }
}

fn backup_sqlite_database(state: &AppState, destination: &Path) -> AppResult<()> {
    let pool = state.db_pool();
    let src = pool.get()?;
    src.busy_timeout(MAINTENANCE_BUSY_TIMEOUT)?;

    let mut dst = rusqlite::Connection::open(destination)?;
    let backup = rusqlite::backup::Backup::new(&src, &mut dst)?;
    backup.run_to_completion(
        /* pages per step */ 256,
        Duration::from_millis(10),
        None,
    )?;
    Ok(())
}

fn is_wal_artifact(name: &OsStr, db_file_name: Option<&OsStr>) -> bool {
    let Some(db_name) = db_file_name.and_then(|value| value.to_str()) else {
        return false;
    };
    let Some(name_str) = name.to_str() else {
        return false;
    };
    name_str == format!("{db_name}-wal")
        || name_str == format!("{db_name}-shm")
        || name_str == format!("{db_name}-journal")
}

fn prune_old_snapshots(backups_dir: &Path, keep: usize) {
    let read_dir = match fs::read_dir(backups_dir) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "failed to enumerate backups dir for pruning ({}): {error}",
                backups_dir.display()
            );
            return;
        }
    };

    let mut snapshots: Vec<PathBuf> = read_dir
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false)
                && name_str.starts_with("snapshot-")
                && !name_str.ends_with(".tmp")
        })
        .map(|entry| entry.path())
        .collect();

    // Snapshot names use YYYYMMDD-HHMMSS so lexicographic sort = chronological.
    snapshots.sort();

    if snapshots.len() <= keep {
        return;
    }

    let to_remove = snapshots.len() - keep;
    for path in snapshots.into_iter().take(to_remove) {
        if let Err(error) = fs::remove_dir_all(&path) {
            log::warn!("failed to prune old backup {}: {error}", path.display());
        }
    }
}

fn copy_directory(source: &Path, destination: &Path) -> AppResult<()> {
    fs::create_dir_all(destination)?;
    for entry in WalkDir::new(source).min_depth(1) {
        let entry = entry?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|_| AppError::internal("failed to build backup path"))?;
        let target = destination.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }

    Ok(())
}

fn clear_cache_entries(root: &Path) -> AppResult<u64> {
    let mut bytes_freed = 0_u64;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let entry_name = entry.file_name();
        if !is_cache_entry_name(&entry_name) {
            continue;
        }

        let path = entry.path();
        // Use symlink_metadata so we don't traverse into a symlink/junction
        // pointing outside the data root.
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            log::warn!("skipping symlink during cache clear: {}", path.display());
            continue;
        }

        if meta.is_dir() {
            // remove_dir_all in Rust >= 1.69 refuses to follow Windows
            // reparse points, so this is safe even for unusual junctions.
            bytes_freed = bytes_freed.saturating_add(directory_size(&path).unwrap_or(0));
            fs::remove_dir_all(&path)?;
        } else if meta.is_file() {
            bytes_freed = bytes_freed.saturating_add(meta.len());
            fs::remove_file(&path)?;
        }
    }

    Ok(bytes_freed)
}

fn is_cache_entry_name(name: &OsStr) -> bool {
    let Some(value) = name.to_str() else {
        return false;
    };
    let lower = value.to_lowercase();
    // Top-level entries we are willing to wipe wholesale. Looser matches
    // (substring "cache") would catch innocent neighbours like
    // `cached_keys.json` or `mycache.lock`.
    matches!(lower.as_str(), "cache" | "caches") || lower.ends_with(".cache")
}
