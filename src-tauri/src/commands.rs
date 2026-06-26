use crate::archive::ArchivePlanner;
use crate::control_service::{
    control_service_discovery_path, read_control_service_discovery, ControlServiceDiscovery,
    ControlServiceHandle,
};
use crate::control_service_host::{
    control_service_host_status as build_control_service_host_status, start_control_service_host,
    ControlServiceHostStatus,
};
use crate::daemon::RunOnceReport;
use crate::daemon_control::{
    build_daemon_status, list_exception_entries as read_exception_entries,
    list_holding_entries as read_holding_entries, list_recent_pipeline_runs,
    resolve_exception_entry as resolve_exception_in_repo, run_daemon_once, DaemonControlRuntime,
    DaemonControlStatus,
};
use crate::domain::{
    Actor, ArchiveAction, ArchiveActionLog, ArchivePlan, DimensionCount, Exception,
    ExceptionStatus, FileVersion, HoldingEntry, IngestDecision, IngestItem, IngestItemFilters,
    IngestJobSummary, PipelineRun, ReviewReason, WatchStatus, Work, WorkDetail, WorkFilters,
};
use crate::identifier::normalize_code;
use crate::ingest::IngestEngine;
use crate::provider::{DisabledProvider, ExampleProvider};
use crate::scanner::Scanner;
use crate::storage::Repository;
use crate::thumbnail::{
    clear_thumbnail_cache as clear_thumbnail_cache_dir, get_or_create_thumbnail as create_thumbnail,
    thumbnail_cache_summary as read_thumbnail_cache_summary, ThumbnailCacheSummary,
    DEFAULT_THUMBNAIL_CACHE_LIMIT_BYTES,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use tauri::{Builder, Manager, State};

pub struct AppState {
    pub source_roots: Mutex<Vec<String>>,
    pub archive_root: Mutex<Option<String>>,
    pub ingest_items: Mutex<Vec<IngestItem>>,
    pub repository: Mutex<Option<Repository>>,
    pub metadata_provider_enabled: Mutex<bool>,
    pub archive_plans: Mutex<Vec<ArchivePlan>>,
    pub archive_logs: Mutex<Vec<ArchiveActionLog>>,
    pub daemon_runtime: Mutex<DaemonControlRuntime>,
    pub control_service: Mutex<Option<ControlServiceHandle>>,
    pub control_service_error: Mutex<Option<String>>,
    pub next_job_id: Mutex<i64>,
    pub next_item_id: Mutex<i64>,
    pub next_plan_id: Mutex<i64>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            source_roots: Mutex::new(Vec::new()),
            archive_root: Mutex::new(None),
            ingest_items: Mutex::new(Vec::new()),
            repository: Mutex::new(None),
            metadata_provider_enabled: Mutex::new(false),
            archive_plans: Mutex::new(Vec::new()),
            archive_logs: Mutex::new(Vec::new()),
            daemon_runtime: Mutex::new(DaemonControlRuntime::default()),
            control_service: Mutex::new(None),
            control_service_error: Mutex::new(None),
            next_job_id: Mutex::new(1),
            next_item_id: Mutex::new(1),
            next_plan_id: Mutex::new(1),
        }
    }
}

impl AppState {
    /// Return a serializable snapshot of the app-owned control service host.
    pub fn control_service_host_status(
        &self,
        app_data_dir: &Path,
        last_error: Option<String>,
    ) -> ControlServiceHostStatus {
        let handle_guard = self.control_service.lock().ok();
        let handle = handle_guard.as_ref().and_then(|guard| guard.as_ref());
        build_control_service_host_status(app_data_dir, handle, last_error)
    }
}

