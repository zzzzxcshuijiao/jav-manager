use crate::identifier::normalize_code;
use crate::nfo::parse_nfo_document;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const INVENTORY_DETAIL_LIMIT: usize = 1000;
const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts",
];
const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// Resource categories surfaced to the frontend inventory preview.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryResourceKind {
    Video,
    Nfo,
    Poster,
    Fanart,
    Thumb,
    Screenshot,
    Gif,
    Image,
    Other,
}

/// Evidence that explains where a normalized code came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryCodeEvidence {
    pub source: String,
    pub code: String,
    pub value: String,
}

/// One scanned file that may belong to an inventory work preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryResource {
    pub path: PathBuf,
    pub file_name: String,
    pub kind: InventoryResourceKind,
    pub size_bytes: u64,
    pub code: Option<String>,
    pub evidence: Vec<InventoryCodeEvidence>,
    pub warnings: Vec<String>,
}

/// Work-level status tags used for filtering and risk review.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryStatus {
    Ready,
    MissingNfo,
    MissingVideo,
    MultiVideo,
    MultiNfo,
    CodeConflict,
    DuplicateCandidate,
    NfoParseError,
    AssetOnly,
    Orphan,
}

/// Review queue assigned to one inventory work after read-only pairing rules run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryReviewBucket {
    AutoReady,
    NeedsReview,
    Blocked,
    AssetCandidate,
}

/// Confidence level for the pairing recommendation shown in the inventory UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryConfidence {
    High,
    Medium,
    Low,
}

/// Role assigned to one scanned resource by the pairing rules.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryResourceRoleKind {
    PrimaryVideo,
    SecondaryVideo,
    DuplicateVideo,
    PrimaryNfo,
    SecondaryNfo,
    Poster,
    Fanart,
    Thumb,
    Screenshot,
    Gif,
    Image,
    Other,
}

/// Explanation for one resource's role in the proposed pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryResourceRole {
    pub path: PathBuf,
    pub role: InventoryResourceRoleKind,
    pub reason: String,
    pub selected: bool,
    pub needs_review: bool,
}

/// Work-level read-only pairing decision used by the review UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryResolution {
    pub bucket: InventoryReviewBucket,
    pub primary_video: Option<PathBuf>,
    pub primary_nfo: Option<PathBuf>,
    pub recommended: String,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
    pub confidence: InventoryConfidence,
}

/// A planned target path. Stage 7A describes actions but never executes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryPreviewAction {
    pub from_path: PathBuf,
    pub to_path: Option<PathBuf>,
    pub kind: InventoryResourceKind,
    pub conflict: Option<String>,
}

/// One code-grouped preview entry in the inventory report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryWorkPreview {
    pub code: String,
    pub statuses: Vec<InventoryStatus>,
    pub resources: Vec<InventoryResource>,
    pub target_dir: Option<PathBuf>,
    pub actions: Vec<InventoryPreviewAction>,
    pub resolution: InventoryResolution,
    pub resource_roles: Vec<InventoryResourceRole>,
}

/// Aggregate counts for a read-only inventory preview.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventorySummary {
    pub total_files: usize,
    pub works: usize,
    pub asset_candidates: usize,
    pub ready: usize,
    pub missing_nfo: usize,
    pub missing_video: usize,
    pub multi_video: usize,
    pub multi_nfo: usize,
    pub code_conflict: usize,
    pub duplicate_candidate: usize,
    pub orphans: usize,
}

/// Read-only inventory preview report returned by the command layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryPreviewReport {
    pub generated_at: String,
    pub roots: Vec<PathBuf>,
    pub archive_root: Option<PathBuf>,
    pub summary: InventorySummary,
    pub works: Vec<InventoryWorkPreview>,
    pub asset_candidates: Vec<InventoryWorkPreview>,
    pub orphans: Vec<InventoryResource>,
    pub warnings: Vec<String>,
    pub truncated: bool,
}

/// Scan arbitrary roots and return a code-grouped inventory preview without writing files.
pub fn preview_inventory_roots(
    roots: &[PathBuf],
    archive_root: Option<&Path>,
) -> Result<InventoryPreviewReport> {
    let mut warnings = Vec::new();
    let mut resources = Vec::new();
    let mut seen_files = BTreeSet::new();
    for root in roots {
        if !root.exists() {
            warnings.push(format!("扫描根目录不存在：{}", root.to_string_lossy()));
            continue;
        }
        for entry in WalkDir::new(root).follow_links(false) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warnings.push(format_walkdir_warning(root, err.path(), &err.to_string()));
                    continue;
                }
            };
            let path = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            if !seen_files.insert(inventory_seen_key(path)) {
                continue;
            }
            push_classified_resource(path, &mut resources, &mut warnings);
        }
    }
    Ok(build_report(roots, archive_root, resources, warnings))
}

