use crate::aria2::{Aria2Transport, HttpAria2Transport};
use crate::daemon::{CompletionPolicy, DaemonConfig, HeadlessDaemon, RunOnceReport};
use crate::domain::{Exception, ExceptionStatus, HoldingEntry, PipelineRun, ScrapedWorkMetadata};
use crate::pipeline::{ScrapeCoordinator, ScraperSource};
use crate::remote_scraper::{
    build_remote_scraper_sources, HttpRemoteMetadataClient, RemoteMetadataHttpClient,
    RemoteScraperSettings,
};
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

/// Owns scraper instances long enough to lend them to `ScrapeCoordinator`.
pub struct ConfiguredPipelineScrapers {
    sources: Vec<Box<dyn ScraperSource>>,
}

impl ConfiguredPipelineScrapers {
    /// Build production scraper sources from repository settings.
    pub fn from_repository(repo: &Repository, metadata_enabled: bool) -> Result<Self> {
        let settings = repo.get_remote_scraper_settings()?;
        let client = HttpRemoteMetadataClient::from_settings(&settings)?;
        Self::with_remote_client(&settings, metadata_enabled, client)
    }

    /// Build scraper sources with an injected client for tests.
    pub fn with_remote_client<C>(
        settings: &RemoteScraperSettings,
        metadata_enabled: bool,
        remote_client: C,
    ) -> Result<Self>
    where
        C: RemoteMetadataHttpClient + Clone + 'static,
    {
        let normalized = settings.normalized()?;
        let mut sources = build_remote_scraper_sources(&normalized, remote_client)?;
        if metadata_enabled || normalized.include_example_fallback {
            sources.push(Box::new(ExamplePipelineScraper));
        }
        if sources.is_empty() {
            bail!("元数据源未开启，自动管线不会用空 scraper 处理真实文件");
        }
        Ok(Self { sources })
    }

    /// Borrow scraper sources for the pipeline coordinator.
    pub fn coordinator(&self) -> ScrapeCoordinator<'_> {
        ScrapeCoordinator {
            sources: self.sources.iter().map(|source| source.as_ref()).collect(),
        }
    }
}

/// Return whether the current settings provide any metadata source for the
/// automatic pipeline, including the legacy local example toggle.
pub fn metadata_source_available(
    metadata_provider_enabled: bool,
    settings: &RemoteScraperSettings,
) -> Result<bool> {
    if metadata_provider_enabled {
        return Ok(true);
    }
    let normalized = settings.normalized()?;
    if normalized.include_example_fallback {
        return Ok(true);
    }
    Ok(!normalized.enabled_sources()?.is_empty())
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
    let remote_settings = repo.get_remote_scraper_settings()?;
    let metadata_available = metadata_source_available(metadata_enabled, &remote_settings)?;

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
        metadata_source: if metadata_available {
            MetadataSource::Example
        } else {
            MetadataSource::Disabled
        },
    })
}

/// Run one synchronous daemon pass through the Stage 3 core. The command layer
/// refuses to process files while metadata sources are disabled so a real
/// library is not accidentally routed into scrape-failure exceptions.
pub fn run_daemon_once(
    repo: &Repository,
    runtime: &mut DaemonControlRuntime,
    metadata_enabled: bool,
) -> Result<RunOnceReport> {
    run_daemon_once_with_aria2_transport(repo, runtime, metadata_enabled, HttpAria2Transport)
}

/// Run one daemon pass with an injectable aria2 transport so tests do not need
/// a real aria2 process.
pub fn run_daemon_once_with_aria2_transport<T: Aria2Transport>(
    repo: &Repository,
    runtime: &mut DaemonControlRuntime,
    metadata_enabled: bool,
    aria2_transport: T,
) -> Result<RunOnceReport> {
    let remote_settings = repo.get_remote_scraper_settings()?;
    let remote_client = HttpRemoteMetadataClient::from_settings(&remote_settings)?;
    run_daemon_once_with_transports(
        repo,
        runtime,
        metadata_enabled,
        aria2_transport,
        remote_client,
    )
}

/// Run one daemon pass with injectable aria2 and remote scraper transports for tests.
pub fn run_daemon_once_with_transports<T, C>(
    repo: &Repository,
    runtime: &mut DaemonControlRuntime,
    metadata_enabled: bool,
    aria2_transport: T,
    remote_client: C,
) -> Result<RunOnceReport>
where
    T: Aria2Transport,
    C: RemoteMetadataHttpClient + Clone + 'static,
{
    if runtime.paused {
        return Ok(RunOnceReport::default());
    }

    let remote_settings = repo.get_remote_scraper_settings()?;
    let scrapers = ConfiguredPipelineScrapers::with_remote_client(
        &remote_settings,
        metadata_enabled,
        remote_client,
    )?;
    let config = DaemonConfig::load(repo)?;
    let coordinator = scrapers.coordinator();
    let mut daemon = HeadlessDaemon::with_completion_policy(
        repo,
        config,
        coordinator,
        CompletionPolicy {
            sample_delay: Duration::ZERO,
        },
    );
    let aria2_settings = repo.get_aria2_settings()?;
    let aria2 = daemon.poll_aria2_once(&aria2_settings, aria2_transport)?;

    match daemon.run_once() {
        Ok(mut report) => {
            report.aria2 = aria2;
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
