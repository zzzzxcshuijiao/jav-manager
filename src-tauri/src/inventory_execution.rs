use crate::inventory::{
    InventoryPreviewAction, InventoryPreviewReport, InventoryResourceKind, InventoryReviewBucket,
    InventoryWorkPreview,
};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

/// File operation mode supported by the inventory execution stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryExecutionMode {
    Copy,
}

/// User request for executing a previously generated inventory preview report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryExecutionRequest {
    pub mode: InventoryExecutionMode,
    pub selected_codes: Vec<String>,
}

/// Per-action result status returned to the UI after an inventory execution run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryExecutionActionStatus {
    Copied,
    Failed,
    RolledBack,
}

/// One file-level execution log entry for a copied or rolled-back inventory action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryExecutionActionLog {
    pub code: String,
    pub kind: InventoryResourceKind,
    pub from_path: PathBuf,
    pub to_path: PathBuf,
    pub status: InventoryExecutionActionStatus,
    pub message: Option<String>,
    pub bytes: u64,
}

/// Summary of a copy-only inventory execution run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryExecutionReport {
    pub mode: InventoryExecutionMode,
    pub started_at: String,
    pub finished_at: String,
    pub requested_works: usize,
    pub executed_works: usize,
    pub skipped_works: usize,
    pub planned_actions: usize,
    pub copied_actions: usize,
    pub failed_actions: usize,
    pub rolled_back_actions: usize,
    pub bytes_copied: u64,
    pub logs: Vec<InventoryExecutionActionLog>,
}

#[derive(Debug, Clone)]
struct PreparedInventoryAction {
    code: String,
    kind: InventoryResourceKind,
    from_path: PathBuf,
    to_path: PathBuf,
}

#[derive(Debug, Clone)]
struct PreparedInventoryExecution {
    requested_works: usize,
    skipped_works: usize,
    archive_root_canonical: PathBuf,
    actions: Vec<PreparedInventoryAction>,
}

