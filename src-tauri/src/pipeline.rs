use crate::archive::normalized_file_name;
use crate::domain::{
    ArchiveAsset, CodeKind, CompletedFile, Exception, ExceptionKind, ExceptionStatus, HoldingEntry,
    HoldingReason, PipelineOutcome, PipelineStepRecord, ScrapeJob, ScrapeStatus,
    ScrapedWorkMetadata, WatchStatus, Work,
};
use crate::identifier::extract_code_from_text;
use crate::nfo::render_scraped_nfo;
use crate::scanner::is_video_file;
use crate::storage::Repository;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

/// Minimal aria2 task state needed by the completion detector. Stage 3 will
/// populate this from JSON-RPC; Stage 2 keeps it pure and testable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aria2TaskSnapshot {
    pub status: String,
    pub completed_length: u64,
    pub total_length: u64,
}

/// True when aria2 reports a complete task and byte counts agree.
pub fn is_aria2_complete(snapshot: &Aria2TaskSnapshot) -> bool {
    snapshot.status == "complete"
        && snapshot.total_length > 0
        && snapshot.completed_length == snapshot.total_length
}

/// One filesystem sample used by the non-aria2 completion heuristic. Stage 3
/// controls the delay between samples; Stage 2 owns the deterministic compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionSnapshot {
    pub path: PathBuf,
    pub exists: bool,
    pub is_video: bool,
    pub has_aria2_control: bool,
    pub size_bytes: u64,
    pub modified_at: Option<SystemTime>,
    pub can_open: bool,
}

impl CompletionSnapshot {
    /// Capture the file state needed to decide whether a non-aria2 file is
    /// stable enough to process.
    pub fn capture(path: &Path) -> anyhow::Result<Self> {
        let metadata = std::fs::metadata(path).ok();
        let control = path.with_extension(format!(
            "{}.aria2",
            path.extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default()
        ));
        Ok(Self {
            path: path.to_path_buf(),
            exists: metadata.is_some(),
            is_video: is_video_file(path),
            has_aria2_control: control.exists(),
            size_bytes: metadata.as_ref().map(|m| m.len()).unwrap_or(0),
            modified_at: metadata.and_then(|m| m.modified().ok()),
            can_open: std::fs::OpenOptions::new().read(true).open(path).is_ok(),
        })
    }
}

/// Fallback completion check for non-aria2 files. A file is complete only when
/// both samples agree on size and mtime, no sibling aria2 control file exists,
/// and the file can be opened for reading.
pub fn is_heuristically_complete(first: &CompletionSnapshot, second: &CompletionSnapshot) -> bool {
    first.path == second.path
        && first.exists
        && second.exists
        && first.is_video
        && second.is_video
        && !first.has_aria2_control
        && !second.has_aria2_control
        && first.size_bytes == second.size_bytes
        && first.modified_at == second.modified_at
        && first.can_open
        && second.can_open
}

/// Classification outcome after the pipeline inspects a completed file name.
/// Only `Identified` continues to scraping; holding outcomes are quiet triage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineIdentification {
    Identified { normalized_code: String },
    Holding { reason: HoldingReason },
}

/// Extract the canonical studio code from a completed video. Files without a
/// parseable standard code are held for later manual triage, not escalated as
/// exceptions.
pub fn identify_completed_file(file: &CompletedFile) -> PipelineIdentification {
    let searchable_name = file
        .path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(&file.file_name);
    extract_code_from_text(searchable_name)
        .map(|normalized_code| PipelineIdentification::Identified { normalized_code })
        .unwrap_or(PipelineIdentification::Holding {
            reason: HoldingReason::NoCode,
        })
}

/// Scraper boundary used by the automatic pipeline. Real FANZA/JavBus/JavDB
/// adapters can be added behind this trait without changing orchestration.
pub trait ScraperSource: Send + Sync {
    fn name(&self) -> &str;
    fn lookup(&self, normalized_code: &str) -> anyhow::Result<Option<ScrapedWorkMetadata>>;
}

/// User-configured ordered scraper list. The first source returning metadata
/// wins; failures are recorded for retry and diagnostics.
pub struct ScrapeCoordinator<'a> {
    pub sources: Vec<&'a dyn ScraperSource>,
}

/// Stable context for recording scrape attempts before a work exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrapeContext {
    pub work_id: Option<i64>,
    pub normalized_code: String,
    pub object_path: PathBuf,
    pub pipeline_run_id: Option<i64>,
}

/// Scrape failure categories that matter to the exception router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineScrapeError {
    AllSourcesFailed,
}