// Normalize a scanned file path for duplicate suppression across repeated or overlapping roots.
fn inventory_seen_key(path: &Path) -> String {
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

// Downgrade one unreadable/disappearing file to a report warning so one bad file never aborts a scan.
fn push_classified_resource(
    path: &Path,
    resources: &mut Vec<InventoryResource>,
    warnings: &mut Vec<String>,
) {
    match classify_resource(path) {
        Ok(resource) => resources.push(resource),
        Err(err) => warnings.push(format!("文件分类失败：{}：{}", path.to_string_lossy(), err)),
    }
}

// Preserve the most specific WalkDir location available when directory traversal fails.
fn format_walkdir_warning(root: &Path, path: Option<&Path>, error: &str) -> String {
    let location = path.unwrap_or(root);
    format!("读取目录项失败：{}：{}", location.to_string_lossy(), error)
}

// Build a read-only resource DTO from one filesystem path.
fn classify_resource(path: &Path) -> Result<InventoryResource> {
    let metadata = fs::metadata(path)?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let kind = if extension == "nfo" {
        InventoryResourceKind::Nfo
    } else if VIDEO_EXTS.contains(&extension.as_str()) {
        InventoryResourceKind::Video
    } else if extension == "gif" {
        InventoryResourceKind::Gif
    } else if IMAGE_EXTS.contains(&extension.as_str()) {
        image_kind_from_stem(&stem)
    } else {
        InventoryResourceKind::Other
    };

    let mut warnings = Vec::new();
    let mut evidence = Vec::new();
    let stem_code = push_code_evidence(&mut evidence, "file_stem", &stem);
    let parent_code = path
        .parent()
        .and_then(Path::file_name)
        .map(|value| value.to_string_lossy().to_string())
        .and_then(|parent| push_code_evidence(&mut evidence, "parent_dir", &parent));
    let nfo_code = if kind == InventoryResourceKind::Nfo {
        nfo_code_evidence(path, &mut evidence, &mut warnings)
    } else {
        None
    };

    Ok(InventoryResource {
        path: path.to_path_buf(),
        file_name,
        kind,
        size_bytes: metadata.len(),
        code: nfo_code.or(stem_code).or(parent_code),
        evidence,
        warnings,
    })
}

// Classify known image naming conventions without assuming a fixed scraper.
fn image_kind_from_stem(stem: &str) -> InventoryResourceKind {
    let lower = stem.to_ascii_lowercase();
    let suffix = lower
        .rsplit(|ch| matches!(ch, '-' | '_' | '.' | ' '))
        .next()
        .unwrap_or("");

    if is_bare_code_stem(stem) || matches!(suffix, "poster" | "cover" | "ps") {
        return InventoryResourceKind::Poster;
    }
    if matches!(suffix, "fanart" | "background" | "pl") {
        return InventoryResourceKind::Fanart;
    }
    if matches!(suffix, "thumb" | "thumbnail") {
        return InventoryResourceKind::Thumb;
    }
    if matches!(suffix, "shot" | "screenshot" | "sample") || has_numbered_image_suffix(&lower) {
        return InventoryResourceKind::Screenshot;
    }

    InventoryResourceKind::Image
}

// Assemble the final report after all roots have been scanned.
fn build_report(
    roots: &[PathBuf],
    archive_root: Option<&Path>,
    resources: Vec<InventoryResource>,
    warnings: Vec<String>,
) -> InventoryPreviewReport {
    let total_files = resources.len();
    let mut grouped: BTreeMap<String, Vec<InventoryResource>> = BTreeMap::new();
    let mut orphans = Vec::new();

    for resource in resources {
        if let Some(code) = resource.code.clone() {
            grouped.entry(code).or_default().push(resource);
        } else {
            orphans.push(resource);
        }
    }

    let previews: Vec<InventoryWorkPreview> = grouped
        .into_iter()
        .map(|(code, resources)| build_work_preview(code, resources, archive_root))
        .collect();
    let (mut asset_candidates, mut works): (Vec<_>, Vec<_>) = previews
        .into_iter()
        .partition(|work| work.statuses.contains(&InventoryStatus::AssetOnly));
    let summary = summarize_works(&works, &asset_candidates, &orphans, total_files);
    let truncated = works.len() > INVENTORY_DETAIL_LIMIT
        || asset_candidates.len() > INVENTORY_DETAIL_LIMIT
        || orphans.len() > INVENTORY_DETAIL_LIMIT;
    if works.len() > INVENTORY_DETAIL_LIMIT {
        works.truncate(INVENTORY_DETAIL_LIMIT);
    }
    if asset_candidates.len() > INVENTORY_DETAIL_LIMIT {
        asset_candidates.truncate(INVENTORY_DETAIL_LIMIT);
    }
    if orphans.len() > INVENTORY_DETAIL_LIMIT {
        orphans.truncate(INVENTORY_DETAIL_LIMIT);
    }

    InventoryPreviewReport {
        generated_at: Utc::now().to_rfc3339(),
        roots: roots.to_vec(),
        archive_root: archive_root.map(Path::to_path_buf),
        summary,
        works,
        asset_candidates,
        orphans,
        warnings,
        truncated,
    }
}

// Convert one normalized-code bucket into a frontend preview entry.
fn build_work_preview(
    code: String,
    mut resources: Vec<InventoryResource>,
    archive_root: Option<&Path>,
) -> InventoryWorkPreview {
    resources.sort_by(|left, right| left.path.cmp(&right.path));
    let target_dir = archive_root.map(|root| root.join(&code));
    let statuses = work_statuses(&resources);
    let video_indexes = video_action_indexes(&code, &resources);
    let mut actions: Vec<InventoryPreviewAction> = resources
        .iter()
        .map(|resource| {
            let action_index = video_indexes.get(&resource.path).copied().unwrap_or(0);
            preview_action(&code, resource, action_index, target_dir.as_deref())
        })
        .collect();
    actions.sort_by_key(|action| preview_action_sort_key(action, &video_indexes));
    mark_duplicate_action_targets(&mut actions);
    let (resolution, resource_roles) = build_resolution(&code, &resources, &statuses);

    InventoryWorkPreview {
        code,
        statuses,
        resources,
        target_dir,
        actions,
        resolution,
        resource_roles,
    }
}

// Build the first read-only pairing explanation for one code group.
fn build_resolution(
    code: &str,
    resources: &[InventoryResource],
    statuses: &[InventoryStatus],
) -> (InventoryResolution, Vec<InventoryResourceRole>) {
    let (primary_video, video_reason) = select_primary_video(code, resources);
    let (primary_nfo, nfo_reason) = select_primary_nfo(code, resources, primary_video);
    let mut reasons = Vec::new();
    if let Some(reason) = video_reason {
        reasons.push(reason);
    }
    if let Some(reason) = nfo_reason {
        reasons.push(reason);
    }

    let warnings = resolution_warnings(primary_video, primary_nfo, statuses);
    let blockers = Vec::new();
    let bucket = resolution_bucket(statuses, primary_video, primary_nfo);
    let confidence = resolution_confidence(&bucket, primary_video, primary_nfo);
    let recommended = resolution_recommendation(&bucket, primary_video, primary_nfo);
    let roles = build_resource_roles(resources, primary_video, primary_nfo);

    (
        InventoryResolution {
            bucket,
            primary_video: primary_video.map(|resource| resource.path.clone()),
            primary_nfo: primary_nfo.map(|resource| resource.path.clone()),
            recommended,
            reasons,
            warnings,
            blockers,
            confidence,
        },
        roles,
    )
}

// Return videos in primary-selection order while keeping deterministic path fallback.
fn sorted_videos_for_resolution<'a>(
    code: &str,
    resources: &'a [InventoryResource],
) -> Vec<&'a InventoryResource> {
    let mut videos: Vec<&InventoryResource> = resources
        .iter()
        .filter(|resource| resource.kind == InventoryResourceKind::Video)
        .collect();
    let has_primary_anchor = videos
        .iter()
        .any(|resource| video_has_primary_anchor(code, &resource.path));
    videos.sort_by_key(|resource| video_resolution_sort_key(code, resource, has_primary_anchor));
    videos
}

