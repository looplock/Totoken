use std::path::Path;

use crate::error::AppResult;
use crate::utils::{fs, hash};

#[derive(Debug, Clone)]
pub struct FileFingerprint {
    pub size_bytes: i64,
    pub mtime_ms: i64,
    pub fast: String,
    pub strong: String,
}

#[derive(Debug, Clone)]
pub struct FastFileFingerprint {
    pub size_bytes: i64,
    pub mtime_ms: i64,
    pub fast: String,
}

pub fn build_fast_fingerprint(size_bytes: i64, mtime_ms: i64) -> String {
    format!("{size_bytes}:{mtime_ms}")
}

pub fn fingerprint_file_fast(path: &Path) -> AppResult<FastFileFingerprint> {
    let metadata = std::fs::metadata(path)?;
    let size_bytes = metadata.len() as i64;
    let mtime_ms = fs::metadata_mtime_ms(&metadata)?;

    Ok(FastFileFingerprint {
        size_bytes,
        mtime_ms,
        fast: build_fast_fingerprint(size_bytes, mtime_ms),
    })
}

pub fn fingerprint_file(path: &Path) -> AppResult<FileFingerprint> {
    let fast = fingerprint_file_fast(path)?;
    let content = std::fs::read(path)?;

    Ok(FileFingerprint {
        size_bytes: fast.size_bytes,
        mtime_ms: fast.mtime_ms,
        fast: fast.fast,
        strong: hash::sha256_bytes(&content),
    })
}

pub fn fingerprint_files_fast(paths: &[std::path::PathBuf]) -> AppResult<FastFileFingerprint> {
    let mut total_size_bytes = 0_i64;
    let mut latest_mtime_ms = 0_i64;
    let mut fast_parts = Vec::with_capacity(paths.len());

    for path in paths {
        let fingerprint = fingerprint_file_fast(path)?;
        total_size_bytes += fingerprint.size_bytes;
        latest_mtime_ms = latest_mtime_ms.max(fingerprint.mtime_ms);
        fast_parts.push(format!("{}:{}", path.to_string_lossy(), fingerprint.fast));
    }

    Ok(FastFileFingerprint {
        size_bytes: total_size_bytes,
        mtime_ms: latest_mtime_ms,
        fast: hash::sha256_text(&fast_parts.join("\n")),
    })
}

pub fn fingerprint_files(paths: &[std::path::PathBuf]) -> AppResult<FileFingerprint> {
    let fast = fingerprint_files_fast(paths)?;
    let mut strong_parts = Vec::with_capacity(paths.len());

    for path in paths {
        let fingerprint = fingerprint_file(path)?;
        strong_parts.push(format!("{}:{}", path.to_string_lossy(), fingerprint.strong));
    }

    Ok(FileFingerprint {
        size_bytes: fast.size_bytes,
        mtime_ms: fast.mtime_ms,
        fast: fast.fast,
        strong: hash::sha256_text(&strong_parts.join("\n")),
    })
}