impl<'a> ScrapeCoordinator<'a> {
    /// Try each source in order, recording one scrape job per attempt.
    pub fn scrape(
        &self,
        repo: &Repository,
        context: &ScrapeContext,
    ) -> Result<ScrapedWorkMetadata, PipelineScrapeError> {
        for source in &self.sources {
            match source.lookup(&context.normalized_code) {
                Ok(Some(metadata)) => {
                    let _ = repo.record_scrape_job(&scrape_job(
                        context,
                        source.name(),
                        ScrapeStatus::Success,
                        None,
                    ));
                    return Ok(metadata);
                }
                Ok(None) => {
                    let _ = repo.record_scrape_job(&scrape_job(
                        context,
                        source.name(),
                        ScrapeStatus::Failed,
                        Some("not found".to_string()),
                    ));
                }
                Err(error) => {
                    let _ = repo.record_scrape_job(&scrape_job(
                        context,
                        source.name(),
                        ScrapeStatus::Failed,
                        Some(error.to_string()),
                    ));
                }
            }
        }
        Err(PipelineScrapeError::AllSourcesFailed)
    }
}

fn scrape_job(
    context: &ScrapeContext,
    source: &str,
    status: ScrapeStatus,
    error: Option<String>,
) -> ScrapeJob {
    ScrapeJob {
        id: None,
        work_id: context.work_id,
        normalized_code: Some(context.normalized_code.clone()),
        object_path: Some(context.object_path.to_string_lossy().to_string()),
        pipeline_run_id: context.pipeline_run_id,
        source: source.to_string(),
        status,
        attempts: 1,
        last_attempted_at: Some(chrono::Utc::now().to_rfc3339()),
        error,
    }
}

/// Complete filesystem plan for one pipeline archive operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveLayoutPlan {
    pub work_dir: PathBuf,
    pub video_target: PathBuf,
    pub nfo_target: PathBuf,
    pub assets: Vec<ArchiveAsset>,
}

/// Build the target layout for one completed file and its scraped metadata.
/// The plan performs no writes and picks the next free version suffix.
pub fn plan_archive_layout(
    archive_root: &Path,
    file: &CompletedFile,
    metadata: &ScrapedWorkMetadata,
    asset_roots: &[PathBuf],
) -> anyhow::Result<ArchiveLayoutPlan> {
    let code = &metadata.normalized_code;
    let work_dir = archive_root.join(code);
    let mut version = 1usize;
    let file_name = loop {
        let candidate = normalized_file_name(code, &file.path, version);
        if !work_dir.join(&candidate).exists() {
            break candidate;
        }
        version += 1;
    };
    let assets = discover_assets(code, metadata.cover_path.as_ref(), asset_roots);
    Ok(ArchiveLayoutPlan {
        video_target: work_dir.join(file_name),
        nfo_target: work_dir.join(format!("{code}.nfo")),
        work_dir,
        assets,
    })
}

fn discover_assets(
    code: &str,
    cover_path: Option<&PathBuf>,
    asset_roots: &[PathBuf],
) -> Vec<ArchiveAsset> {
    let mut assets = Vec::new();
    if let Some(path) = cover_path {
        assets.push(ArchiveAsset {
            source_path: path.clone(),
            relative_target: PathBuf::from("poster.jpg"),
        });
    }

    for root in asset_roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            let path = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            let code_lower = code.to_ascii_lowercase();
            if !lower.starts_with(&code_lower) {
                continue;
            }
            let relative_target = if lower.ends_with(".gif") {
                PathBuf::from(format!("{code}.gif"))
            } else if lower.contains("-shot") || lower.contains("-screenshot") {
                PathBuf::from("screenshot").join(name)
            } else if lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".png")
                || lower.ends_with(".webp")
            {
                PathBuf::from("poster.jpg")
            } else {
                continue;
            };
            if !assets
                .iter()
                .any(|asset| asset.relative_target == relative_target)
            {
                assets.push(ArchiveAsset {
                    source_path: path.to_path_buf(),
                    relative_target,
                });
            }
        }
    }
    assets
}

/// Result of a successful archive execution. Paths are final archive paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutedArchive {
    pub video_path: PathBuf,
    pub nfo_path: PathBuf,
}

/// Execute one archive layout. Video transfer is staged through a temporary
/// target and verified before the source is removed; NFO/assets are writes or
/// copies. If a later step fails, the video is restored when possible.
pub fn execute_archive_layout(
    plan: &ArchiveLayoutPlan,
    file: &CompletedFile,
    metadata: &ScrapedWorkMetadata,
) -> anyhow::Result<ExecutedArchive> {
    fs::create_dir_all(&plan.work_dir)?;
    stage_video_move(file, &plan.video_target)?;
    let execution = (|| -> anyhow::Result<()> {
        fs::write(&plan.nfo_target, render_scraped_nfo(metadata))?;
        for asset in &plan.assets {
            let target = plan.work_dir.join(&asset.relative_target);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            if !target.exists() {
                fs::copy(&asset.source_path, &target)?;
            }
        }
        Ok(())
    })();

    if let Err(error) = execution {
        let _ = fs::remove_file(&plan.nfo_target);
        let _ = restore_video(&plan.video_target, &file.path);
        return Err(error);
    }

    Ok(ExecutedArchive {
        video_path: plan.video_target.clone(),
        nfo_path: plan.nfo_target.clone(),
    })
}

