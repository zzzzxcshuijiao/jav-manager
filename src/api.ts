import { invoke } from "@tauri-apps/api/core";

export type IngestDecision = "AutoArchive" | "NeedsReview" | "DuplicateCandidate" | "Failed" | "Ignored";
export type ReviewReason =
  | "MissingCode"
  | "LowConfidence"
  | "ProviderFailed"
  | "CodeConflict"
  | "DuplicateFile"
  | "MoveFailed";

export interface ProviderMetadata {
  provider: string;
  title_zh?: string | null;
  original_title?: string | null;
  aliases: string[];
  summary?: string | null;
  cover_url?: string | null;
  release_date?: string | null;
  confidence: number;
}

export interface CodeConflictEvidence {
  path_code: string;
  nfo_code: string;
  nfo_path: string;
}

export interface IngestItem {
  id?: number | null;
  job_id?: number | null;
  source_root: string;
  path: string;
  file_name: string;
  size_bytes: number;
  duration_seconds?: number | null;
  width?: number | null;
  height?: number | null;
  codec?: string | null;
  normalized_code?: string | null;
  confidence: number;
  decision: IngestDecision;
  review_reasons: ReviewReason[];
  code_conflict?: CodeConflictEvidence | null;
  metadata?: ProviderMetadata | null;
  candidate_work_id?: number | null;
  file_hash?: string | null;
}

export interface IngestItemFilters {
  decision?: IngestDecision | null;
  review_reason?: ReviewReason | null;
  has_code?: boolean | null;
}

export interface IngestJobSummary {
  id: number;
  status: string;
  total_items: number;
  auto_count: number;
  review_count: number;
  failed_count: number;
}

export interface Work {
  id?: number | null;
  normalized_code: string;
  title_zh?: string | null;
  original_title?: string | null;
  aliases: string[];
  summary?: string | null;
  cover_path?: string | null;
  tags: string[];
  lists: string[];
  rating?: number | null;
  watch_status: "Unwatched" | "Watched" | "Favorite";
}

export type WatchStatus = Work["watch_status"];

export interface FileVersion {
  id?: number | null;
  work_id?: number | null;
  source_root: string;
  original_path: string;
  archived_path?: string | null;
  original_file_name: string;
  normalized_file_name?: string | null;
  size_bytes: number;
  duration_seconds?: number | null;
  width?: number | null;
  height?: number | null;
  codec?: string | null;
  file_hash?: string | null;
}

export interface ArchiveAction {
  item_id?: number | null;
  work_code: string;
  from_path: string;
  to_path: string;
  original_file_name: string;
  normalized_file_name: string;
}

export interface ArchivePlan {
  id?: number | null;
  actions: ArchiveAction[];
  conflicts: ArchiveConflict[];
}

export interface ArchiveConflict {
  item_id?: number | null;
  path: string;
  reason: ReviewReason;
  message: string;
}

export interface ArchiveActionLog {
  id?: number | null;
  item_id?: number | null;
  job_id?: number | null;
  from_path: string;
  to_path: string;
  status: string;
  message?: string | null;
  created_at?: string | null;
}

export interface ThumbnailCacheSummary {
  file_count: number;
  total_bytes: number;
}

export interface CommandResult<T> {
  data: T;
}

async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  const result = await invoke<CommandResult<T>>(name, args);
  return result.data;
}

export const api = {
  configureSourceRoots(paths: string[]) {
    return command<string[]>("configure_source_roots", { paths });
  },
  configureArchiveRoot(path: string) {
    return command<string>("configure_archive_root", { path });
  },
  getSourceRoots() {
    return command<string[]>("get_source_roots");
  },
  getArchiveRoot() {
    return command<string | null>("get_archive_root");
  },
  configureMetadataProviderEnabled(enabled: boolean) {
    return command<boolean>("configure_metadata_provider_enabled", { enabled });
  },
  getMetadataProviderEnabled() {
    return command<boolean>("get_metadata_provider_enabled");
  },
  startScan(sourceRootIds: string[]) {
    return command<IngestJobSummary>("start_scan", { sourceRootIds });
  },
  getIngestJob(jobId: number) {
    return command<IngestJobSummary>("get_ingest_job", { jobId });
  },
  getLatestIngestJob() {
    return command<IngestJobSummary | null>("get_latest_ingest_job");
  },
  listIngestItems(jobId: number, filters?: IngestItemFilters) {
    return command<IngestItem[]>("list_ingest_items", { jobId, filters });
  },
  listWorks() {
    return command<Work[]>("list_works");
  },
  listFileVersionsForWork(workId: number) {
    return command<FileVersion[]>("list_file_versions_for_work", { workId });
  },
  previewArchivePlan(itemIds: number[]) {
    return command<ArchivePlan>("preview_archive_plan", { itemIds });
  },
  executeArchivePlan(planId: number) {
    return command<ArchiveActionLog[]>("execute_archive_plan", { planId });
  },
  listArchiveActionLogs() {
    return command<ArchiveActionLog[]>("list_archive_action_logs");
  },
  resolveMatch(itemId: number, normalizedCode: string, workId?: number | null) {
    return command<boolean>("resolve_match", { itemId, normalizedCode, workId });
  },
  mergeVersions(workId: number, fileVersionIds: number[]) {
    return command<boolean>("merge_versions", { workId, fileVersionIds });
  },
  retryMetadata(itemIds: number[]) {
    return command<IngestItem[]>("retry_metadata", { itemIds });
  },
  revalidateMoveFailedItems(itemIds: number[]) {
    return command<IngestItem[]>("revalidate_move_failed_items", { itemIds });
  },
  ignoreDuplicateItems(itemIds: number[]) {
    return command<IngestItem[]>("ignore_duplicate_items", { itemIds });
  },
  updateWorkProfile(workId: number, tags: string[], lists: string[], rating: number | null, status: WatchStatus) {
    return command<Work>("update_work_profile", { workId, tags, lists, rating, status });
  },
  openFileInSystem(path: string) {
    return command<boolean>("open_file_in_system", { path });
  },
  openPathInFileManager(path: string) {
    return command<boolean>("open_path_in_file_manager", { path });
  },
  getOrCreateThumbnail(path: string) {
    return command<string | null>("get_or_create_thumbnail", { path });
  },
  getThumbnailCacheSummary() {
    return command<ThumbnailCacheSummary>("get_thumbnail_cache_summary");
  },
  clearThumbnailCache() {
    return command<ThumbnailCacheSummary>("clear_thumbnail_cache");
  }
};
