use crate::aria2::{Aria2Client, Aria2Transport};
use crate::domain::CompletedFile;
use crate::domain::PipelineOutcome;
use crate::pipeline::{
    is_heuristically_complete, AutoPipeline, CompletionSnapshot, ScrapeCoordinator,
};
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
    known_keys: HashSet<PathBuf>,
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
            known_keys: HashSet::new(),
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

    /// Poll one aria2 GID and enqueue completed selected video files.
    pub fn scan_aria2_gid<T: Aria2Transport>(
        &mut self,
        client: &Aria2Client<T>,
        gid: &str,
    ) -> Result<ScanReport> {
        if self.state == DaemonState::Paused {
            return Ok(ScanReport::default());
        }

        self.state = DaemonState::Scanning;
        let result = self.scan_aria2_gid_inner(client, gid);
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

    fn scan_aria2_gid_inner<T: Aria2Transport>(
        &mut self,
        client: &Aria2Client<T>,
        gid: &str,
    ) -> Result<ScanReport> {
        let status = client.tell_status(gid)?;
        let selection = status.completed_selection()?;
        let mut report = ScanReport {
            scanned_files: selection.scanned_files,
            skipped_files: selection.skipped_files,
            ..ScanReport::default()
        };

        for file in selection.files {
            if self.queue_completed_file(file) {
                report.queued_files += 1;
            }
        }

        Ok(report)
    }

    fn scan_roots(&mut self) -> Result<ScanReport> {
        let mut report = ScanReport::default();
        let mut candidates = Vec::new();

        for root in &self.config.source_roots {
            if !root.exists() {
                report.skipped_files += 1;
                continue;
            }
            for entry in WalkDir::new(root)
                .into_iter()
                .filter_map(|entry| entry.ok())
            {
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
        if !self.known_keys.insert(key) {
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

    /// Pause future scans and processing without clearing queued files.
    pub fn pause(&mut self) {
        self.state = DaemonState::Paused;
    }

    /// Resume a paused daemon and keep any files that were already queued.
    pub fn resume(&mut self) {
        if self.state == DaemonState::Paused {
            self.state = DaemonState::Idle;
        }
    }

    /// Process one queued file through the Stage 2 AutoPipeline.
    pub fn process_next(&mut self) -> Result<ProcessReport> {
        if self.state == DaemonState::Paused {
            return Ok(ProcessReport::default());
        }

        let Some(queued) = self.queue.pop_front() else {
            return Ok(ProcessReport::default());
        };
        let key = queue_key(&queued.path);

        self.state = DaemonState::Processing;
        let completed = queued.into_completed_file();
        let result = {
            let pipeline = AutoPipeline {
                repo: self.repo,
                archive_root: self.config.archive_root.clone(),
                asset_roots: self.config.asset_roots.clone(),
                scrapers: ScrapeCoordinator {
                    sources: self.scrapers.sources.iter().copied().collect(),
                },
            };
            pipeline.process_completed_file(completed)
        };

        let mut report = ProcessReport {
            processed: 1,
            ..ProcessReport::default()
        };

        match result {
            Ok(outcome) => self.apply_outcome(&outcome, &mut report),
            Err(error) => {
                report.failed = 1;
                self.last_error = Some(error.to_string());
                self.state = DaemonState::Error;
                self.processed += 1;
                self.known_keys.remove(&key);
                return Ok(report);
            }
        }

        if report.failed > 0 {
            self.known_keys.remove(&key);
        }
        self.processed += 1;
        self.state = DaemonState::Idle;
        Ok(report)
    }

    fn apply_outcome(&mut self, outcome: &PipelineOutcome, report: &mut ProcessReport) {
        match outcome.status.as_str() {
            "archived" => report.archived = 1,
            "holding" => report.holding = 1,
            "exception" => report.exceptions = 1,
            "failed" => report.failed = 1,
            other => {
                report.failed = 1;
                self.last_error = Some(format!("unknown pipeline outcome: {other}"));
            }
        }
    }

    /// Run one synchronous daemon pass: scan once, then drain the queue until
    /// it is empty, paused, or a processing error is recorded.
    pub fn run_once(&mut self) -> Result<RunOnceReport> {
        let scan = self.scan_now()?;
        let mut process = ProcessReport::default();

        while self.state != DaemonState::Paused && !self.queue.is_empty() {
            let next = self.process_next()?;
            process.processed += next.processed;
            process.archived += next.archived;
            process.holding += next.holding;
            process.exceptions += next.exceptions;
            process.failed += next.failed;
            if next.failed > 0 {
                break;
            }
        }

        Ok(RunOnceReport { scan, process })
    }
}

impl QueuedFile {
    /// Convert a queued file back into the Stage 2 CompletedFile DTO.
    pub fn into_completed_file(self) -> CompletedFile {
        CompletedFile {
            path: self.path,
            file_name: self.file_name,
            size_bytes: self.size_bytes,
            file_hash: self.file_hash,
        }
    }
}

fn queue_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
