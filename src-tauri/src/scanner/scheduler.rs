use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use log::{debug, error, info};
use serde::Serialize;
use tauri::AppHandle;

use crate::config::AUTO_SCAN_IDLE_BACKOFF_SECONDS;
use crate::error::AppResult;
use crate::events;
use crate::scanner::{ScanRequest, ScanSummary};
use crate::settings::{self, SchedulerSettings};
use crate::source_settings::{self, ScannableSourceTarget};
use crate::state::{AppState, AutoScanSignal};

const AUTO_SCAN_TRIGGER: &str = "auto";
pub const AUTO_SCAN_SOURCE_APP: &str = "auto";
const DEFAULT_RETRY_INTERVAL_SECONDS: u64 = 30;
const SCAN_MODE_AUTO: &str = "auto";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerPreviewView {
    pub series: Vec<f64>,
    pub threshold: f64,
    pub unit: String,
    pub based_on_live_telemetry: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SchedulerTelemetry {
    pub files_parsed: u64,
    pub sessions_changed: u64,
    pub consecutive_idle_runs: u64,
    pub ewma_change_rate_percent: f64,
}

#[derive(Debug, Default, Clone, Copy)]
struct SchedulerRuntimeState {
    consecutive_idle_runs: u64,
    ewma_change_rate_percent: Option<f64>,
}

enum AutoScanRunOutcome {
    Completed(ScanSummary),
    Failed {
        started_at: chrono::DateTime<Utc>,
        root_path: String,
        error_message: String,
    },
    Busy,
    Disabled,
}

impl SchedulerRuntimeState {
    fn record_scan(
        &mut self,
        settings: &SchedulerSettings,
        summary: &ScanSummary,
    ) -> SchedulerTelemetry {
        let current_change_rate_percent = if summary.files_parsed == 0 {
            0.0
        } else {
            (summary.sessions_changed as f64 / summary.files_parsed as f64) * 100.0
        };

        let previous_ewma = self
            .ewma_change_rate_percent
            .unwrap_or(current_change_rate_percent);
        let ewma_change_rate_percent = (settings.ewma_alpha * current_change_rate_percent)
            + ((1.0 - settings.ewma_alpha) * previous_ewma);

        if summary.sessions_changed == 0 {
            self.consecutive_idle_runs = self.consecutive_idle_runs.saturating_add(1);
        } else {
            self.consecutive_idle_runs = 0;
        }

        self.ewma_change_rate_percent = Some(ewma_change_rate_percent);

        SchedulerTelemetry {
            files_parsed: summary.files_parsed,
            sessions_changed: summary.sessions_changed,
            consecutive_idle_runs: self.consecutive_idle_runs,
            ewma_change_rate_percent,
        }
    }
}

pub fn start_auto_scan_worker(app: AppHandle, state: AppState) {
    let (sender, receiver) = mpsc::channel();
    state.install_auto_scan_signal_sender(sender);

    thread::spawn(move || run_auto_scan_loop(app, state, receiver));
}

pub fn compute_next_interval(settings: &SchedulerSettings, telemetry: SchedulerTelemetry) -> u64 {
    if !settings.adaptive_scanning {
        return clamp_interval(settings.base_interval, settings);
    }

    if telemetry.files_parsed == 0 {
        return settings.max_interval;
    }

    if telemetry.sessions_changed == 0 {
        let idle_interval = settings.base_interval.saturating_add(
            telemetry
                .consecutive_idle_runs
                .saturating_mul(AUTO_SCAN_IDLE_BACKOFF_SECONDS),
        );
        return clamp_interval(idle_interval, settings);
    }

    let threshold = settings.change_rate_threshold as f64;
    let ewma = telemetry.ewma_change_rate_percent.max(0.0);

    if ewma >= threshold {
        let acceleration = (ewma / threshold).clamp(1.0, 4.0);
        let accelerated = (settings.base_interval as f64 / acceleration).round() as u64;
        return clamp_interval(accelerated, settings);
    }

    let utilization = (ewma / threshold).clamp(0.0, 1.0);
    let stretched = settings.base_interval as f64
        + ((settings.max_interval - settings.base_interval) as f64 * (1.0 - utilization));

    clamp_interval(stretched.round() as u64, settings)
}

pub fn build_scheduler_preview(
    settings: &SchedulerSettings,
    raw_change_rate_history_percent: &[f64],
) -> SchedulerPreviewView {
    let based_on_live_telemetry = !raw_change_rate_history_percent.is_empty();
    let input_series = if based_on_live_telemetry {
        raw_change_rate_history_percent.to_vec()
    } else {
        fallback_change_rate_series(settings.change_rate_threshold)
    };

    SchedulerPreviewView {
        series: compute_ewma_series(&input_series, settings.ewma_alpha),
        threshold: settings.change_rate_threshold as f64,
        unit: "percent".to_string(),
        based_on_live_telemetry,
    }
}

fn run_auto_scan_loop(app: AppHandle, state: AppState, receiver: Receiver<AutoScanSignal>) {
    let mut runtime_state = SchedulerRuntimeState::default();
    let mut reschedule_without_scan = false;

    loop {
        let settings = match settings::get_settings(&app) {
            Ok(value) => value,
            Err(error) => {
                error!("Failed to load settings for auto scan: {error}");
                let wait_seconds = DEFAULT_RETRY_INTERVAL_SECONDS;
                match receiver.recv_timeout(Duration::from_secs(wait_seconds.max(1))) {
                    Ok(AutoScanSignal::RefreshNow) | Err(RecvTimeoutError::Timeout) => {}
                    Ok(AutoScanSignal::SettingsChanged) => {
                        reschedule_without_scan = true;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        debug!("Auto scan worker disconnected; shutting down");
                        break;
                    }
                }
                continue;
            }
        };

        let wait_seconds;
        if settings.scheduler.scan_mode != SCAN_MODE_AUTO {
            state.mark_auto_scan_paused();
            wait_seconds = DEFAULT_RETRY_INTERVAL_SECONDS;
            debug!(
                "Auto scan paused for {} because scan mode is manual",
                AUTO_SCAN_SOURCE_APP
            );
            match receiver.recv_timeout(Duration::from_secs(wait_seconds.max(1))) {
                Ok(AutoScanSignal::RefreshNow)
                | Ok(AutoScanSignal::SettingsChanged)
                | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    debug!("Auto scan worker disconnected; shutting down");
                    break;
                }
            }
            continue;
        }

        if reschedule_without_scan {
            reschedule_without_scan = false;
            wait_seconds = clamp_interval(settings.scheduler.base_interval, &settings.scheduler);
            state.mark_auto_scan_rescheduled(Some(wait_seconds));
            debug!(
                "Auto scan settings changed for {}; rescheduled next run in {}s",
                AUTO_SCAN_SOURCE_APP, wait_seconds
            );
            match receiver.recv_timeout(Duration::from_secs(wait_seconds.max(1))) {
                Ok(AutoScanSignal::RefreshNow) => {
                    debug!(
                        "Received auto scan refresh signal for {}; running immediately",
                        AUTO_SCAN_SOURCE_APP
                    );
                }
                Ok(AutoScanSignal::SettingsChanged) => {
                    reschedule_without_scan = true;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    debug!("Auto scan worker disconnected; shutting down");
                    break;
                }
            }
            continue;
        }

        match run_auto_scan_once(&app, &state) {
            Ok(AutoScanRunOutcome::Completed(summary)) => {
                let telemetry = runtime_state.record_scan(&settings.scheduler, &summary);
                wait_seconds = compute_next_interval(&settings.scheduler, telemetry);
                state.mark_scan_completed(
                    &summary,
                    Some(telemetry.ewma_change_rate_percent),
                    Some(telemetry.consecutive_idle_runs),
                    Some(wait_seconds),
                );
                events::emit_scan_completed(&app, Some(AUTO_SCAN_SOURCE_APP), &summary);
                info!(
                    "Auto scan finished for {}: files_parsed={}, sessions_changed={}, next_run_in={}s",
                    AUTO_SCAN_SOURCE_APP,
                    summary.files_parsed,
                    summary.sessions_changed,
                    wait_seconds
                );
            }
            Ok(AutoScanRunOutcome::Failed {
                started_at,
                root_path,
                error_message,
            }) => {
                error!(
                    "Auto scan failed for {}: {}",
                    AUTO_SCAN_SOURCE_APP, error_message
                );
                wait_seconds =
                    clamp_interval(settings.scheduler.base_interval, &settings.scheduler);
                state.mark_scan_failed(
                    AUTO_SCAN_TRIGGER,
                    &root_path,
                    error_message.clone(),
                    Some(wait_seconds),
                );
                events::emit_scan_failed(
                    &app,
                    AUTO_SCAN_TRIGGER,
                    Some(AUTO_SCAN_SOURCE_APP),
                    &root_path,
                    started_at,
                    &error_message,
                );
            }
            Ok(AutoScanRunOutcome::Busy) => {
                wait_seconds =
                    clamp_interval(settings.scheduler.base_interval, &settings.scheduler);
                debug!(
                    "Auto scan skipped for {} because another scan is already running; retry in {}s",
                    AUTO_SCAN_SOURCE_APP,
                    wait_seconds
                );
            }
            Ok(AutoScanRunOutcome::Disabled) => {
                wait_seconds =
                    clamp_interval(settings.scheduler.base_interval, &settings.scheduler);
                debug!(
                    "Auto scan skipped for {} because no enabled scannable source is configured; retry in {}s",
                    AUTO_SCAN_SOURCE_APP, wait_seconds
                );
            }
            Err(error) => {
                error!(
                    "Auto scan setup failed for {}: {error}",
                    AUTO_SCAN_SOURCE_APP
                );
                wait_seconds =
                    clamp_interval(settings.scheduler.base_interval, &settings.scheduler);
                let failed_root_path = source_settings::list_enabled_scannable_sources(
                    &app,
                    &state.source_settings_store(),
                )
                .map(|sources| format_root_paths(&sources))
                .unwrap_or_default();
                state.mark_scan_failed(
                    AUTO_SCAN_TRIGGER,
                    &failed_root_path,
                    error.to_string(),
                    Some(wait_seconds),
                );
                events::emit_scan_failed(
                    &app,
                    AUTO_SCAN_TRIGGER,
                    Some(AUTO_SCAN_SOURCE_APP),
                    &failed_root_path,
                    Utc::now(),
                    &error.to_string(),
                );
            }
        }

        match receiver.recv_timeout(Duration::from_secs(wait_seconds.max(1))) {
            Ok(AutoScanSignal::RefreshNow) => {
                debug!(
                    "Received auto scan refresh signal for {}; running immediately",
                    AUTO_SCAN_SOURCE_APP
                );
            }
            Ok(AutoScanSignal::SettingsChanged) => {
                debug!(
                    "Received auto scan settings change signal for {}; rescheduling without immediate scan",
                    AUTO_SCAN_SOURCE_APP
                );
                reschedule_without_scan = true;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                debug!("Auto scan worker disconnected; shutting down");
                break;
            }
        }
    }
}

