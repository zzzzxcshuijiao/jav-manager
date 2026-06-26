import { invoke } from "@tauri-apps/api/core";
import {
  createDaemonControlClient,
  type ControlServiceDiscovery,
  type DaemonControlChannel,
} from "./daemonClient";

export type { ControlServiceDiscovery, DaemonControlChannel } from "./daemonClient";

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
  normalized_code?: string | null;
  source_code?: string | null;
  code_kind: CodeKind;
  title_zh?: string | null;
  original_title?: string | null;
  aliases: string[];
  summary?: string | null;
  outline?: string | null;
  cover_path?: string | null;
  poster_path?: string | null;
  thumb_path?: string | null;
  fanart_path?: string | null;
  screenshot_path?: string | null;
  gif_path?: string | null;
  tags: string[];
  genres: string[];
  sets: string[];
  lists: string[];
  rating?: number | null;
  rating_value?: number | null;
  rating_max?: number | null;
  rating_votes?: number | null;
  criticrating?: number | null;
  watch_status: "Unwatched" | "WantToWatch" | "Watching" | "Watched" | "OnHold" | "Favorite";
  studio?: string | null;
  label?: string | null;
  director?: string | null;
  release_date?: string | null;
  runtime_minutes?: number | null;
  year?: number | null;
  website?: string | null;
  mpaa?: string | null;
  has_video: boolean;
}

export type WatchStatus = Work["watch_status"];

export type CodeKind = "standard" | "non_standard";

export type DaemonState = "Idle" | "Scanning" | "Processing" | "Paused" | "Error";
export type MetadataSource = "example" | "disabled";

export interface DaemonControlStatus {
  state: DaemonState;
  configured: boolean;
  source_roots: string[];
  archive_root?: string | null;
  asset_roots: string[];
  queued: number;
  processed: number;
  last_error?: string | null;
  open_exceptions: number;
  holding_items: number;
  recent_runs: number;
  metadata_source: MetadataSource;
}

export interface DaemonScanReport {
  scanned_files: number;
  queued_files: number;
  skipped_files: number;
}

export interface DaemonProcessReport {
  processed: number;
  archived: number;
  holding: number;
  exceptions: number;
  failed: number;
}

export interface DaemonRunOnceReport {
  scan: DaemonScanReport;
  process: DaemonProcessReport;
}

export type HoldingReason = "NoCode" | "ShortVideo" | "NonJapanese" | "Unrecognizable";

export interface HoldingEntry {
  id?: number | null;
  path: string;
  file_name: string;
  size_bytes: number;
  reason: HoldingReason;
  created_at?: string | null;
}

export type ExceptionKind = "CodeConflict" | "DuplicateCandidate" | "ScrapeFailed";
export type ExceptionStatus = "Open" | "Ignored" | "Resolved";

export interface ExceptionEntry {
  id?: number | null;
  object_path: string;
  kind: ExceptionKind;
  evidence_json: string;
  status: ExceptionStatus;
  created_at?: string | null;
  resolved_at?: string | null;
}

export interface PipelineRun {
  id?: number | null;
  file_path: string;
  started_at?: string | null;
  finished_at?: string | null;
  steps_json: string;
  status: string;
  error?: string | null;
}

export interface WorkDetail {
  work: Work;
  actors: Actor[];
  tags: Tag[];
  sets: WorkSet[];
  file_versions: FileVersion[];
  ratings: WorkRating[];
}

export interface Tag {
  id: number;
  name: string;
}

export interface WorkSet {
  id: number;
  name: string;
}

export interface WorkRating {
  id?: number | null;
  work_id?: number | null;
  name: string;
  max: number;
  value: number;
  votes?: number | null;
}

export interface DimensionCount {
  id: number;
  name: string;
  work_count: number;
}

export interface MigrationWorkPlan {
  code: string;
  nfo_path: string;
  video_paths: string[];
  target_dir: string;
}

export interface MigrationPlan {
  works: MigrationWorkPlan[];
  total_nfos: number;
  matched_videos: number;
  unmatched_nfos: number;
}

export interface PooledWork {
  code: string;
  nfo_path: string | null;
  videos: string[];
  poster: string | null;
  fanart: string | null;
  thumb: string | null;
  screenshots: string[];
  gifs: string[];
}

export interface ResourcePool {
  works: PooledWork[];
  total_nfos: number;
  total_videos: number;
  total_images: number;
  orphan_videos: number;
  orphan_images: number;
}

export interface UnifiedMigrationWorkPlan {
  code: string;
  nfo_path: string | null;
  videos: string[];
  poster: string | null;
  fanart: string | null;
  thumb: string | null;
  screenshots: string[];
  gifs: string[];
  target_dir: string;
}

