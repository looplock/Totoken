use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc, Mutex, MutexGuard, TryLockError,
};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::db::DbPool;
use crate::error::{AppError, AppResult};
use crate::scanner::{ScanSummary, Scanner};
use crate::source_settings::SourceSettingsStore;

const MAX_CHANGE_RATE_HISTORY_POINTS: usize = 24;

#[derive(Debug, Clone, Copy)]
pub enum AutoScanSignal {
    RefreshNow,
    SettingsChanged,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoScanStatusView {
    pub source_app: String,
    pub root_path: String,
    pub scanner_busy: bool,
    pub active_trigger_type: Option<String>,
    pub is_auto_scan_running: bool,
    pub current_interval_seconds: Option<u64>,
    pub next_scan_at: Option<DateTime<Utc>>,
    pub last_scan_started_at: Option<DateTime<Utc>>,
    pub last_scan_ended_at: Option<DateTime<Utc>>,
    pub last_successful_scan_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub current_ewma_change_rate_percent: Option<f64>,
    pub consecutive_idle_runs: u64,
    pub last_summary: Option<ScanSummary>,
}

#[derive(Debug, Default, Clone)]
struct AutoScanRuntimeState {
    scanner_busy: bool,
    active_trigger_type: Option<String>,
    is_auto_scan_running: bool,
    root_path: Option<String>,
    current_interval_seconds: Option<u64>,
    next_scan_at: Option<DateTime<Utc>>,
    last_scan_started_at: Option<DateTime<Utc>>,
    last_scan_ended_at: Option<DateTime<Utc>>,
    last_successful_scan_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
    current_ewma_change_rate_percent: Option<f64>,
    consecutive_idle_runs: u64,
    last_summary: Option<ScanSummary>,
    raw_change_rate_history_percent: Vec<f64>,
}

#[derive(Clone)]
pub struct AppState {
    db_pool: DbPool,
    scanner: Arc<Scanner>,
    source_settings_store: SourceSettingsStore,
    storage_restart_required: Arc<AtomicBool>,
    scan_lock: Arc<Mutex<()>>,
    auto_scan_signal_sender: Arc<Mutex<Option<Sender<AutoScanSignal>>>>,
    auto_scan_runtime: Arc<Mutex<AutoScanRuntimeState>>,
}

impl AppState {
    pub fn new(db_pool: DbPool) -> Self {
        Self {
            scanner: Arc::new(Scanner::new(db_pool.clone())),
            db_pool,
            source_settings_store: SourceSettingsStore::default(),
            storage_restart_required: Arc::new(AtomicBool::new(false)),
            scan_lock: Arc::new(Mutex::new(())),
            auto_scan_signal_sender: Arc::new(Mutex::new(None)),
            auto_scan_runtime: Arc::new(Mutex::new(AutoScanRuntimeState::default())),
        }
    }

    pub fn db_pool(&self) -> DbPool {
        self.db_pool.clone()
    }

    pub fn scanner(&self) -> Arc<Scanner> {
        self.scanner.clone()
    }

    pub fn source_settings_store(&self) -> SourceSettingsStore {
        self.source_settings_store.clone()
    }

    pub fn mark_storage_restart_required(&self) {
        self.storage_restart_required.store(true, Ordering::SeqCst);
    }

    pub fn storage_restart_required(&self) -> bool {
        self.storage_restart_required.load(Ordering::SeqCst)
    }

    pub fn log_error(
        &self,
        category: &str,
        source: &str,
        action: &str,
        message: impl Into<String>,
        details: Option<String>,
        _context: BTreeMap<String, String>,
    ) {
        let message = message.into();
        if let Some(details) = details {
            log::error!(target: category, "{source}.{action}: {message}; details: {details}");
        } else {
            log::error!(target: category, "{source}.{action}: {message}");
        }
    }

    pub fn log_warning(
        &self,
        category: &str,
        source: &str,
        action: &str,
        message: impl Into<String>,
        details: Option<String>,
        _context: BTreeMap<String, String>,
    ) {
        let message = message.into();
        if let Some(details) = details {
            log::warn!(target: category, "{source}.{action}: {message}; details: {details}");
        } else {
            log::warn!(target: category, "{source}.{action}: {message}");
        }
    }

    pub fn try_acquire_scan_lock(&self) -> AppResult<MutexGuard<'_, ()>> {
        match self.scan_lock.try_lock() {
            Ok(guard) => Ok(guard),
            Err(TryLockError::WouldBlock) => Err(AppError::validation("a scan is already running")),
            Err(TryLockError::Poisoned(_)) => {
                self.log_error(
                    "system",
                    "scanner",
                    "try_acquire_scan_lock",
                    "Scan lock is in a poisoned state",
                    None,
                    BTreeMap::new(),
                );
                Err(AppError::internal("scan lock is in a poisoned state"))
            }
        }
    }