// Rank videos for both resolution and action targets so both surfaces pick the same primary file.
fn video_resolution_sort_key(
    code: &str,
    resource: &InventoryResource,
    has_primary_anchor: bool,
) -> (u8, usize, std::cmp::Reverse<u64>, PathBuf) {
    let stem = resource
        .path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    if normalize_code(&stem).as_deref() == Some(code) && whole_stem_is_single_code(&stem) {
        return (0, 0, std::cmp::Reverse(0), resource.path.clone());
    }
    if let Some(part_index) = explicit_video_part_index(&stem) {
        if part_index == 1 {
            return (1, 0, std::cmp::Reverse(0), resource.path.clone());
        }
        if has_primary_anchor {
            return (2, part_index, std::cmp::Reverse(0), resource.path.clone());
        }
    }
    (
        if has_primary_anchor { 3 } else { 2 },
        usize::MAX,
        std::cmp::Reverse(resource.size_bytes),
        resource.path.clone(),
    )
}

// Detect whether a video can serve as a direct primary anchor before fallback sizing.
fn video_has_primary_anchor(code: &str, path: &Path) -> bool {
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    if normalize_code(&stem).as_deref() == Some(code) && whole_stem_is_single_code(&stem) {
        return true;
    }
    if explicit_video_part_index(&stem) == Some(1) {
        return true;
    }
    false
}