export interface UnifiedMigrationPlan {
  works: UnifiedMigrationWorkPlan[];
  total_works: number;
  total_videos: number;
  total_images: number;
}

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

export interface Actor {
  id?: number | null;
  primary_name: string;
  avatar_path?: string | null;
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

export interface RebuildError {
  nfo_path: string;
  message: string;
}

export interface RebuildReport {
  nfos_scanned: number;
  works_created: number;
  works_merged: number;
  tags_extracted: number;
  sets_extracted: number;
  actors_extracted: number;
  file_versions_created: number;
  errors: RebuildError[];
}

export interface CommandResult<T> {
  data: T;
}

async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  const result = await invoke<CommandResult<T>>(name, args);
  return result.data;
}

const daemonClient = createDaemonControlClient({
  command,
  getDiscovery: () => command<ControlServiceDiscovery | null>("get_control_service_discovery"),
});

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
  getControlServiceDiscovery() {
    return command<ControlServiceDiscovery | null>("get_control_service_discovery");
  },
  getDaemonControlChannel(): DaemonControlChannel {
    return daemonClient.getChannel();
  },
  getDaemonStatus() {
    return daemonClient.getStatus();
  },
  pauseDaemon() {
    return daemonClient.pause();
  },
  resumeDaemon() {
    return daemonClient.resume();
  },
  runDaemonOnce() {
    return daemonClient.runOnce();
  },
  listHoldingEntries() {
    return daemonClient.listHolding();
  },
  listExceptionEntries() {
    return daemonClient.listExceptions();
  },
  resolveExceptionEntry(id: number, status: Exclude<ExceptionStatus, "Open">) {
    return daemonClient.resolveException(id, status);
  },
  listPipelineRuns() {
    return daemonClient.listRuns();
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
  listWorkActors(workId: number) {
    return command<Actor[]>("list_work_actors", { workId });
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
  deleteItems(itemIds: number[]) {
    return command<IngestItem[]>("delete_items", { itemIds });
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
  },
  previewRebuild(sourceRoots: string[]) {
    return command<RebuildReport>("preview_rebuild", { sourceRoots });
  },
  rebuildLibraryFromNfo(sourceRoots: string[]) {
    return command<RebuildReport>("rebuild_library_from_nfo", { sourceRoots });
  },
  configurePosterDirs(posterDir: string, screenshotDir: string, gifDir: string) {
    return command<boolean>("configure_poster_dirs", { posterDir, screenshotDir, gifDir });
  },
  getPosterDirs() {
    return command<{ poster_dir: string | null; screenshot_dir: string | null; gif_dir: string | null }>("get_poster_dirs");
  },
  listWorkDetail(workId: number) {
    return command<WorkDetail | null>("list_work_detail", { workId });
  },
  listTags() {
    return command<DimensionCount[]>("list_tags");
  },
  listSets() {
    return command<DimensionCount[]>("list_sets");
  },
  listStudios() {
    return command<DimensionCount[]>("list_studios");
  },
  listLabels() {
    return command<DimensionCount[]>("list_labels");
  },
  planCentralizedMigration(nfoDir: string, videoDir: string, targetDir: string) {
    return command<MigrationPlan>("plan_centralized_migration", { nfoDir, videoDir, targetDir });
  },
  executeCentralizedMigration(plan: MigrationPlan) {
    return command<number>("execute_centralized_migration", { plan });
  },
  configureResourcePoolDirs(dirs: string[]) {
    return command<string[]>("configure_resource_pool_dirs", { dirs });
  },
  getResourcePoolDirs() {
    return command<string[]>("get_resource_pool_dirs");
  },
  scanResourcePool(dirs: string[]) {
    return command<ResourcePool>("scan_resource_pool", { dirs });
  },
  planUnifiedMigration(dirs: string[], targetDir: string) {
    return command<UnifiedMigrationPlan>("plan_unified_migration", { dirs, targetDir });
  },
  executeUnifiedMigration(plan: UnifiedMigrationPlan) {
    return command<number>("execute_unified_migration", { plan });
  },
  rebuildLibraryFromPool(dirs: string[]) {
    return command<RebuildReport>("rebuild_library_from_pool", { dirs });
  },
  configurePrimaryLibraryDir(dir: string) {
    return command<string>("configure_primary_library_dir", { dir });
  },
  getPrimaryLibraryDir() {
    return command<string | null>("get_primary_library_dir");
  },
  incrementalSync(dirs: string[], primaryDir: string) {
    return command<RebuildReport>("incremental_sync", { dirs, primaryDir });
  }
};
