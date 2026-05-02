use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use walkdir::WalkDir;

use crate::error::{AppError, AppResult};

const BOOTSTRAP_DIR_NAME: &str = ".totoken";
const STORAGE_CONFIG_FILE_NAME: &str = "storage.json";
const STORAGE_PENDING_CLEANUP_FILE_NAME: &str = "storage-pending-cleanup.json";
const STORAGE_MIGRATION_MARKER_FILE_NAME: &str = ".totoken-migration-in-progress";
const DB_FILE_NAME: &str = "totoken.db";

#[derive(Debug, Clone)]
pub struct StoragePaths {
    bootstrap_dir: PathBuf,
    config_path: PathBuf,
    pending_cleanup_path: PathBuf,
    data_dir: PathBuf,
    db_path: PathBuf,
    using_default_dir: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageConfigView {
    pub bootstrap_dir: String,
    pub data_dir: String,
    pub db_path: String,
    pub using_default_dir: bool,
    pub restart_required: bool,
    pub pending_cleanup_path: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageBootstrapConfig {
    data_dir: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingCleanupConfig {
    old_data_dir: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct MigrationInProgressConfig {
    source_data_dir: String,
    target_data_dir: String,
}

impl StoragePaths {
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn to_view(&self, restart_required: bool) -> StorageConfigView {
        let pending_cleanup = read_pending_cleanup_config(&self.pending_cleanup_path)
            .ok()
            .and_then(|config| config.old_data_dir);
        StorageConfigView {
            bootstrap_dir: path_to_string(&self.bootstrap_dir),
            data_dir: path_to_string(&self.data_dir),
            db_path: path_to_string(&self.db_path),
            using_default_dir: self.using_default_dir,
            restart_required,
            pending_cleanup_path: pending_cleanup,
        }
    }
}

pub fn resolve_storage_paths(app: &AppHandle) -> AppResult<StoragePaths> {
    let home_dir = app.path().home_dir().map_err(|error| {
        AppError::internal(format!("Failed to resolve home directory: {error}"))
    })?;

    let legacy_bootstrap_dir = home_dir.join(BOOTSTRAP_DIR_NAME);
    let bootstrap_dir = default_bootstrap_dir(app, &home_dir)?;
    migrate_legacy_bootstrap_dir(&legacy_bootstrap_dir, &bootstrap_dir)?;
    fs::create_dir_all(&bootstrap_dir)?;

    let config_path = bootstrap_dir.join(STORAGE_CONFIG_FILE_NAME);
    let pending_cleanup_path = bootstrap_dir.join(STORAGE_PENDING_CLEANUP_FILE_NAME);
    let migration_marker_path = bootstrap_dir.join(STORAGE_MIGRATION_MARKER_FILE_NAME);
    recover_interrupted_storage_migration(
        &migration_marker_path,
        &config_path,
        &pending_cleanup_path,
        &bootstrap_dir,
    )?;
    let bootstrap_config = read_bootstrap_config(&config_path)?;

    let data_dir = bootstrap_config
        .data_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| bootstrap_dir.clone());
    fs::create_dir_all(&data_dir)?;

    let using_default_dir = data_dir == bootstrap_dir;
    let db_path = data_dir.join(DB_FILE_NAME);

    cleanup_pending_data_dir(&pending_cleanup_path, &data_dir, &bootstrap_dir)?;

    Ok(StoragePaths {
        bootstrap_dir,
        config_path,
        pending_cleanup_path,
        data_dir,
        db_path,
        using_default_dir,
    })
}

fn default_bootstrap_dir(app: &AppHandle, home_dir: &Path) -> AppResult<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        Ok(home_dir.join(BOOTSTRAP_DIR_NAME))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = home_dir;
        app.path().app_data_dir().map_err(|error| {
            AppError::internal(format!("Failed to resolve app data directory: {error}"))
        })
    }
}

fn migrate_legacy_bootstrap_dir(legacy_dir: &Path, bootstrap_dir: &Path) -> AppResult<()> {
    if legacy_dir == bootstrap_dir || !legacy_dir.exists() || bootstrap_dir.exists() {
        return Ok(());
    }

    if let Some(parent) = bootstrap_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(legacy_dir, bootstrap_dir)?;
    Ok(())
}

pub fn set_data_dir(app: &AppHandle, data_dir: Option<String>) -> AppResult<StorageConfigView> {
    let current = resolve_storage_paths(app)?;
    let default_dir = current.bootstrap_dir.clone();

    let target_dir =
        normalize_data_dir_input(data_dir, &default_dir)?.unwrap_or_else(|| default_dir.clone());
    if current.data_dir == target_dir {
        return Ok(current.to_view(false));
    }

    validate_target_data_dir(app, &current, &target_dir)?;
    prepare_target_data_dir(&current, &target_dir)?;
    begin_storage_migration(&current, &target_dir)?;
    migrate_storage_contents(&current, &target_dir)?;
    schedule_old_data_dir_cleanup(&current, &target_dir)?;

    if target_dir == default_dir {
        if current.config_path.exists() {
            fs::remove_file(&current.config_path)?;
        }
    } else {
        let config = StorageBootstrapConfig {
            data_dir: Some(path_to_string(&target_dir)),
        };
        write_bootstrap_config(&current.config_path, &config)?;
    }
    finish_storage_migration(&current, &target_dir)?;

    let resolved = resolve_storage_paths(app)?;
    Ok(resolved.to_view(true))
}

fn normalize_data_dir_input(
    data_dir: Option<String>,
    default_dir: &Path,
) -> AppResult<Option<PathBuf>> {
    let Some(raw_value) = data_dir else {
        return Ok(None);
    };

    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let candidate = PathBuf::from(trimmed);
    if !candidate.is_absolute() {
        return Err(AppError::validation(
            "storage data directory must be an absolute path",
        ));
    }

    if candidate == default_dir {
        return Ok(None);
    }

    Ok(Some(candidate))
}

fn validate_target_data_dir(
    app: &AppHandle,
    current: &StoragePaths,
    target_dir: &Path,
) -> AppResult<()> {
    if is_root_directory(target_dir) {
        return Err(AppError::validation(
            "storage data directory cannot be a filesystem root",
        ));
    }

    for protected_dir in protected_system_dirs(app)? {
        let is_default_dir = paths_are_same(target_dir, &current.bootstrap_dir)?;
        if path_is_same_or_within(target_dir, &protected_dir)?
            && !(is_default_dir && paths_are_same(target_dir, &protected_dir)?)
        {
            return Err(AppError::validation(format!(
                "storage data directory cannot be inside protected system path {}",
                protected_dir.display()
            )));
        }
    }

    if path_is_same_or_within(target_dir, current.data_dir())?
        || path_is_same_or_within(current.data_dir(), target_dir)?
    {
        return Err(AppError::validation(
            "storage data directory cannot overlap the current storage directory",
        ));
    }

    Ok(())
}

fn prepare_target_data_dir(current: &StoragePaths, target_dir: &Path) -> AppResult<()> {
    fs::create_dir_all(target_dir)?;
    ensure_directory_is_writable(target_dir)?;

    let mut allowed_entries = Vec::new();
    if target_dir == current.bootstrap_dir {
        if let Some(file_name) = current.config_path.file_name() {
            allowed_entries.push(file_name.to_string_lossy().to_string());
        }
    }

    let has_unexpected_content = fs::read_dir(target_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .any(|name| !allowed_entries.iter().any(|allowed| allowed == &name));
    if has_unexpected_content {
        return Err(AppError::validation(
            "storage data directory must be empty before migration",
        ));
    }

    Ok(())
}

fn begin_storage_migration(current: &StoragePaths, target_dir: &Path) -> AppResult<()> {
    let marker = MigrationInProgressConfig {
        source_data_dir: path_to_string(current.data_dir()),
        target_data_dir: path_to_string(target_dir),
    };
    write_migration_marker(&current.bootstrap_dir, &marker)?;
    write_migration_marker(target_dir, &marker)?;
    Ok(())
}

fn finish_storage_migration(current: &StoragePaths, target_dir: &Path) -> AppResult<()> {
    remove_migration_marker(target_dir)?;
    remove_migration_marker(&current.bootstrap_dir)?;
    Ok(())
}

fn migrate_storage_contents(current: &StoragePaths, target_dir: &Path) -> AppResult<()> {
    checkpoint_database(&current.db_path)?;

    for entry in fs::read_dir(current.data_dir())? {
        let entry = entry?;
        let source_path = entry.path();
        if source_path == current.config_path {
            continue;
        }

        let destination = target_dir.join(entry.file_name());
        if entry.metadata()?.is_dir() {
            copy_directory(&source_path, &destination)?;
        } else {
            fs::copy(&source_path, &destination)?;
        }
    }

    Ok(())
}

fn schedule_old_data_dir_cleanup(current: &StoragePaths, target_dir: &Path) -> AppResult<()> {
    if current.data_dir == target_dir {
        return Ok(());
    }

    let pending_cleanup = PendingCleanupConfig {
        old_data_dir: Some(path_to_string(current.data_dir())),
    };
    write_pending_cleanup_config(&current.pending_cleanup_path, &pending_cleanup)
}

fn cleanup_pending_data_dir(
    pending_cleanup_path: &Path,
    active_data_dir: &Path,
    bootstrap_dir: &Path,
) -> AppResult<()> {
    if !pending_cleanup_path.exists() {
        return Ok(());
    }

    let pending = read_pending_cleanup_config(pending_cleanup_path)?;
    let Some(old_data_dir) = pending.old_data_dir else {
        let _ = fs::remove_file(pending_cleanup_path);
        return Ok(());
    };

    let old_data_dir = PathBuf::from(old_data_dir);
    if !old_data_dir.exists() {
        let _ = fs::remove_file(pending_cleanup_path);
        return Ok(());
    }

    if paths_are_same(&old_data_dir, active_data_dir)? {
        let _ = fs::remove_file(pending_cleanup_path);
        return Ok(());
    }

    if paths_are_same(&old_data_dir, bootstrap_dir)? {
        let _ = remove_storage_directory_contents_except(
            &old_data_dir,
            &[
                STORAGE_CONFIG_FILE_NAME,
                STORAGE_PENDING_CLEANUP_FILE_NAME,
                STORAGE_MIGRATION_MARKER_FILE_NAME,
            ],
        );
    } else if old_data_dir.join(DB_FILE_NAME).exists() {
        let _ = remove_storage_directory_contents(&old_data_dir);
    } else {
        let _ = fs::remove_dir_all(&old_data_dir);
    }

    let _ = fs::remove_file(pending_cleanup_path);
    Ok(())
}

fn recover_interrupted_storage_migration(
    migration_marker_path: &Path,
    config_path: &Path,
    pending_cleanup_path: &Path,
    bootstrap_dir: &Path,
) -> AppResult<()> {
    if !migration_marker_path.exists() {
        return Ok(());
    }

    let marker = read_migration_marker(migration_marker_path)?;
    let target_dir = PathBuf::from(&marker.target_data_dir);
    let bootstrap_config = read_bootstrap_config(config_path)?;
    let configured_data_dir = bootstrap_config
        .data_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| bootstrap_dir.to_path_buf());

    if configured_data_dir == target_dir {
        remove_migration_marker(&target_dir)?;
        remove_migration_marker(bootstrap_dir)?;
        return Ok(());
    }

    rollback_storage_migration_target(&target_dir, bootstrap_dir)?;
    let _ = fs::remove_file(pending_cleanup_path);
    remove_migration_marker(bootstrap_dir)?;
    Ok(())
}

fn rollback_storage_migration_target(target_dir: &Path, bootstrap_dir: &Path) -> AppResult<()> {
    if !target_dir.exists() || !target_dir.join(STORAGE_MIGRATION_MARKER_FILE_NAME).exists() {
        return Ok(());
    }

    if target_dir == bootstrap_dir {
        remove_storage_directory_contents_except(
            target_dir,
            &[
                STORAGE_CONFIG_FILE_NAME,
                STORAGE_MIGRATION_MARKER_FILE_NAME,
                STORAGE_PENDING_CLEANUP_FILE_NAME,
            ],
        )?;
        return Ok(());
    }

    fs::remove_dir_all(target_dir)?;
    Ok(())
}

fn remove_storage_directory_contents(path: &Path) -> AppResult<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let target = entry.path();
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(target)?;
        } else {
            fs::remove_file(target)?;
        }
    }

    fs::remove_dir(path)?;
    Ok(())
}