// Choose the primary video and return a human-readable reason for the UI.
fn select_primary_video<'a>(
    code: &str,
    resources: &'a [InventoryResource],
) -> (Option<&'a InventoryResource>, Option<String>) {
    let videos = sorted_videos_for_resolution(code, resources);
    let Some(selected) = videos.first().copied() else {
        return (None, None);
    };
    let stem = selected
        .path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let reason =
        if normalize_code(&stem).as_deref() == Some(code) && whole_stem_is_single_code(&stem) {
            "推荐主视频：文件名是裸番号视频".to_string()
        } else if explicit_video_part_index(&stem) == Some(1) {
            "推荐主视频：文件名标记为第一段视频".to_string()
        } else if videos.len() > 1 {
            "推荐主视频：未找到裸番号或第一段，选择体积最大视频".to_string()
        } else {
            "推荐主视频：唯一视频资源".to_string()
        };
    (Some(selected), Some(reason))
}

// Rank NFO files by code agreement, bare-code filename, same directory, then path.
fn nfo_resolution_sort_key(
    code: &str,
    resource: &InventoryResource,
    primary_video: Option<&InventoryResource>,
) -> (u8, u8, u8, PathBuf) {
    let nfo_matches_code = resource
        .evidence
        .iter()
        .any(|evidence| evidence.source == "nfo_num" && evidence.code == code);
    let stem = resource
        .path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let bare_stem =
        normalize_code(&stem).as_deref() == Some(code) && whole_stem_is_single_code(&stem);
    let same_dir = primary_video
        .and_then(|video| video.path.parent())
        .zip(resource.path.parent())
        .map(|(left, right)| left == right)
        .unwrap_or(false);
    (
        if nfo_matches_code { 0 } else { 1 },
        if bare_stem { 0 } else { 1 },
        if same_dir { 0 } else { 1 },
        resource.path.clone(),
    )
}

// Choose the primary NFO and return a human-readable reason for the UI.
fn select_primary_nfo<'a>(
    code: &str,
    resources: &'a [InventoryResource],
    primary_video: Option<&InventoryResource>,
) -> (Option<&'a InventoryResource>, Option<String>) {
    let mut nfos: Vec<&InventoryResource> = resources
        .iter()
        .filter(|resource| resource.kind == InventoryResourceKind::Nfo)
        .collect();
    nfos.sort_by_key(|resource| nfo_resolution_sort_key(code, resource, primary_video));
    let Some(selected) = nfos.first().copied() else {
        return (None, None);
    };
    let key = nfo_resolution_sort_key(code, selected, primary_video);
    let reason = if key.0 == 0 {
        "推荐主 NFO：NFO <num> 与番号一致".to_string()
    } else if key.1 == 0 {
        "推荐主 NFO：文件名是裸番号".to_string()
    } else if key.2 == 0 {
        "推荐主 NFO：与主视频位于同一目录".to_string()
    } else {
        "推荐主 NFO：按路径排序选择稳定候选".to_string()
    };
    (Some(selected), Some(reason))
}

// Produce the minimal Task 1 warning list without expanding Task 2 blockers.
fn resolution_warnings(
    primary_video: Option<&InventoryResource>,
    primary_nfo: Option<&InventoryResource>,
    statuses: &[InventoryStatus],
) -> Vec<String> {
    let mut warnings = Vec::new();
    if primary_video.is_none() && !statuses.contains(&InventoryStatus::AssetOnly) {
        warnings.push("未找到可作为主视频的资源".to_string());
    }
    if primary_nfo.is_none() && !statuses.contains(&InventoryStatus::AssetOnly) {
        warnings.push("未找到可作为主 NFO 的资源".to_string());
    }
    warnings
}

