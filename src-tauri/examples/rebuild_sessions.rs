use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use app_lib::db::init_db_with_path;
use app_lib::scanner::{ScanRequest, ScanSummary, Scanner};
use app_lib::source_settings::SourceSettingsState;
use chrono::Utc;

#[derive(Clone)]
struct ScanTarget {
    app: String,
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
        return Err("no enabled scan targets configured".into());
    }

    clear_session_data(&pool)?;

    let scanner = Scanner::new(pool.clone());
    let run_id = scanner.create_scan_run("manual")?;
    let mut summary = ScanSummary {
        trigger_type: "manual".to_string(),
        root_path: "rebuild_sessions".to_string(),
        started_at: Utc::now(),
        files_seen: 0,
        files_parsed: 0,
        files_skipped: 0,
        files_failed: 0,
        sessions_changed: 0,
        error_count: 0,
    };

    let scan_result = (|| -> Result<(), Box<dyn Error>> {
        for target in targets {
            if !target.path.exists() {
                eprintln!(
                    "skip missing target: {} {}",
                    target.app,
                    target.path.display()
                );
                continue;
            }

            println!("scan {} {}", target.app, target.path.display());
            let target_summary = scanner.scan(ScanRequest {
                root_path: target.path.clone(),
                source_app: target.app.clone(),
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
                "rebuild complete: seen={}, parsed={}, skipped={}, failed={}, sessions_changed={}, errors={}",
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
    let mut targets = Vec::new();

    for item in &settings.items {
        if !item.enabled {
            continue;
        }
        if item.app != "claude_code"
            && item.app != "codex"
            && item.app != "cursor"
            && item.app != "opencode"
            && item.app != "kilocode"
            && item.app != "kiro"
        {
            continue;
        }

        for scan_path in &item.scan_paths {
            targets.push(ScanTarget {
                app: item.app.clone(),
                path: PathBuf::from(scan_path),
            });
        }
    }

    targets
}

fn clear_session_data(pool: &app_lib::db::DbPool) -> Result<(), Box<dyn Error>> {
    let mut conn = pool.get()?;
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM scan_runs", [])?;
    tx.execute("DELETE FROM source_files_cache", [])?;
    tx.execute("DELETE FROM session_requests", [])?;
    tx.execute("DELETE FROM token_usage_events", [])?;
    tx.execute("DELETE FROM session_observations", [])?;
    tx.execute("DELETE FROM session_token_totals", [])?;
    tx.execute("DELETE FROM session_source_refs", [])?;
    tx.execute("DELETE FROM sessions", [])?;
    tx.commit()?;
    Ok(())
}

fn merge_summary(summary: &mut ScanSummary, next: ScanSummary) {
    summary.files_seen += next.files_seen;
    summary.files_parsed += next.files_parsed;
    summary.files_skipped += next.files_skipped;
    summary.files_failed += next.files_failed;
    summary.sessions_changed += next.sessions_changed;
    summary.error_count += next.error_count;
}