#[derive(Debug, Clone)]
struct CreatedInventoryTarget {
    code: String,
    kind: InventoryResourceKind,
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

/// Execute a preview report's safe inventory plan in copy-only mode.
pub fn execute_inventory_report(
    report: &InventoryPreviewReport,
    request: &InventoryExecutionRequest,
) -> Result<InventoryExecutionReport> {
    let prepared = prepare_inventory_execution(report, request)?;
    let started_at = Utc::now().to_rfc3339();
    let run_id = format!("{}-{}", std::process::id(), Utc::now().timestamp_micros());
    let mut logs = Vec::new();
    let mut created_targets = Vec::new();
    let mut copied_counts_by_code: BTreeMap<String, usize> = BTreeMap::new();
    let mut planned_counts_by_code: BTreeMap<String, usize> = BTreeMap::new();
    let mut copied_actions = 0;
    let mut failed_actions = 0;
    let mut rolled_back_actions = 0;
    let mut bytes_copied = 0;

    for action in &prepared.actions {
        *planned_counts_by_code
            .entry(action.code.clone())
            .or_default() += 1;
    }

    for (index, action) in prepared.actions.iter().enumerate() {
        match copy_prepared_action(action, &prepared.archive_root_canonical, &run_id, index) {
            Ok(created) => {
                copied_actions += 1;
                bytes_copied += created.bytes;
                created_targets.push(created.clone());
                *copied_counts_by_code
                    .entry(action.code.clone())
                    .or_default() += 1;
                logs.push(InventoryExecutionActionLog {
                    code: action.code.clone(),
                    kind: action.kind.clone(),
                    from_path: action.from_path.clone(),
                    to_path: action.to_path.clone(),
                    status: InventoryExecutionActionStatus::Copied,
                    message: None,
                    bytes: created.bytes,
                });
            }
            Err(error) => {
                failed_actions += 1;
                logs.push(InventoryExecutionActionLog {
                    code: action.code.clone(),
                    kind: action.kind.clone(),
                    from_path: action.from_path.clone(),
                    to_path: action.to_path.clone(),
                    status: InventoryExecutionActionStatus::Failed,
                    message: Some(error.to_string()),
                    bytes: 0,
                });
                rolled_back_actions += rollback_created_targets(&created_targets, &mut logs);
                break;
            }
        }
    }

    let executed_works = if failed_actions == 0 {
        planned_counts_by_code
            .iter()
            .filter(|(code, planned_count)| {
                copied_counts_by_code.get(*code).copied().unwrap_or(0) == **planned_count
            })
            .count()
    } else {
        0
    };

    Ok(InventoryExecutionReport {
        mode: request.mode,
        started_at,
        finished_at: Utc::now().to_rfc3339(),
        requested_works: prepared.requested_works,
        executed_works,
        skipped_works: prepared.skipped_works,
        planned_actions: prepared.actions.len(),
        copied_actions,
        failed_actions,
        rolled_back_actions,
        bytes_copied,
        logs,
    })
}

/// Build and validate the action list before any filesystem write starts.
fn prepare_inventory_execution(
    report: &InventoryPreviewReport,
    request: &InventoryExecutionRequest,
) -> Result<PreparedInventoryExecution> {
    match request.mode {
        InventoryExecutionMode::Copy => {}
    }

    let archive_root = report
        .archive_root
        .as_ref()
        .ok_or_else(|| anyhow!("未配置整理目标目录"))?;
    let selected_codes = normalized_selected_codes(&request.selected_codes);
    if selected_codes.is_empty() && report.summary.works > report.works.len() {
        bail!("盘点报告作品明细已截断，不能执行全部作品");
    }

    let selected_works = select_inventory_works(report, &selected_codes)?;
    if selected_works.is_empty() {
        bail!("没有可复制整理的作品");
    }

    let skipped_works = if selected_codes.is_empty() {
        report
            .works
            .iter()
            .filter(|work| !is_auto_ready_work(work))
            .count()
    } else {
        0
    };

    let archive_root_canonical = ensure_execution_archive_root(archive_root)?;
    let mut target_keys = BTreeSet::new();
    let mut actions = Vec::new();
    for work in &selected_works {
        for action in &work.resolution.execution_plan.actions {
            actions.push(validate_inventory_action(
                &work.code,
                action,
                archive_root,
                &archive_root_canonical,
                &mut target_keys,
            )?);
        }
    }
    if actions.is_empty() {
        bail!("安全执行计划没有可复制动作");
    }

    Ok(PreparedInventoryExecution {
        requested_works: selected_works.len(),
        skipped_works,
        archive_root_canonical,
        actions,
    })
}

/// Normalize user-selected codes while preserving deterministic request order.
fn normalized_selected_codes(codes: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    codes
        .iter()
        .map(|code| code.trim())
        .filter(|code| !code.is_empty())
        .filter_map(|code| {
            let owned = code.to_string();
            if seen.insert(owned.clone()) {
                Some(owned)
            } else {
                None
            }
        })
        .collect()
}

/// Select all auto-ready works or validate the explicit user-selected work list.
fn select_inventory_works<'a>(
    report: &'a InventoryPreviewReport,
    selected_codes: &[String],
) -> Result<Vec<&'a InventoryWorkPreview>> {
    if selected_codes.is_empty() {
        return Ok(report
            .works
            .iter()
            .filter(|work| is_auto_ready_work(work))
            .collect());
    }

    let works_by_code: BTreeMap<&str, &InventoryWorkPreview> = report
        .works
        .iter()
        .map(|work| (work.code.as_str(), work))
        .collect();
    let mut works = Vec::new();
    for code in selected_codes {
        let work = works_by_code
            .get(code.as_str())
            .copied()
            .ok_or_else(|| anyhow!("未找到选中的作品：{code}"))?;
        if !is_auto_ready_work(work) {
            bail!("作品 {code} 不是可自动整理状态");
        }
        works.push(work);
    }
    Ok(works)
}

/// Check the Stage 7B.1 readiness contract used by inventory execution.
fn is_auto_ready_work(work: &InventoryWorkPreview) -> bool {
    work.resolution.bucket == InventoryReviewBucket::AutoReady
        && work.resolution.execution_plan.ready
}

