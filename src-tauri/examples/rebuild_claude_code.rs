use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use app_lib::db::init_db_with_path;
use app_lib::pricing::CostEstimationPolicy;
use app_lib::scanner::{ScanRequest, ScanSummary, Scanner};
use app_lib::source_settings::SourceSettingsState;
use chrono::{Local, Utc};
use rusqlite::params;

const CLAUDE_SOURCE_APP: &str = "claude_code";

#[derive(Clone)]
struct ScanTarget {
    path: PathBuf,
}

#[derive(Clone)]
struct RebuildTarget {
    session_id: String,
    source_path: PathBuf,
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
        return Err("no enabled claude_code scan targets configured".into());
    }

    let backup_path = create_sqlite_backup(&pool, &bootstrap_dir)?;
    println!("backup created: {}", backup_path.display());

    let scanner = Scanner::new(pool.clone());
    let rebuild_targets = load_claude_rebuild_targets(&pool)?;
    println!(
        "rebuilding {} existing Claude sessions",
        rebuild_targets.len()
    );

    let mut rebuilt_sessions = 0_u64;
    let mut skipped_missing_sessions = 0_u64;
    let mut rebuild_errors = 0_u64;
    for target in &rebuild_targets {
        if !target.source_path.exists() {
            skipped_missing_sessions += 1;
            eprintln!(
                "skip missing source session: {} {}",
                target.session_id,
                target.source_path.display()
            );
            continue;
        }

        match scanner
            .ensure_session_message_index(&target.session_id, CostEstimationPolicy::default())
        {
            Ok(true) => {
                rebuilt_sessions += 1;
            }
            Ok(false) => {}
            Err(error) => {
                rebuild_errors += 1;
                eprintln!(
                    "rebuild failed for session {}: {}",
                    target.session_id, error
                );
            }
        }
    }

    let sanitized = sanitize_stale_placeholder_models(&pool)?;
    if sanitized > 0 {
        println!("sanitized {} stale placeholder model values", sanitized);
    }

    if rebuild_errors > 0 {
        println!(
            "rebuild stage completed with {} session errors and {} missing-source skips",
            rebuild_errors, skipped_missing_sessions
        );
    } else if skipped_missing_sessions > 0 {
        println!(
            "rebuild stage completed with {} missing-source skips",
            skipped_missing_sessions
        );
    }

    let run_id = scanner.create_scan_run("manual")?;
    let mut summary = ScanSummary {
        trigger_type: "manual".to_string(),
        root_path: "claude_code targeted rebuild".to_string(),
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

            println!("scan {} {}", CLAUDE_SOURCE_APP, target.path.display());
            let target_summary = scanner.scan(
                ScanRequest {
                    root_path: target.path.clone(),
                    source_app: CLAUDE_SOURCE_APP.to_string(),
                    trigger_type: "manual".to_string(),
                    create_run: false,
                },
                CostEstimationPolicy::default(),
            )?;
            merge_summary(&mut summary, target_summary);
        }
        Ok(())
    })();

    match scan_result {
        Ok(()) => {
            scanner.complete_scan_run(&run_id, &summary)?;
            println!(
                "claude rebuild complete: rebuilt_sessions={}, missing_source_skips={}, rebuild_errors={}, seen={}, parsed={}, skipped={}, failed={}, sessions_changed={}, errors={}",
                rebuilt_sessions,
                skipped_missing_sessions,
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
        .filter(|item| item.enabled && item.app == CLAUDE_SOURCE_APP)
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
        "totoken-claude-rebuild-{}.db",
        Local::now().format("%Y%m%d-%H%M%S")
    );
    let backup_path = backup_dir.join(backup_name);
    let backup_sql_path = backup_path.to_string_lossy().replace('\'', "''");

    let conn = pool.get()?;
    conn.query_row("PRAGMA wal_checkpoint(FULL)", [], |_| Ok(()))?;
    conn.execute_batch(&format!("VACUUM INTO '{}'", backup_sql_path))?;

    Ok(backup_path)
}

fn load_claude_rebuild_targets(
    pool: &app_lib::db::DbPool,
) -> Result<Vec<RebuildTarget>, Box<dyn Error>> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT
            s.id,
            ref.source_path
         FROM sessions s
         INNER JOIN session_source_refs ref ON ref.session_id = s.id
         WHERE s.source_app = ?1
           AND ref.last_linked_at = (
                SELECT MAX(ref2.last_linked_at)
                FROM session_source_refs ref2
                WHERE ref2.session_id = s.id
           )
         ORDER BY s.discovered_last_at DESC, s.id DESC",
    )?;
    let rows = stmt.query_map(params![CLAUDE_SOURCE_APP], |row| {
        Ok(RebuildTarget {
            session_id: row.get(0)?,
            source_path: PathBuf::from(row.get::<_, String>(1)?),
        })
    })?;

    let mut targets = Vec::new();
    for row in rows {
        targets.push(row?);
    }
    Ok(targets)
}

fn merge_summary(summary: &mut ScanSummary, next: ScanSummary) {
    summary.files_seen += next.files_seen;
    summary.files_parsed += next.files_parsed;
    summary.files_skipped += next.files_skipped;
    summary.files_failed += next.files_failed;
    summary.sessions_changed += next.sessions_changed;
    summary.error_count += next.error_count;
}

fn sanitize_stale_placeholder_models(pool: &app_lib::db::DbPool) -> Result<usize, Box<dyn Error>> {
    let conn = pool.get()?;
    let tx = conn.unchecked_transaction()?;

    let mut sanitized = 0_usize;
    sanitized += tx.execute(
        "UPDATE sessions
         SET model_first = NULL
         WHERE source_app = ?1 AND model_first = '<synthetic>'",
        params![CLAUDE_SOURCE_APP],
    )?;
    sanitized += tx.execute(
        "UPDATE sessions
         SET model_last = NULL
         WHERE source_app = ?1 AND model_last = '<synthetic>'",
        params![CLAUDE_SOURCE_APP],
    )?;
    sanitized += tx.execute(
        "UPDATE session_requests
         SET model = NULL
         WHERE source_app = ?1 AND model = '<synthetic>'",
        params![CLAUDE_SOURCE_APP],
    )?;
    sanitized += tx.execute(
        "UPDATE token_usage_events
         SET model = NULL
         WHERE source_app = ?1 AND model = '<synthetic>'",
        params![CLAUDE_SOURCE_APP],
    )?;
    sanitized += tx.execute(
        "UPDATE session_observations
         SET source_model = NULL
         WHERE session_id IN (
             SELECT id FROM sessions WHERE source_app = ?1
         )
           AND source_model = '<synthetic>'",
        params![CLAUDE_SOURCE_APP],
    )?;

    tx.commit()?;
    Ok(sanitized)
}
