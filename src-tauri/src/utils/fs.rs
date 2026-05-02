use std::fs::Metadata;
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::error::AppResult;

pub fn canonicalize_to_string(path: &Path) -> AppResult<String> {
    Ok(std::fs::canonicalize(path)?.to_string_lossy().to_string())
}

pub fn metadata_mtime_ms(metadata: &Metadata) -> AppResult<i64> {
    Ok(metadata.modified()?.duration_since(UNIX_EPOCH)?.as_millis() as i64)
}
