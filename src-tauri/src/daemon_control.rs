use crate::daemon::{CompletionPolicy, DaemonConfig, HeadlessDaemon, RunOnceReport};
use crate::domain::{Exception, ExceptionStatus, HoldingEntry, PipelineRun, ScrapedWorkMetadata};
use crate::pipeline::{ScrapeCoordinator, ScraperSource};
use crate::storage::Repository;
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// In-memory state owned by the Tauri command layer. Stage 4 does not keep a
/// long-running daemon instance, so this stores only cross-command control
/// flags and counters that are safe to preserve between synchronous calls.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DaemonControlRuntime {
    pub paused: bool,
    pub processed: usize,
    pub last_error: Option<String>,
}

/// Metadata source mode exposed to the frontend status panel. The values are
/// lower-case to match the TypeScript union used by `src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetadataSource {
    Example,
    Disabled,
}

/// Serializable daemon status for the Stage 4 command bridge. Queue length is
/// always zero outside an active `run_once` call because this stage creates a
/// fresh daemon core per command instead of keeping a background worker alive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonControlStatus {
    pub state: String,
    pub configured: bool,
    pub source_roots: Vec<String>,
    pub archive_root: Option<String>,
    pub asset_roots: Vec<String>,
    pub queued: usize,
    pub processed: usize,
    pub last_error: Option<String>,
    pub open_exceptions: usize,
    pub holding_items: usize,
    pub recent_runs: usize,
    pub metadata_source: MetadataSource,
}

/// Local deterministic scraper used by Stage 4 to prove the pipeline wiring
/// without network access or real external metadata providers.
pub struct ExamplePipelineScraper;

impl ScraperSource for ExamplePipelineScraper {
    fn name(&self) -> &str {
        "example"
    }

    fn lookup(&self, normalized_code: &str) -> Result<Option<ScrapedWorkMetadata>> {
        Ok(Some(ScrapedWorkMetadata {
            source: self.name().to_string(),
            normalized_code: normalized_code.to_string(),
            title: format!("{normalized_code} 本地示例标题"),
            original_title: Some(format!("{normalized_code} Example Title")),
            summary: Some("阶段4本地示例元数据，用于验证自动管线连线。".to_string()),
            actors: Vec::new(),
            genres: Vec::new(),
            studio: Some("Example Studio".to_string()),
            director: None,
            release_date: None,
            cover_path: None,
        }))
    }
}

/// Build a frontend status snapshot from SQLite settings, queue tables, and
/// command-layer runtime flags. This function performs no filesystem writes.
pub fn build_daemon_status(
    repo: &Repository,
    runtime: &DaemonControlRuntime,
    metadata_enabled: bool,
) -> Result<DaemonControlStatus> {
    let source_roots = repo
        .get_source_roots()?
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let archive_root = repo
        .get_archive_root()?
        .map(|path| path.to_string_lossy().to_string());
    let asset_roots = repo
        .get_resource_pool_dirs()?
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let exceptions = repo.list_exceptions()?;
    let open_exceptions = exceptions
        .iter()
        .filter(|entry| entry.status == ExceptionStatus::Open)
        .count();
    let holding_items = repo.list_holding()?.len();
    let recent_runs = repo.list_pipeline_runs()?.len().min(10);

    Ok(DaemonControlStatus {
        state: if runtime.paused { "Paused" } else { "Idle" }.to_string(),
        configured: archive_root.is_some(),
        source_roots,
        archive_root,
        asset_roots,
        queued: 0,
        processed: runtime.processed,
        last_error: runtime.last_error.clone(),
        open_exceptions,
        holding_items,
        recent_runs,
        metadata_source: if metadata_enabled {
            MetadataSource::Example
        } else {
            MetadataSource::Disabled
        },
    })
}

/// Run one synchronous daemon pass through the Stage 3 core. The command layer
/// refuses to process files while the example metadata source is disabled so a
/// real library is not accidentally routed into scrape-failure exceptions.
pub fn run_daemon_once(
    repo: &Repository,
    runtime: &mut DaemonControlRuntime,
    metadata_enabled: bool,
) -> Result<RunOnceReport> {
    if runtime.paused {
        return Ok(RunOnceReport::default());
    }
    if !metadata_enabled {
        bail!("示例元数据源未开启，阶段 4 不会用空 scraper 处理真实文件");
    }

    let scraper = ExamplePipelineScraper;
    let config = DaemonConfig::load(repo)?;
    let mut daemon = HeadlessDaemon::with_completion_policy(
        repo,
        config,
        ScrapeCoordinator {
            sources: vec![&scraper],
        },
        CompletionPolicy {
            sample_delay: Duration::ZERO,
        },
    );

    match daemon.run_once() {
        Ok(report) => {
            runtime.processed += report.process.processed;
            runtime.last_error = daemon.status().last_error;
            Ok(report)
        }
        Err(error) => {
            runtime.last_error = Some(error.to_string());
            Err(error)
        }
    }
}

/// Return every holding row for the frontend review panel, newest first.
pub fn list_holding_entries(repo: &Repository) -> Result<Vec<HoldingEntry>> {
    repo.list_holding()
}

/// Return every exception row for the frontend review panel, newest first.
pub fn list_exception_entries(repo: &Repository) -> Result<Vec<Exception>> {
    repo.list_exceptions()
}

/// Resolve or ignore an exception row after the user has acted on it.
pub fn resolve_exception_entry(
    repo: &Repository,
    id: i64,
    status: ExceptionStatus,
) -> Result<Exception> {
    repo.resolve_exception(id, status)?;
    repo.list_exceptions()?
        .into_iter()
        .find(|exception| exception.id == Some(id))
        .ok_or_else(|| anyhow!("exception not found"))
}

/// Return recent pipeline run rows for the status panel. The repository already
/// orders newest first; Stage 4 caps the UI feed to ten rows.
pub fn list_recent_pipeline_runs(repo: &Repository) -> Result<Vec<PipelineRun>> {
    let mut runs = repo.list_pipeline_runs()?;
    runs.truncate(10);
    Ok(runs)
}
