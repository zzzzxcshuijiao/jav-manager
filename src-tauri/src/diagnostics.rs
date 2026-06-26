use crate::control_service_host::ControlServiceHostStatus;
use crate::daemon_control::DaemonControlStatus;
use crate::domain::{Exception, ExceptionStatus, HoldingEntry, PipelineRun, ScrapeJob};
use crate::storage::Repository;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_MAX_FILES: usize = 3;
const MAX_TAIL_LIMIT: usize = 200;
const SNAPSHOT_PIPELINE_LIMIT: usize = 10;
const SNAPSHOT_SCRAPE_LIMIT: usize = 20;
const SNAPSHOT_EXCEPTION_LIMIT: usize = 20;
const SNAPSHOT_HOLDING_LIMIT: usize = 20;

/// Severity for one structured diagnostic event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticLevel {
    Info,
    Warn,
    Error,
}

/// One JSONL diagnostic event written by the local diagnostics subsystem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticLogEntry {
    pub timestamp: String,
    pub level: DiagnosticLevel,
    pub target: String,
    pub message: String,
    pub context: Value,
}

/// Redacted settings summary included in a diagnostic export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticSettingsSummary {
    pub source_root_count: usize,
    pub archive_root_configured: bool,
    pub metadata_provider_enabled: bool,
    pub aria2_enabled: bool,
    pub aria2_host: String,
    pub aria2_port: u16,
    pub aria2_secret_configured: bool,
    pub aria2_tracked_gids: usize,
    pub remote_scraper_enabled: bool,
    pub remote_scraper_sources: usize,
    pub remote_scraper_proxy_url: Option<String>,
}

/// Serializable support snapshot exported for local troubleshooting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticSnapshot {
    pub generated_at: String,
    pub app_data_dir: String,
    pub control_service: Option<ControlServiceHostStatus>,
    pub daemon: Option<DaemonControlStatus>,
    pub settings: Option<DiagnosticSettingsSummary>,
    pub recent_pipeline_runs: Vec<PipelineRun>,
    pub recent_scrape_jobs: Vec<ScrapeJob>,
    pub open_exceptions: Vec<Exception>,
    pub holding_items: Vec<HoldingEntry>,
    pub recent_logs: Vec<DiagnosticLogEntry>,
}

/// Inputs needed to assemble a diagnostic snapshot without coupling to Tauri.
pub struct DiagnosticSnapshotInput<'a> {
    pub app_data_dir: &'a Path,
    pub repository: Option<&'a Repository>,
    pub control_service: Option<ControlServiceHostStatus>,
    pub daemon: Option<DaemonControlStatus>,
    pub recent_logs: Vec<DiagnosticLogEntry>,
}

/// Summary returned to the UI after a diagnostic snapshot is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticExportResult {
    pub path: String,
    pub logs: usize,
    pub pipeline_runs: usize,
    pub scrape_jobs: usize,
    pub open_exceptions: usize,
    pub holding_items: usize,
}

/// App-data backed JSONL writer for local diagnostic events.
#[derive(Debug, Clone)]
pub struct DiagnosticsWriter {
    log_dir: PathBuf,
    log_path: PathBuf,
    max_bytes: u64,
    max_files: usize,
}

impl DiagnosticsWriter {
    /// Create a diagnostics writer under `<log_dir>/media-manager.jsonl`.
    pub fn new(log_dir: PathBuf) -> Result<Self> {
        Self::new_with_limits(log_dir, DEFAULT_MAX_BYTES, DEFAULT_MAX_FILES)
    }

    /// Create a diagnostics writer with explicit limits for deterministic tests.
    pub fn new_with_limits(log_dir: PathBuf, max_bytes: u64, max_files: usize) -> Result<Self> {
        fs::create_dir_all(&log_dir)?;
        Ok(Self {
            log_path: log_dir.join("media-manager.jsonl"),
            log_dir,
            max_bytes: max_bytes.max(1),
            max_files: max_files.max(1),
        })
    }

