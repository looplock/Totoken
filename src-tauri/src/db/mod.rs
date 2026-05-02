use std::path::Path;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;

use crate::config::{SCAN_RUN_RETENTION_LIMIT, SCAN_RUN_STALE_TIMEOUT_MINUTES};
use crate::error::AppResult;
use crate::storage::StoragePaths;

pub mod repo;

pub type DbPool = Pool<SqliteConnectionManager>;

pub fn init_db(storage_paths: &StoragePaths) -> AppResult<DbPool> {
    init_db_with_path(storage_paths.db_path())
}

pub fn init_db_with_path(db_path: &Path) -> AppResult<DbPool> {
    let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            ",
        )
    });
    let pool = Pool::new(manager)?;

    let mut conn = pool.get()?;
    run_migrations(&mut conn)?;
    recover_interrupted_scan_runs(&conn)?;
    cleanup_scan_run_history(&conn)?;

    Ok(pool)
}

#[derive(Clone, Copy)]
struct Migration {
    version: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: "0001_init",
        sql: include_str!("migrations/0001_init.sql"),
    },
    Migration {
        version: "0002_session_list_indexes",
        sql: include_str!("migrations/0002_session_list_indexes.sql"),
    },
];

fn run_migrations(conn: &mut rusqlite::Connection) -> AppResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version TEXT PRIMARY KEY,
            applied_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        ",
    )?;

    for migration in MIGRATIONS {
        let already_applied = conn.query_row(
            "SELECT 1 FROM schema_migrations WHERE version = ?1 LIMIT 1",
            params![migration.version],
            |_| Ok(()),
        );

        if already_applied.is_ok() {
            continue;
        }

        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        mark_migration_applied(&tx, migration.version)?;
        tx.commit()?;
    }

    Ok(())
}

fn mark_migration_applied(conn: &rusqlite::Connection, version: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO schema_migrations (version) VALUES (?1)",
        params![version],
    )?;
    Ok(())
}