fn stage_video_move(file: &CompletedFile, target: &Path) -> anyhow::Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let moving = target.with_extension(format!(
        "{}.moving",
        target
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
    ));
    if moving.exists() {
        fs::remove_file(&moving)?;
    }
    if target.exists() {
        return Err(anyhow::anyhow!(
            "archive target already exists: {}",
            target.display()
        ));
    }
    fs::copy(&file.path, &moving)?;
    let copied_size = fs::metadata(&moving)?.len();
    if copied_size != file.size_bytes {
        let _ = fs::remove_file(&moving);
        return Err(anyhow::anyhow!("staged video size mismatch"));
    }
    fs::rename(&moving, target)?;
    fs::remove_file(&file.path)?;
    Ok(())
}

fn restore_video(target: &Path, original: &Path) -> anyhow::Result<()> {
    if original.exists() {
        if target.exists() {
            let _ = fs::remove_file(target);
        }
        return Ok(());
    }
    if target.exists() {
        fs::rename(target, original)?;
    }
    Ok(())
}

/// Persist a successfully archived file as a work plus one file version.
/// User-owned fields are left at defaults so future manual edits stay separate
/// from scraper-owned metadata.
pub fn persist_pipeline_success(
    repo: &Repository,
    file: &CompletedFile,
    metadata: &ScrapedWorkMetadata,
    executed: &ExecutedArchive,
) -> anyhow::Result<i64> {
    let archive_dir = executed
        .video_path
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let poster_path = archive_dir.join("poster.jpg");
    let gif_path = archive_dir.join(format!("{}.gif", metadata.normalized_code));
    let work = Work {
        id: None,
        normalized_code: Some(metadata.normalized_code.clone()),
        source_code: Some(metadata.normalized_code.clone()),
        code_kind: CodeKind::Standard,
        title_zh: Some(metadata.title.clone()),
        original_title: metadata.original_title.clone(),
        aliases: Vec::new(),
        summary: metadata.summary.clone(),
        outline: None,
        cover_path: metadata.cover_path.clone(),
        poster_path: poster_path.exists().then_some(poster_path),
        thumb_path: None,
        fanart_path: None,
        screenshot_path: None,
        gif_path: gif_path.exists().then_some(gif_path),
        tags: metadata.genres.clone(),
        sets: Vec::new(),
        lists: Vec::new(),
        rating: None,
        rating_value: None,
        rating_max: None,
        rating_votes: None,
        criticrating: None,
        watch_status: WatchStatus::Unwatched,
        genres: metadata.genres.clone(),
        studio: metadata.studio.clone(),
        label: None,
        director: metadata.director.clone(),
        release_date: metadata.release_date.clone(),
        runtime_minutes: None,
        year: metadata
            .release_date
            .as_ref()
            .and_then(|date| date.get(0..4))
            .and_then(|year| year.parse::<i32>().ok()),
        website: None,
        mpaa: None,
        has_video: true,
        ratings: Vec::new(),
        watch_progress_seconds: None,
        last_played_at: None,
    };
    let work_id = repo.upsert_work(&work)?;
    repo.set_work_actors(work_id, &metadata.actors, &metadata.source)?;
    repo.set_work_tags(work_id, &metadata.genres)?;
    let normalized_file_name = executed
        .video_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&file.file_name);
    let source_root = file.path.parent().unwrap_or_else(|| Path::new(""));
    repo.upsert_file_version_for_work(
        work_id,
        source_root,
        &file.path,
        Some(&executed.video_path),
        &file.file_name,
        Some(normalized_file_name),
        file.size_bytes,
        file.file_hash.as_deref(),
    )?;
    Ok(work_id)
}

/// Stage 2 automatic pipeline core. It processes one already-complete file and
/// performs all durable writes synchronously; Stage 3 will call this from the
/// daemon worker loop.
pub struct AutoPipeline<'a> {
    pub repo: &'a Repository,
    pub archive_root: PathBuf,
    pub asset_roots: Vec<PathBuf>,
    pub scrapers: ScrapeCoordinator<'a>,
}