/// Validate one execution-plan action and convert it into an owned prepared action.
fn validate_inventory_action(
    code: &str,
    action: &InventoryPreviewAction,
    archive_root: &Path,
    archive_root_canonical: &Path,
    target_keys: &mut BTreeSet<String>,
) -> Result<PreparedInventoryAction> {
    if action.conflict.as_deref().unwrap_or_default().trim().len() > 0 {
        bail!("作品 {code} 的执行计划动作仍有冲突");
    }
    let to_path = action
        .to_path
        .clone()
        .ok_or_else(|| anyhow!("作品 {code} 的执行计划缺少目标路径"))?;
    if !path_is_inside_root(&to_path, archive_root)? {
        bail!("作品 {code} 的目标路径位于整理目标目录之外");
    }
    if !target_existing_parent_is_inside_root(&to_path, archive_root_canonical)? {
        bail!("作品 {code} 的目标路径位于整理目标目录之外");
    }
    let from_metadata = fs::metadata(&action.from_path)
        .with_context(|| format!("源文件不存在：{}", action.from_path.to_string_lossy()))?;
    if !from_metadata.is_file() {
        bail!("源路径不是文件：{}", action.from_path.to_string_lossy());
    }
    if to_path.exists() {
        bail!("目标路径已存在：{}", to_path.to_string_lossy());
    }
    let target_key = normalized_path_key(&to_path)?;
    if !target_keys.insert(target_key) {
        bail!(
            "同一批复制中存在重复目标路径：{}",
            to_path.to_string_lossy()
        );
    }

    Ok(PreparedInventoryAction {
        code: code.to_string(),
        kind: action.kind.clone(),
        from_path: action.from_path.clone(),
        to_path,
    })
}