    /// Return the active JSONL file path.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Append one redacted JSONL event to the active diagnostics file.
    pub fn append(
        &self,
        level: DiagnosticLevel,
        target: impl Into<String>,
        message: impl Into<String>,
        context: Value,
    ) -> Result<()> {
        self.rotate_if_needed()?;
        let entry = DiagnosticLogEntry {
            timestamp: Utc::now().to_rfc3339(),
            level,
            target: target.into(),
            message: message.into(),
            context: redact_diagnostic_value(context),
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;
        writeln!(file, "{}", serde_json::to_string(&entry)?)?;
        Ok(())
    }

    /// Read the last `limit` valid events from the active log, oldest to newest.
    pub fn tail(&self, limit: usize) -> Result<Vec<DiagnosticLogEntry>> {
        let limit = limit.clamp(1, MAX_TAIL_LIMIT);
        if !self.log_path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&self.log_path)?;
        let reader = BufReader::new(file);
        let mut entries = VecDeque::with_capacity(limit);
        for line in reader.lines() {
            let line = line?;
            if let Ok(entry) = serde_json::from_str::<DiagnosticLogEntry>(&line) {
                if entries.len() == limit {
                    entries.pop_front();
                }
                entries.push_back(entry);
            }
        }
        Ok(entries.into_iter().collect())
    }

    fn rotate_if_needed(&self) -> Result<()> {
        if !self.log_path.exists() || fs::metadata(&self.log_path)?.len() < self.max_bytes {
            return Ok(());
        }
        for index in (1..=self.max_files).rev() {
            let from = self.rotated_path(index);
            if !from.exists() {
                continue;
            }
            if index == self.max_files {
                fs::remove_file(from)?;
            } else {
                fs::rename(from, self.rotated_path(index + 1))?;
            }
        }
        fs::rename(&self.log_path, self.rotated_path(1))?;
        Ok(())
    }

    fn rotated_path(&self, index: usize) -> PathBuf {
        self.log_dir.join(format!("media-manager.jsonl.{index}"))
    }
}

/// Recursively redact sensitive diagnostic context values before writing them.
pub fn redact_diagnostic_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(redact_map(map)),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(redact_diagnostic_value).collect())
        }
        other => other,
    }
}

fn redact_map(map: Map<String, Value>) -> Map<String, Value> {
    map.into_iter()
        .map(|(key, value)| {
            let lower = key.to_ascii_lowercase();
            let redacted = if is_secret_key(&lower) {
                Value::String("***".to_string())
            } else if lower.contains("proxy") {
                match value {
                    Value::String(url) => Value::String(redact_proxy_url(&url)),
                    other => redact_diagnostic_value(other),
                }
            } else {
                redact_diagnostic_value(value)
            };
            (key, redacted)
        })
        .collect()
}

fn is_secret_key(key: &str) -> bool {
    ["secret", "token", "password", "authorization", "cookie"]
        .iter()
        .any(|needle| key.contains(needle))
}

/// Remove credentials from proxy URLs while preserving the endpoint location.
pub fn redact_proxy_url(value: &str) -> String {
    let Some((scheme, rest)) = value.split_once("://") else {
        return value.to_string();
    };
    let Some((_, host)) = rest.split_once('@') else {
        return value.to_string();
    };
    format!("{scheme}://***@{host}")
}

/// Build one in-memory diagnostic snapshot from repository summaries and recent logs.
pub fn build_diagnostic_snapshot(input: DiagnosticSnapshotInput<'_>) -> Result<DiagnosticSnapshot> {
    let (settings, recent_pipeline_runs, recent_scrape_jobs, open_exceptions, holding_items) =
        if let Some(repo) = input.repository {
            let settings = Some(build_settings_summary(repo)?);
            let mut pipeline_runs = repo.list_pipeline_runs()?;
            pipeline_runs.truncate(SNAPSHOT_PIPELINE_LIMIT);
            let mut scrape_jobs = repo.list_scrape_jobs()?;
            scrape_jobs.truncate(SNAPSHOT_SCRAPE_LIMIT);
            let mut exceptions: Vec<Exception> = repo
                .list_exceptions()?
                .into_iter()
                .filter(|entry| entry.status == ExceptionStatus::Open)
                .collect();
            exceptions.truncate(SNAPSHOT_EXCEPTION_LIMIT);
            let mut holding = repo.list_holding()?;
            holding.truncate(SNAPSHOT_HOLDING_LIMIT);
            (settings, pipeline_runs, scrape_jobs, exceptions, holding)
        } else {
            (None, Vec::new(), Vec::new(), Vec::new(), Vec::new())
        };

    Ok(DiagnosticSnapshot {
        generated_at: Utc::now().to_rfc3339(),
        app_data_dir: input.app_data_dir.to_string_lossy().to_string(),
        control_service: input.control_service.map(redact_control_service_status),
        daemon: input.daemon.map(redact_daemon_status),
        settings,
        recent_pipeline_runs: redact_pipeline_runs(recent_pipeline_runs),
        recent_scrape_jobs: redact_scrape_jobs(recent_scrape_jobs),
        open_exceptions: redact_exceptions(open_exceptions),
        holding_items,
        recent_logs: input.recent_logs,
    })
}

