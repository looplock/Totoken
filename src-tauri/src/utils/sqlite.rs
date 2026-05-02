use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::error::AppResult;

const SNAPSHOT_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct SqliteSnapshot {
    conn: Option<Connection>,
    path: PathBuf,
}

impl SqliteSnapshot {
    pub fn open(source_path: &Path, label: &str) -> AppResult<Self> {
        let source = Connection::open_with_flags(
            source_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;
        source.busy_timeout(SNAPSHOT_BUSY_TIMEOUT)?;

        let path = std::env::temp_dir().join(format!(
            "totoken-{label}-snapshot-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let mut destination = Connection::open(&path)?;
        destination.busy_timeout(SNAPSHOT_BUSY_TIMEOUT)?;

        {
            let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
            backup.run_to_completion(256, Duration::from_millis(10), None)?;
        }

        drop(destination);
        drop(source);

        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;
        conn.busy_timeout(SNAPSHOT_BUSY_TIMEOUT)?;

        Ok(Self {
            conn: Some(conn),
            path,
        })
    }

    pub fn connection(&self) -> &Connection {
        self.conn
            .as_ref()
            .expect("sqlite snapshot connection should be open")
    }
}

impl Drop for SqliteSnapshot {
    fn drop(&mut self) {
        let _ = self.conn.take();
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(sqlite_sidecar_path(&self.path, "-wal"));
        let _ = std::fs::remove_file(sqlite_sidecar_path(&self.path, "-shm"));
    }
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{}", path.to_string_lossy(), suffix))
}

#[cfg(test)]
mod tests {
    use super::SqliteSnapshot;

    #[test]
    fn snapshot_reads_copied_database() {
        let path = std::env::temp_dir().join(format!(
            "totoken-sqlite-snapshot-test-{}.db",
            crate::utils::ids::new_uuid()
        ));
        let conn = rusqlite::Connection::open(&path).expect("open source db");
        conn.execute_batch(
            "CREATE TABLE items (value TEXT NOT NULL);
             INSERT INTO items (value) VALUES ('copied');",
        )
        .expect("seed source db");
        drop(conn);

        let snapshot = SqliteSnapshot::open(&path, "test").expect("open snapshot");
        let value: String = snapshot
            .connection()
            .query_row("SELECT value FROM items LIMIT 1", [], |row| row.get(0))
            .expect("read snapshot");

        assert_eq!(value, "copied");
        let _ = std::fs::remove_file(path);
    }
}