/// Copy one validated action through a temporary destination file.
fn copy_prepared_action(
    action: &PreparedInventoryAction,
    archive_root_canonical: &Path,
    run_id: &str,
    index: usize,
) -> Result<CreatedInventoryTarget> {
    let parent = action
        .to_path
        .parent()
        .ok_or_else(|| anyhow!("目标路径没有父目录：{}", action.to_path.to_string_lossy()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("创建目标目录失败：{}", parent.to_string_lossy()))?;
    if !target_existing_parent_is_inside_root(&action.to_path, archive_root_canonical)? {
        bail!("目标路径位于整理目标目录之外");
    }
    let temp_path = temporary_copy_path(&action.to_path, run_id, index)?;
    let source_size = fs::metadata(&action.from_path)
        .with_context(|| format!("读取源文件失败：{}", action.from_path.to_string_lossy()))?
        .len();
    let (copied_size, sha256) = copy_source_to_new_temp(&action.from_path, &temp_path)?;
    let temp_size = fs::metadata(&temp_path)
        .with_context(|| format!("读取临时文件失败：{}", temp_path.to_string_lossy()))?
        .len();
    if copied_size != source_size || temp_size != source_size {
        let _ = fs::remove_file(&temp_path);
        bail!("复制后大小校验失败：{}", action.from_path.to_string_lossy());
    }
    persist_temp_without_clobber(&temp_path, &action.to_path)
        .with_context(|| format!("提交复制结果失败：{}", action.to_path.to_string_lossy()))?;
    Ok(CreatedInventoryTarget {
        code: action.code.clone(),
        kind: action.kind.clone(),
        path: action.to_path.clone(),
        bytes: source_size,
        sha256,
    })
}

/// Build a same-directory temp file path that will not collide with existing files.
fn temporary_copy_path(to_path: &Path, run_id: &str, index: usize) -> Result<PathBuf> {
    let parent = to_path
        .parent()
        .ok_or_else(|| anyhow!("目标路径没有父目录：{}", to_path.to_string_lossy()))?;
    let file_name = to_path
        .file_name()
        .ok_or_else(|| anyhow!("目标路径没有文件名：{}", to_path.to_string_lossy()))?
        .to_string_lossy();
    for attempt in 0..100 {
        let candidate = parent.join(format!(
            ".{file_name}.mm-copying-{run_id}-{index}-{attempt}"
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!("无法生成临时复制文件名：{}", to_path.to_string_lossy());
}

/// Copy the source into a newly created temp file while computing its content hash.
fn copy_source_to_new_temp(from_path: &Path, temp_path: &Path) -> Result<(u64, String)> {
    let mut input = File::open(from_path)
        .with_context(|| format!("打开源文件失败：{}", from_path.to_string_lossy()))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .with_context(|| format!("创建临时复制文件失败：{}", temp_path.to_string_lossy()))?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .with_context(|| format!("读取源文件失败：{}", from_path.to_string_lossy()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .with_context(|| format!("写入临时复制文件失败：{}", temp_path.to_string_lossy()))?;
        total += read as u64;
    }
    output
        .sync_all()
        .with_context(|| format!("同步临时复制文件失败：{}", temp_path.to_string_lossy()))?;
    Ok((total, hex_digest(hasher.finalize().as_slice())))
}

/// Make the copied temp file visible at the final path without replacing an existing file.
fn persist_temp_without_clobber(temp_path: &Path, to_path: &Path) -> Result<()> {
    match fs::hard_link(temp_path, to_path) {
        Ok(()) => {
            let _ = fs::remove_file(temp_path);
            Ok(())
        }
        Err(hard_link_error) => {
            if to_path.exists() {
                let _ = fs::remove_file(temp_path);
                bail!("目标路径已存在：{}", to_path.to_string_lossy());
            }
            match copy_temp_to_new_target(temp_path, to_path) {
                Ok(()) => {
                    let _ = fs::remove_file(temp_path);
                    Ok(())
                }
                Err(error) => {
                    let _ = fs::remove_file(temp_path);
                    Err(error).with_context(|| {
                        format!("hard link fallback after failure: {}", hard_link_error)
                    })
                }
            }
        }
    }
}

/// Fallback for filesystems that cannot hard-link: stream temp into a create-new final file.
fn copy_temp_to_new_target(temp_path: &Path, to_path: &Path) -> Result<()> {
    let mut input = File::open(temp_path)
        .with_context(|| format!("打开临时复制文件失败：{}", temp_path.to_string_lossy()))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to_path)
        .with_context(|| format!("创建目标文件失败：{}", to_path.to_string_lossy()))?;
    if let Err(error) = io::copy(&mut input, &mut output) {
        let _ = fs::remove_file(to_path);
        return Err(error)
            .with_context(|| format!("写入目标文件失败：{}", to_path.to_string_lossy()));
    }
    if let Err(error) = output.sync_all() {
        let _ = fs::remove_file(to_path);
        return Err(error)
            .with_context(|| format!("同步目标文件失败：{}", to_path.to_string_lossy()));
    }
    Ok(())
}

/// Remove files created earlier in the same run after a runtime failure.
fn rollback_created_targets(
    created_targets: &[CreatedInventoryTarget],
    logs: &mut Vec<InventoryExecutionActionLog>,
) -> usize {
    let mut rolled_back = 0;
    for target in created_targets.iter().rev() {
        match created_target_still_matches(target) {
            Ok(true) if fs::remove_file(&target.path).is_ok() => {
                rolled_back += 1;
                logs.push(InventoryExecutionActionLog {
                    code: target.code.clone(),
                    kind: target.kind.clone(),
                    from_path: target.path.clone(),
                    to_path: target.path.clone(),
                    status: InventoryExecutionActionStatus::RolledBack,
                    message: Some("复制失败后已删除本轮生成的目标文件".to_string()),
                    bytes: 0,
                });
            }
            Ok(true) => {
                logs.push(InventoryExecutionActionLog {
                    code: target.code.clone(),
                    kind: target.kind.clone(),
                    from_path: target.path.clone(),
                    to_path: target.path.clone(),
                    status: InventoryExecutionActionStatus::Failed,
                    message: Some("回滚失败：无法删除本轮生成的目标文件".to_string()),
                    bytes: 0,
                });
            }
            Ok(false) | Err(_) => {
                logs.push(InventoryExecutionActionLog {
                    code: target.code.clone(),
                    kind: target.kind.clone(),
                    from_path: target.path.clone(),
                    to_path: target.path.clone(),
                    status: InventoryExecutionActionStatus::Failed,
                    message: Some("回滚跳过：目标文件已不存在或已被外部修改".to_string()),
                    bytes: 0,
                });
            }
        }
    }
    rolled_back
}

/// Confirm that a rollback target still has the bytes and hash created by this run.
fn created_target_still_matches(target: &CreatedInventoryTarget) -> Result<bool> {
    let metadata = match fs::metadata(&target.path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(false),
    };
    if !metadata.is_file() || metadata.len() != target.bytes {
        return Ok(false);
    }
    Ok(file_sha256(&target.path)? == target.sha256)
}

/// Compute a file SHA-256 hash for rollback identity checks.
fn file_sha256(path: &Path) -> Result<String> {
    let mut input =
        File::open(path).with_context(|| format!("打开文件失败：{}", path.to_string_lossy()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .with_context(|| format!("读取文件失败：{}", path.to_string_lossy()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(hasher.finalize().as_slice()))
}

/// Format a digest as lowercase hexadecimal without adding another dependency.
fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Ensure the archive root exists and return its canonical path for link-aware checks.
fn ensure_execution_archive_root(archive_root: &Path) -> Result<PathBuf> {
    if archive_root.exists() {
        let metadata = fs::metadata(archive_root)
            .with_context(|| format!("读取整理目标目录失败：{}", archive_root.to_string_lossy()))?;
        if !metadata.is_dir() {
            bail!("整理目标不是目录：{}", archive_root.to_string_lossy());
        }
    } else {
        fs::create_dir_all(archive_root)
            .with_context(|| format!("创建整理目标目录失败：{}", archive_root.to_string_lossy()))?;
    }
    fs::canonicalize(archive_root)
        .with_context(|| format!("解析整理目标目录失败：{}", archive_root.to_string_lossy()))
}

/// Check the nearest existing target parent after resolving links and junctions.
fn target_existing_parent_is_inside_root(
    path: &Path,
    archive_root_canonical: &Path,
) -> Result<bool> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("目标路径没有父目录：{}", path.to_string_lossy()))?;
    let existing_parent = nearest_existing_parent(parent)
        .ok_or_else(|| anyhow!("目标父目录无法校验：{}", parent.to_string_lossy()))?;
    let existing_parent = fs::canonicalize(&existing_parent)
        .with_context(|| format!("解析目标父目录失败：{}", existing_parent.to_string_lossy()))?;
    path_starts_with_canonical_root(&existing_parent, archive_root_canonical)
}

/// Find the closest existing ancestor for a path without requiring the path itself to exist.
fn nearest_existing_parent(path: &Path) -> Option<PathBuf> {
    let mut candidate = path.to_path_buf();
    loop {
        if candidate.exists() {
            return Some(candidate);
        }
        if !candidate.pop() {
            return None;
        }
    }
}

/// Compare canonical paths with Windows case folding.
fn path_starts_with_canonical_root(path: &Path, root: &Path) -> Result<bool> {
    let path_key = normalized_existing_path_key(path)?;
    let root_key = normalized_existing_path_key(root)?;
    Ok(path_key == root_key || path_key.starts_with(&format!("{root_key}/")))
}

/// Normalize an existing canonical path for string-prefix comparison.
fn normalized_existing_path_key(path: &Path) -> Result<String> {
    let key = path.to_string_lossy().replace('\\', "/");
    let key = key.trim_end_matches('/').to_string();
    if cfg!(windows) {
        Ok(key.to_ascii_lowercase())
    } else {
        Ok(key)
    }
}

/// Return true when `path` is the root itself or a descendant of `root`.
fn path_is_inside_root(path: &Path, root: &Path) -> Result<bool> {
    let path_key = normalized_path_key(path)?;
    let root_key = normalized_path_key(root)?;
    Ok(path_key == root_key || path_key.starts_with(&format!("{root_key}/")))
}

/// Convert a path into an absolute lexical comparison key without requiring it to exist.
fn normalized_path_key(path: &Path) -> Result<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let normalized = normalize_lexical_path(&absolute);
    let key = normalized.to_string_lossy().replace('\\', "/");
    let key = key.trim_end_matches('/').to_string();
    if cfg!(windows) {
        Ok(key.to_ascii_lowercase())
    } else {
        Ok(key)
    }
}

/// Collapse `.` and `..` path components for safety checks without touching the filesystem.
fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}