impl Drop for AppState {
    /// Shut down the hosted loopback control service when Tauri releases app state.
    fn drop(&mut self) {
        if let Ok(slot) = self.control_service.get_mut() {
            if let Some(handle) = slot.take() {
                let _ = handle.shutdown();
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult<T> {
    pub data: T,
}

#[tauri::command]
pub fn configure_source_roots(
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<String>>, String> {
    if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        repo.set_source_roots(&path_bufs)
            .map_err(|error| error.to_string())?;
    }
    *state.source_roots.lock().map_err(|error| error.to_string())? = paths.clone();
    Ok(CommandResult { data: paths })
}

#[tauri::command]
pub fn configure_archive_root(
    path: String,
    state: State<'_, AppState>,
) -> Result<CommandResult<String>, String> {
    if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        repo.set_archive_root(Path::new(&path))
            .map_err(|error| error.to_string())?;
    }
    *state.archive_root.lock().map_err(|error| error.to_string())? = Some(path.clone());
    Ok(CommandResult { data: path })
}

#[tauri::command]
pub fn get_source_roots(state: State<'_, AppState>) -> Result<CommandResult<Vec<String>>, String> {
    let paths = state
        .source_roots
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    Ok(CommandResult { data: paths })
}

#[tauri::command]
pub fn get_archive_root(state: State<'_, AppState>) -> Result<CommandResult<Option<String>>, String> {
    let path = state
        .archive_root
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    Ok(CommandResult { data: path })
}

#[tauri::command]
pub fn configure_metadata_provider_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<CommandResult<bool>, String> {
    if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        repo.set_metadata_provider_enabled(enabled)
            .map_err(|error| error.to_string())?;
    }
    *state
        .metadata_provider_enabled
        .lock()
        .map_err(|error| error.to_string())? = enabled;
    Ok(CommandResult { data: enabled })
}

#[tauri::command]
pub fn get_metadata_provider_enabled(
    state: State<'_, AppState>,
) -> Result<CommandResult<bool>, String> {
    Ok(CommandResult {
        data: *state
            .metadata_provider_enabled
            .lock()
            .map_err(|error| error.to_string())?,
    })
}

/// Read the Stage 5A loopback control service discovery document from app data.
#[tauri::command]
pub fn get_control_service_discovery(
    app: tauri::AppHandle,
) -> Result<CommandResult<Option<ControlServiceDiscovery>>, String> {
    let app_data = app.path().app_data_dir().map_err(|error| error.to_string())?;
    let discovery_path = control_service_discovery_path(&app_data);
    Ok(CommandResult {
        data: read_control_service_discovery(&discovery_path).map_err(|error| error.to_string())?,
    })
}

/// Return the app-owned control service host status for diagnostics.
#[tauri::command]
pub fn get_control_service_host_status(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<CommandResult<ControlServiceHostStatus>, String> {
    let app_data = app.path().app_data_dir().map_err(|error| error.to_string())?;
    let last_error = state
        .control_service_error
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    Ok(CommandResult {
        data: state.control_service_host_status(&app_data, last_error),
    })
}

/// Return the Stage 4 daemon control status for the settings page.
#[tauri::command]
pub fn get_daemon_status(
    state: State<'_, AppState>,
) -> Result<CommandResult<DaemonControlStatus>, String> {
    Ok(CommandResult {
        data: read_daemon_status(&state)?,
    })
}

/// Pause future synchronous daemon runs without interrupting a running file operation.
#[tauri::command]
pub fn pause_daemon(
    state: State<'_, AppState>,
) -> Result<CommandResult<DaemonControlStatus>, String> {
    {
        let mut runtime = state
            .daemon_runtime
            .lock()
            .map_err(|error| error.to_string())?;
        runtime.paused = true;
    }
    Ok(CommandResult {
        data: read_daemon_status(&state)?,
    })
}

/// Resume future synchronous daemon runs and return the refreshed status.
#[tauri::command]
pub fn resume_daemon(
    state: State<'_, AppState>,
) -> Result<CommandResult<DaemonControlStatus>, String> {
    {
        let mut runtime = state
            .daemon_runtime
            .lock()
            .map_err(|error| error.to_string())?;
        runtime.paused = false;
    }
    Ok(CommandResult {
        data: read_daemon_status(&state)?,
    })
}

/// Run one Stage 4 daemon pass through the headless core.
#[tauri::command]
pub fn run_daemon_once_command(
    state: State<'_, AppState>,
) -> Result<CommandResult<RunOnceReport>, String> {
    let metadata_enabled = configured_metadata_provider_enabled(&state)?;
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    let mut runtime = state
        .daemon_runtime
        .lock()
        .map_err(|error| error.to_string())?;
    match run_daemon_once(repo, &mut runtime, metadata_enabled) {
        Ok(report) => Ok(CommandResult { data: report }),
        Err(error) => {
            runtime.last_error = Some(error.to_string());
            Err(error.to_string())
        }
    }
}

/// Return files parked in the holding pen for manual triage.
#[tauri::command]
pub fn list_holding_entries(
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<HoldingEntry>>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    Ok(CommandResult {
        data: read_holding_entries(repo).map_err(|error| error.to_string())?,
    })
}

/// Return exception queue rows for the automatic pipeline panel.
#[tauri::command]
pub fn list_exception_entries(
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<Exception>>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    Ok(CommandResult {
        data: read_exception_entries(repo).map_err(|error| error.to_string())?,
    })
}

/// Mark one exception as resolved or ignored after user review.
#[tauri::command]
pub fn resolve_exception_entry_command(
    id: i64,
    status: String,
    state: State<'_, AppState>,
) -> Result<CommandResult<bool>, String> {
    let status = parse_exception_status_command(&status)?;
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    resolve_exception_in_repo(repo, id, status).map_err(|error| error.to_string())?;
    Ok(CommandResult { data: true })
}

/// Return recent automatic pipeline runs, newest first.
#[tauri::command]
pub fn list_pipeline_runs(
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<PipelineRun>>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    Ok(CommandResult {
        data: list_recent_pipeline_runs(repo).map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn start_scan(
    source_root_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<CommandResult<IngestJobSummary>, String> {
    let configured_roots = state
        .source_roots
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let roots = if source_root_ids.is_empty() {
        configured_roots
    } else {
        configured_roots
            .into_iter()
            .filter(|root| source_root_ids.contains(root))
            .collect()
    };
    let root_paths: Vec<PathBuf> = roots.into_iter().map(PathBuf::from).collect();
    let mut items = Scanner::scan_sources(&root_paths).map_err(|error| error.to_string())?;
    let provider_enabled = configured_metadata_provider_enabled(&state)?;
    items = decide_items_with_provider_enabled(items, provider_enabled);

    let job_id = if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        let job_id = repo
            .create_ingest_job(&root_paths, &items)
            .map_err(|error| error.to_string())?;
        items = repo
            .list_ingest_items(job_id)
            .map_err(|error| error.to_string())?;
        job_id
    } else {
        let mut next_job_id = state
            .next_job_id
            .lock()
            .map_err(|error| error.to_string())?;
        let job_id = *next_job_id;
        *next_job_id += 1;
        let mut next_item_id = state
            .next_item_id
            .lock()
            .map_err(|error| error.to_string())?;
        for item in items.iter_mut() {
            item.id = Some(*next_item_id);
            *next_item_id += 1;
            item.job_id = Some(job_id);
        }
        job_id
    };
    for item in items.iter_mut() {
        if item.job_id.is_none() {
            item.job_id = Some(job_id);
        }
    }

    let summary = summarize_items(job_id, &items);
    *state
        .ingest_items
        .lock()
        .map_err(|error| error.to_string())? = items;

    Ok(CommandResult { data: summary })
}

#[tauri::command]
pub fn get_ingest_job(
    job_id: i64,
    state: State<'_, AppState>,
) -> Result<CommandResult<IngestJobSummary>, String> {
    if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        let job = repo
            .get_ingest_job(job_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("ingest job {job_id} was not found"))?;
        return Ok(CommandResult { data: job });
    }

    let items = state
        .ingest_items
        .lock()
        .map_err(|error| error.to_string())?;
    let job_items: Vec<IngestItem> = items
        .iter()
        .filter(|item| item.job_id == Some(job_id))
        .cloned()
        .collect();
    Ok(CommandResult {
        data: summarize_items(job_id, &job_items),
    })
}

#[tauri::command]
pub fn get_latest_ingest_job(
    state: State<'_, AppState>,
) -> Result<CommandResult<Option<IngestJobSummary>>, String> {
    if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        return Ok(CommandResult {
            data: repo
                .get_latest_ingest_job()
                .map_err(|error| error.to_string())?,
        });
    }

    let items = state
        .ingest_items
        .lock()
        .map_err(|error| error.to_string())?;
    let Some(job_id) = items.iter().filter_map(|item| item.job_id).max() else {
        return Ok(CommandResult { data: None });
    };
    let job_items: Vec<IngestItem> = items
        .iter()
        .filter(|item| item.job_id == Some(job_id))
        .cloned()
        .collect();
    Ok(CommandResult {
        data: Some(summarize_items(job_id, &job_items)),
    })
}

#[tauri::command]
pub fn list_ingest_items(
    _job_id: i64,
    filters: Option<IngestItemFilters>,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<IngestItem>>, String> {
    if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        let items = if let Some(filters) = filters {
            repo.list_ingest_items_filtered(_job_id, &filters)
        } else {
            repo.list_ingest_items(_job_id)
        }
        .map_err(|error| error.to_string())?;
        return Ok(CommandResult { data: items });
    }

    let mut items: Vec<IngestItem> = state
        .ingest_items
        .lock()
        .map_err(|error| error.to_string())?
        .iter()
        .filter(|item| item.job_id == Some(_job_id))
        .cloned()
        .collect();
    if let Some(filters) = filters {
        items.retain(|item| item_matches_filters(item, &filters));
    }
    Ok(CommandResult { data: items })
}

#[tauri::command]
pub fn list_works(state: State<'_, AppState>) -> Result<CommandResult<Vec<Work>>, String> {
    if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        return Ok(CommandResult {
            data: repo.list_works().map_err(|error| error.to_string())?,
        });
    }
    Ok(CommandResult { data: vec![] })
}

#[tauri::command]
pub fn list_file_versions_for_work(
    work_id: i64,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<FileVersion>>, String> {
    if work_id <= 0 {
        return Err("work_id must be positive".to_string());
    }
    let repo_guard = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    Ok(CommandResult {
        data: repo
            .list_file_versions_for_work(work_id)
            .map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn preview_archive_plan(
    item_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> Result<CommandResult<ArchivePlan>, String> {
    let archive_root = state
        .archive_root
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or_else(|| "archive root is not configured".to_string())?;
    let candidate_items = if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        if item_ids.is_empty() {
            if let Some(job) = repo
                .get_latest_ingest_job()
                .map_err(|error| error.to_string())?
            {
                repo.list_archive_candidate_items_for_job(job.id)
                    .map_err(|error| error.to_string())?
            } else {
                Vec::new()
            }
        } else {
            repo.list_archive_candidate_items_by_ids(&item_ids)
                .map_err(|error| error.to_string())?
        }
    } else {
        let items = state
            .ingest_items
            .lock()
            .map_err(|error| error.to_string())?;
        items
            .iter()
            .filter(|item| {
                item.id
                    .map(|id| item_ids.is_empty() || item_ids.contains(&id))
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    };
    let selected: Vec<IngestItem> = candidate_items
        .into_iter()
        .filter(|item| item.decision == IngestDecision::AutoArchive)
        .collect();
    let planner = ArchivePlanner::new(PathBuf::from(archive_root));
    let mut plan = planner
        .preview(&selected)
        .map_err(|error| error.to_string())?;
    let mut next_plan_id = state
        .next_plan_id
        .lock()
        .map_err(|error| error.to_string())?;
    plan.id = Some(*next_plan_id);
    *next_plan_id += 1;
    state
        .archive_plans
        .lock()
        .map_err(|error| error.to_string())?
        .push(plan.clone());
    Ok(CommandResult { data: plan })
}

#[tauri::command]
pub fn execute_archive_plan(
    plan_id: i64,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<ArchiveActionLog>>, String> {
    let plan = state
        .archive_plans
        .lock()
        .map_err(|error| error.to_string())?
        .iter()
        .find(|plan| plan.id == Some(plan_id))
        .cloned()
        .ok_or_else(|| format!("archive plan {plan_id} was not found"))?;
    if !plan.conflicts.is_empty() {
        return Err("archive plan has unresolved conflicts".to_string());
    }
    let source_roots = configured_source_roots(&state)?;
    let archive_root = configured_archive_root(&state)?;
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let logs = execute_archive_actions(
        &plan.actions,
        &source_roots,
        &archive_root,
        repo_guard.as_ref(),
    )?;

    state
        .archive_logs
        .lock()
        .map_err(|error| error.to_string())?
        .extend(logs.clone());
    Ok(CommandResult { data: logs })
}

#[tauri::command]
pub fn list_archive_action_logs(
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<ArchiveActionLog>>, String> {
    if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        return Ok(CommandResult {
            data: repo
                .list_archive_action_logs()
                .map_err(|error| error.to_string())?,
        });
    }
    Ok(CommandResult {
        data: state
            .archive_logs
            .lock()
            .map_err(|error| error.to_string())?
            .clone(),
    })
}

pub fn execute_archive_actions(
    actions: &[ArchiveAction],
    source_roots: &[PathBuf],
    archive_root: &PathBuf,
    repository: Option<&Repository>,
) -> Result<Vec<ArchiveActionLog>, String> {
    let mut logs = Vec::new();
    for action in actions {
        let result = (|| -> Result<(), String> {
            validate_archive_action_paths(action, source_roots, archive_root)?;
            if action.to_path.exists() {
                return Err("destination already exists".to_string());
            }
            if let Some(parent) = action.to_path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::rename(&action.from_path, &action.to_path).map_err(|error| error.to_string())?;
            Ok(())
        })();

        let mut log = ArchiveActionLog {
            id: None,
            item_id: action.item_id,
            job_id: None,
            from_path: action.from_path.clone(),
            to_path: action.to_path.clone(),
            status: if result.is_ok() { "moved" } else { "failed" }.to_string(),
            message: result.err(),
            created_at: None,
        };
        if let Some(repo) = repository {
            let log_id = repo
                .record_archive_action(&log)
                .map_err(|error| error.to_string())?;
            if let Some(stored_log) = repo
                .get_archive_action_log(log_id)
                .map_err(|error| error.to_string())?
            {
                log = stored_log;
            } else {
                log.id = Some(log_id);
            }
        }
        logs.push(log);
    }

    Ok(logs)
}

#[tauri::command]
pub fn resolve_match(
    item_id: i64,
    work_id: Option<i64>,
    normalized_code: Option<String>,
    state: State<'_, AppState>,
) -> Result<CommandResult<bool>, String> {
    if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        if let Some(work_id) = work_id {
            repo.resolve_ingest_item_to_work(item_id, work_id)
                .map_err(|error| error.to_string())?;
        } else {
            repo.resolve_ingest_item(item_id, normalized_code.clone())
                .map_err(|error| error.to_string())?;
        }
        let mut items = state
            .ingest_items
            .lock()
            .map_err(|error| error.to_string())?;
        if let Some(item) = items.iter_mut().find(|item| item.id == Some(item_id)) {
            if let Some(job_id) = item.job_id {
                if let Some(stored) = repo
                    .list_ingest_items(job_id)
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .find(|stored| stored.id == Some(item_id))
                {
                    *item = stored;
                }
            }
        }
        return Ok(CommandResult { data: true });
    }

    let mut items = state
        .ingest_items
        .lock()
        .map_err(|error| error.to_string())?;
    let item = items
        .iter_mut()
        .find(|item| item.id == Some(item_id))
        .ok_or_else(|| format!("ingest item {item_id} was not found"))?;
    if let Some(code) = normalized_code {
        let normalized = normalize_code(&code)
            .ok_or_else(|| format!("invalid normalized_code: {code}"))?;
        item.normalized_code = Some(normalized);
    }
    item.review_reasons.retain(|reason| {
        !matches!(
            reason,
            ReviewReason::MissingCode | ReviewReason::LowConfidence | ReviewReason::CodeConflict
        )
    });
    item.confidence = 1.0;
    item.decision = IngestDecision::AutoArchive;
    Ok(CommandResult { data: true })
}

#[tauri::command]
pub fn merge_versions(
    work_id: i64,
    file_version_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> Result<CommandResult<bool>, String> {
    if work_id <= 0 {
        return Err("work_id must be positive".to_string());
    }
    if file_version_ids.is_empty() {
        return Err("file_version_ids cannot be empty".to_string());
    }
    let repo_guard = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    repo.merge_file_versions_into_work(work_id, &file_version_ids)
        .map_err(|error| error.to_string())?;
    Ok(CommandResult { data: true })
}

#[tauri::command]
pub fn retry_metadata(
    item_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<IngestItem>>, String> {
    let provider_enabled = configured_metadata_provider_enabled(&state)?;
    if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        let updated = if provider_enabled {
            let engine = IngestEngine::new(ExampleProvider);
            repo.retry_metadata_for_items(&engine, &item_ids)
        } else {
            let engine = IngestEngine::new(DisabledProvider);
            repo.retry_metadata_for_items(&engine, &item_ids)
        }
        .map_err(|error| error.to_string())?;
        let mut items = state
            .ingest_items
            .lock()
            .map_err(|error| error.to_string())?;
        for updated_item in &updated {
            if let Some(id) = updated_item.id {
                if let Some(item) = items.iter_mut().find(|item| item.id == Some(id)) {
                    *item = updated_item.clone();
                }
            }
        }
        return Ok(CommandResult { data: updated });
    }

    let mut items = state
        .ingest_items
        .lock()
        .map_err(|error| error.to_string())?;
    let mut updated = Vec::new();
    for item in items.iter_mut() {
        let Some(id) = item.id else {
            continue;
        };
        if !item_ids.is_empty() && !item_ids.contains(&id) {
            continue;
        }
        item.review_reasons
            .retain(|reason| reason != &ReviewReason::ProviderFailed);
        let decided = decide_items_with_provider_enabled(vec![item.clone()], provider_enabled)
            .into_iter()
            .next()
            .expect("one item was provided");
        *item = decided.clone();
        updated.push(decided);
    }
    Ok(CommandResult { data: updated })
}

#[tauri::command]
pub fn revalidate_move_failed_items(
    item_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<IngestItem>>, String> {
    let repo_guard = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    let updated = repo
        .revalidate_move_failed_items(&item_ids)
        .map_err(|error| error.to_string())?;

    let mut items = state
        .ingest_items
        .lock()
        .map_err(|error| error.to_string())?;
    for updated_item in &updated {
        if let Some(id) = updated_item.id {
            if let Some(item) = items.iter_mut().find(|item| item.id == Some(id)) {
                *item = updated_item.clone();
            }
        }
    }
    Ok(CommandResult { data: updated })
}

#[tauri::command]
pub fn ignore_duplicate_items(
    item_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<IngestItem>>, String> {
    let repo_guard = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    let updated = repo
        .ignore_duplicate_items(&item_ids)
        .map_err(|error| error.to_string())?;

    let mut items = state
        .ingest_items
        .lock()
        .map_err(|error| error.to_string())?;
    for updated_item in &updated {
        if let Some(id) = updated_item.id {
            if let Some(item) = items.iter_mut().find(|item| item.id == Some(id)) {
                *item = updated_item.clone();
            }
        }
    }
    Ok(CommandResult { data: updated })
}


#[tauri::command]
pub fn delete_items(
    item_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<IngestItem>>, String> {
    let repo_guard = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    let updated = repo
        .delete_items(&item_ids)
        .map_err(|error| error.to_string())?;

    let mut items = state
        .ingest_items
        .lock()
        .map_err(|error| error.to_string())?;
    for updated_item in &updated {
        if let Some(id) = updated_item.id {
            if let Some(item) = items.iter_mut().find(|item| item.id == Some(id)) {
                *item = updated_item.clone();
            }
        }
    }
    Ok(CommandResult { data: updated })
}

#[tauri::command]
pub fn list_work_actors(
    work_id: i64,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<Actor>>, String> {
    if work_id <= 0 {
        return Err("work_id must be positive".to_string());
    }
    let repo_guard = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    Ok(CommandResult {
        data: repo
            .list_work_actors(work_id)
            .map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn list_tags(state: State<'_, AppState>) -> Result<CommandResult<Vec<DimensionCount>>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Ok(CommandResult { data: vec![] });
    };
    Ok(CommandResult {
        data: repo.list_tags().map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn list_sets(state: State<'_, AppState>) -> Result<CommandResult<Vec<DimensionCount>>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Ok(CommandResult { data: vec![] });
    };
    Ok(CommandResult {
        data: repo.list_sets().map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn list_studios(
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<DimensionCount>>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Ok(CommandResult { data: vec![] });
    };
    Ok(CommandResult {
        data: repo.list_studios().map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn list_labels(
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<DimensionCount>>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Ok(CommandResult { data: vec![] });
    };
    Ok(CommandResult {
        data: repo.list_labels().map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn list_work_actors_for_name(
    name: String,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<Actor>>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Ok(CommandResult { data: vec![] });
    };
    Ok(CommandResult {
        data: repo
            .list_work_actors_for_name(&name)
            .map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn list_works_filtered(
    filters: WorkFilters,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<Work>>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Ok(CommandResult { data: vec![] });
    };
    Ok(CommandResult {
        data: repo
            .list_works_filtered(filters)
            .map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn list_work_detail(
    work_id: i64,
    state: State<'_, AppState>,
) -> Result<CommandResult<Option<WorkDetail>>, String> {
    if work_id <= 0 {
        return Err("work_id must be positive".to_string());
    }
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Ok(CommandResult { data: None });
    };
    Ok(CommandResult {
        data: repo
            .get_work_detail(work_id)
            .map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn preview_rebuild(
    source_roots: Vec<String>,
    state: State<'_, AppState>,
) -> Result<CommandResult<crate::library_rebuild::RebuildReport>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    let roots: Vec<PathBuf> = source_roots.into_iter().map(PathBuf::from).collect();
    Ok(CommandResult {
        data: repo
            .preview_rebuild(&roots)
            .map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn rebuild_library_from_nfo(
    source_roots: Vec<String>,
    state: State<'_, AppState>,
) -> Result<CommandResult<crate::library_rebuild::RebuildReport>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    let roots: Vec<PathBuf> = source_roots.into_iter().map(PathBuf::from).collect();
    // Build the external image index from configured poster/screenshot/gif dirs
    // so rebuild backfills covers and previews for works without NFO artwork.
    let (poster_dir, screenshot_dir, gif_dir) = repo
        .get_poster_dirs()
        .map_err(|error| error.to_string())?;
    let empty = std::path::PathBuf::new();
    let poster_index = crate::poster_index::PosterIndex::scan(
        poster_dir.as_deref().unwrap_or(&empty),
        screenshot_dir.as_deref().unwrap_or(&empty),
        gif_dir.as_deref().unwrap_or(&empty),
    );
    Ok(CommandResult {
        data: repo
            .rebuild_library(&roots, &poster_index)
            .map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn configure_poster_dirs(
    poster_dir: String,
    screenshot_dir: String,
    gif_dir: String,
    state: State<'_, AppState>,
) -> Result<CommandResult<bool>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    repo.set_poster_dirs(
        std::path::Path::new(&poster_dir),
        std::path::Path::new(&screenshot_dir),
        std::path::Path::new(&gif_dir),
    )
    .map_err(|error| error.to_string())?;
    Ok(CommandResult { data: true })
}

#[derive(serde::Serialize)]
pub struct PosterDirs {
    pub poster_dir: Option<String>,
    pub screenshot_dir: Option<String>,
    pub gif_dir: Option<String>,
}

#[tauri::command]
pub fn get_poster_dirs(
    state: State<'_, AppState>,
) -> Result<CommandResult<PosterDirs>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    let (poster, screenshot, gif) = repo
        .get_poster_dirs()
        .map_err(|error| error.to_string())?;
    Ok(CommandResult {
        data: PosterDirs {
            poster_dir: poster.map(|p| p.to_string_lossy().to_string()),
            screenshot_dir: screenshot.map(|p| p.to_string_lossy().to_string()),
            gif_dir: gif.map(|p| p.to_string_lossy().to_string()),
        },
    })
}

#[tauri::command]
pub fn update_work_profile(
    work_id: i64,
    tags: Vec<String>,
    lists: Vec<String>,
    rating: Option<u8>,
    status: String,
    state: State<'_, AppState>,
) -> Result<CommandResult<Work>, String> {
    if let Some(value) = rating {
        if value > 10 {
            return Err("rating must be between 0 and 10".to_string());
        }
    }
    let watch_status = parse_watch_status(&status);
    if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        let work = repo
            .update_work_profile(work_id, tags, lists, rating, watch_status)
            .map_err(|error| error.to_string())?;
        return Ok(CommandResult { data: work });
    }
    Err("repository is not available".to_string())
}

#[tauri::command]
pub fn open_file_in_system(path: String) -> Result<CommandResult<bool>, String> {
    let path = require_existing_path(Path::new(&path))?;
    open_system_path(&path)?;
    Ok(CommandResult { data: true })
}

#[tauri::command]
pub fn open_path_in_file_manager(path: String) -> Result<CommandResult<bool>, String> {
    let path = require_existing_path(Path::new(&path))?;
    let directory = if path.is_dir() {
        path
    } else {
        existing_file_parent_directory(&path)?
    };
    open_system_path(&directory)?;
    Ok(CommandResult { data: true })
}

#[tauri::command]
pub fn get_or_create_thumbnail(
    path: String,
    app: tauri::AppHandle,
) -> Result<CommandResult<Option<PathBuf>>, String> {
    let video_path = require_existing_path(Path::new(&path))?;
    let cache_root = thumbnail_cache_root(&app)?;
    let thumbnail = create_thumbnail(
        &video_path,
        &cache_root,
        DEFAULT_THUMBNAIL_CACHE_LIMIT_BYTES,
    )
    .map_err(|error| error.to_string())?;
    Ok(CommandResult { data: thumbnail })
}

#[tauri::command]
pub fn get_thumbnail_cache_summary(
    app: tauri::AppHandle,
) -> Result<CommandResult<ThumbnailCacheSummary>, String> {
    let cache_root = thumbnail_cache_root(&app)?;
    let summary = read_thumbnail_cache_summary(&cache_root).map_err(|error| error.to_string())?;
    Ok(CommandResult { data: summary })
}

#[tauri::command]
pub fn clear_thumbnail_cache(
    app: tauri::AppHandle,
) -> Result<CommandResult<ThumbnailCacheSummary>, String> {
    let cache_root = thumbnail_cache_root(&app)?;
    let summary = clear_thumbnail_cache_dir(&cache_root).map_err(|error| error.to_string())?;
    Ok(CommandResult { data: summary })
}

#[tauri::command]
pub fn plan_centralized_migration(
    nfo_dir: String,
    video_dir: String,
    target_dir: String,
) -> Result<CommandResult<crate::domain::MigrationPlan>, String> {
    let plan = crate::migration::plan_migration(
        Path::new(&nfo_dir),
        Path::new(&video_dir),
        Path::new(&target_dir),
    )
    .map_err(|error| error.to_string())?;
    Ok(CommandResult { data: plan })
}

#[tauri::command]
pub fn execute_centralized_migration(
    plan: crate::domain::MigrationPlan,
) -> Result<CommandResult<usize>, String> {
    let migrated = crate::migration::execute_migration(&plan)
        .map_err(|error| error.to_string())?;
    Ok(CommandResult { data: migrated })
}

#[tauri::command]
pub fn configure_resource_pool_dirs(
    dirs: Vec<String>,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<String>>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    let paths: Vec<PathBuf> = dirs.iter().map(PathBuf::from).collect();
    repo.set_resource_pool_dirs(&paths)
        .map_err(|error| error.to_string())?;
    Ok(CommandResult { data: dirs })
}

#[tauri::command]
pub fn get_resource_pool_dirs(
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<String>>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    let dirs = repo
        .get_resource_pool_dirs()
        .map_err(|error| error.to_string())?;
    let strings: Vec<String> = dirs.into_iter().map(|p| p.to_string_lossy().to_string()).collect();
    Ok(CommandResult { data: strings })
}

#[tauri::command]
pub async fn scan_resource_pool(
    dirs: Vec<String>,
) -> Result<CommandResult<crate::domain::ResourcePool>, String> {
    let roots: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
    // Run the recursive walk + NFO parsing off the main thread so the WebView
    // stays responsive while scanning hundreds of NFOs.
    let pool = tauri::async_runtime::spawn_blocking(move || {
        crate::resource_pool::scan_resource_pool(&roots)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|error| error.to_string())?;
    Ok(CommandResult { data: pool })
}

#[tauri::command]
pub async fn plan_unified_migration(
    dirs: Vec<String>,
    target_dir: String,
) -> Result<CommandResult<crate::domain::UnifiedMigrationPlan>, String> {
    let roots: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        let pool = crate::resource_pool::scan_resource_pool(&roots)?;
        Ok::<_, anyhow::Error>(crate::migration::plan_unified_migration(
            &pool,
            Path::new(&target_dir),
        ))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|error| error.to_string())?;
    Ok(CommandResult { data: plan })
}

#[tauri::command]
pub async fn execute_unified_migration(
    plan: crate::domain::UnifiedMigrationPlan,
) -> Result<CommandResult<usize>, String> {
    let migrated = tauri::async_runtime::spawn_blocking(move || {
        crate::migration::execute_unified_migration(&plan)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|error| error.to_string())?;
    Ok(CommandResult { data: migrated })
}

#[tauri::command]
pub async fn rebuild_library_from_pool(
    dirs: Vec<String>,
    state: State<'_, AppState>,
) -> Result<CommandResult<crate::library_rebuild::RebuildReport>, String> {
    // 1. Scan the pool off the main thread (slow: walks dirs + parses NFOs).
    let roots: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
    let pool = tauri::async_runtime::spawn_blocking(move || {
        crate::resource_pool::scan_resource_pool(&roots)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|error| error.to_string())?;

    // 2. DB writes stay on the main thread (fast: batched inside one transaction).
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    Ok(CommandResult {
        data: repo
            .rebuild_library_from_pool(&pool)
            .map_err(|error| error.to_string())?,
    })
}

#[tauri::command]
pub fn configure_primary_library_dir(
    dir: String,
    state: State<'_, AppState>,
) -> Result<CommandResult<String>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    repo.set_primary_library_dir(Path::new(&dir))
        .map_err(|error| error.to_string())?;
    Ok(CommandResult { data: dir })
}

#[tauri::command]
pub fn get_primary_library_dir(
    state: State<'_, AppState>,
) -> Result<CommandResult<Option<String>>, String> {
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    let dir = repo
        .get_primary_library_dir()
        .map_err(|error| error.to_string())?;
    Ok(CommandResult {
        data: dir.map(|p| p.to_string_lossy().to_string()),
    })
}

/// Incremental sync: copy missing resources from the pool into the primary
/// library dir, then upsert works (no clear). The everyday "add new / fill
/// gaps" path that keeps the primary dir as the single authority.
#[tauri::command]
pub async fn incremental_sync(
    dirs: Vec<String>,
    primary_dir: String,
    state: State<'_, AppState>,
) -> Result<CommandResult<crate::library_rebuild::RebuildReport>, String> {
    let roots: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
    let primary = PathBuf::from(&primary_dir);
    // 1. Scan pool + copy resources into primary (off main thread).
    let pool = tauri::async_runtime::spawn_blocking(move || -> Result<_, anyhow::Error> {
        let pool = crate::resource_pool::scan_resource_pool(&roots)?;
        // Physical fill happens here so the (locked) DB step below only upserts.
        for work in &pool.works {
            let work_dir = primary.join(&work.code);
            let _ = crate::migration::sync_work_into_primary(work, &work_dir, &primary);
        }
        Ok(pool)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|error| error.to_string())?;

    // 2. Upsert (no clear) on the main thread.
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    Ok(CommandResult {
        data: repo
            .incremental_sync_from_pool(&pool, Path::new(&primary_dir))
            .map_err(|error| error.to_string())?,
    })
}

pub fn build_app() -> Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            configure_source_roots,
            configure_archive_root,
            get_source_roots,
            get_archive_root,
            configure_metadata_provider_enabled,
            get_metadata_provider_enabled,
            get_control_service_discovery,
            get_control_service_host_status,
            get_daemon_status,
            pause_daemon,
            resume_daemon,
            run_daemon_once_command,
            list_holding_entries,
            list_exception_entries,
            resolve_exception_entry_command,
            list_pipeline_runs,
            start_scan,
            get_ingest_job,
            get_latest_ingest_job,
            list_ingest_items,
            list_works,
            list_tags,
            list_sets,
            list_studios,
            list_labels,
            list_work_actors_for_name,
            list_works_filtered,
            list_work_detail,
            preview_rebuild,
            rebuild_library_from_nfo,
            configure_poster_dirs,
            get_poster_dirs,
            list_file_versions_for_work,
            list_work_actors,
            preview_archive_plan,
            execute_archive_plan,
            list_archive_action_logs,
            resolve_match,
            merge_versions,
            retry_metadata,
            revalidate_move_failed_items,
            ignore_duplicate_items,
            delete_items,
            update_work_profile,
            open_file_in_system,
            open_path_in_file_manager,
            get_or_create_thumbnail,
            get_thumbnail_cache_summary,
            clear_thumbnail_cache,
            plan_centralized_migration,
            execute_centralized_migration,
            configure_resource_pool_dirs,
            get_resource_pool_dirs,
            scan_resource_pool,
            plan_unified_migration,
            execute_unified_migration,
            rebuild_library_from_pool,
            configure_primary_library_dir,
            get_primary_library_dir,
            incremental_sync
        ])
        .setup(|app| {
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|error| error.to_string())?;
            fs::create_dir_all(&app_data).map_err(|error| error.to_string())?;
            let repo = open_repository(&app_data.join("library.sqlite"))
                .map_err(|error| error.to_string())?;
            let state = app.state::<AppState>();
            let source_roots: Vec<String> = repo
                .get_source_roots()
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect();
            let archive_root = repo
                .get_archive_root()
                .map_err(|error| error.to_string())?
                .map(|path| path.to_string_lossy().to_string());
            let metadata_provider_enabled = repo
                .get_metadata_provider_enabled()
                .map_err(|error| error.to_string())?;
            *state.source_roots.lock().map_err(|error| error.to_string())? = source_roots;
            *state.archive_root.lock().map_err(|error| error.to_string())? = archive_root;
            *state
                .metadata_provider_enabled
                .lock()
                .map_err(|error| error.to_string())? = metadata_provider_enabled;
            *state.repository.lock().map_err(|error| error.to_string())? = Some(repo);
            match start_control_service_host(&app_data) {
                Ok(handle) => {
                    *state
                        .control_service
                        .lock()
                        .map_err(|error| error.to_string())? = Some(handle);
                    *state
                        .control_service_error
                        .lock()
                        .map_err(|error| error.to_string())? = None;
                }
                Err(error) => {
                    *state
                        .control_service_error
                        .lock()
                        .map_err(|lock_error| lock_error.to_string())? = Some(error.to_string());
                }
            }
            Ok(())
        })
}

fn thumbnail_cache_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?
        .join("thumbnails"))
}

pub fn open_repository(path: &Path) -> anyhow::Result<Repository> {
    let repo = Repository::open(path)?;
    repo.migrate()?;
    Ok(repo)
}

fn summarize_items(job_id: i64, items: &[IngestItem]) -> IngestJobSummary {
    let total_items = items.len();
    let auto_count = items
        .iter()
        .filter(|item| item.decision == IngestDecision::AutoArchive)
        .count();
    let failed_count = items
        .iter()
        .filter(|item| item.decision == IngestDecision::Failed)
        .count();
    let review_count = items
        .iter()
        .filter(|item| {
            matches!(
                item.decision,
                IngestDecision::NeedsReview | IngestDecision::DuplicateCandidate
            )
        })
        .count();
    IngestJobSummary {
        id: job_id,
        status: "completed".to_string(),
        total_items,
        auto_count,
        review_count,
        failed_count,
    }
}

pub fn decide_items_with_provider_enabled(
    items: Vec<IngestItem>,
    provider_enabled: bool,
) -> Vec<IngestItem> {
    if provider_enabled {
        let engine = IngestEngine::new(ExampleProvider);
        items.into_iter().map(|item| engine.decide(item)).collect()
    } else {
        let engine = IngestEngine::new(DisabledProvider);
        items.into_iter().map(|item| engine.decide(item)).collect()
    }
}

fn item_matches_filters(item: &IngestItem, filters: &IngestItemFilters) -> bool {
    if let Some(decision) = filters.decision.as_ref() {
        if &item.decision != decision {
            return false;
        }
    }
    if let Some(reason) = filters.review_reason.as_ref() {
        if !item.review_reasons.contains(reason) {
            return false;
        }
    }
    if let Some(has_code) = filters.has_code {
        if item.normalized_code.is_some() != has_code {
            return false;
        }
    }
    true
}

fn parse_watch_status(value: &str) -> WatchStatus {
    match value {
        "Watched" | "watched" => WatchStatus::Watched,
        "Favorite" | "favorite" => WatchStatus::Favorite,
        "WantToWatch" | "want_to_watch" | "wanttowatch" => WatchStatus::WantToWatch,
        "Watching" | "watching" => WatchStatus::Watching,
        "OnHold" | "on_hold" | "onhold" => WatchStatus::OnHold,
        _ => WatchStatus::Unwatched,
    }
}

fn parse_exception_status_command(value: &str) -> Result<ExceptionStatus, String> {
    match value {
        "Ignored" | "ignored" => Ok(ExceptionStatus::Ignored),
        "Resolved" | "resolved" => Ok(ExceptionStatus::Resolved),
        _ => Err("exception status must be Ignored or Resolved".to_string()),
    }
}

pub fn validate_archive_action_paths(
    action: &ArchiveAction,
    source_roots: &[PathBuf],
    archive_root: &PathBuf,
) -> Result<(), String> {
    let from_path = existing_canonical_path(&action.from_path)?;
    let source_roots = canonical_existing_roots(source_roots)?;
    if !source_roots.iter().any(|root| from_path.starts_with(root)) {
        return Err("archive source path is outside configured source roots".to_string());
    }

    let archive_root = existing_canonical_path(archive_root)?;
    let to_parent = action
        .to_path
        .parent()
        .ok_or_else(|| "archive destination has no parent".to_string())?;
    let to_parent = if to_parent.exists() {
        existing_canonical_path(to_parent)?
    } else {
        let nearest = nearest_existing_parent(to_parent)
            .ok_or_else(|| "archive destination parent cannot be validated".to_string())?;
        existing_canonical_path(&nearest)?
    };
    if !to_parent.starts_with(&archive_root) {
        return Err("archive destination path is outside configured archive root".to_string());
    }

    Ok(())
}

fn configured_source_roots(state: &State<'_, AppState>) -> Result<Vec<PathBuf>, String> {
    let roots = state
        .source_roots
        .lock()
        .map_err(|error| error.to_string())?;
    Ok(roots.iter().map(PathBuf::from).collect())
}

fn configured_archive_root(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    let root = state
        .archive_root
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or_else(|| "archive root is not configured".to_string())?;
    Ok(PathBuf::from(root))
}

fn configured_metadata_provider_enabled(state: &State<'_, AppState>) -> Result<bool, String> {
    Ok(*state
        .metadata_provider_enabled
        .lock()
        .map_err(|error| error.to_string())?)
}

fn read_daemon_status(state: &State<'_, AppState>) -> Result<DaemonControlStatus, String> {
    let metadata_enabled = configured_metadata_provider_enabled(state)?;
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let Some(repo) = repo_guard.as_ref() else {
        return Err("repository is not available".to_string());
    };
    let runtime = state
        .daemon_runtime
        .lock()
        .map_err(|error| error.to_string())?;
    build_daemon_status(repo, &runtime, metadata_enabled).map_err(|error| error.to_string())
}

fn canonical_existing_roots(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    paths
        .iter()
        .map(|path| existing_canonical_path(path))
        .collect()
}

fn existing_canonical_path(path: &std::path::Path) -> Result<PathBuf, String> {
    path.canonicalize().map_err(|error| error.to_string())
}

pub fn require_existing_path(path: &std::path::Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("path does not exist: {}", path.to_string_lossy()));
    }
    Ok(path.to_path_buf())
}

pub fn existing_file_parent_directory(path: &std::path::Path) -> Result<PathBuf, String> {
    let path = require_existing_path(path)?;
    if path.is_dir() {
        return Ok(path);
    }
    let parent = path
        .parent()
        .ok_or_else(|| format!("path has no parent directory: {}", path.to_string_lossy()))?;
    require_existing_path(parent)
}

fn open_system_path(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let status = Command::new("cmd")
        .args(["/C", "start", "", &path.to_string_lossy()])
        .status()
        .map_err(|error| error.to_string())?;

    #[cfg(target_os = "macos")]
    let status = Command::new("open")
        .arg(path)
        .status()
        .map_err(|error| error.to_string())?;

    #[cfg(all(unix, not(target_os = "macos")))]
    let status = Command::new("xdg-open")
        .arg(path)
        .status()
        .map_err(|error| error.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("system opener exited with status: {status}"))
    }
}

fn nearest_existing_parent(path: &std::path::Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        if current.exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_watch_status_parser_accepts_stage1_statuses() {
        assert_eq!(parse_watch_status("WantToWatch"), WatchStatus::WantToWatch);
        assert_eq!(parse_watch_status("Watching"), WatchStatus::Watching);
        assert_eq!(parse_watch_status("OnHold"), WatchStatus::OnHold);
        assert_eq!(parse_watch_status("watched"), WatchStatus::Watched);
        assert_eq!(parse_watch_status("favorite"), WatchStatus::Favorite);
        assert_eq!(parse_watch_status("unknown"), WatchStatus::Unwatched);
    }

    #[test]
    fn default_app_state_reports_no_control_service_handle() {
        let state = AppState::default();
        let error = Some("startup failed".to_string());
        let tmp = tempfile::tempdir().unwrap();

        let status = state.control_service_host_status(tmp.path(), error.clone());

        assert!(!status.running);
        assert_eq!(status.port, None);
        assert_eq!(status.last_error, error);
        assert!(status.discovery_path.ends_with("control-service.json"));
    }
}
