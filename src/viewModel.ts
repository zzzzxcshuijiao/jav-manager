import type {
  ArchiveActionLog,
  ArchivePlan,
  CodeConflictEvidence,
  DaemonControlChannel,
  DaemonRunOnceReport,
  DaemonState,
  DiagnosticExportResult,
  DiagnosticLogEntry,
  ExceptionKind,
  ExceptionStatus,
  FileVersion,
  HoldingReason,
  IngestDecision,
  IngestItem,
  IngestItemFilters,
  InventoryConfidence,
  InventoryExportResult,
  InventoryPreviewAction,
  InventoryPreviewReport,
  InventoryResource,
  InventoryResourceRoleKind,
  InventoryReviewBucket,
  InventoryStatus,
  InventoryWorkPreview,
  RebuildReport,
  RemoteScraperSettings,
  ReviewReason,
  SelfCheckOverall,
  SelfCheckReport,
  SelfCheckSeverity,
  Work
} from "./api";

export type DecisionFilter = IngestDecision | "All";
export type ReviewReasonFilter = ReviewReason | "All";
export type CodePresenceFilter = "All" | "HasCode" | "MissingCode";
export type WorkbenchView = "ingest" | "review" | "archive" | "inventory" | "settings" | "library";
export type WorkStatusFilter = Work["watch_status"] | "All";
export type InventoryFilter = "all" | `status:${InventoryStatus}` | `review:${InventoryReviewBucket}` | "orphan";
export type InventoryStatusFilter = InventoryFilter;

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
    inventory: "一键盘点",
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

