use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use app_lib::db::init_db_with_path;
use app_lib::scanner::{ScanRequest, ScanSummary, Scanner};
use app_lib::source_settings::SourceSettingsState;
use chrono::{Local, Utc};
use rusqlite::params;

const OPENCODE_SOURCE_APP: &str = "opencode";

#[derive(Clone)]
struct ScanTarget {
    path: PathBuf,
}

fn main() -> Result<(), Box<dyn Error>> {
    let home_dir = resolve_home_dir()?;
    let bootstrap_dir = home_dir.join(".totoken");
    let db_path = bootstrap_dir.join("totoken.db");
    let sources_path = bootstrap_dir.join("config").join("sources.json");

    let pool = init_db_with_path(&db_path)?;
    let settings = load_source_settings(&sources_path)?;
    let targets = build_targets(&settings);
    if targets.is_empty() {
        return Err("no enabled opencode scan targets configured".into());
    }

    let backup_path = create_sqlite_backup(&pool, &bootstrap_dir)?;
    println!("backup created: {}", backup_path.display());

    let scanner = Scanner::new(pool.clone());
    let session_ids = load_source_session_ids(&pool, OPENCODE_SOURCE_APP)?;
    println!(
        "rebuilding {} existing OpenCode sessions",
        session_ids.len()
    );

    let mut rebuilt_sessions = 0_u64;
    let mut rebuild_errors = 0_u64;
    for session_id in &session_ids {
        match scanner.ensure_session_message_index(session_id) {
            Ok(true) => {
                rebuilt_sessions += 1;
            }
            Ok(false) => {}
            Err(error) => {
                rebuild_errors += 1;
                eprintln!("rebuild failed for session {}: {}", session_id, error);
            }
        }
    }

    let run_id = scanner.create_scan_run("manual")?;
    let mut summary = ScanSummary {
        trigger_type: "manual".to_string(),
        root_path: "opencode targeted rebuild".to_string(),
        started_at: Utc::now(),
        files_seen: 0,
        files_parsed: 0,
        files_skipped: 0,
        files_failed: 0,
        sessions_changed: rebuilt_sessions,
        error_count: rebuild_errors,
    };

    let scan_result = (|| -> Result<(), Box<dyn Error>> {
        for target in targets {
            if !target.path.exists() {
                eprintln!("skip missing target: {}", target.path.display());
                continue;
            }

            println!("scan {} {}", OPENCODE_SOURCE_APP, target.path.display());
            let target_summary = scanner.scan(ScanRequest {
                root_path: target.path.clone(),
                source_app: OPENCODE_SOURCE_APP.to_string(),
                trigger_type: "manual".to_string(),
                create_run: false,
            })?;
            merge_summary(&mut summary, target_summary);
        }
        Ok(())
    })();

    match scan_result {
        Ok(()) => {
            scanner.complete_scan_run(&run_id, &summary)?;
            println!(
                "opencode rebuild complete: rebuilt_sessions={}, rebuild_errors={}, seen={}, parsed={}, skipped={}, failed={}, sessions_changed={}, errors={}",
                rebuilt_sessions,
                rebuild_errors,
                summary.files_seen,
                summary.files_parsed,
                summary.files_skipped,
                summary.files_failed,
                summary.sessions_changed,
                summary.error_count
            );
            Ok(())
        }
        Err(error) => {
            let _ = scanner.fail_scan_run(&run_id, &summary);
            Err(error)
        }
    }
}

fn resolve_home_dir() -> Result<PathBuf, Box<dyn Error>> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| "failed to resolve home directory".into())
}

fn load_source_settings(path: &Path) -> Result<SourceSettingsState, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

fn build_targets(settings: &SourceSettingsState) -> Vec<ScanTarget> {
    settings
        .items
        .iter()
        .filter(|item| item.enabled && item.app == OPENCODE_SOURCE_APP)
        .flat_map(|item| item.scan_paths.iter())
        .map(|scan_path| ScanTarget {
            path: PathBuf::from(scan_path),
        })
        .collect()
}

fn create_sqlite_backup(
    pool: &app_lib::db::DbPool,
    bootstrap_dir: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    let backup_dir = bootstrap_dir.join("backups");
    fs::create_dir_all(&backup_dir)?;
    let backup_name = format!(
        "totoken-opencode-rebuild-{}.db",
        Local::now().format("%Y%m%d-%H%M%S")
    );
    let backup_path = backup_dir.join(backup_name);
    let backup_sql_path = backup_path.to_string_lossy().replace('\'', "''");

    let conn = pool.get()?;
    conn.query_row("PRAGMA wal_checkpoint(FULL)", [], |_| Ok(()))?;
    conn.execute_batch(&format!("VACUUM INTO '{}'", backup_sql_path))?;

    Ok(backup_path)
}

fn load_source_session_ids(
    pool: &app_lib::db::DbPool,
    source_app: &str,
) -> Result<Vec<String>, Box<dyn Error>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id
         FROM sessions
         WHERE source_app = ?1
         ORDER BY discovered_last_at DESC, id DESC",
    )?;
    let rows = stmt.query_map(params![source_app], |row| row.get::<_, String>(0))?;

    let mut session_ids = Vec::new();
    for row in rows {
        session_ids.push(row?);
    }
    Ok(session_ids)
}

fn merge_summary(summary: &mut ScanSummary, next: ScanSummary) {
    summary.files_seen += next.files_seen;
    summary.files_parsed += next.files_parsed;
    summary.files_skipped += next.files_skipped;
    summary.files_failed += next.files_failed;
    summary.sessions_changed += next.sessions_changed;
    summary.error_count += next.error_count;
}
