use crate::identifier::normalize_code;
use crate::inventory::InventoryResourceKind;
use crate::inventory_move::{move_file_no_clobber, SystemInventoryMoveStrategy};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const POST_MIGRATION_DETAIL_LIMIT: usize = 1000;
const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts",
];
const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];
const QUARANTINE_MARKERS: &[&str] = &[".mm-source-delete-", ".mm-moving-", ".mm-copying-"];

/// High-level category for one post-migration review group.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostMigrationGroupKind {
    Quarantine,
    MultiVideo,
    AssetOnly,
    ExternalAsset,
}

/// File operation kind that can be executed after a post-migration preview.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostMigrationActionKind {
    Move,
    DeleteQuarantine,
    RestoreQuarantine,
}

/// Execution result status for one post-migration action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostMigrationExecutionStatus {
    Moved,
    Deleted,
    Restored,
    Skipped,
    Failed,
}

/// One file discovered during post-migration review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostMigrationResource {
    pub path: PathBuf,
    pub file_name: String,
    pub kind: InventoryResourceKind,
    pub size_bytes: u64,
    pub code: String,
}

/// One executable or blocked action proposed by the post-migration preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostMigrationAction {
    pub id: String,
    pub code: String,
    pub kind: PostMigrationActionKind,
    pub resource_kind: InventoryResourceKind,
    pub from_path: PathBuf,
    pub to_path: Option<PathBuf>,
    pub bytes: u64,
    pub conflict: Option<String>,
    pub note: String,
}

/// One code-grouped review item shown after the main inventory migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostMigrationGroup {
    pub code: String,
    pub kind: PostMigrationGroupKind,
    pub source_dir: PathBuf,
    pub archive_dir: PathBuf,
    pub resources: Vec<PostMigrationResource>,
    pub actions: Vec<PostMigrationAction>,
    pub warnings: Vec<String>,
}

/// Aggregate counters for the post-migration review report.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostMigrationSummary {
    pub scanned_files: usize,
    pub groups: usize,
    pub quarantine_files: usize,
    pub cleanup_candidates: usize,
    pub restore_candidates: usize,
    pub multi_video_groups: usize,
    pub asset_only_groups: usize,
    pub external_asset_groups: usize,
    pub ready_actions: usize,
    pub blocked_actions: usize,
    pub move_actions: usize,
    pub delete_actions: usize,
    pub restore_actions: usize,
    pub bytes_planned: u64,
}

/// Read-only report for residual files left after the main inventory migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostMigrationReviewReport {
    pub generated_at: String,
    pub roots: Vec<PathBuf>,
    pub archive_root: PathBuf,
    pub summary: PostMigrationSummary,
    pub groups: Vec<PostMigrationGroup>,
    pub warnings: Vec<String>,
    pub truncated: bool,
}

/// Request for executing a previously generated post-migration review report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostMigrationExecutionRequest {
    pub selected_action_ids: Vec<String>,
}

/// One file-level execution log for the post-migration supplemental stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostMigrationExecutionLog {
    pub action_id: String,
    pub code: String,
    pub kind: PostMigrationActionKind,
    pub resource_kind: InventoryResourceKind,
    pub from_path: PathBuf,
    pub to_path: Option<PathBuf>,
    pub status: PostMigrationExecutionStatus,
    pub message: Option<String>,
    pub bytes: u64,
}

/// Execution report for post-migration cleanup and supplemental moves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostMigrationExecutionReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_path: Option<String>,
    pub started_at: String,
    pub finished_at: String,
    pub requested_actions: usize,
    pub executed_actions: usize,
    pub moved_actions: usize,
    pub deleted_actions: usize,
    pub restored_actions: usize,
    pub skipped_actions: usize,
    pub failed_actions: usize,
    pub bytes_moved: u64,
    pub bytes_deleted: u64,
    pub bytes_restored: u64,
    pub logs: Vec<PostMigrationExecutionLog>,
}

