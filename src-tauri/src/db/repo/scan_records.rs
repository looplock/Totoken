use super::*;

impl Repository {
    pub fn scan_records_list(
        &self,
        query: Option<ScanRecordsListQuery>,
    ) -> AppResult<ScanRecordsListResponse> {
        let limit = query
            .and_then(|value| value.limit)
            .unwrap_or(DEFAULT_SCAN_RUN_LIMIT)
            .clamp(1, MAX_SCAN_RUN_LIMIT);
        let conn = self.pool.get()?;

        let mut stmt = conn.prepare(
            "SELECT
                r.id,
                COALESCE(r.trigger_type, 'manual'),
                COALESCE(r.status, 'unknown'),
                r.started_at,
                r.ended_at,
                r.files_seen,
                r.files_parsed,
                COALESCE(r.files_skipped, 0),
                COALESCE(r.files_failed, 0),
                r.sessions_changed,
                r.error_count
             FROM scan_runs r
             WHERE NOT (
                 COALESCE(r.status, '') = 'failed'
                 AND COALESCE(r.files_seen, 0) = 0
                 AND COALESCE(r.files_parsed, 0) = 0
                 AND COALESCE(r.files_skipped, 0) = 0
                 AND COALESCE(r.files_failed, 0) = 0
                 AND COALESCE(r.sessions_changed, 0) = 0
                 AND COALESCE(r.error_count, 0) = 0
             )
             ORDER BY r.started_at DESC, r.id DESC
             LIMIT ?1",
        )?;

        let items = stmt
            .query_map(params![limit], map_scan_run_list_item_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ScanRecordsListResponse { items })
    }
}

fn map_scan_run_list_item_row(row: &Row) -> rusqlite::Result<ScanRunListItem> {
    let started_at = row.get::<_, chrono::DateTime<chrono::Utc>>(3)?;
    let ended_at = row.get::<_, Option<chrono::DateTime<chrono::Utc>>>(4)?;
    let duration_ms = ended_at
        .map(|value| (value - started_at).num_milliseconds())
        .map(|value| value.max(0));

    Ok(ScanRunListItem {
        id: row.get(0)?,
        trigger_type: row.get(1)?,
        status: row.get(2)?,
        started_at,
        ended_at,
        duration_ms,
        files_seen: row.get(5)?,
        files_parsed: row.get(6)?,
        files_skipped: row.get(7)?,
        files_failed: row.get(8)?,
        sessions_changed: row.get(9)?,
        error_count: row.get(10)?,
    })
}