// Assign the coarse Task 1 review bucket before later blocker rules are added.
fn resolution_bucket(
    statuses: &[InventoryStatus],
    primary_video: Option<&InventoryResource>,
    primary_nfo: Option<&InventoryResource>,
) -> InventoryReviewBucket {
    if statuses.contains(&InventoryStatus::AssetOnly) {
        return InventoryReviewBucket::AssetCandidate;
    }
    if statuses.contains(&InventoryStatus::Ready)
        && primary_video.is_some()
        && primary_nfo.is_some()
    {
        return InventoryReviewBucket::AutoReady;
    }
    InventoryReviewBucket::NeedsReview
}

// Convert the coarse Task 1 bucket and selected anchors into confidence.
fn resolution_confidence(
    bucket: &InventoryReviewBucket,
    primary_video: Option<&InventoryResource>,
    primary_nfo: Option<&InventoryResource>,
) -> InventoryConfidence {
    match bucket {
        InventoryReviewBucket::AutoReady => InventoryConfidence::High,
        InventoryReviewBucket::AssetCandidate | InventoryReviewBucket::Blocked => {
            InventoryConfidence::Low
        }
        InventoryReviewBucket::NeedsReview if primary_video.is_some() || primary_nfo.is_some() => {
            InventoryConfidence::Medium
        }
        InventoryReviewBucket::NeedsReview => InventoryConfidence::Low,
    }
}

// Summarize the Task 1 recommendation in one frontend-friendly sentence.
fn resolution_recommendation(
    bucket: &InventoryReviewBucket,
    primary_video: Option<&InventoryResource>,
    primary_nfo: Option<&InventoryResource>,
) -> String {
    match bucket {
        InventoryReviewBucket::AutoReady => "可自动整理".to_string(),
        InventoryReviewBucket::AssetCandidate => "素材候选，等待人工补配视频或 NFO".to_string(),
        InventoryReviewBucket::Blocked => "存在阻断，不能自动整理".to_string(),
        InventoryReviewBucket::NeedsReview if primary_video.is_none() => {
            "需人工确认：缺少主视频".to_string()
        }
        InventoryReviewBucket::NeedsReview if primary_nfo.is_none() => {
            "需人工确认：缺少主 NFO".to_string()
        }
        InventoryReviewBucket::NeedsReview => "需人工确认后再整理".to_string(),
    }
}

// Assign each scanned resource a role label for the frontend detail panel.
fn build_resource_roles(
    resources: &[InventoryResource],
    primary_video: Option<&InventoryResource>,
    primary_nfo: Option<&InventoryResource>,
) -> Vec<InventoryResourceRole> {
    resources
        .iter()
        .map(|resource| {
            let is_primary_video = primary_video
                .map(|selected| selected.path.as_path() == resource.path.as_path())
                .unwrap_or(false);
            let is_primary_nfo = primary_nfo
                .map(|selected| selected.path.as_path() == resource.path.as_path())
                .unwrap_or(false);
            let role = match resource.kind {
                InventoryResourceKind::Video if is_primary_video => {
                    InventoryResourceRoleKind::PrimaryVideo
                }
                InventoryResourceKind::Video => InventoryResourceRoleKind::SecondaryVideo,
                InventoryResourceKind::Nfo if is_primary_nfo => {
                    InventoryResourceRoleKind::PrimaryNfo
                }
                InventoryResourceKind::Nfo => InventoryResourceRoleKind::SecondaryNfo,
                InventoryResourceKind::Poster => InventoryResourceRoleKind::Poster,
                InventoryResourceKind::Fanart => InventoryResourceRoleKind::Fanart,
                InventoryResourceKind::Thumb => InventoryResourceRoleKind::Thumb,
                InventoryResourceKind::Screenshot => InventoryResourceRoleKind::Screenshot,
                InventoryResourceKind::Gif => InventoryResourceRoleKind::Gif,
                InventoryResourceKind::Image => InventoryResourceRoleKind::Image,
                InventoryResourceKind::Other => InventoryResourceRoleKind::Other,
            };
            let selected = matches!(
                role,
                InventoryResourceRoleKind::PrimaryVideo | InventoryResourceRoleKind::PrimaryNfo
            );
            let needs_review = matches!(
                role,
                InventoryResourceRoleKind::SecondaryVideo | InventoryResourceRoleKind::SecondaryNfo
            );
            InventoryResourceRole {
                path: resource.path.clone(),
                reason: resource_role_reason(resource, &role, selected),
                role,
                selected,
                needs_review,
            }
        })
        .collect()
}