#[derive(Debug, Clone)]
struct ScannedResidual {
    path: PathBuf,
    file_name: String,
    original_file_name: String,
    kind: InventoryResourceKind,
    size_bytes: u64,
    code: Option<String>,
    parent: PathBuf,
    quarantine: bool,
}

/// Scan source roots and build a post-migration supplemental plan without writing files.
pub fn preview_post_migration_roots(
    roots: &[PathBuf],
    archive_root: &Path,
) -> Result<PostMigrationReviewReport> {
    let mut warnings = Vec::new();
    let mut residuals = Vec::new();
    let archive_root = archive_root.to_path_buf();
    let archive_key = canonical_key(&archive_root);
    let archive_codes = archive_work_codes(&archive_root)?;
    let mut seen_files = BTreeSet::new();

    for root in roots {
        if !root.exists() {
            warnings.push(format!("复盘根目录不存在：{}", root.to_string_lossy()));
            continue;
        }
        for entry in WalkDir::new(root).follow_links(false) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(format!("读取复盘目录失败：{error}"));
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path_is_under_key(path, &archive_key) {
                continue;
            }
            if !seen_files.insert(canonical_key(path)) {
                continue;
            }
            match classify_residual(path) {
                Ok(residual) => residuals.push(residual),
                Err(error) => warnings.push(format!(
                    "复盘文件分类失败：{}：{}",
                    path.to_string_lossy(),
                    error
                )),
            }
        }
    }

    Ok(build_review_report(
        roots,
        archive_root,
        archive_codes,
        residuals,
        warnings,
    ))
}

/// Execute every ready action in a post-migration review report, or a selected subset.
pub fn execute_post_migration_report(
    report: &PostMigrationReviewReport,
    request: &PostMigrationExecutionRequest,
) -> Result<PostMigrationExecutionReport> {
    let selected: BTreeSet<_> = request.selected_action_ids.iter().cloned().collect();
    let all_actions = report.groups.iter().flat_map(|group| group.actions.iter());
    let actions: Vec<_> = all_actions
        .filter(|action| {
            if selected.is_empty() {
                action.conflict.is_none()
            } else {
                selected.contains(&action.id)
            }
        })
        .collect();
    let started_at = Utc::now().to_rfc3339();
    let mut logs = Vec::new();
    let mut moved_actions = 0;
    let mut deleted_actions = 0;
    let mut restored_actions = 0;
    let mut skipped_actions = 0;
    let mut failed_actions = 0;
    let mut bytes_moved = 0;
    let mut bytes_deleted = 0;
    let mut bytes_restored = 0;

    for action in actions {
        if let Some(conflict) = &action.conflict {
            skipped_actions += 1;
            logs.push(execution_log(
                action,
                PostMigrationExecutionStatus::Skipped,
                Some(format!("动作存在冲突，已跳过：{conflict}")),
            ));
            continue;
        }
        let result = validate_post_migration_action(report, action)
            .and_then(|()| execute_post_migration_action(action));
        match result {
            Ok(status) => {
                match status {
                    PostMigrationExecutionStatus::Moved => {
                        moved_actions += 1;
                        bytes_moved += action.bytes;
                    }
                    PostMigrationExecutionStatus::Deleted => {
                        deleted_actions += 1;
                        bytes_deleted += action.bytes;
                    }
                    PostMigrationExecutionStatus::Restored => {
                        restored_actions += 1;
                        bytes_restored += action.bytes;
                    }
                    PostMigrationExecutionStatus::Skipped => skipped_actions += 1,
                    PostMigrationExecutionStatus::Failed => failed_actions += 1,
                }
                logs.push(execution_log(action, status, None));
            }
            Err(error) => {
                failed_actions += 1;
                logs.push(execution_log(
                    action,
                    PostMigrationExecutionStatus::Failed,
                    Some(error.to_string()),
                ));
            }
        }
    }

    Ok(PostMigrationExecutionReport {
        report_path: None,
        started_at,
        finished_at: Utc::now().to_rfc3339(),
        requested_actions: logs.len(),
        executed_actions: moved_actions + deleted_actions + restored_actions,
        moved_actions,
        deleted_actions,
        restored_actions,
        skipped_actions,
        failed_actions,
        bytes_moved,
        bytes_deleted,
        bytes_restored,
        logs,
    })
}

