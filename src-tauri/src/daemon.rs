use crate::pipeline::ScrapeCoordinator;
use crate::storage::Repository;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

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
}