// Explain why a resource received its role.
fn resource_role_reason(
    resource: &InventoryResource,
    role: &InventoryResourceRoleKind,
    selected: bool,
) -> String {
    match role {
        InventoryResourceRoleKind::PrimaryVideo if selected => "推荐作为主视频".to_string(),
        InventoryResourceRoleKind::SecondaryVideo => {
            "同番号额外视频，需要复核是分段、版本还是重复".to_string()
        }
        InventoryResourceRoleKind::DuplicateVideo => "疑似重复视频，需要人工复核".to_string(),
        InventoryResourceRoleKind::PrimaryNfo if selected => "推荐作为主 NFO".to_string(),
        InventoryResourceRoleKind::SecondaryNfo => "同番号额外 NFO，需要复核是否保留".to_string(),
        InventoryResourceRoleKind::Poster => "按文件名识别为 poster".to_string(),
        InventoryResourceRoleKind::Fanart => "按文件名识别为 fanart".to_string(),
        InventoryResourceRoleKind::Thumb => "按文件名识别为 thumb".to_string(),
        InventoryResourceRoleKind::Screenshot => "按文件名识别为 screenshot".to_string(),
        InventoryResourceRoleKind::Gif => "GIF 素材，随作品归组".to_string(),
        InventoryResourceRoleKind::Image => "其他图片素材，随作品归组".to_string(),
        InventoryResourceRoleKind::Other => {
            format!("其他资源：{}", resource.file_name)
        }
        InventoryResourceRoleKind::PrimaryVideo | InventoryResourceRoleKind::PrimaryNfo => {
            "推荐作为主资源".to_string()
        }
    }
}

// Assign video target slots using the same pairing order as resolution.primary_video.
fn video_action_indexes(code: &str, resources: &[InventoryResource]) -> BTreeMap<PathBuf, usize> {
    sorted_videos_for_resolution(code, resources)
        .into_iter()
        .enumerate()
        .map(|(index, resource)| (resource.path.clone(), index))
        .collect()
}

// Keep the action list in the same semantic order used for assigning preview video names.
fn preview_action_sort_key(
    action: &InventoryPreviewAction,
    video_indexes: &BTreeMap<PathBuf, usize>,
) -> (u8, usize, PathBuf) {
    match action.kind {
        InventoryResourceKind::Video => {
            let index = video_indexes
                .get(&action.from_path)
                .copied()
                .unwrap_or(usize::MAX);
            (0, index, action.from_path.clone())
        }
        InventoryResourceKind::Nfo => (1, 0, action.from_path.clone()),
        InventoryResourceKind::Poster => (2, 0, action.from_path.clone()),
        InventoryResourceKind::Fanart => (3, 0, action.from_path.clone()),
        InventoryResourceKind::Thumb => (4, 0, action.from_path.clone()),
        InventoryResourceKind::Screenshot => (5, 0, action.from_path.clone()),
        InventoryResourceKind::Gif => (6, 0, action.from_path.clone()),
        InventoryResourceKind::Image => (7, 0, action.from_path.clone()),
        InventoryResourceKind::Other => (8, 0, action.from_path.clone()),
    }
}

// Extract common CD/part/disc suffix numbers such as CODE-CD2 or CODE_part02.
fn explicit_video_part_index(stem: &str) -> Option<usize> {
    stem.rsplit(|ch| matches!(ch, '-' | '_' | '.' | ' '))
        .find_map(|token| {
            let token = token.to_ascii_lowercase();
            ["cd", "part", "disc"]
                .iter()
                .find_map(|marker| token.strip_prefix(marker))
                .map(|digits| digits.parse::<usize>().unwrap_or(usize::MAX))
        })
}

// Mark generated target collisions inside one preview batch without hiding pre-existing disk conflicts.
fn mark_duplicate_action_targets(actions: &mut [InventoryPreviewAction]) {
    let mut indexes_by_target: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, action) in actions.iter().enumerate() {
        if let Some(to_path) = &action.to_path {
            indexes_by_target
                .entry(duplicate_target_key(to_path))
                .or_default()
                .push(index);
        }
    }

    for indexes in indexes_by_target.values() {
        if indexes.len() < 2 {
            continue;
        }
        for index in indexes {
            append_conflict(&mut actions[*index].conflict, "target_duplicate");
        }
    }
}