/// Rebuild the review report immediately before execution so file operations use backend-generated paths.
pub fn execute_post_migration_roots(
    roots: &[PathBuf],
    archive_root: &Path,
    request: &PostMigrationExecutionRequest,
) -> Result<PostMigrationExecutionReport> {
    let report = preview_post_migration_roots(roots, archive_root)?;
    execute_post_migration_report(&report, request)
}

fn validate_post_migration_action(
    report: &PostMigrationReviewReport,
    action: &PostMigrationAction,
) -> Result<()> {
    if !path_is_under_any_root(&action.from_path, &report.roots) {
        bail!(
            "补迁源路径不在复盘入口目录内：{}",
            action.from_path.to_string_lossy()
        );
    }
    match action.kind {
        PostMigrationActionKind::Move | PostMigrationActionKind::DeleteQuarantine => {
            let to_path = action
                .to_path
                .as_ref()
                .ok_or_else(|| anyhow!("动作缺少目标路径"))?;
            if !path_is_under_root(to_path, &report.archive_root) {
                bail!("补迁目标不在整理目标目录内：{}", to_path.to_string_lossy());
            }
        }
        PostMigrationActionKind::RestoreQuarantine => {
            let to_path = action
                .to_path
                .as_ref()
                .ok_or_else(|| anyhow!("动作缺少恢复目标路径"))?;
            if !path_is_under_any_root(to_path, &report.roots) {
                bail!(
                    "隔离恢复目标不在复盘入口目录内：{}",
                    to_path.to_string_lossy()
                );
            }
        }
    }
    Ok(())
}

fn build_review_report(
    roots: &[PathBuf],
    archive_root: PathBuf,
    archive_codes: BTreeSet<String>,
    residuals: Vec<ScannedResidual>,
    warnings: Vec<String>,
) -> PostMigrationReviewReport {
    let scanned_files = residuals.len();
    let mut groups = Vec::new();
    groups.extend(build_quarantine_groups(&archive_root, &residuals));
    groups.extend(build_multi_video_groups(&archive_root, &residuals));
    groups.extend(build_asset_only_groups(
        &archive_root,
        &archive_codes,
        &residuals,
    ));
    mark_duplicate_targets(&mut groups);
    groups.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.source_dir.cmp(&right.source_dir))
    });
    let mut summary = summarize_groups(&groups);
    summary.scanned_files = scanned_files;
    let truncated = groups.len() > POST_MIGRATION_DETAIL_LIMIT;
    if groups.len() > POST_MIGRATION_DETAIL_LIMIT {
        groups.truncate(POST_MIGRATION_DETAIL_LIMIT);
    }

    PostMigrationReviewReport {
        generated_at: Utc::now().to_rfc3339(),
        roots: roots.to_vec(),
        archive_root,
        summary,
        groups,
        warnings,
        truncated,
    }
}

fn classify_residual(path: &Path) -> Result<ScannedResidual> {
    let metadata = fs::metadata(path)?;
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let original_file_name = original_name_from_quarantine(&file_name).unwrap_or(file_name.clone());
    let kind = classify_kind(&original_file_name);
    let code = normalize_code(&original_file_name).or_else(|| {
        path.parent()
            .and_then(Path::file_name)
            .and_then(|value| normalize_code(&value.to_string_lossy()))
    });
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("文件没有父目录：{}", path.to_string_lossy()))?
        .to_path_buf();
    Ok(ScannedResidual {
        path: path.to_path_buf(),
        file_name,
        original_file_name,
        kind,
        size_bytes: metadata.len(),
        code,
        parent,
        quarantine: is_quarantine_file_name(path),
    })
}