/** Parse the one-click inventory textarea into unique root paths without treating Windows separators as delimiters. */
export function parseInventoryRootsInput(value: string): string[] {
  const seen = new Set<string>();
  return value
    .split(/\r?\n/)
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

/** Format an inventory work status into the compact Chinese label shown by preview rows. */
export function formatInventoryStatus(status: InventoryStatus): string {
  const labels: Record<InventoryStatus, string> = {
    ready: "可整理",
    missing_nfo: "缺 NFO",
    missing_video: "缺视频",
    multi_video: "多视频",
    multi_nfo: "多 NFO",
    code_conflict: "番号冲突",
    duplicate_candidate: "疑似重复",
    nfo_parse_error: "NFO 解析失败",
    asset_only: "素材候选",
    orphan: "孤儿资源"
  };
  return labels[status];
}

/** Format a Stage 7B inventory review bucket for filter buttons and detail headers. */
export function formatInventoryReviewBucket(bucket: InventoryReviewBucket): string {
  const labels: Record<InventoryReviewBucket, string> = {
    auto_ready: "可自动整理",
    needs_review: "需人工确认",
    blocked: "阻断",
    asset_candidate: "素材候选"
  };
  return labels[bucket];
}

/** Format the confidence label for the inventory resolution panel. */
export function formatInventoryConfidence(confidence: InventoryConfidence): string {
  const labels: Record<InventoryConfidence, string> = {
    high: "高",
    medium: "中",
    low: "低"
  };
  return labels[confidence];
}

/** Format a resource pairing role assigned by the inventory resolver. */
export function formatInventoryResourceRole(role: InventoryResourceRoleKind): string {
  const labels: Record<InventoryResourceRoleKind, string> = {
    primary_video: "主视频",
    secondary_video: "副视频",
    duplicate_video: "疑似重复视频",
    primary_nfo: "主 NFO",
    secondary_nfo: "副 NFO",
    poster: "封面",
    fanart: "背景图",
    thumb: "缩略图",
    screenshot: "截图",
    gif: "GIF",
    image: "图片",
    other: "其他资源"
  };
  return labels[role];
}

/** Summarize one work's read-only pairing recommendation. */
export function formatInventoryResolutionSummary(work: InventoryWorkPreview): string {
  return `${work.resolution.recommended} · 置信度 ${formatInventoryConfidence(work.resolution.confidence)}`;
}

/** Filter inventory work previews by Stage 7B review bucket or existing Stage 7A status. */
export function filterInventoryWorks(works: InventoryWorkPreview[], filter: InventoryFilter): InventoryWorkPreview[] {
  if (filter === "all") {
    return works;
  }
  if (filter === "orphan") {
    return [];
  }
  if (filter.startsWith("review:")) {
    const bucket = filter.slice("review:".length) as InventoryReviewBucket;
    return works.filter((work) => work.resolution.bucket === bucket);
  }
  const status = filter.slice("status:".length) as InventoryStatus;
  return works.filter((work) => work.statuses.includes(status));
}

/** Format inventory export output for the global status line. */
export function formatInventoryExportSummary(result: InventoryExportResult): string {
  return `已导出盘点结果：${result.path}（作品 ${result.works}，素材候选 ${result.asset_candidates}，孤儿 ${result.orphans}）。`;
}

/** Return orphan resources that should remain visible for the current inventory status filter. */
export function inventoryOrphansForFilter(
  report: InventoryPreviewReport | null,
  filter: InventoryStatusFilter
): InventoryResource[] {
  if (!report) {
    return [];
  }
  return filter === "orphan" ? report.orphans : [];
}

/** Summarize an inventory preview report for status-line feedback. */
export function formatInventorySummary(report: InventoryPreviewReport): string {
  const s = report.summary;
  const suffix = report.truncated ? " 结果过多，作品、素材候选和孤儿资源明细各最多展示 1000 项。" : "";
  return `识别 ${s.works} 部作品，素材候选 ${s.asset_candidates} 组：可自动整理 ${s.auto_ready}，需人工确认 ${s.needs_review}，阻断 ${s.blocked}，缺 NFO ${s.missing_nfo}，缺视频 ${s.missing_video}，冲突 ${s.code_conflict}，孤儿 ${s.orphans}。${suffix}`;
}

/** Format the planned target path and comma-separated conflict tokens for one inventory preview action. */
export function formatInventoryActionTarget(action: InventoryPreviewAction): string {
  if (!action.to_path) {
    return "未配置归档根目录";
  }
  const conflictLabels: Record<string, string> = {
    target_exists: "目标已存在",
    target_duplicate: "目标重复"
  };
  const conflicts = action.conflict
    ?.split(",")
    .map((token) => token.trim())
    .filter(Boolean) ?? [];
  if (conflicts.length === 0) {
    return action.to_path;
  }
  const details = conflicts.map((token) => conflictLabels[token] ?? `存在冲突：${token}`);
  return `${action.to_path}（${details.join("，")}）`;
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

/** Format remote scraper settings for compact settings-page feedback. */
export function formatRemoteScraperSettingsSummary(settings: RemoteScraperSettings): string {
  if (!settings.enabled) {
    return "已停用";
  }
  const enabledSources = settings.sources.filter((source) => source.enabled).length;
  const fallback = settings.include_example_fallback
    ? "保留示例 fallback"
    : "不使用示例 fallback";
  return `已启用 · ${enabledSources} 个远程源 · ${fallback}`;
}

/** Format a diagnostic severity for compact log rows. */
export function formatDiagnosticLevel(level: DiagnosticLogEntry["level"]): string {
  const labels: Record<DiagnosticLogEntry["level"], string> = {
    Info: "信息",
    Warn: "警告",
    Error: "错误"
  };
  return labels[level] ?? level;
}

/** Format one diagnostic log entry for the settings diagnostics list. */
export function formatDiagnosticLogLine(entry: DiagnosticLogEntry): string {
  return `${entry.timestamp} · ${formatDiagnosticLevel(entry.level)} · ${entry.target} · ${entry.message}`;
}

/** Format the diagnostic export result for the global status line. */
export function formatDiagnosticExportSummary(result: DiagnosticExportResult): string {
  return `已导出诊断快照：${result.path}（日志 ${result.logs} 条，管线 ${result.pipeline_runs} 条，刮削 ${result.scrape_jobs} 条，异常 ${result.open_exceptions} 条，搁置 ${result.holding_items} 条）`;
}

/** Format one self-check item severity for the automatic pipeline panel. */
export function formatSelfCheckSeverity(severity: SelfCheckSeverity): string {
  const labels: Record<SelfCheckSeverity, string> = {
    pass: "通过",
    warn: "警告",
    fail: "失败"
  };
  return labels[severity] ?? severity;
}

/** Format the self-check overall status for compact feedback. */
export function formatSelfCheckOverall(overall: SelfCheckOverall): string {
  const labels: Record<SelfCheckOverall, string> = {
    pass: "自检通过",
    warn: "自检有警告",
    fail: "自检失败"
  };
  return labels[overall] ?? overall;
}

/** Summarize a full self-check report for the global status line. */
export function formatSelfCheckSummary(report: SelfCheckReport): string {
  const counts = report.checks.reduce(
    (next, item) => {
      next[item.severity] += 1;
      return next;
    },
    { pass: 0, warn: 0, fail: 0 } as Record<SelfCheckSeverity, number>
  );
  return `${formatSelfCheckOverall(report.overall)}：通过 ${counts.pass} 项，警告 ${counts.warn} 项，失败 ${counts.fail} 项。`;
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
