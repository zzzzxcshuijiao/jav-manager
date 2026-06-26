import type {
  ArchiveActionLog,
  ArchivePlan,
  CodeConflictEvidence,
  DaemonControlChannel,
  DaemonRunOnceReport,
  DaemonState,
  ExceptionKind,
  ExceptionStatus,
  FileVersion,
  HoldingReason,
  IngestDecision,
  IngestItem,
  IngestItemFilters,
  RebuildReport,
  ReviewReason,
  Work
} from "./api";

export type DecisionFilter = IngestDecision | "All";
export type ReviewReasonFilter = ReviewReason | "All";
export type CodePresenceFilter = "All" | "HasCode" | "MissingCode";
export type WorkbenchView = "ingest" | "review" | "archive" | "settings" | "library";
export type WorkStatusFilter = Work["watch_status"] | "All";

export interface DashboardStats {
  total: number;
  auto: number;
  review: number;
  failed: number;
  ignored: number;
  duplicateCodes: string[];
}

export interface ReviewQueueSummary {
  total: number;
  byReason: Record<ReviewReason, number>;
}

export interface LabelCount {
  label: string;
  count: number;
}

export interface ArchiveExecutionSummary {
  moved: number;
  failed: number;
  message: string;
}

const allReviewReasons: ReviewReason[] = [
  "MissingCode",
  "LowConfidence",
  "ProviderFailed",
  "CodeConflict",
  "DuplicateFile",
  "MoveFailed"
];

export function buildDashboardStats(items: IngestItem[]): DashboardStats {
  const codeCounts = new Map<string, number>();
  for (const item of items) {
    if (item.normalized_code) {
      codeCounts.set(item.normalized_code, (codeCounts.get(item.normalized_code) ?? 0) + 1);
    }
  }

  return {
    total: items.length,
    auto: items.filter((item) => item.decision === "AutoArchive").length,
    review: items.filter((item) => item.decision === "NeedsReview" || item.decision === "DuplicateCandidate").length,
    failed: items.filter((item) => item.decision === "Failed").length,
    ignored: items.filter((item) => item.decision === "Ignored").length,
    duplicateCodes: [...codeCounts.entries()].filter(([, count]) => count > 1).map(([code]) => code)
  };
}

export function filterItems(items: IngestItem[], decision: DecisionFilter): IngestItem[] {
  if (decision === "All") {
    return items;
  }

  return items.filter((item) => item.decision === decision);
}

export function buildIngestItemFilters(
  decision: DecisionFilter,
  reviewReason: ReviewReasonFilter,
  codePresence: CodePresenceFilter
): IngestItemFilters | undefined {
  const filters: IngestItemFilters = {};
  if (decision !== "All") {
    filters.decision = decision;
  }
  if (reviewReason !== "All") {
    filters.review_reason = reviewReason;
  }
  if (codePresence === "HasCode") {
    filters.has_code = true;
  }
  if (codePresence === "MissingCode") {
    filters.has_code = false;
  }
  return filters.decision || filters.review_reason || filters.has_code != null ? filters : undefined;
}

export function applyIngestItemFilters(items: IngestItem[], filters?: IngestItemFilters): IngestItem[] {
  if (!filters) {
    return items;
  }

  return items.filter((item) => {
    if (filters.decision && item.decision !== filters.decision) {
      return false;
    }
    if (filters.review_reason && !item.review_reasons.includes(filters.review_reason)) {
      return false;
    }
    if (filters.has_code != null && Boolean(item.normalized_code) !== filters.has_code) {
      return false;
    }
    return true;
  });
}

export function viewItemsForMode(items: IngestItem[], view: WorkbenchView): IngestItem[] {
  if (view !== "review") {
    return items;
  }
  return items.filter(
    (item) => item.decision === "NeedsReview" || item.decision === "DuplicateCandidate" || item.decision === "Failed"
  );
}

export function buildReviewQueueSummary(items: IngestItem[]): ReviewQueueSummary {
  const byReason = Object.fromEntries(allReviewReasons.map((reason) => [reason, 0])) as Record<ReviewReason, number>;
  const reviewItems = viewItemsForMode(items, "review");
  for (const item of reviewItems) {
    for (const reason of item.review_reasons) {
      byReason[reason] += 1;
    }
  }
  return {
    total: reviewItems.length,
    byReason
  };
}