// Build the comparison key used for detecting same-batch target collisions.
fn duplicate_target_key(path: &Path) -> String {
    let key = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

// Append one conflict token while preserving deterministic comma-separated order.
fn append_conflict(conflict: &mut Option<String>, token: &str) {
    match conflict {
        Some(existing) if existing.split(',').any(|part| part == token) => {}
        Some(existing) if !existing.is_empty() => {
            existing.push(',');
            existing.push_str(token);
        }
        _ => {
            *conflict = Some(token.to_string());
        }
    }
}

// Summarize work statuses while keeping the report total independent of truncation.
fn summarize_works(
    works: &[InventoryWorkPreview],
    asset_candidates: &[InventoryWorkPreview],
    orphans: &[InventoryResource],
    total_files: usize,
) -> InventorySummary {
    let mut summary = InventorySummary {
        total_files,
        works: works.len(),
        asset_candidates: asset_candidates.len(),
        orphans: orphans.len(),
        ..InventorySummary::default()
    };

    for work in works {
        if work.statuses.contains(&InventoryStatus::Ready) {
            summary.ready += 1;
        }
        if work.statuses.contains(&InventoryStatus::MissingNfo) {
            summary.missing_nfo += 1;
        }
        if work.statuses.contains(&InventoryStatus::MissingVideo) {
            summary.missing_video += 1;
        }
        if work.statuses.contains(&InventoryStatus::MultiVideo) {
            summary.multi_video += 1;
        }
        if work.statuses.contains(&InventoryStatus::MultiNfo) {
            summary.multi_nfo += 1;
        }
        if work.statuses.contains(&InventoryStatus::CodeConflict) {
            summary.code_conflict += 1;
        }
        if work.statuses.contains(&InventoryStatus::DuplicateCandidate) {
            summary.duplicate_candidate += 1;
        }
    }

    summary
}

// Produce work-level status tags from the resource mix and per-file evidence.
fn work_statuses(resources: &[InventoryResource]) -> Vec<InventoryStatus> {
    let video_count = resources
        .iter()
        .filter(|resource| resource.kind == InventoryResourceKind::Video)
        .count();
    let nfo_count = resources
        .iter()
        .filter(|resource| resource.kind == InventoryResourceKind::Nfo)
        .count();
    let has_work_anchor = video_count > 0 || nfo_count > 0;

    let mut statuses = BTreeSet::new();
    if !has_work_anchor {
        statuses.insert(InventoryStatus::AssetOnly);
    }
    if video_count > 0 && nfo_count > 0 {
        statuses.insert(InventoryStatus::Ready);
    }
    if video_count > 0 && nfo_count == 0 {
        statuses.insert(InventoryStatus::MissingNfo);
    }
    if video_count == 0 && nfo_count > 0 {
        statuses.insert(InventoryStatus::MissingVideo);
    }
    if video_count > 1 {
        statuses.insert(InventoryStatus::MultiVideo);
    }
    if nfo_count > 1 {
        statuses.insert(InventoryStatus::MultiNfo);
    }
    if resources.iter().any(resource_has_conflicting_evidence) {
        statuses.insert(InventoryStatus::CodeConflict);
    }
    if has_duplicate_video_candidate(resources) {
        statuses.insert(InventoryStatus::DuplicateCandidate);
    }
    if resources.iter().any(|resource| {
        resource
            .warnings
            .iter()
            .any(|warning| warning.contains("NFO 解析失败"))
    }) {
        statuses.insert(InventoryStatus::NfoParseError);
    }

    statuses.into_iter().collect()
}

// Flag same-size videos in one code group as a cheap preview-only duplicate candidate signal.
fn has_duplicate_video_candidate(resources: &[InventoryResource]) -> bool {
    let mut sizes = BTreeSet::new();
    resources
        .iter()
        .filter(|resource| resource.kind == InventoryResourceKind::Video && resource.size_bytes > 0)
        .any(|resource| !sizes.insert(resource.size_bytes))
}

// A resource is conflicting when different code sources disagree on the normalized code.
fn resource_has_conflicting_evidence(resource: &InventoryResource) -> bool {
    resource
        .evidence
        .iter()
        .map(|evidence| evidence.code.as_str())
        .collect::<BTreeSet<_>>()
        .len()
        > 1
}

// Build one read-only file action for the archive preview without touching the target path.
fn preview_action(
    code: &str,
    resource: &InventoryResource,
    index: usize,
    target_dir: Option<&Path>,
) -> InventoryPreviewAction {
    let to_path =
        target_dir.map(|target_dir| target_dir.join(target_relative_path(code, resource, index)));
    let mut action = InventoryPreviewAction {
        from_path: resource.path.clone(),
        to_path,
        kind: resource.kind.clone(),
        conflict: None,
    };
    if action
        .to_path
        .as_ref()
        .map(|path| path.exists())
        .unwrap_or(false)
    {
        append_conflict(&mut action.conflict, "target_exists");
    }
    action
}

// Choose the stable archive-relative name for one resource kind.
fn target_relative_path(code: &str, resource: &InventoryResource, index: usize) -> PathBuf {
    match resource.kind {
        InventoryResourceKind::Video => PathBuf::from(named_with_original_extension(
            if index == 0 {
                code.to_string()
            } else {
                format!("{code}-v{}", index + 1)
            },
            resource,
        )),
        InventoryResourceKind::Nfo => PathBuf::from(format!("{code}.nfo")),
        InventoryResourceKind::Poster => {
            PathBuf::from(named_with_original_extension("poster", resource))
        }
        InventoryResourceKind::Fanart => {
            PathBuf::from(named_with_original_extension("fanart", resource))
        }
        InventoryResourceKind::Thumb => {
            PathBuf::from(named_with_original_extension("thumb", resource))
        }
        InventoryResourceKind::Screenshot => PathBuf::from("screenshots").join(&resource.file_name),
        InventoryResourceKind::Gif => PathBuf::from("gifs").join(&resource.file_name),
        InventoryResourceKind::Image | InventoryResourceKind::Other => {
            PathBuf::from("images").join(&resource.file_name)
        }
    }
}

// Reuse the source extension for normalized target names when one exists.
fn named_with_original_extension(name: impl Into<String>, resource: &InventoryResource) -> String {
    let name = name.into();
    match resource
        .path
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some(extension) if !extension.is_empty() => format!("{name}.{extension}"),
        _ => name,
    }
}