/// Write a diagnostic snapshot JSON file under `<app_data_dir>/diagnostics`.
pub fn export_diagnostic_snapshot(
    app_data_dir: &Path,
    snapshot: &DiagnosticSnapshot,
) -> Result<DiagnosticExportResult> {
    let output_dir = app_data_dir.join("diagnostics");
    fs::create_dir_all(&output_dir)?;
    let path = output_dir.join(format!(
        "diagnostics-{}.json",
        Utc::now().format("%Y%m%d-%H%M%S")
    ));
    fs::write(&path, serde_json::to_string_pretty(snapshot)?)?;
    Ok(DiagnosticExportResult {
        path: path.to_string_lossy().to_string(),
        logs: snapshot.recent_logs.len(),
        pipeline_runs: snapshot.recent_pipeline_runs.len(),
        scrape_jobs: snapshot.recent_scrape_jobs.len(),
        open_exceptions: snapshot.open_exceptions.len(),
        holding_items: snapshot.holding_items.len(),
    })
}

fn build_settings_summary(repo: &Repository) -> Result<DiagnosticSettingsSummary> {
    let source_roots = repo.get_source_roots()?;
    let archive_root = repo.get_archive_root()?;
    let metadata_provider_enabled = repo.get_metadata_provider_enabled()?;
    let aria2 = repo.get_aria2_settings()?;
    let remote_scraper = repo.get_remote_scraper_settings()?;
    Ok(DiagnosticSettingsSummary {
        source_root_count: source_roots.len(),
        archive_root_configured: archive_root.is_some(),
        metadata_provider_enabled,
        aria2_enabled: aria2.enabled,
        aria2_host: aria2.host,
        aria2_port: aria2.port,
        aria2_secret_configured: aria2.secret.is_some(),
        aria2_tracked_gids: aria2.tracked_gids.len(),
        remote_scraper_enabled: remote_scraper.enabled,
        remote_scraper_sources: remote_scraper.sources.len(),
        remote_scraper_proxy_url: remote_scraper.proxy_url.as_deref().map(redact_proxy_url),
    })
}

fn redact_control_service_status(
    mut status: ControlServiceHostStatus,
) -> ControlServiceHostStatus {
    status.last_error = status
        .last_error
        .map(|value| redact_diagnostic_text(&value));
    status
}

fn redact_daemon_status(mut status: DaemonControlStatus) -> DaemonControlStatus {
    status.last_error = status
        .last_error
        .map(|value| redact_diagnostic_text(&value));
    status
}

fn redact_pipeline_runs(runs: Vec<PipelineRun>) -> Vec<PipelineRun> {
    runs.into_iter()
        .map(|mut run| {
            run.error = run.error.map(|value| redact_diagnostic_text(&value));
            run
        })
        .collect()
}

fn redact_scrape_jobs(jobs: Vec<ScrapeJob>) -> Vec<ScrapeJob> {
    jobs.into_iter()
        .map(|mut job| {
            job.error = job.error.map(|value| redact_diagnostic_text(&value));
            job
        })
        .collect()
}

fn redact_exceptions(exceptions: Vec<Exception>) -> Vec<Exception> {
    exceptions
        .into_iter()
        .map(|mut entry| {
            entry.evidence_json = redact_diagnostic_text(&entry.evidence_json);
            entry
        })
        .collect()
}

fn redact_diagnostic_text(value: &str) -> String {
    if let Ok(json) = serde_json::from_str::<Value>(value) {
        return serde_json::to_string(&redact_diagnostic_value(json))
            .unwrap_or_else(|_| "***".to_string());
    }
    value
        .split_whitespace()
        .map(redact_proxy_url)
        .collect::<Vec<_>>()
        .join(" ")
}