    pub fn try_acquire_scan_lock_if_available(&self) -> AppResult<Option<MutexGuard<'_, ()>>> {
        match self.scan_lock.try_lock() {
            Ok(guard) => Ok(Some(guard)),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Poisoned(_)) => {
                self.log_error(
                    "system",
                    "scanner",
                    "try_acquire_scan_lock_if_available",
                    "Scan lock is in a poisoned state",
                    None,
                    BTreeMap::new(),
                );
                Err(AppError::internal("scan lock is in a poisoned state"))
            }
        }
    }

    pub fn install_auto_scan_signal_sender(&self, sender: Sender<AutoScanSignal>) {
        match self.auto_scan_signal_sender.lock() {
            Ok(mut slot) => {
                *slot = Some(sender);
            }
            Err(error) => {
                log::warn!("auto_scan_signal_sender poisoned during install: {error}");
                self.log_warning(
                    "system",
                    "state",
                    "install_auto_scan_signal_sender",
                    "auto_scan_signal_sender poisoned during install",
                    Some(error.to_string()),
                    BTreeMap::new(),
                );
            }
        }
    }

    pub fn notify_auto_scan_refresh(&self) {
        self.notify_auto_scan(AutoScanSignal::RefreshNow, "notify_auto_scan_refresh");
    }

    pub fn notify_auto_scan_settings_changed(&self) {
        self.notify_auto_scan(
            AutoScanSignal::SettingsChanged,
            "notify_auto_scan_settings_changed",
        );
    }

    fn notify_auto_scan(&self, signal: AutoScanSignal, action_name: &str) {
        match self.auto_scan_signal_sender.lock() {
            Ok(slot) => {
                if let Some(sender) = slot.as_ref() {
                    let _ = sender.send(signal);
                }
            }
            Err(error) => {
                log::warn!("auto_scan_signal_sender poisoned during refresh notify: {error}");
                self.log_warning(
                    "system",
                    "state",
                    action_name,
                    "auto_scan_signal_sender poisoned during refresh notify",
                    Some(error.to_string()),
                    BTreeMap::new(),
                );
            }
        }
    }

    fn with_auto_scan_runtime<R>(
        &self,
        action_name: &str,
        fallback: R,
        action: impl FnOnce(&mut AutoScanRuntimeState) -> R,
    ) -> R {
        match self.auto_scan_runtime.lock() {
            Ok(mut runtime) => action(&mut runtime),
            Err(error) => {
                let details = error.to_string();
                log::warn!("auto_scan_runtime poisoned during {action_name}: {details}");
                self.log_warning(
                    "system",
                    "state",
                    action_name,
                    format!("auto_scan_runtime poisoned during {action_name}"),
                    Some(details),
                    BTreeMap::new(),
                );
                fallback
            }
        }
    }

    pub fn mark_auto_scan_paused(&self) {
        self.with_auto_scan_runtime("mark_auto_scan_paused", (), |runtime| {
            if runtime.active_trigger_type.as_deref() == Some("auto") {
                runtime.active_trigger_type = None;
            }
            runtime.is_auto_scan_running = false;
            runtime.current_interval_seconds = None;
            runtime.next_scan_at = None;
        });
    }

    pub fn mark_auto_scan_rescheduled(&self, next_interval_seconds: Option<u64>) {
        self.with_auto_scan_runtime("mark_auto_scan_rescheduled", (), |runtime| {
            let now = Utc::now();
            runtime.current_interval_seconds = next_interval_seconds;
            runtime.next_scan_at = next_interval_seconds
                .and_then(|seconds| Duration::try_seconds(seconds as i64))
                .map(|duration| now + duration);
        });
    }

    pub fn mark_scan_started(&self, trigger_type: &str, root_path: &str) {
        self.with_auto_scan_runtime("mark_scan_started", (), |runtime| {
            runtime.scanner_busy = true;
            runtime.active_trigger_type = Some(trigger_type.to_string());
            runtime.is_auto_scan_running = trigger_type == "auto";
            if trigger_type == "auto" {
                runtime.root_path = Some(root_path.to_string());
                runtime.last_scan_started_at = Some(Utc::now());
                runtime.last_error = None;
            }
        });
    }

    pub fn mark_scan_completed(
        &self,
        summary: &ScanSummary,
        current_ewma_change_rate_percent: Option<f64>,
        consecutive_idle_runs: Option<u64>,
        next_interval_seconds: Option<u64>,
    ) {
        self.with_auto_scan_runtime("mark_scan_completed", (), |runtime| {
            let now = Utc::now();
            runtime.scanner_busy = false;
            if runtime.active_trigger_type.as_deref() == Some(summary.trigger_type.as_str()) {
                runtime.active_trigger_type = None;
            }
            if summary.trigger_type == "auto" {
                runtime.is_auto_scan_running = false;
            }

            if summary.trigger_type == "auto" {
                runtime.root_path = Some(summary.root_path.clone());
                runtime.last_scan_ended_at = Some(now);
                runtime.last_successful_scan_at = Some(now);
                runtime.last_error = None;
                runtime.last_summary = Some(summary.clone());
                runtime.current_interval_seconds = next_interval_seconds;
                runtime.next_scan_at = next_interval_seconds
                    .and_then(|seconds| Duration::try_seconds(seconds as i64))
                    .map(|duration| now + duration);
                runtime.current_ewma_change_rate_percent = current_ewma_change_rate_percent;
                runtime.consecutive_idle_runs =
                    consecutive_idle_runs.unwrap_or(runtime.consecutive_idle_runs);

                let raw_change_rate_percent = if summary.files_parsed == 0 {
                    0.0
                } else {
                    (summary.sessions_changed as f64 / summary.files_parsed as f64) * 100.0
                };

                runtime
                    .raw_change_rate_history_percent
                    .push(raw_change_rate_percent);
                if runtime.raw_change_rate_history_percent.len() > MAX_CHANGE_RATE_HISTORY_POINTS {
                    let overflow = runtime.raw_change_rate_history_percent.len()
                        - MAX_CHANGE_RATE_HISTORY_POINTS;
                    runtime.raw_change_rate_history_percent.drain(0..overflow);
                }
            }
        });
    }

    pub fn mark_scan_failed(
        &self,
        trigger_type: &str,
        root_path: &str,
        error_message: String,
        next_interval_seconds: Option<u64>,
    ) {
        self.with_auto_scan_runtime("mark_scan_failed", (), |runtime| {
            let now = Utc::now();
            runtime.scanner_busy = false;
            if runtime.active_trigger_type.as_deref() == Some(trigger_type) {
                runtime.active_trigger_type = None;
            }
            if trigger_type == "auto" {
                runtime.is_auto_scan_running = false;
            }

            if trigger_type == "auto" {
                runtime.root_path = Some(root_path.to_string());
                runtime.last_scan_ended_at = Some(now);
                runtime.last_error = Some(error_message);
                runtime.current_interval_seconds = next_interval_seconds;
                runtime.next_scan_at = next_interval_seconds
                    .and_then(|seconds| Duration::try_seconds(seconds as i64))
                    .map(|duration| now + duration);
            }
        });
    }

    pub fn auto_scan_status_view(
        &self,
        source_app: &str,
        default_root_path: String,
    ) -> AutoScanStatusView {
        let fallback = AutoScanStatusView {
            source_app: source_app.to_string(),
            root_path: default_root_path.clone(),
            scanner_busy: false,
            active_trigger_type: None,
            is_auto_scan_running: false,
            current_interval_seconds: None,
            next_scan_at: None,
            last_scan_started_at: None,
            last_scan_ended_at: None,
            last_successful_scan_at: None,
            last_error: None,
            current_ewma_change_rate_percent: None,
            consecutive_idle_runs: 0,
            last_summary: None,
        };

        self.with_auto_scan_runtime("auto_scan_status_view", fallback, |runtime| {
            AutoScanStatusView {
                source_app: source_app.to_string(),
                root_path: runtime.root_path.clone().unwrap_or(default_root_path),
                scanner_busy: runtime.scanner_busy,
                active_trigger_type: runtime.active_trigger_type.clone(),
                is_auto_scan_running: runtime.is_auto_scan_running,
                current_interval_seconds: runtime.current_interval_seconds,
                next_scan_at: runtime.next_scan_at,
                last_scan_started_at: runtime.last_scan_started_at,
                last_scan_ended_at: runtime.last_scan_ended_at,
                last_successful_scan_at: runtime.last_successful_scan_at,
                last_error: runtime.last_error.clone(),
                current_ewma_change_rate_percent: runtime.current_ewma_change_rate_percent,
                consecutive_idle_runs: runtime.consecutive_idle_runs,
                last_summary: runtime.last_summary.clone(),
            }
        })
    }

    pub fn raw_change_rate_history_percent(&self) -> Vec<f64> {
        self.with_auto_scan_runtime("raw_change_rate_history_percent", Vec::new(), |runtime| {
            runtime.raw_change_rate_history_percent.clone()
        })
    }
}