fn run_auto_scan_once(app: &AppHandle, state: &AppState) -> AppResult<AutoScanRunOutcome> {
    let sources =
        source_settings::list_enabled_scannable_sources(app, &state.source_settings_store())?;
    if sources.is_empty() {
        return Ok(AutoScanRunOutcome::Disabled);
    }
    let root_path_string = format_root_paths(&sources);

    let Some(_scan_guard) = state.try_acquire_scan_lock_if_available()? else {
        return Ok(AutoScanRunOutcome::Busy);
    };

    let scan_started_at = Utc::now();
    state.mark_scan_started(AUTO_SCAN_TRIGGER, &root_path_string);
    events::emit_scan_started(
        app,
        AUTO_SCAN_TRIGGER,
        Some(AUTO_SCAN_SOURCE_APP),
        &root_path_string,
    );

    let scanner = state.scanner();
    let run_id = scanner.create_scan_run(AUTO_SCAN_TRIGGER)?;
    let mut summary = ScanSummary {
        trigger_type: AUTO_SCAN_TRIGGER.to_string(),
        root_path: root_path_string,
        started_at: scan_started_at,
        files_seen: 0,
        files_parsed: 0,
        files_skipped: 0,
        files_failed: 0,
        sessions_changed: 0,
        error_count: 0,
    };

    let result = (|| -> AppResult<()> {
        for source in sources {
            if !source.path.exists() {
                continue;
            }

            let source_summary = scanner.scan(ScanRequest {
                root_path: source.path,
                source_app: source.app,
                trigger_type: AUTO_SCAN_TRIGGER.to_string(),
                create_run: false,
            })?;
            summary.files_seen += source_summary.files_seen;
            summary.files_parsed += source_summary.files_parsed;
            summary.files_skipped += source_summary.files_skipped;
            summary.files_failed += source_summary.files_failed;
            summary.sessions_changed += source_summary.sessions_changed;
            summary.error_count += source_summary.error_count;
        }

        Ok(())
    })();

    match result {
        Ok(()) => {
            scanner.complete_scan_run(&run_id, &summary)?;
            Ok(AutoScanRunOutcome::Completed(summary))
        }
        Err(error) => {
            let _ = scanner.fail_scan_run(&run_id, &summary);
            Ok(AutoScanRunOutcome::Failed {
                started_at: scan_started_at,
                root_path: summary.root_path.clone(),
                error_message: error.to_string(),
            })
        }
    }
}

