use std::path::PathBuf;

use chrono::Utc;
use tauri::State;

use super::{ensure_storage_runtime_current, run_blocking};
use crate::error::{AppError, AppResult};
use crate::events;
use crate::pricing::CostEstimationPolicy;
use crate::scanner::{ScanRequest, ScanSummary, Scanner};
use crate::settings;
use crate::source_settings;
use crate::state::AppState;

#[tauri::command]
pub async fn start_scan(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
    source_app: Option<String>,
) -> AppResult<()> {
    ensure_storage_runtime_current(&state)?;
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation("scan path cannot be empty"));
    }
    let scan_path = PathBuf::from(trimmed);
    if !scan_path.is_absolute() {
        return Err(AppError::validation(
            "scan path must be an absolute directory path",
        ));
    }
    if scan_path
        .metadata()
        .map(|metadata| !metadata.is_dir())
        .unwrap_or(true)
    {
        return Err(AppError::validation(
            "scan path must point to an existing directory",
        ));
    }

    let requested_source_app = source_app
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::validation("scan source app is required"))?;

    let source_settings_store = state.source_settings_store();
    let cost_estimation_policy = settings::get_cost_estimation_policy(&app)?;
    let started_root_path =
        format_configured_source_targets(&app, &source_settings_store, &requested_source_app)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| trimmed.to_string());

    let state = state.inner().clone();
    let app_for_scan = app.clone();
    let trimmed = trimmed.to_string();
    let requested_source_app_for_scan = requested_source_app.clone();
    let started_root_path_for_scan = started_root_path.clone();
    let scan_result = run_blocking(move || {
        let _scan_guard = state.try_acquire_scan_lock()?;
        let scanner = state.scanner();
        if !configured_source_targets_have_scannable_data(
            &app_for_scan,
            &scanner,
            &state.source_settings_store(),
            &requested_source_app_for_scan,
            &trimmed,
        )? {
            return Ok(None);
        }

        let scan_started_at = Utc::now();
        state.mark_scan_started("manual", &started_root_path_for_scan);
        events::emit_scan_started(
            &app_for_scan,
            "manual",
            Some(&requested_source_app_for_scan),
            &started_root_path_for_scan,
        );
        let result = scan_configured_source_targets(
            &app_for_scan,
            &scanner,
            &state.source_settings_store(),
            &requested_source_app_for_scan,
            &trimmed,
            cost_estimation_policy,
        );

        match result {
            Ok(summary) => {
                let summary = ScanSummary {
                    started_at: scan_started_at,
                    ..summary
                };
                state.mark_scan_completed(&summary, None, None, None);
                events::emit_scan_completed(
                    &app_for_scan,
                    Some(&requested_source_app_for_scan),
                    &summary,
                );
                Ok(Some(summary))
            }
            Err(error) => {
                state.mark_scan_failed(
                    "manual",
                    &started_root_path_for_scan,
                    error.to_string(),
                    None,
                );
                events::emit_scan_failed(
                    &app_for_scan,
                    "manual",
                    Some(&requested_source_app_for_scan),
                    &started_root_path_for_scan,
                    scan_started_at,
                    &error.to_string(),
                );
                Err(error)
            }
        }
    })
    .await;
    match scan_result {
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    }
}

fn configured_source_targets_have_scannable_data(
    app: &tauri::AppHandle,
    scanner: &Scanner,
    source_settings_store: &source_settings::SourceSettingsStore,
    source_app: &str,
    fallback_path: &str,
) -> AppResult<bool> {
    let configured_targets =
        source_settings::list_scannable_sources_for_app(app, source_settings_store, source_app)?;
    if configured_targets.is_empty() {
        return scanner.has_scannable_data(PathBuf::from(fallback_path), source_app);
    }

    for target in configured_targets {
        if !target.path.exists() {
            continue;
        }
        if scanner.has_scannable_data(target.path, source_app)? {
            return Ok(true);
        }
    }

    Ok(false)
}

fn scan_configured_source_targets(
    app: &tauri::AppHandle,
    scanner: &Scanner,
    source_settings_store: &source_settings::SourceSettingsStore,
    source_app: &str,
    fallback_path: &str,
    cost_estimation_policy: CostEstimationPolicy,
) -> AppResult<ScanSummary> {
    let configured_targets =
        source_settings::list_scannable_sources_for_app(app, source_settings_store, source_app)?;
    if configured_targets.is_empty() {
        return scanner.scan(
            ScanRequest {
                root_path: PathBuf::from(fallback_path),
                source_app: source_app.to_string(),
                trigger_type: "manual".to_string(),
                create_run: true,
            },
            cost_estimation_policy,
        );
    }

    let run_id = scanner.create_scan_run("manual")?;
    let root_path = configured_targets
        .iter()
        .map(|target| format!("{}: {}", target.app, target.path.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" | ");
    let mut summary = ScanSummary {
        trigger_type: "manual".to_string(),
        root_path,
        started_at: Utc::now(),
        files_seen: 0,
        files_parsed: 0,
        files_skipped: 0,
        files_failed: 0,
        sessions_changed: 0,
        error_count: 0,
    };

    let result = (|| -> AppResult<()> {
        for target in configured_targets {
            if !target.path.exists() {
                continue;
            }

            let target_summary = scanner.scan(
                ScanRequest {
                    root_path: target.path,
                    source_app: source_app.to_string(),
                    trigger_type: "manual".to_string(),
                    create_run: false,
                },
                cost_estimation_policy,
            )?;
            summary.files_seen += target_summary.files_seen;
            summary.files_parsed += target_summary.files_parsed;
            summary.files_skipped += target_summary.files_skipped;
            summary.files_failed += target_summary.files_failed;
            summary.sessions_changed += target_summary.sessions_changed;
            summary.error_count += target_summary.error_count;
        }

        Ok(())
    })();

    match result {
        Ok(()) => {
            scanner.complete_scan_run(&run_id, &summary)?;
            Ok(summary)
        }
        Err(error) => {
            let _ = scanner.fail_scan_run(&run_id, &summary);
            Err(error)
        }
    }
}

fn format_configured_source_targets(
    app: &tauri::AppHandle,
    source_settings_store: &source_settings::SourceSettingsStore,
    source_app: &str,
) -> AppResult<String> {
    let configured_targets =
        source_settings::list_scannable_sources_for_app(app, source_settings_store, source_app)?;
    Ok(configured_targets
        .iter()
        .map(|target| format!("{}: {}", target.app, target.path.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" | "))
}