fn remove_storage_directory_contents_except(
    path: &Path,
    preserved_names: &[&str],
) -> AppResult<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        if preserved_names
            .iter()
            .any(|preserved| *preserved == file_name)
        {
            continue;
        }

        let target = entry.path();
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(target)?;
        } else {
            fs::remove_file(target)?;
        }
    }

    Ok(())
}

fn checkpoint_database(db_path: &Path) -> AppResult<()> {
    if !db_path.exists() {
        return Ok(());
    }

    let conn = rusqlite::Connection::open(db_path)?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

fn ensure_directory_is_writable(path: &Path) -> AppResult<()> {
    let probe_path = path.join(".totoken-write-test");
    fs::write(&probe_path, b"ok")?;
    fs::remove_file(probe_path)?;
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> AppResult<()> {
    fs::create_dir_all(destination)?;
    for entry in WalkDir::new(source).min_depth(1) {
        let entry = entry?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|_| AppError::internal("failed to build storage migration path"))?;
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

fn protected_system_dirs(app: &AppHandle) -> AppResult<Vec<PathBuf>> {
    let mut protected = Vec::new();

    #[cfg(target_os = "windows")]
    {
        protected.extend(
            [
                std::env::var_os("SystemRoot"),
                std::env::var_os("ProgramFiles"),
                std::env::var_os("ProgramFiles(x86)"),
                std::env::var_os("ProgramData"),
            ]
            .into_iter()
            .flatten()
            .map(PathBuf::from),
        );
    }

    #[cfg(not(target_os = "windows"))]
    {
        protected.extend(
            ["/", "/bin", "/etc", "/usr", "/var", "/System", "/Library"]
                .into_iter()
                .map(PathBuf::from),
        );
    }

    let app_data_dir = app.path().app_data_dir().map_err(|error| {
        AppError::internal(format!("Failed to resolve app data directory: {error}"))
    })?;
    protected.push(app_data_dir);

    Ok(protected)
}

fn path_is_same_or_within(path: &Path, root: &Path) -> AppResult<bool> {
    let normalized_path = normalize_for_comparison(path)?;
    let normalized_root = normalize_for_comparison(root)?;
    Ok(normalized_path == normalized_root || normalized_path.starts_with(&normalized_root))
}

fn paths_are_same(left: &Path, right: &Path) -> AppResult<bool> {
    Ok(normalize_for_comparison(left)? == normalize_for_comparison(right)?)
}

fn normalize_for_comparison(path: &Path) -> AppResult<PathBuf> {
    if path.exists() {
        return Ok(fs::canonicalize(path)?);
    }

    let parent = path.parent().ok_or_else(|| {
        AppError::validation("storage data directory must have an existing parent directory")
    })?;
    let canonical_parent = fs::canonicalize(parent)?;
    let leaf = path.file_name().ok_or_else(|| {
        AppError::validation("storage data directory must have a valid final path segment")
    })?;
    Ok(canonical_parent.join(leaf))
}

fn is_root_directory(path: &Path) -> bool {
    path.parent().is_none()
}

fn read_bootstrap_config(config_path: &Path) -> AppResult<StorageBootstrapConfig> {
    if !config_path.exists() {
        return Ok(StorageBootstrapConfig::default());
    }

    let content = fs::read_to_string(config_path)?;
    if content.trim().is_empty() {
        return Ok(StorageBootstrapConfig::default());
    }

    Ok(serde_json::from_str(&content)?)
}

fn write_bootstrap_config(config_path: &Path, config: &StorageBootstrapConfig) -> AppResult<()> {
    let content = serde_json::to_string_pretty(config)?;
    fs::write(config_path, content)?;
    Ok(())
}

fn read_pending_cleanup_config(config_path: &Path) -> AppResult<PendingCleanupConfig> {
    if !config_path.exists() {
        return Ok(PendingCleanupConfig::default());
    }

    let content = fs::read_to_string(config_path)?;
    if content.trim().is_empty() {
        return Ok(PendingCleanupConfig::default());
    }

    Ok(serde_json::from_str(&content)?)
}

fn write_pending_cleanup_config(
    config_path: &Path,
    config: &PendingCleanupConfig,
) -> AppResult<()> {
    let content = serde_json::to_string_pretty(config)?;
    fs::write(config_path, content)?;
    Ok(())
}

fn read_migration_marker(marker_path: &Path) -> AppResult<MigrationInProgressConfig> {
    let content = fs::read_to_string(marker_path)?;
    Ok(serde_json::from_str(&content)?)
}

fn write_migration_marker(directory: &Path, marker: &MigrationInProgressConfig) -> AppResult<()> {
    fs::create_dir_all(directory)?;
    let content = serde_json::to_string_pretty(marker)?;
    fs::write(directory.join(STORAGE_MIGRATION_MARKER_FILE_NAME), content)?;
    Ok(())
}

fn remove_migration_marker(directory: &Path) -> AppResult<()> {
    let marker_path = directory.join(STORAGE_MIGRATION_MARKER_FILE_NAME);
    if marker_path.exists() {
        fs::remove_file(marker_path)?;
    }
    Ok(())
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn recovers_uncommitted_storage_migration_by_removing_target() -> AppResult<()> {
        let root = temp_storage_test_dir("storage-rollback");
        let bootstrap_dir = root.join("bootstrap");
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        fs::create_dir_all(&bootstrap_dir)?;
        fs::create_dir_all(&source_dir)?;
        fs::create_dir_all(&target_dir)?;
        fs::write(target_dir.join(DB_FILE_NAME), b"partial")?;

        let config_path = bootstrap_dir.join(STORAGE_CONFIG_FILE_NAME);
        let pending_cleanup_path = bootstrap_dir.join(STORAGE_PENDING_CLEANUP_FILE_NAME);
        let marker_path = bootstrap_dir.join(STORAGE_MIGRATION_MARKER_FILE_NAME);
        write_pending_cleanup_config(
            &pending_cleanup_path,
            &PendingCleanupConfig {
                old_data_dir: Some(path_to_string(&source_dir)),
            },
        )?;
        let marker = MigrationInProgressConfig {
            source_data_dir: path_to_string(&source_dir),
            target_data_dir: path_to_string(&target_dir),
        };
        write_migration_marker(&bootstrap_dir, &marker)?;
        write_migration_marker(&target_dir, &marker)?;

        recover_interrupted_storage_migration(
            &marker_path,
            &config_path,
            &pending_cleanup_path,
            &bootstrap_dir,
        )?;

        assert!(!target_dir.exists());
        assert!(!marker_path.exists());
        assert!(!pending_cleanup_path.exists());
        cleanup_temp_storage_test_dir(&root);
        Ok(())
    }

    #[test]
    fn recovers_committed_storage_migration_by_clearing_markers() -> AppResult<()> {
        let root = temp_storage_test_dir("storage-commit");
        let bootstrap_dir = root.join("bootstrap");
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        fs::create_dir_all(&bootstrap_dir)?;
        fs::create_dir_all(&source_dir)?;
        fs::create_dir_all(&target_dir)?;

        let config_path = bootstrap_dir.join(STORAGE_CONFIG_FILE_NAME);
        let pending_cleanup_path = bootstrap_dir.join(STORAGE_PENDING_CLEANUP_FILE_NAME);
        let marker_path = bootstrap_dir.join(STORAGE_MIGRATION_MARKER_FILE_NAME);
        write_bootstrap_config(
            &config_path,
            &StorageBootstrapConfig {
                data_dir: Some(path_to_string(&target_dir)),
            },
        )?;
        let marker = MigrationInProgressConfig {
            source_data_dir: path_to_string(&source_dir),
            target_data_dir: path_to_string(&target_dir),
        };
        write_migration_marker(&bootstrap_dir, &marker)?;
        write_migration_marker(&target_dir, &marker)?;

        recover_interrupted_storage_migration(
            &marker_path,
            &config_path,
            &pending_cleanup_path,
            &bootstrap_dir,
        )?;

        assert!(target_dir.exists());
        assert!(!marker_path.exists());
        assert!(!target_dir.join(STORAGE_MIGRATION_MARKER_FILE_NAME).exists());
        cleanup_temp_storage_test_dir(&root);
        Ok(())
    }

    #[test]
    fn pending_cleanup_preserves_bootstrap_config_when_default_data_dir_moves() -> AppResult<()> {
        let root = temp_storage_test_dir("storage-bootstrap-cleanup");
        let bootstrap_dir = root.join("bootstrap");
        let active_data_dir = root.join("custom-data");
        fs::create_dir_all(&bootstrap_dir)?;
        fs::create_dir_all(&active_data_dir)?;

        let config_path = bootstrap_dir.join(STORAGE_CONFIG_FILE_NAME);
        let pending_cleanup_path = bootstrap_dir.join(STORAGE_PENDING_CLEANUP_FILE_NAME);
        let marker_path = bootstrap_dir.join(STORAGE_MIGRATION_MARKER_FILE_NAME);
        write_bootstrap_config(
            &config_path,
            &StorageBootstrapConfig {
                data_dir: Some(path_to_string(&active_data_dir)),
            },
        )?;
        write_pending_cleanup_config(
            &pending_cleanup_path,
            &PendingCleanupConfig {
                old_data_dir: Some(path_to_string(&bootstrap_dir)),
            },
        )?;
        fs::write(&marker_path, b"marker")?;
        fs::write(bootstrap_dir.join(DB_FILE_NAME), b"db")?;
        fs::write(bootstrap_dir.join("totoken.db-wal"), b"wal")?;
        fs::create_dir_all(bootstrap_dir.join("cache"))?;
        fs::write(bootstrap_dir.join("cache").join("entry"), b"cache")?;

        cleanup_pending_data_dir(&pending_cleanup_path, &active_data_dir, &bootstrap_dir)?;

        assert!(bootstrap_dir.exists());
        assert!(config_path.exists());
        assert!(marker_path.exists());
        assert!(!pending_cleanup_path.exists());
        assert!(!bootstrap_dir.join(DB_FILE_NAME).exists());
        assert!(!bootstrap_dir.join("totoken.db-wal").exists());
        assert!(!bootstrap_dir.join("cache").exists());

        cleanup_temp_storage_test_dir(&root);
        Ok(())
    }

    fn temp_storage_test_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()))
    }

    fn cleanup_temp_storage_test_dir(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }
}