fn recover_interrupted_scan_runs(conn: &rusqlite::Connection) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM scan_runs
         WHERE status = 'running'
           AND COALESCE(files_seen, 0) = 0
           AND COALESCE(files_parsed, 0) = 0
           AND COALESCE(files_skipped, 0) = 0
           AND COALESCE(files_failed, 0) = 0
           AND COALESCE(sessions_changed, 0) = 0
           AND COALESCE(error_count, 0) = 0",
        [],
    )?;
    tx.execute(
        "UPDATE scan_runs
         SET status = 'failed',
             ended_at = COALESCE(ended_at, started_at),
             error_count = CASE
                 WHEN COALESCE(error_count, 0) > 0 THEN error_count
                 ELSE 1
             END
         WHERE status = 'running'",
        [],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn cleanup_scan_run_history(conn: &rusqlite::Connection) -> AppResult<()> {
    conn.execute(
        "DELETE FROM scan_runs
         WHERE status = 'failed'
           AND COALESCE(files_seen, 0) = 0
           AND COALESCE(files_parsed, 0) = 0
           AND COALESCE(files_skipped, 0) = 0
           AND COALESCE(files_failed, 0) = 0
           AND COALESCE(sessions_changed, 0) = 0
           AND COALESCE(error_count, 0) = 0",
        [],
    )?;
    conn.execute(
        "DELETE FROM scan_runs
         WHERE trigger_type = 'auto'
           AND status = 'completed'
           AND COALESCE(files_parsed, 0) = 0
           AND COALESCE(files_failed, 0) = 0
           AND COALESCE(sessions_changed, 0) = 0
           AND COALESCE(error_count, 0) = 0",
        [],
    )?;
    conn.execute(
        "UPDATE scan_runs
         SET status = 'failed',
             ended_at = COALESCE(ended_at, started_at),
             error_count = CASE
                 WHEN COALESCE(error_count, 0) > 0 THEN error_count
                 ELSE 1
             END
         WHERE status = 'running'
           AND started_at < DATETIME('now', ?1)",
        params![format!("-{} minutes", SCAN_RUN_STALE_TIMEOUT_MINUTES)],
    )?;
    conn.execute(
        "DELETE FROM scan_runs
         WHERE status != 'running'
           AND id IN (
               SELECT id
               FROM scan_runs
               WHERE status != 'running'
               ORDER BY started_at DESC, id DESC
               LIMIT -1 OFFSET ?1
           )",
        params![SCAN_RUN_RETENTION_LIMIT],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use chrono::Utc;
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    #[test]
    fn cleanup_scan_run_history_caps_non_running_rows_to_recent_limit() -> AppResult<()> {
        let db_path = temp_db_path("scan-run-history-retention");
        let pool = init_db_with_path(&db_path)?;
        let conn = pool.get()?;

        for index in 0..60 {
            let started_at = Utc::now() - Duration::minutes(index);
            conn.execute(
                "INSERT INTO scan_runs (
                    id, trigger_type, started_at, ended_at, status, files_seen, files_parsed
                 ) VALUES (?1, 'manual', ?2, ?2, 'completed', 1, 1)",
                params![format!("recent-run-{index:02}"), started_at],
            )?;
        }
        conn.execute(
            "INSERT INTO scan_runs (
                id, trigger_type, started_at, status, files_seen, files_parsed
             ) VALUES (?1, 'manual', ?2, 'running', 1, 1)",
            params!["active-running", Utc::now()],
        )?;

        cleanup_scan_run_history(&conn)?;

        let kept_count: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM scan_runs
             WHERE id LIKE 'recent-run-%'",
            [],
            |row| row.get(0),
        )?;
        let running_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM scan_runs WHERE id = 'active-running'",
            [],
            |row| row.get(0),
        )?;
        let newest_kept: i64 = conn.query_row(
            "SELECT COUNT(*) FROM scan_runs WHERE id = 'recent-run-00'",
            [],
            |row| row.get(0),
        )?;
        let cutoff_kept: i64 = conn.query_row(
            "SELECT COUNT(*) FROM scan_runs WHERE id = 'recent-run-49'",
            [],
            |row| row.get(0),
        )?;
        let oldest_trimmed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM scan_runs WHERE id = 'recent-run-59'",
            [],
            |row| row.get(0),
        )?;

        assert_eq!(kept_count, SCAN_RUN_RETENTION_LIMIT);
        assert_eq!(running_count, 1);
        assert_eq!(newest_kept, 1);
        assert_eq!(cutoff_kept, 1);
        assert_eq!(oldest_trimmed, 0);

        drop(conn);
        drop(pool);
        cleanup_temp_db(&db_path);

        Ok(())
    }

    #[test]
    fn recover_interrupted_scan_runs_drops_empty_running_rows_and_fails_the_rest() -> AppResult<()>
    {
        let db_path = temp_db_path("scan-run-recovery");
        let pool = init_db_with_path(&db_path)?;
        let conn = pool.get()?;
        let started_at = Utc::now() - Duration::minutes(5);

        conn.execute(
            "INSERT INTO scan_runs (
                id, trigger_type, started_at, status, files_seen, files_parsed, error_count
             ) VALUES (?1, 'manual', ?2, 'running', 0, 0, 0)",
            params!["empty-running", started_at],
        )?;
        conn.execute(
            "INSERT INTO scan_runs (
                id, trigger_type, started_at, status, files_seen, files_parsed, error_count
             ) VALUES (?1, 'manual', ?2, 'running', 3, 1, 0)",
            params!["active-running", started_at],
        )?;

        recover_interrupted_scan_runs(&conn)?;

        let empty_exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM scan_runs WHERE id = 'empty-running'",
            [],
            |row| row.get(0),
        )?;
        let recovered_row: (String, i64, Option<chrono::DateTime<chrono::Utc>>) = conn.query_row(
            "SELECT status, error_count, ended_at
             FROM scan_runs
             WHERE id = 'active-running'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

        assert_eq!(empty_exists, 0);
        assert_eq!(recovered_row.0, "failed");
        assert_eq!(recovered_row.1, 1);
        assert!(recovered_row.2.is_some());

        drop(conn);
        drop(pool);
        cleanup_temp_db(&db_path);

        Ok(())
    }

    fn temp_db_path(prefix: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("{prefix}-{}.db", Uuid::new_v4()));
        path
    }

    fn cleanup_temp_db(db_path: &Path) {
        let _ = fs::remove_file(db_path);
        let wal = PathBuf::from(format!("{}-wal", db_path.display()));
        let shm = PathBuf::from(format!("{}-shm", db_path.display()));
        let _ = fs::remove_file(wal);
        let _ = fs::remove_file(shm);
    }
}