fn format_root_paths(sources: &[ScannableSourceTarget]) -> String {
    sources
        .iter()
        .map(|source| format!("{}: {}", source.app, source.path.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn clamp_interval(value: u64, settings: &SchedulerSettings) -> u64 {
    value.clamp(settings.min_interval, settings.max_interval)
}

fn compute_ewma_series(values: &[f64], alpha: f64) -> Vec<f64> {
    let mut result = Vec::with_capacity(values.len());
    let mut previous = None;

    for &value in values {
        let next = previous
            .map(|prev| (alpha * value) + ((1.0 - alpha) * prev))
            .unwrap_or(value);
        result.push((next * 10.0).round() / 10.0);
        previous = Some(next);
    }

    result
}

fn fallback_change_rate_series(threshold: u64) -> Vec<f64> {
    let threshold = threshold.max(1) as f64;
    (0..22)
        .map(|index| {
            let wave = ((index as f64 * 1.7).sin() + 1.0) / 2.0;
            let burst = if index % 7 == 1 { 1.15 } else { 0.0 };
            let decay = (22 - index) as f64 / 22.0;
            threshold * (0.28 + (wave * 0.72 * decay) + burst)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{compute_next_interval, SchedulerTelemetry};
    use crate::settings::SchedulerSettings;

    fn test_settings() -> SchedulerSettings {
        SchedulerSettings {
            scan_mode: "auto".to_string(),
            base_interval: 60,
            min_interval: 15,
            max_interval: 300,
            adaptive_scanning: true,
            ewma_alpha: 0.3,
            change_rate_threshold: 5,
        }
    }

    #[test]
    fn uses_max_interval_when_no_files_were_parsed() {
        let settings = test_settings();
        let interval = compute_next_interval(
            &settings,
            SchedulerTelemetry {
                files_parsed: 0,
                sessions_changed: 0,
                consecutive_idle_runs: 2,
                ewma_change_rate_percent: 0.0,
            },
        );

        assert_eq!(interval, settings.max_interval);
    }

    #[test]
    fn accelerates_when_change_rate_is_high() {
        let settings = test_settings();
        let interval = compute_next_interval(
            &settings,
            SchedulerTelemetry {
                files_parsed: 10,
                sessions_changed: 4,
                consecutive_idle_runs: 0,
                ewma_change_rate_percent: 40.0,
            },
        );

        assert!(interval < settings.base_interval);
        assert!(interval >= settings.min_interval);
    }

    #[test]
    fn stretches_when_change_rate_is_low() {
        let settings = test_settings();
        let interval = compute_next_interval(
            &settings,
            SchedulerTelemetry {
                files_parsed: 10,
                sessions_changed: 1,
                consecutive_idle_runs: 0,
                ewma_change_rate_percent: 1.0,
            },
        );

        assert!(interval > settings.base_interval);
        assert!(interval <= settings.max_interval);
    }
}