fn classify_kind(file_name: &str) -> InventoryResourceKind {
    let path = Path::new(file_name);
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    if extension == "nfo" {
        InventoryResourceKind::Nfo
    } else if VIDEO_EXTS.contains(&extension.as_str()) {
        InventoryResourceKind::Video
    } else if extension == "gif" {
        InventoryResourceKind::Gif
    } else if IMAGE_EXTS.contains(&extension.as_str()) {
        image_kind_from_stem(&stem)
    } else {
        InventoryResourceKind::Other
    }
}

fn image_kind_from_stem(stem: &str) -> InventoryResourceKind {
    let lower = stem.to_ascii_lowercase();
    let suffix = lower
        .rsplit(|ch| matches!(ch, '-' | '_' | '.' | ' '))
        .next()
        .unwrap_or("");
    if normalize_code(stem)
        .map(|code| lower == code.to_ascii_lowercase())
        .unwrap_or(false)
        || matches!(suffix, "poster" | "cover" | "ps")
    {
        return InventoryResourceKind::Poster;
    }
    if matches!(suffix, "fanart" | "background" | "pl") {
        return InventoryResourceKind::Fanart;
    }
    if matches!(suffix, "thumb" | "thumbnail") {
        return InventoryResourceKind::Thumb;
    }
    if matches!(suffix, "shot" | "screenshot" | "sample") || lower.contains("-shot") {
        return InventoryResourceKind::Screenshot;
    }
    InventoryResourceKind::Image
}

fn build_quarantine_groups(
    archive_root: &Path,
    residuals: &[ScannedResidual],
) -> Vec<PostMigrationGroup> {
    residuals
        .iter()
        .filter(|residual| residual.quarantine)
        .filter_map(|residual| {
            let code = residual.code.clone()?;
            let archive_dir = archive_root.join(&code);
            let restore_path = residual.parent.join(&residual.original_file_name);
            let (kind, to_path, conflict, note) = if let Some(target_path) =
                verified_quarantine_target(residual, &code, &archive_dir)
            {
                (
                    PostMigrationActionKind::DeleteQuarantine,
                    Some(target_path),
                    None,
                    "目标文件已存在且内容一致，可清理源侧隔离残留".to_string(),
                )
            } else if archive_has_same_kind_candidate(residual, &code, &archive_dir) {
                (
                    PostMigrationActionKind::DeleteQuarantine,
                    quarantine_target_candidates(residual, &code, &archive_dir)
                        .into_iter()
                        .next(),
                    Some("target_unverified".to_string()),
                    "归档目录存在同类候选但未通过内容校验，不能自动清理或恢复隔离残留".to_string(),
                )
            } else if restore_path.exists() {
                (
                    PostMigrationActionKind::RestoreQuarantine,
                    Some(restore_path),
                    Some("restore_target_exists".to_string()),
                    "原始源文件名已存在，不能自动恢复隔离残留".to_string(),
                )
            } else {
                (
                    PostMigrationActionKind::RestoreQuarantine,
                    Some(restore_path),
                    None,
                    "目标文件缺失，可把隔离残留恢复成原始源文件".to_string(),
                )
            };
            Some(PostMigrationGroup {
                code: code.clone(),
                kind: PostMigrationGroupKind::Quarantine,
                source_dir: residual.parent.clone(),
                archive_dir,
                resources: vec![resource_from_residual(residual, &code)],
                actions: vec![PostMigrationAction {
                    id: action_id(&kind, &residual.path),
                    code,
                    kind,
                    resource_kind: residual.kind.clone(),
                    from_path: residual.path.clone(),
                    to_path,
                    bytes: residual.size_bytes,
                    conflict,
                    note,
                }],
                warnings: Vec::new(),
            })
        })
        .collect()
}

