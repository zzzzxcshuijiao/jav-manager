use crate::domain::CompletedFile;
use crate::pipeline::{is_heuristically_complete, CompletionSnapshot};
use crate::pipeline::ScrapeCoordinator;
use crate::scanner::is_video_file;
use crate::storage::Repository;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;
use walkdir::WalkDir;

/// Runtime configuration for the headless daemon core. It is loaded from
/// SQLite settings and contains only local filesystem roots needed by Stage 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    pub source_roots: Vec<PathBuf>,
    pub archive_root: PathBuf,
    pub asset_roots: Vec<PathBuf>,
}

impl DaemonConfig {
    /// Load daemon roots from Repository settings. Missing source or asset
    /// roots are allowed, but archive_root is required before processing.
    pub fn load(repo: &Repository) -> Result<Self> {
        let archive_root = repo
            .get_archive_root()?
            .ok_or_else(|| anyhow!("archive_root is required before daemon processing"))?;
        Ok(Self {
            source_roots: repo.get_source_roots()?,
            archive_root,
            asset_roots: repo.get_resource_pool_dirs()?,
        })
    }
}

/// Sampling delay between two completion snapshots. Tests set this to zero;
/// production callers can use a non-zero delay without changing daemon logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionPolicy {
    pub sample_delay: Duration,
}

impl Default for CompletionPolicy {
    fn default() -> Self {
        Self {
            sample_delay: Duration::from_secs(1),
        }
    }
}

/// In-memory lifecycle state exposed to future control interfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DaemonState {
    Idle,
    Scanning,
    Processing,
    Paused,
    Error,
}

/// Snapshot of daemon state suitable for Tauri commands or a local HTTP API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub state: DaemonState,
    pub queued: usize,
    pub processed: usize,
    pub last_error: Option<String>,
}

/// One file queued by the daemon after completion checks pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size_bytes: u64,
    pub file_hash: Option<String>,
}

/// Summary of one scan pass over configured source roots.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanReport {
    pub scanned_files: usize,
    pub queued_files: usize,
    pub skipped_files: usize,
}

/// Summary of files processed from the daemon queue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessReport {
    pub processed: usize,
    pub archived: usize,
    pub holding: usize,
    pub exceptions: usize,
    pub failed: usize,
}

/// Combined report returned by run_once: scan counts plus processing counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOnceReport {
    pub scan: ScanReport,
    pub process: ProcessReport,
}

/// Pure Rust daemon core. It owns only in-memory queue/state and delegates all
/// durable writes to Repository and AutoPipeline.
pub struct HeadlessDaemon<'a> {
    pub repo: &'a Repository,
    pub config: DaemonConfig,
    pub scrapers: ScrapeCoordinator<'a>,
    pub completion_policy: CompletionPolicy,
    state: DaemonState,
    queue: VecDeque<QueuedFile>,
    queued_keys: HashSet<PathBuf>,
    processed: usize,
    last_error: Option<String>,
}

impl<'a> HeadlessDaemon<'a> {
    /// Create a daemon with the default completion sampling delay.
    pub fn new(
        repo: &'a Repository,
        config: DaemonConfig,
        scrapers: ScrapeCoordinator<'a>,
    ) -> Self {
        Self::with_completion_policy(repo, config, scrapers, CompletionPolicy::default())
    }

    /// Create a daemon with an explicit sampling policy for deterministic tests.
    pub fn with_completion_policy(
        repo: &'a Repository,
        config: DaemonConfig,
        scrapers: ScrapeCoordinator<'a>,
        completion_policy: CompletionPolicy,
    ) -> Self {
        Self {
            repo,
            config,
            scrapers,
            completion_policy,
            state: DaemonState::Idle,
            queue: VecDeque::new(),
            queued_keys: HashSet::new(),
            processed: 0,
            last_error: None,
        }
    }

    /// Return an in-memory status snapshot without reading or writing SQLite.
    pub fn status(&self) -> DaemonStatus {
        DaemonStatus {
            state: self.state.clone(),
            queued: self.queue.len(),
            processed: self.processed,
            last_error: self.last_error.clone(),
        }
    }

    /// Scan configured source roots and enqueue only stable completed videos.
    /// This method never runs the pipeline and never writes media rows.
    pub fn scan_now(&mut self) -> Result<ScanReport> {
        if self.state == DaemonState::Paused {
            return Ok(ScanReport::default());
        }

        self.state = DaemonState::Scanning;
        let result = self.scan_roots();
        self.state = if result.is_ok() {
            DaemonState::Idle
        } else {
            DaemonState::Error
        };
        if let Err(error) = &result {
            self.last_error = Some(error.to_string());
        }
        result
    }

    fn scan_roots(&mut self) -> Result<ScanReport> {
        let mut report = ScanReport::default();
        let mut candidates = Vec::new();

        for root in &self.config.source_roots {
            if !root.exists() {
                report.skipped_files += 1;
                continue;
            }
            for entry in WalkDir::new(root).into_iter().filter_map(|entry| entry.ok()) {
                if entry.file_type().is_file() {
                    candidates.push(entry.into_path());
                }
            }
        }

        candidates.sort();

        for path in candidates {
            report.scanned_files += 1;
            if !is_video_file(&path) {
                report.skipped_files += 1;
                continue;
            }

            let first = CompletionSnapshot::capture(&path)?;
            if !self.completion_policy.sample_delay.is_zero() {
                std::thread::sleep(self.completion_policy.sample_delay);
            }
            let second = CompletionSnapshot::capture(&path)?;
            if !is_heuristically_complete(&first, &second) {
                report.skipped_files += 1;
                continue;
            }

            let completed = CompletedFile::from_path(&path)?;
            if self.queue_completed_file(completed) {
                report.queued_files += 1;
            }
        }

        Ok(report)
    }

    fn queue_completed_file(&mut self, file: CompletedFile) -> bool {
        let key = queue_key(&file.path);
        if !self.queued_keys.insert(key) {
            return false;
        }
        self.queue.push_back(QueuedFile {
            path: file.path,
            file_name: file.file_name,
            size_bytes: file.size_bytes,
            file_hash: file.file_hash,
        });
        true
    }
}

fn queue_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