export function duplicateCandidatesForItem(items: IngestItem[], item: IngestItem | null): IngestItem[] {
  if (!item?.file_hash) {
    return [];
  }
  return items.filter(
    (candidate) =>
      candidate.file_hash === item.file_hash &&
      candidate.id !== item.id &&
      candidate.review_reasons.includes("DuplicateFile")
  );
}

export function formatCodeConflictEvidence(conflict?: CodeConflictEvidence | null): string | null {
  if (!conflict) {
    return null;
  }
  return `路径 ${conflict.path_code} / NFO ${conflict.nfo_code}`;
}

export function coverPreviewPathForItem(item: IngestItem | null, work: Work | null): string | null {
  const workCover = work?.cover_path?.trim();
  if (workCover) {
    return workCover;
  }
  const itemCover = item?.metadata?.cover_url?.trim();
  return itemCover || null;
}

export function workbenchViewTitle(view: WorkbenchView): string {
  const labels: Record<WorkbenchView, string> = {
    ingest: "入库队列",
    review: "待处理队列",
    archive: "迁移计划",
    settings: "设置",
    library: "作品库"
  };
  return labels[view];
}

export function formatBytes(value: number): string {
  if (value < 1024) {
    return `${value} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let size = value / 1024;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }
  return `${size.toFixed(size >= 10 ? 1 : 2)} ${units[unitIndex]}`;
}

export function formatDuration(value?: number | null): string {
  if (value == null) {
    return "未探测";
  }
  const hours = Math.floor(value / 3600);
  const minutes = Math.floor((value % 3600) / 60);
  const seconds = value % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  }
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

export function formatMediaInfo({
  width,
  height,
  codec
}: {
  width?: number | null;
  height?: number | null;
  codec?: string | null;
}): string {
  const parts = [];
  if (width != null && height != null) {
    parts.push(`${width}x${height}`);
  }
  if (codec) {
    parts.push(codec);
  }
  return parts.length > 0 ? parts.join(" / ") : "未探测";
}

export function formatFileVersionSummary(version: FileVersion): string {
  const name = version.normalized_file_name ?? version.original_file_name;
  const archiveState = version.archived_path ? "已归档" : "未迁移";
  return [
    name,
    formatBytes(version.size_bytes),
    formatDuration(version.duration_seconds),
    formatMediaInfo(version),
    archiveState
  ].join(" · ");
}

export function autoArchiveIds(items: IngestItem[]): number[] {
  return items
    .filter((item) => item.decision === "AutoArchive" && item.id != null)
    .map((item) => item.id as number);
}

export function archivePreviewIds(items: IngestItem[], selectedIds: Set<number>): number[] {
  if (selectedIds.size === 0) {
    return autoArchiveIds(items);
  }
  return items
    .filter((item) => item.id != null && selectedIds.has(item.id) && item.decision === "AutoArchive")
    .map((item) => item.id as number);
}

export function selectedItemIds(items: IngestItem[], selectedIds: Set<number>): number[] {
  return items
    .filter((item) => item.id != null && selectedIds.has(item.id))
    .map((item) => item.id as number);
}

export function resolvableSelectedItems(items: IngestItem[], selectedIds: Set<number>): IngestItem[] {
  return items.filter(
    (item) => item.id != null && selectedIds.has(item.id) && normalizeManualCodeInput(item.normalized_code ?? "") != null
  );
}

export function revalidatableMoveFailedItems(items: IngestItem[], selectedIds: Set<number>): IngestItem[] {
  return items.filter(
    (item) => item.id != null && selectedIds.has(item.id) && item.review_reasons.includes("MoveFailed")
  );
}

export function ignorableDuplicateItems(items: IngestItem[], selectedIds: Set<number>): IngestItem[] {
  return items.filter(
    (item) =>
      item.id != null &&
      selectedIds.has(item.id) &&
      item.decision === "DuplicateCandidate" &&
      item.review_reasons.includes("DuplicateFile")
  );
}

export function replaceIngestItem(items: IngestItem[], storedItem: IngestItem): IngestItem[] {
  return items.map((item) => (item.id === storedItem.id ? storedItem : item));
}

export function normalizeManualCodeInput(value: string): string | null {
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

export function parseDelimitedListInput(value: string): string[] {
  const seen = new Set<string>();
  return value
    .split(/[,\n/]+/)
    .map((entry) => entry.trim())
    .filter((entry) => {
      if (!entry || seen.has(entry)) {
        return false;
      }
      seen.add(entry);
      return true;
    });
}

export function parseProfileRatingInput(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }
  const parsed = Number(trimmed);
  if (!Number.isInteger(parsed) || parsed < 0 || parsed > 10) {
    return null;
  }
  return parsed;
}

export function canExecuteArchivePlan(plan: ArchivePlan | null): boolean {
  return Boolean(plan?.id && plan.actions.length > 0 && plan.conflicts.length === 0);
}

export function archiveExecutionSummary(logs: ArchiveActionLog[], historicalLogCount: number): ArchiveExecutionSummary {
  const moved = logs.filter((log) => log.status === "moved").length;
  const failed = logs.length - moved;
  return {
    moved,
    failed,
    message: `迁移执行完成：${moved} 个成功，${failed} 个失败；历史日志 ${historicalLogCount} 条。`
  };
}

export function mergeableWorksForItem(works: Work[], item: IngestItem | null): Work[] {
  if (!item?.candidate_work_id) {
    return works;
  }
  return works.filter((work) => work.id !== item.candidate_work_id);
}

export function mergeVersionTargetWorks(works: Work[], currentWork: Work | null): Work[] {
  if (!currentWork?.id) {
    return works;
  }
  return works.filter((work) => work.id !== currentWork.id);
}

export function filterWorksForLibrary(works: Work[], query: string, status: WorkStatusFilter): Work[] {
  const trimmed = query.trim().toLocaleLowerCase();
  const compactQuery = compactSearchText(trimmed);
  return works.filter((work) => {
    if (status !== "All" && work.watch_status !== status) {
      return false;
    }
    if (!trimmed) {
      return true;
    }
    return [
      work.normalized_code,
      work.title_zh,
      work.original_title,
      ...work.aliases,
      ...work.tags,
      ...work.lists
    ]
      .filter(Boolean)
      .some((value) => {
        const text = String(value).toLocaleLowerCase();
        return text.includes(trimmed) || compactSearchText(text).includes(compactQuery);
      });
  });
}

export function buildWorkTagSummary(works: Work[]): LabelCount[] {
  const counts = new Map<string, number>();
  for (const work of works) {
    for (const tag of work.tags) {
      counts.set(tag, (counts.get(tag) ?? 0) + 1);
    }
  }
  return [...counts.entries()]
    .map(([label, count]) => ({ label, count }))
    .sort((left, right) => right.count - left.count || left.label.localeCompare(right.label));
}

export function findIngestItemForWork(items: IngestItem[], work: Work | null): IngestItem | null {
  if (!work) {
    return null;
  }
  if (work.id != null) {
    const linkedItem = items.find((item) => item.candidate_work_id === work.id);
    if (linkedItem) {
      return linkedItem;
    }
  }
  return items.find((item) => item.normalized_code === work.normalized_code) ?? null;
}

function compactSearchText(value: string): string {
  return value.replace(/[\s_-]+/g, "");
}

export function formatWorkOption(work: Work): string {
  const title = work.title_zh ?? work.original_title ?? "";
  return title ? `#${work.id} · ${work.normalized_code} · ${title}` : `#${work.id} · ${work.normalized_code}`;
}

export type RebuildMode = "preview" | "rebuild";

export function formatRebuildReport(mode: RebuildMode, report: RebuildReport): string {
  const verb = mode === "preview" ? "预览完成" : "重建完成";
  const errorPart = report.errors.length > 0 ? `，${report.errors.length} 个 NFO 解析失败` : "";
  return `${verb}：${report.nfos_scanned} 个 NFO，${report.works_created} 个作品，${report.works_merged} 个多文件合并组${errorPart}。`;
}

export function formatDaemonState(state: DaemonState): string {
  const labels: Record<DaemonState, string> = {
    Idle: "空闲",
    Scanning: "扫描中",
    Processing: "处理中",
    Paused: "已暂停",
    Error: "错误"
  };
  return labels[state];
}

/** Format the active daemon control channel for the settings status line. */
export function formatDaemonChannel(channel: DaemonControlChannel): string {
  const labels: Record<DaemonControlChannel, string> = {
    service: "本地服务",
    command: "命令桥",
    none: "未连接"
  };
  return labels[channel];
}

export function formatHoldingReason(reason: HoldingReason): string {
  const labels: Record<HoldingReason, string> = {
    NoCode: "缺少番号",
    ShortVideo: "短视频",
    NonJapanese: "非日系内容",
    Unrecognizable: "无法识别"
  };
  return labels[reason];
}

export function formatExceptionKind(kind: ExceptionKind): string {
  const labels: Record<ExceptionKind, string> = {
    CodeConflict: "番号冲突",
    DuplicateCandidate: "重复候选",
    ScrapeFailed: "刮削失败"
  };
  return labels[kind];
}

export function formatExceptionStatus(status: ExceptionStatus): string {
  const labels: Record<ExceptionStatus, string> = {
    Open: "待处理",
    Ignored: "已忽略",
    Resolved: "已解决"
  };
  return labels[status];
}

export function formatPipelineStatus(status: string): string {
  const labels: Record<string, string> = {
    running: "运行中",
    archived: "已归档",
    holding: "已搁置",
    exception: "异常",
    failed: "失败"
  };
  return labels[status] ?? status;
}

export function summarizeRunOnceReport(report: DaemonRunOnceReport): string {
  const sections = [
    `扫描 ${report.scan.scanned_files} 个文件，入队 ${report.scan.queued_files} 个，跳过 ${report.scan.skipped_files} 个`,
    `处理 ${report.process.processed} 个：归档 ${report.process.archived}，搁置 ${report.process.holding}，异常 ${report.process.exceptions}，失败 ${report.process.failed}。`
  ];
  const aria2 = report.aria2;
  const hasAria2Activity = aria2
    ? aria2.attempted_gids > 0 ||
      aria2.completed_gids > 0 ||
      aria2.queued_files > 0 ||
      aria2.skipped_files > 0 ||
      aria2.failed_gids > 0
    : false;
  if (aria2?.enabled && hasAria2Activity) {
    const skipped = aria2.skipped_files > 0 ? `，跳过 ${aria2.skipped_files} 个` : "";
    sections.unshift(
      `aria2 尝试 ${aria2.attempted_gids} 个 GID，完成 ${aria2.completed_gids} 个，入队 ${aria2.queued_files} 个${skipped}，失败 ${aria2.failed_gids} 个`
    );
  }
  return sections.join("；");
}

export function shortEvidence(value: string | null | undefined, maxLength = 120): string {
  const trimmed = value?.trim() ?? "";
  if (!trimmed) {
    return "无证据";
  }
  if (trimmed.length <= maxLength) {
    return trimmed;
  }
  return `${trimmed.slice(0, Math.max(0, maxLength - 3))}...`;
}

// --- Library browse layer helpers (Phase H follow-up) ---

/**
 * Splits the library works into a standard-code section and a non-standard
 * section. Standard works carry a studio-pattern code (ABC-123) and anchor the
 * primary catalog; non-standard works have no parseable code and merge on
 * source_code/title, so they live in a separately-collapsible region.
 */
export function partitionWorksByKind(works: Work[]): { standard: Work[]; nonStandard: Work[] } {
  const standard: Work[] = [];
  const nonStandard: Work[] = [];
  for (const work of works) {
    if (work.code_kind === "non_standard") {
      nonStandard.push(work);
    } else {
      standard.push(work);
    }
  }
  return { standard, nonStandard };
}

/**
 * Primary label for a library card: the normalized code when present, otherwise
 * the Chinese title, otherwise the original title, finally a fallback so the
 * card is never blank.
 */
export function libraryCardTitle(work: Work): string {
  return (
    work.normalized_code ||
    work.title_zh ||
    work.original_title ||
    work.source_code ||
    "未命名作品"
  );
}

/**
 * Secondary line under the card title: the preferred title when a code leads,
 * or the original title; omitted entirely when it would just repeat the title.
 */
export function libraryCardSubtitle(work: Work): string {
  const title = libraryCardTitle(work);
  const candidate = work.title_zh || work.original_title || "";
  return candidate && candidate !== title ? candidate : "";
}

/**
 * Picks the best available artwork path for a card thumbnail: prefer the
 * dedicated poster, then the cover, then thumb, then fanart. Returns null when
 * none exist so the caller can render a placeholder.
 */
export function libraryCardArtwork(work: Work): string | null {
  return work.poster_path || work.cover_path || work.thumb_path || work.fanart_path || null;
}

/**
 * Formats runtime minutes into a compact h/m string for card/detail display.
 */
export function formatRuntime(minutes: number | null | undefined): string {
  if (minutes == null || minutes <= 0) {
    return "";
  }
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  if (h > 0 && m > 0) {
    return `${h}时${m}分`;
  }
  if (h > 0) {
    return `${h}小时`;
  }
  return `${m}分钟`;
}