fn verified_quarantine_target(
    residual: &ScannedResidual,
    code: &str,
    archive_dir: &Path,
) -> Option<PathBuf> {
    let source_hash = sha256_file(&residual.path).ok()?;
    quarantine_target_candidates(residual, code, archive_dir)
        .into_iter()
        .find(|candidate| {
            file_size(candidate) == Some(residual.size_bytes)
                && sha256_file(candidate)
                    .map(|target_hash| target_hash == source_hash)
                    .unwrap_or(false)
        })
}

fn archive_has_same_kind_candidate(
    residual: &ScannedResidual,
    code: &str,
    archive_dir: &Path,
) -> bool {
    quarantine_target_candidates(residual, code, archive_dir)
        .into_iter()
        .any(|candidate| candidate.exists())
}

fn quarantine_target_candidates(
    residual: &ScannedResidual,
    code: &str,
    archive_dir: &Path,
) -> Vec<PathBuf> {
    let mut candidates = BTreeSet::new();
    candidates.insert(archive_dir.join(target_relative_path(code, residual, 0)));
    if archive_dir.exists() {
        for entry in WalkDir::new(archive_dir).max_depth(2).follow_links(false) {
            let Ok(entry) = entry else {
                continue;
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if classify_kind(&path.file_name().unwrap_or_default().to_string_lossy())
                == residual.kind
                && same_extension(path, &residual.original_file_name)
            {
                candidates.insert(path.to_path_buf());
            }
        }
    }
    candidates.into_iter().collect()
}

fn build_multi_video_groups(
    archive_root: &Path,
    residuals: &[ScannedResidual],
) -> Vec<PostMigrationGroup> {
    let mut by_dir: BTreeMap<(String, PathBuf), Vec<&ScannedResidual>> = BTreeMap::new();
    for residual in residuals {
        if residual.quarantine {
            continue;
        }
        let Some(code) = &residual.code else {
            continue;
        };
        by_dir
            .entry((code.clone(), residual.parent.clone()))
            .or_default()
            .push(residual);
    }
    by_dir
        .into_iter()
        .filter_map(|((code, source_dir), mut items)| {
            let video_count = items
                .iter()
                .filter(|item| item.kind == InventoryResourceKind::Video)
                .count();
            if video_count < 2 {
                return None;
            }
            items.sort_by(|left, right| left.path.cmp(&right.path));
            let archive_dir = archive_root.join(&code);
            let resources = items
                .iter()
                .map(|item| resource_from_residual(item, &code))
                .collect::<Vec<_>>();
            let actions = actions_for_resources(&code, &archive_dir, &items, true);
            Some(PostMigrationGroup {
                code,
                kind: PostMigrationGroupKind::MultiVideo,
                source_dir,
                archive_dir,
                resources,
                actions,
                warnings: vec![
                    "多视频作品将按稳定 v2/v3 命名补迁，请在执行前确认分集顺序".to_string()
                ],
            })
        })
        .collect()
}

fn build_asset_only_groups(
    archive_root: &Path,
    archive_codes: &BTreeSet<String>,
    residuals: &[ScannedResidual],
) -> Vec<PostMigrationGroup> {
    let mut by_code: BTreeMap<String, Vec<&ScannedResidual>> = BTreeMap::new();
    for residual in residuals {
        if residual.quarantine || residual.kind == InventoryResourceKind::Video {
            continue;
        }
        let Some(code) = &residual.code else {
            continue;
        };
        by_code.entry(code.clone()).or_default().push(residual);
    }
    by_code
        .into_iter()
        .filter_map(|(code, mut items)| {
            if !archive_codes.contains(&code) {
                return None;
            }
            items.sort_by(|left, right| left.path.cmp(&right.path));
            let archive_dir = archive_root.join(&code);
            let source_dir = common_parent(&items);
            let resources = items
                .iter()
                .map(|item| resource_from_residual(item, &code))
                .collect::<Vec<_>>();
            let actions = actions_for_resources(&code, &archive_dir, &items, false);
            let warnings = if looks_like_external_tool_output(&source_dir) {
                vec!["检测到外部工具素材目录，将按普通素材补迁并保留原文件名".to_string()]
            } else {
                Vec::new()
            };
            Some(PostMigrationGroup {
                code,
                kind: PostMigrationGroupKind::AssetOnly,
                source_dir,
                archive_dir,
                resources,
                actions,
                warnings,
            })
        })
        .collect()
}

fn actions_for_resources(
    code: &str,
    archive_dir: &Path,
    items: &[&ScannedResidual],
    include_videos: bool,
) -> Vec<PostMigrationAction> {
    let mut video_index = 0usize;
    items
        .iter()
        .filter_map(|item| {
            if !include_videos && item.kind == InventoryResourceKind::Video {
                return None;
            }
            let index = if item.kind == InventoryResourceKind::Video {
                let index = video_index;
                video_index += 1;
                index
            } else {
                0
            };
            let target = archive_dir.join(target_relative_path(code, item, index));
            let conflict = if target.exists() {
                Some("target_exists".to_string())
            } else {
                None
            };
            Some(PostMigrationAction {
                id: action_id(&PostMigrationActionKind::Move, &item.path),
                code: code.to_string(),
                kind: PostMigrationActionKind::Move,
                resource_kind: item.kind.clone(),
                from_path: item.path.clone(),
                to_path: Some(target),
                bytes: item.size_bytes,
                conflict,
                note: "补迁到集中归档目录".to_string(),
            })
        })
        .collect()
}

fn target_relative_path(code: &str, item: &ScannedResidual, video_index: usize) -> PathBuf {
    match item.kind {
        InventoryResourceKind::Video => {
            let name = if video_index == 0 {
                code.to_string()
            } else {
                format!("{code}-v{}", video_index + 1)
            };
            PathBuf::from(named_with_original_extension(
                &name,
                &item.original_file_name,
            ))
        }
        InventoryResourceKind::Nfo => PathBuf::from(format!("{code}.nfo")),
        InventoryResourceKind::Poster => PathBuf::from(named_with_original_extension(
            "poster",
            &item.original_file_name,
        )),
        InventoryResourceKind::Fanart => PathBuf::from(named_with_original_extension(
            "fanart",
            &item.original_file_name,
        )),
        InventoryResourceKind::Thumb => PathBuf::from(named_with_original_extension(
            "thumb",
            &item.original_file_name,
        )),
        InventoryResourceKind::Screenshot => PathBuf::from("screenshots").join(&item.file_name),
        InventoryResourceKind::Gif => PathBuf::from("gifs").join(&item.file_name),
        InventoryResourceKind::Image | InventoryResourceKind::Other => {
            PathBuf::from("images").join(&item.file_name)
        }
    }
}

fn execute_post_migration_action(
    action: &PostMigrationAction,
) -> Result<PostMigrationExecutionStatus> {
    match action.kind {
        PostMigrationActionKind::Move => {
            let to_path = action
                .to_path
                .as_ref()
                .ok_or_else(|| anyhow!("补迁动作缺少目标路径"))?;
            move_file_no_clobber(&action.from_path, to_path, &SystemInventoryMoveStrategy)?;
            Ok(PostMigrationExecutionStatus::Moved)
        }
        PostMigrationActionKind::DeleteQuarantine => {
            if !is_quarantine_file_name(&action.from_path) {
                bail!(
                    "隔离清理源文件缺少迁移隔离标记：{}",
                    action.from_path.to_string_lossy()
                );
            }
            let target = action
                .to_path
                .as_ref()
                .ok_or_else(|| anyhow!("隔离清理动作缺少校验目标路径"))?;
            let source_size = fs::metadata(&action.from_path)
                .with_context(|| {
                    format!("读取隔离残留失败：{}", action.from_path.to_string_lossy())
                })?
                .len();
            let target_size = fs::metadata(target)
                .with_context(|| format!("读取目标文件失败：{}", target.to_string_lossy()))?
                .len();
            if source_size != target_size {
                bail!("隔离残留与目标大小不一致，拒绝清理");
            }
            if sha256_file(&action.from_path)? != sha256_file(target)? {
                bail!("隔离残留与目标内容不一致，拒绝清理");
            }
            fs::remove_file(&action.from_path).with_context(|| {
                format!("删除隔离残留失败：{}", action.from_path.to_string_lossy())
            })?;
            Ok(PostMigrationExecutionStatus::Deleted)
        }
        PostMigrationActionKind::RestoreQuarantine => {
            let to_path = action
                .to_path
                .as_ref()
                .ok_or_else(|| anyhow!("隔离恢复动作缺少目标路径"))?;
            if to_path.exists() {
                bail!("恢复目标已存在：{}", to_path.to_string_lossy());
            }
            if let Some(parent) = to_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&action.from_path, to_path).with_context(|| {
                format!(
                    "恢复隔离残留失败：{} -> {}",
                    action.from_path.to_string_lossy(),
                    to_path.to_string_lossy()
                )
            })?;
            Ok(PostMigrationExecutionStatus::Restored)
        }
    }
}

