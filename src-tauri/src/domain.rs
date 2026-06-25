use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IngestDecision {
    AutoArchive,
    NeedsReview,
    DuplicateCandidate,
    Failed,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewReason {
    MissingCode,
    LowConfidence,
    ProviderFailed,
    CodeConflict,
    DuplicateFile,
    MoveFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeConflictEvidence {
    pub path_code: String,
    pub nfo_code: String,
    pub nfo_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Work {
    pub id: Option<i64>,
    /// Canonical studio-pattern code (ABC-123) when one could be parsed; NULL
    /// for non-standard identifiers that only carry a `source_code`.
    pub normalized_code: Option<String>,
    /// Free-form identifier exactly as captured from filename/NFO/scraper.
    /// Non-standard works merge on this instead of normalized_code.
    pub source_code: Option<String>,
    pub code_kind: CodeKind,
    pub title_zh: Option<String>,
    pub original_title: Option<String>,
    pub aliases: Vec<String>,
    pub summary: Option<String>,
    pub outline: Option<String>,
    pub cover_path: Option<PathBuf>,
    pub poster_path: Option<PathBuf>,
    pub thumb_path: Option<PathBuf>,
    pub fanart_path: Option<PathBuf>,
    pub screenshot_path: Option<PathBuf>,
    pub gif_path: Option<PathBuf>,
    pub tags: Vec<String>,
    pub sets: Vec<String>,
    pub lists: Vec<String>,
    pub rating: Option<u8>,
    pub rating_value: Option<f32>,
    pub rating_max: Option<i32>,
    pub rating_votes: Option<i64>,
    pub criticrating: Option<f32>,
    pub watch_status: WatchStatus,
    pub genres: Vec<String>,
    pub studio: Option<String>,
    pub label: Option<String>,
    pub director: Option<String>,
    pub release_date: Option<String>,
    pub runtime_minutes: Option<i64>,
    pub year: Option<i32>,
    pub website: Option<String>,
    pub mpaa: Option<String>,
    pub has_video: bool,
    pub ratings: Vec<WorkRating>,
    pub watch_progress_seconds: Option<i64>,
    pub last_played_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchStatus {
    Unwatched,
    Watched,
    Favorite,
    WantToWatch,
    Watching,
    OnHold,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileVersion {
    pub id: Option<i64>,
    pub work_id: Option<i64>,
    pub source_root: PathBuf,
    pub original_path: PathBuf,
    pub archived_path: Option<PathBuf>,
    pub original_file_name: String,
    pub normalized_file_name: Option<String>,
    pub size_bytes: u64,
    pub duration_seconds: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codec: Option<String>,
    pub file_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestItem {
    pub id: Option<i64>,
    pub job_id: Option<i64>,
    pub source_root: PathBuf,
    pub path: PathBuf,
    pub file_name: String,
    pub size_bytes: u64,
    pub duration_seconds: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codec: Option<String>,
    pub normalized_code: Option<String>,
    pub confidence: f32,
    pub decision: IngestDecision,
    pub review_reasons: Vec<ReviewReason>,
    pub code_conflict: Option<CodeConflictEvidence>,
    pub metadata: Option<ProviderMetadata>,
    pub candidate_work_id: Option<i64>,
    pub file_hash: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestItemFilters {
    pub decision: Option<IngestDecision>,
    pub review_reason: Option<ReviewReason>,
    pub has_code: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestJobSummary {
    pub id: i64,
    pub status: String,
    pub total_items: usize,
    pub auto_count: usize,
    pub review_count: usize,
    pub failed_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub provider: String,
    pub title_zh: Option<String>,
    pub original_title: Option<String>,
    pub aliases: Vec<String>,
    pub summary: Option<String>,
    pub cover_url: Option<String>,
    pub release_date: Option<String>,
    pub confidence: f32,
    pub actors: Vec<String>,
    pub genres: Vec<String>,
    pub studio: Option<String>,
    pub director: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveAction {
    pub item_id: Option<i64>,
    pub work_code: String,
    pub from_path: PathBuf,
    pub to_path: PathBuf,
    pub original_file_name: String,
    pub normalized_file_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivePlan {
    pub id: Option<i64>,
    pub actions: Vec<ArchiveAction>,
    pub conflicts: Vec<ArchiveConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveConflict {
    pub item_id: Option<i64>,
    pub path: PathBuf,
    pub reason: ReviewReason,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveActionLog {
    pub id: Option<i64>,
    pub item_id: Option<i64>,
    pub job_id: Option<i64>,
    pub from_path: PathBuf,
    pub to_path: PathBuf,
    pub status: String,
    pub message: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkProfile {
    pub work_id: i64,
    pub tags: Vec<String>,
    pub lists: Vec<String>,
    pub rating: Option<u8>,
    pub status: WatchStatus,
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub id: Option<i64>,
    pub primary_name: String,
    pub avatar_path: Option<PathBuf>,
}

/// Whether a work's code follows the canonical studio pattern or is a
/// non-standard identifier (free-form site codes, site-release slugs, etc.).
/// Lets downstream code treat well-formed codes differently from ad-hoc ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeKind {
    Standard,
    Nonstandard,
}

/// A single normalized rating from one source. Stored per-source so multiple
/// scrapers (javdb, javlibrary, fanza, ...) can contribute ratings without
/// overwriting each other before the caller merges them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkRating {
    pub source: String,
    pub value: f32,
    pub max: i32,
    pub votes: Option<i64>,
}

/// A normalized tag shared across works. The `Work.tags` Vec<String> stays as
/// the legacy JSON bag; this struct backs the normalized `tags`/`work_tags`
/// relation tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub id: Option<i64>,
    pub name: String,
}

/// A named set/collection a work belongs to (NFO `<set>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSet {
    pub id: Option<i64>,
    pub name: String,
}

/// A release label. Registry-only in this task (no work link yet).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub id: Option<i64>,
    pub name: String,
}

/// A studio. Registry-only in this task (no work link yet); the scalar
/// `works.studio` column remains the persisted value on Work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Studio {
    pub id: Option<i64>,
    pub name: String,
}

/// Full-fidelity read model for a single work: the scalar Work plus every
/// normalized relation (actors, tags, sets, file versions, ratings).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkDetail {
    pub work: Work,
    pub actors: Vec<Actor>,
    pub tags: Vec<Tag>,
    pub sets: Vec<WorkSet>,
    pub file_versions: Vec<FileVersion>,
    pub ratings: Vec<WorkRating>,
}

/// Dimension browsing filter for `list_works_filtered`. Every populated Vec
/// is AND-ed together: a work must match every requested tag, set, actor,
/// studio, and label id, plus the code-kind and has-video toggles. Empty Vecs
/// and a `None` has_video contribute no constraint, so an all-default filter
/// returns every work.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkFilters {
    pub tag_ids: Vec<i64>,
    pub set_ids: Vec<i64>,
    pub actor_ids: Vec<i64>,
    pub studio_ids: Vec<i64>,
    pub label_ids: Vec<i64>,
    pub code_kinds: Vec<CodeKind>,
    pub has_video: Option<bool>,
}

/// One row of a dimension listing (a tag, set, studio, or label) with the
/// number of works attached to it. Powers the collapsible dimension panel and
/// the AND-filter UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DimensionCount {
    pub id: i64,
    pub name: String,
    pub work_count: i64,
}

/// Centralized archive migration: one work to be migrated from scattered
/// NFO + video into a single work directory (code/code-v2.mp4 + code.nfo + artwork).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationWorkPlan {
    pub code: String,
    pub nfo_path: PathBuf,
    pub video_paths: Vec<PathBuf>,
    pub target_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub works: Vec<MigrationWorkPlan>,
    pub total_nfos: usize,
    pub matched_videos: usize,
    pub unmatched_nfos: usize,
}

/// One work's aggregated resources discovered in the unified resource pool:
/// a NFO (the authority that defines the work), plus every video/image matched
/// to the same normalized code across all scanned directories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PooledWork {
    pub code: String,
    pub nfo_path: Option<PathBuf>,
    pub videos: Vec<PathBuf>,
    pub poster: Option<PathBuf>,
    pub fanart: Option<PathBuf>,
    pub thumb: Option<PathBuf>,
    pub screenshots: Vec<PathBuf>,
    pub gifs: Vec<PathBuf>,
}

impl PooledWork {
    pub fn new(code: String) -> Self {
        Self {
            code,
            nfo_path: None,
            videos: Vec::new(),
            poster: None,
            fanart: None,
            thumb: None,
            screenshots: Vec::new(),
            gifs: Vec::new(),
        }
    }
}

/// Result of scanning the unified resource pool: works keyed by code (driven by
/// NFOs), plus aggregate counts and resources that could not be tied to a code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResourcePool {
    pub works: Vec<PooledWork>,
    pub total_nfos: usize,
    pub total_videos: usize,
    pub total_images: usize,
    pub orphan_videos: usize,
    pub orphan_images: usize,
}

/// One work's unified migration into a self-contained directory. Every path is
/// the source location; `target_dir` is where they will be consolidated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnifiedMigrationWorkPlan {
    pub code: String,
    pub nfo_path: Option<PathBuf>,
    pub videos: Vec<PathBuf>,
    pub poster: Option<PathBuf>,
    pub fanart: Option<PathBuf>,
    pub thumb: Option<PathBuf>,
    pub screenshots: Vec<PathBuf>,
    pub gifs: Vec<PathBuf>,
    pub target_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UnifiedMigrationPlan {
    pub works: Vec<UnifiedMigrationWorkPlan>,
    pub total_works: usize,
    pub total_videos: usize,
    pub total_images: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScrapeStatus {
    Pending,
    Success,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrapeJob {
    pub id: Option<i64>,
    pub work_id: i64,
    pub source: String,
    pub status: ScrapeStatus,
    pub attempts: i64,
    pub last_attempted_at: Option<String>,
    pub error: Option<String>,
}

/// The kind of issue an exception row represents. Drives the review-queue UI:
/// code conflicts need a manual code choice, duplicate candidates need a
/// keep/ignore decision, scrape failures need a retry or manual override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExceptionKind {
    CodeConflict,
    DuplicateCandidate,
    ScrapeFailed,
}

/// Lifecycle of an exception. `Open` is the default for newly recorded rows;
/// the user moves rows to `Ignored` (dismissed) or `Resolved` (acted on).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExceptionStatus {
    Open,
    Ignored,
    Resolved,
}

/// One row in the `exceptions` review queue. `evidence_json` is a free-form
/// JSON blob whose shape depends on `kind` (conflicting codes, duplicate paths,
/// scrape errors), keeping the schema stable as the queue evolves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exception {
    pub id: Option<i64>,
    pub object_path: String,
    pub kind: ExceptionKind,
    pub evidence_json: String,
    pub status: ExceptionStatus,
    pub created_at: Option<String>,
    pub resolved_at: Option<String>,
}

/// Why a file was parked in the `holding` table instead of being
/// auto-classified into a work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoldingReason {
    NoCode,
    ShortVideo,
    NonJapanese,
    Unrecognizable,
}

/// A file that couldn't be auto-classified, held separately from the exception
/// queue for later manual triage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldingEntry {
    pub id: Option<i64>,
    pub path: String,
    pub file_name: String,
    pub size_bytes: i64,
    pub reason: HoldingReason,
    pub created_at: Option<String>,
}

/// One executed pipeline run for a single file: the per-step outcome as a JSON
/// blob, overall status, timing, and an optional error. The status bar and
/// debug views read this to show what happened to a file end-to-end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: Option<i64>,
    pub file_path: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub steps_json: String,
    pub status: String,
    pub error: Option<String>,
}