impl<'a> AutoPipeline<'a> {
    /// Process one completed file through identification, duplicate check,
    /// scraping, archive execution, and SQLite persistence.
    pub fn process_completed_file(&self, file: CompletedFile) -> anyhow::Result<PipelineOutcome> {
        let mut steps = Vec::new();
        let run_id = self.repo.start_pipeline_run(&file.path)?;

        let result = (|| -> anyhow::Result<PipelineOutcome> {
            match identify_completed_file(&file) {
                PipelineIdentification::Holding { reason } => {
                    steps.push(step("identify", "holding", Some("missing standard code")));
                    let id = self.repo.add_holding(&HoldingEntry {
                        id: None,
                        path: file.path.to_string_lossy().to_string(),
                        file_name: file.file_name.clone(),
                        size_bytes: file.size_bytes as i64,
                        reason,
                        created_at: None,
                    })?;
                    Ok(PipelineOutcome {
                        status: "holding".to_string(),
                        work_id: None,
                        archived_video_path: None,
                        holding_id: Some(id),
                        exception_id: None,
                        steps: steps.clone(),
                    })
                }
                PipelineIdentification::Identified { normalized_code } => {
                    steps.push(step("identify", "ok", Some(&normalized_code)));
                    let Some(hash) = file.file_hash.as_deref() else {
                        let id = self.record_exception(
                            &file,
                            ExceptionKind::DuplicateCandidate,
                            serde_json::json!({
                                "normalized_code": normalized_code,
                                "message": "fingerprint unavailable"
                            })
                            .to_string(),
                        )?;
                        return Ok(exception_outcome(id, &steps));
                    };

                    let matches = self.repo.find_file_versions_by_hash(hash)?;
                    if !matches.is_empty() {
                        let evidence = serde_json::json!({
                            "normalized_code": normalized_code,
                            "file_hash": hash,
                            "matches": matches.iter().filter_map(|version| version.id).collect::<Vec<_>>(),
                        });
                        let id = self.record_exception(
                            &file,
                            ExceptionKind::DuplicateCandidate,
                            evidence.to_string(),
                        )?;
                        return Ok(exception_outcome(id, &steps));
                    }

                    let scrape_context = ScrapeContext {
                        work_id: None,
                        normalized_code: normalized_code.clone(),
                        object_path: file.path.clone(),
                        pipeline_run_id: Some(run_id),
                    };
                    match self.scrapers.scrape(self.repo, &scrape_context) {
                        Ok(metadata) => {
                            steps.push(step("scrape", "ok", Some(&metadata.source)));
                            let plan = plan_archive_layout(
                                &self.archive_root,
                                &file,
                                &metadata,
                                &self.asset_roots,
                            )?;
                            steps.push(step("archive", "planned", Some(&metadata.normalized_code)));
                            let executed = execute_archive_layout(&plan, &file, &metadata)?;
                            steps.push(step("archive", "ok", Some("filesystem finalized")));
                            let work_id =
                                persist_pipeline_success(self.repo, &file, &metadata, &executed)?;
                            steps.push(step("persist", "ok", Some(&work_id.to_string())));
                            Ok(PipelineOutcome {
                                status: "archived".to_string(),
                                work_id: Some(work_id),
                                archived_video_path: Some(executed.video_path),
                                holding_id: None,
                                exception_id: None,
                                steps: steps.clone(),
                            })
                        }
                        Err(PipelineScrapeError::AllSourcesFailed) => {
                            steps.push(step("scrape", "failed", Some("all sources failed")));
                            let evidence =
                                serde_json::json!({ "normalized_code": normalized_code });
                            let id = self.record_exception(
                                &file,
                                ExceptionKind::ScrapeFailed,
                                evidence.to_string(),
                            )?;
                            Ok(exception_outcome(id, &steps))
                        }
                    }
                }
            }
        })();

        match result {
            Ok(outcome) => {
                self.repo
                    .finish_pipeline_run(run_id, &outcome.status, &outcome.steps, None)?;
                Ok(outcome)
            }
            Err(error) => {
                steps.push(step("pipeline", "failed", Some(&error.to_string())));
                self.repo.finish_pipeline_run(
                    run_id,
                    "failed",
                    &steps,
                    Some(&error.to_string()),
                )?;
                Err(error)
            }
        }
    }

    fn record_exception(
        &self,
        file: &CompletedFile,
        kind: ExceptionKind,
        evidence_json: String,
    ) -> anyhow::Result<i64> {
        self.repo.record_exception(&Exception {
            id: None,
            object_path: file.path.to_string_lossy().to_string(),
            kind,
            evidence_json,
            status: ExceptionStatus::Open,
            created_at: None,
            resolved_at: None,
        })
    }
}

fn exception_outcome(exception_id: i64, steps: &[PipelineStepRecord]) -> PipelineOutcome {
    PipelineOutcome {
        status: "exception".to_string(),
        work_id: None,
        archived_video_path: None,
        holding_id: None,
        exception_id: Some(exception_id),
        steps: steps.to_vec(),
    }
}

fn step(step: &str, status: &str, message: Option<&str>) -> PipelineStepRecord {
    PipelineStepRecord {
        step: step.to_string(),
        status: status.to_string(),
        message: message.map(ToString::to_string),
    }
}