fn execution_log(
    action: &PostMigrationAction,
    status: PostMigrationExecutionStatus,
    message: Option<String>,
) -> PostMigrationExecutionLog {
    PostMigrationExecutionLog {
        action_id: action.id.clone(),
        code: action.code.clone(),
        kind: action.kind.clone(),
        resource_kind: action.resource_kind.clone(),
        from_path: action.from_path.clone(),
        to_path: action.to_path.clone(),
        status,
        message,
        bytes: action.bytes,
    }
}

fn summarize_groups(groups: &[PostMigrationGroup]) -> PostMigrationSummary {
    let mut summary = PostMigrationSummary {
        groups: groups.len(),
        ..Default::default()
    };
    for group in groups {
        match group.kind {
            PostMigrationGroupKind::Quarantine => summary.quarantine_files += group.resources.len(),
            PostMigrationGroupKind::MultiVideo => summary.multi_video_groups += 1,
            PostMigrationGroupKind::AssetOnly => summary.asset_only_groups += 1,
            PostMigrationGroupKind::ExternalAsset => summary.external_asset_groups += 1,
        }
        for action in &group.actions {
            if action.conflict.is_some() {
                summary.blocked_actions += 1;
            } else {
                summary.ready_actions += 1;
                summary.bytes_planned += action.bytes;
            }
            match action.kind {
                PostMigrationActionKind::Move => summary.move_actions += 1,
                PostMigrationActionKind::DeleteQuarantine => {
                    summary.delete_actions += 1;
                    if action.conflict.is_none() {
                        summary.cleanup_candidates += 1;
                    }
                }
                PostMigrationActionKind::RestoreQuarantine => {
                    summary.restore_actions += 1;
                    if action.conflict.is_none() {
                        summary.restore_candidates += 1;
                    }
                }
            }
        }
    }
    summary
}