// Add normalized-code evidence and return the normalized value for precedence decisions.
fn push_code_evidence(
    evidence: &mut Vec<InventoryCodeEvidence>,
    source: &str,
    value: &str,
) -> Option<String> {
    let code = normalize_code(value)?;
    if !evidence
        .iter()
        .any(|item| item.source == source && item.code == code && item.value == value)
    {
        evidence.push(InventoryCodeEvidence {
            source: source.to_string(),
            code: code.clone(),
            value: value.to_string(),
        });
    }
    Some(code)
}

// Read an NFO only for code evidence; parse failures remain resource warnings.
fn nfo_code_evidence(
    path: &Path,
    evidence: &mut Vec<InventoryCodeEvidence>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => {
            warnings.push("NFO 解析失败".to_string());
            return None;
        }
    };
    let document = match parse_nfo_document(&text) {
        Ok(document) => document,
        Err(_) => {
            warnings.push("NFO 解析失败".to_string());
            return None;
        }
    };
    document
        .source_code
        .as_deref()
        .and_then(|value| push_code_evidence(evidence, "nfo_num", value))
}

// A bare-code image is usually the poster/cover in existing JAV libraries.
fn is_bare_code_stem(stem: &str) -> bool {
    normalize_code(stem).is_some() && whole_stem_is_single_code(stem)
}

// Detect scraper screenshot names like CODE-01 or CODE_01 without matching CODE-001.
fn has_numbered_image_suffix(stem: &str) -> bool {
    stem.rsplit_once(['-', '_'])
        .map(|(_, suffix)| suffix.len() == 2 && suffix.chars().all(|ch| ch.is_ascii_digit()))
        .unwrap_or(false)
}

// Accept only a single full-stem code, including unpadded forms such as ABP-1.
fn whole_stem_is_single_code(stem: &str) -> bool {
    let trimmed = stem.trim();
    let mut prefix_len = 0usize;
    let mut digit_len = 0usize;
    let mut seen_separator = false;
    let mut seen_digit = false;

    for ch in trimmed.chars() {
        if !seen_digit && ch.is_ascii_alphabetic() {
            if seen_separator {
                return false;
            }
            prefix_len += 1;
        } else if !seen_digit && matches!(ch, '-' | '_' | ' ') {
            if prefix_len == 0 {
                return false;
            }
            seen_separator = true;
        } else if ch.is_ascii_digit() {
            seen_digit = true;
            digit_len += 1;
        } else {
            return false;
        }
    }

    (2..=10).contains(&prefix_len) && (1..=6).contains(&digit_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_errors_are_reported_without_aborting_collection() {
        let tmp = tempfile::tempdir().unwrap();
        let missing_file = tmp.path().join("gone.mp4");
        let mut resources = Vec::new();
        let mut warnings = Vec::new();

        push_classified_resource(&missing_file, &mut resources, &mut warnings);

        assert!(resources.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("文件分类失败"));
        assert!(warnings[0].contains(&missing_file.to_string_lossy().to_string()));
    }

    #[test]
    fn walkdir_warning_includes_child_path_and_error_text() {
        let warning = format_walkdir_warning(
            Path::new("root"),
            Some(Path::new("root/bad-child")),
            "access denied",
        );

        assert!(warning.contains("root/bad-child"));
        assert!(warning.contains("access denied"));
    }
}
