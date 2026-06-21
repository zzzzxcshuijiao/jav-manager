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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchStatus {
    Unwatched,
    Watched,
    Favorite,
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