fn mark_duplicate_targets(groups: &mut [PostMigrationGroup]) {
    let mut seen = BTreeSet::new();
    for action in groups.iter_mut().flat_map(|group| group.actions.iter_mut()) {
        let Some(to_path) = &action.to_path else {
            continue;
        };
        if action.kind != PostMigrationActionKind::Move {
            continue;
        }
        let key = duplicate_target_key(to_path);
        if !seen.insert(key) {
            append_conflict(&mut action.conflict, "target_duplicate");
        }
    }
}

fn append_conflict(target: &mut Option<String>, token: &str) {
    match target {
        Some(current) if !current.split(',').any(|item| item == token) => {
            current.push(',');
            current.push_str(token);
        }
        Some(_) => {}
        None => *target = Some(token.to_string()),
    }
}

fn resource_from_residual(residual: &ScannedResidual, code: &str) -> PostMigrationResource {
    PostMigrationResource {
        path: residual.path.clone(),
        file_name: residual.file_name.clone(),
        kind: residual.kind.clone(),
        size_bytes: residual.size_bytes,
        code: code.to_string(),
    }
}

fn archive_work_codes(archive_root: &Path) -> Result<BTreeSet<String>> {
    let mut codes = BTreeSet::new();
    if !archive_root.exists() {
        return Ok(codes);
    }
    for entry in fs::read_dir(archive_root)
        .with_context(|| format!("读取归档目录失败：{}", archive_root.to_string_lossy()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Some(code) = normalize_code(&entry.file_name().to_string_lossy()) {
            codes.insert(code);
        }
    }
    Ok(codes)
}

fn original_name_from_quarantine(file_name: &str) -> Option<String> {
    let marker_index = QUARANTINE_MARKERS
        .iter()
        .filter_map(|marker| file_name.find(marker))
        .min()?;
    let original = file_name[..marker_index].trim_start_matches('.');
    if original.is_empty() {
        None
    } else {
        Some(original.to_string())
    }
}

fn is_quarantine_file_name(path: &Path) -> bool {
    path.file_name()
        .map(|name| {
            let name = name.to_string_lossy();
            QUARANTINE_MARKERS
                .iter()
                .any(|marker| name.contains(marker))
        })
        .unwrap_or(false)
}

fn named_with_original_extension(name: &str, file_name: &str) -> String {
    match Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some(extension) if !extension.is_empty() => format!("{name}.{extension}"),
        _ => name.to_string(),
    }
}

