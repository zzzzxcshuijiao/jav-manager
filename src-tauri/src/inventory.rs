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
    Orphan,
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
}

/// Aggregate counts for a read-only inventory preview.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventorySummary {
    pub total_files: usize,
    pub works: usize,
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
            push_classified_resource(path, &mut resources, &mut warnings);
        }
    }
    Ok(build_report(roots, archive_root, resources, warnings))
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

    let mut works: Vec<InventoryWorkPreview> = grouped
        .into_iter()
        .map(|(code, resources)| build_work_preview(code, resources, archive_root))
        .collect();
    let summary = summarize_works(&works, &orphans, total_files);
    let truncated = works.len() > INVENTORY_DETAIL_LIMIT;
    if truncated {
        works.truncate(INVENTORY_DETAIL_LIMIT);
    }

    InventoryPreviewReport {
        generated_at: Utc::now().to_rfc3339(),
        roots: roots.to_vec(),
        archive_root: archive_root.map(Path::to_path_buf),
        summary,
        works,
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
    let mut video_index = 0usize;
    let actions = resources
        .iter()
        .map(|resource| {
            let action_index = if resource.kind == InventoryResourceKind::Video {
                let current_index = video_index;
                video_index += 1;
                current_index
            } else {
                0
            };
            preview_action(&code, resource, action_index, target_dir.as_deref())
        })
        .collect();

    InventoryWorkPreview {
        code,
        statuses,
        resources,
        target_dir,
        actions,
    }
}

// Summarize work statuses while keeping the report total independent of truncation.
fn summarize_works(
    works: &[InventoryWorkPreview],
    orphans: &[InventoryResource],
    total_files: usize,
) -> InventorySummary {
    let mut summary = InventorySummary {
        total_files,
        works: works.len(),
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

    let mut statuses = BTreeSet::new();
    if video_count > 0 && nfo_count > 0 {
        statuses.insert(InventoryStatus::Ready);
    }
    if video_count > 0 && nfo_count == 0 {
        statuses.insert(InventoryStatus::MissingNfo);
    }
    if video_count == 0 {
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
    let conflict = to_path
        .as_ref()
        .filter(|path| path.exists())
        .map(|_| "target_exists".to_string());

    InventoryPreviewAction {
        from_path: resource.path.clone(),
        to_path,
        kind: resource.kind.clone(),
        conflict,
    }
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