fn action_id(kind: &PostMigrationActionKind, path: &Path) -> String {
    let kind = match kind {
        PostMigrationActionKind::Move => "move",
        PostMigrationActionKind::DeleteQuarantine => "delete_quarantine",
        PostMigrationActionKind::RestoreQuarantine => "restore_quarantine",
    };
    format!("{kind}:{}", path.to_string_lossy().replace('\\', "/"))
}

fn file_size(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("打开文件失败：{}", path.to_string_lossy()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 64];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("读取文件失败：{}", path.to_string_lossy()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn same_extension(path: &Path, file_name: &str) -> bool {
    let left = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let right = Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    left == right
}

fn common_parent(items: &[&ScannedResidual]) -> PathBuf {
    items
        .iter()
        .map(|item| item.parent.clone())
        .min()
        .unwrap_or_default()
}

fn looks_like_external_tool_output(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    lower.contains("jvedio") || lower.contains("cinemingle") || lower.contains("98tang")
}

fn duplicate_target_key(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

fn canonical_key(path: &Path) -> String {
    let normalized = fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

fn path_is_under_key(path: &Path, root_key: &str) -> bool {
    let key = canonical_key(path);
    key == root_key || key.starts_with(&format!("{root_key}/"))
}

fn path_is_under_any_root(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path_is_under_root(path, root))
}

fn path_is_under_root(path: &Path, root: &Path) -> bool {
    let path_key = lexical_key(path);
    let root_key = lexical_key(root);
    path_key == root_key || path_key.starts_with(&format!("{root_key}/"))
}

fn lexical_key(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => {
                parts.push(prefix.as_os_str().to_string_lossy().replace('\\', "/"));
            }
            std::path::Component::RootDir => parts.push(String::new()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                let _ = parts.pop();
            }
            std::path::Component::Normal(value) => {
                parts.push(value.to_string_lossy().to_string());
            }
        }
    }
    let normalized = parts.join("/").replace('\\', "/");
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}
