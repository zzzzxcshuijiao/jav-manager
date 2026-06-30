import { convertFileSrc } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Archive,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Clock3,
  Database,
  Film,
  FolderInput,
  FolderOpen,
  Image as ImageIcon,
  ListChecks,
  Play,
  RefreshCw,
  Search,
  Settings,
  Star,
 Tags,
  TriangleAlert,
  Trash2,
  X
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type {
  Actor,
  ArchiveActionLog,
  ArchivePlan,
  Aria2Settings,
  DaemonControlChannel,
  DaemonControlStatus,
  DaemonRunOnceReport,
  DiagnosticLogEntry,
  ExceptionEntry,
  FileVersion,
  HoldingEntry,
  IngestDecision,
  IngestItem,
  IngestJobSummary,
  InventoryExecutionReport,
  InventoryPreviewReport,
  InventoryResource,
  InventoryResourceKind,
  PipelineRun,
  PostMigrationGroupKind,
  PostMigrationExecutionReport,
  PostMigrationReviewReport,
  ReviewReason,
  RemoteScraperSettings,
  SelfCheckReport,
  ThumbnailCacheSummary,
  RebuildReport,
  Tag,
  WatchStatus,
  Work,
  WorkDetail,
  WorkRating,
  WorkSet
} from "./api";
import { api } from "./api";
import { demoItems, demoJob, demoPlan } from "./demoData";
import {
  applyIngestItemFilters,
  archiveExecutionSummary,
  archivePreviewIds,
  buildIngestItemFilters,
  buildDashboardStats,
  buildReviewQueueSummary,
  buildWorkTagSummary,
  canExecuteArchivePlan,
  coverPreviewPathForItem,
  duplicateCandidatesForItem,
  findIngestItemForWork,
  formatDaemonChannel,
  formatDaemonState,
  formatDiagnosticExportSummary,
  formatDiagnosticLogLine,
  formatRuntime,
  libraryCardArtwork,
  libraryCardSubtitle,
  libraryCardTitle,
  partitionWorksByKind,
  type CodePresenceFilter,
  type DecisionFilter,
  filterWorksForLibrary,
  formatBytes,
  formatCodeConflictEvidence,
  formatDuration,
  formatExceptionKind,
  formatExceptionStatus,
  formatFileVersionSummary,
  formatHoldingReason,
  formatInventoryActionTarget,
  filterInventoryWorks,
  formatInventoryExecutionPlanSummary,
  formatInventoryExecutionSummary,
  formatInventoryExportSummary,
  formatInventoryResourceRole,
  formatInventoryResolutionSummary,
  formatInventoryReviewBucket,
  formatInventoryStatus,
  formatInventorySummary,
  inventoryOrphansForFilter,
  formatMediaInfo,
  formatPipelineStatus,
  formatPostMigrationExecutionSummary,
  formatPostMigrationSummary,
  formatRemoteScraperSettingsSummary,
  formatSelfCheckSeverity,
  formatSelfCheckSummary,
  formatRebuildReport,
  formatWorkOption,
  ignorableDuplicateItems,
  mergeVersionTargetWorks,
  mergeableWorksForItem,
  normalizeManualCodeInput,
  parseDelimitedListInput,
  parseInventoryRootsInput,
  parseProfileRatingInput,
  replaceIngestItem,
  revalidatableMoveFailedItems,
  resolvableSelectedItems,
  selectedItemIds,
  type ReviewReasonFilter,
  shortEvidence,
  summarizeRunOnceReport,
  viewItemsForMode,
  type InventoryFilter,
  type WorkStatusFilter,
  type WorkbenchView,
  workbenchViewTitle
} from "./viewModel";

const decisionLabels: Record<IngestDecision, string> = {
  AutoArchive: "可自动归档",
  NeedsReview: "待处理",
  DuplicateCandidate: "重复候选",
  Failed: "失败",
  Ignored: "已忽略"
};

const reviewReasonLabels: Record<ReviewReason, string> = {
  MissingCode: "缺少番号",
  LowConfidence: "低置信度",
  ProviderFailed: "元数据失败",
  CodeConflict: "编号冲突",
  DuplicateFile: "重复文件",
  MoveFailed: "移动失败"
};

const availableActions = ["扫描真实目录", "筛选扫描结果", "查看条目详情", "确认匹配", "预览迁移计划"];

const watchStatusLabels: Record<WatchStatus, string> = {
  Unwatched: "未观看",
  WantToWatch: "想看",
  Watching: "观看中",
  Watched: "已观看",
  OnHold: "搁置",
  Favorite: "收藏"
};

const defaultAria2Settings: Aria2Settings = {
  enabled: false,
  host: "127.0.0.1",
  port: 6800,
  path: "/jsonrpc",
  secret: "",
  timeout_ms: 5000,
  poll_interval_secs: 30,
  tracked_gids: []
};

const defaultRemoteScraperSettings: RemoteScraperSettings = {
  enabled: false,
  timeout_ms: 8000,
  user_agent: "media-manager/0.1 local scraper",
  proxy_url: "",
  include_example_fallback: true,
  sources: [
    {
      id: "javdb",
      enabled: false,
      search_url_template: "https://javdb.com/search?q={code}&f=all",
      min_confidence: 0.82
    },
    {
      id: "javbus",
      enabled: false,
      search_url_template: "https://www.javbus.com/search/{code}",
      min_confidence: 0.82
    },
    {
      id: "fanza",
      enabled: false,
      search_url_template: "https://www.dmm.co.jp/digital/videoa/-/list/search/=/?searchstr={code}",
      min_confidence: 0.82
    }
  ]
};

const inventoryFilters: Array<{ value: InventoryFilter; label: string }> = [
  { value: "all", label: "全部" },
  { value: "review:auto_ready", label: "可自动整理" },
  { value: "review:needs_review", label: "需人工确认" },
  { value: "review:blocked", label: "阻断" },
  { value: "review:asset_candidate", label: "素材候选" },
  { value: "status:missing_nfo", label: "缺 NFO" },
  { value: "status:missing_video", label: "缺视频" },
  { value: "status:multi_video", label: "多视频" },
  { value: "status:multi_nfo", label: "多 NFO" },
  { value: "status:code_conflict", label: "番号冲突" },
  { value: "status:duplicate_candidate", label: "疑似重复" },
  { value: "status:nfo_parse_error", label: "NFO 解析失败" },
  { value: "orphan", label: "孤儿资源" }
];

const postMigrationGroupLabels: Record<PostMigrationGroupKind, string> = {
  quarantine: "隔离残留",
  multi_video: "多视频",
  asset_only: "素材补迁",
  external_asset: "外部素材"
};

const inventoryImageKinds = new Set<InventoryResourceKind>(["poster", "fanart", "thumb", "screenshot", "gif", "image"]);

/** 汇总存量预览资源数量，保持作品列表可扫读。 */
function summarizeInventoryResources(resources: InventoryResource[]): string {
  const counts = resources.reduce(
    (acc, resource) => {
      if (resource.kind === "video") {
        acc.video += 1;
      } else if (resource.kind === "nfo") {
        acc.nfo += 1;
      } else if (inventoryImageKinds.has(resource.kind)) {
        acc.image += 1;
      } else {
        acc.other += 1;
      }
      return acc;
    },
    { video: 0, nfo: 0, image: 0, other: 0 }
  );
  const parts = [
    counts.video > 0 ? `视频 ${counts.video}` : null,
    counts.nfo > 0 ? `NFO ${counts.nfo}` : null,
    counts.image > 0 ? `图片 ${counts.image}` : null,
    counts.other > 0 ? `其他 ${counts.other}` : null
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : "暂无资源";
}

export function App() {
  const [sourceRoots, setSourceRoots] = useState("H:/Downloads/A\nH:/Downloads/B\nH:/Inbox");
  const [archiveRoot, setArchiveRoot] = useState("H:/Archive");
  const [metadataProviderEnabled, setMetadataProviderEnabled] = useState(false);
  const [job, setJob] = useState<IngestJobSummary | null>(demoJob);
  const [items, setItems] = useState<IngestItem[]>(demoItems);
  const [plan, setPlan] = useState<ArchivePlan | null>(demoPlan);
  const [archiveLogs, setArchiveLogs] = useState<ArchiveActionLog[]>([]);
  const [works, setWorks] = useState<Work[]>([]);
  const [fileVersions, setFileVersions] = useState<FileVersion[]>([]);
  const [workActors, setWorkActors] = useState<Actor[]>([]);
  const [filter, setFilter] = useState<DecisionFilter>("All");
  const [reviewReasonFilter, setReviewReasonFilter] = useState<ReviewReasonFilter>("All");
  const [codePresenceFilter, setCodePresenceFilter] = useState<CodePresenceFilter>("All");
  const [activeView, setActiveView] = useState<WorkbenchView>("ingest");
  const [visibleItems, setVisibleItems] = useState<IngestItem[]>(demoItems);
  const [selectedItemId, setSelectedItemId] = useState<number | null>(demoItems[0]?.id ?? null);
  const [selectedIds, setSelectedIds] = useState<Set<number>>(() => new Set());
  const [manualCode, setManualCode] = useState(demoItems[0]?.normalized_code ?? "");
  const [mergeWorkId, setMergeWorkId] = useState("");
  const [profileTags, setProfileTags] = useState("");
  const [profileLists, setProfileLists] = useState("");
  const [profileRating, setProfileRating] = useState("");
  const [profileStatus, setProfileStatus] = useState<WatchStatus>("Unwatched");
  const [thumbnailPath, setThumbnailPath] = useState<string | null>(null);
  const [thumbnailCache, setThumbnailCache] = useState<ThumbnailCacheSummary | null>(null);
  const [rebuildReport, setRebuildReport] = useState<RebuildReport | null>(null);
  const [rebuildMode, setRebuildMode] = useState<"preview" | "rebuild">("preview");
  const [resourcePoolDirs, setResourcePoolDirs] = useState("");
  const [primaryLibraryDir, setPrimaryLibraryDir] = useState("");
  const [settingsTab, setSettingsTab] = useState<"pool" | "rebuild" | "cache" | "daemon">("pool");
  const [daemonStatus, setDaemonStatus] = useState<DaemonControlStatus | null>(null);
  const [daemonChannel, setDaemonChannel] = useState<DaemonControlChannel>("none");
  const [daemonReport, setDaemonReport] = useState<DaemonRunOnceReport | null>(null);
  const [holdingEntries, setHoldingEntries] = useState<HoldingEntry[]>([]);
  const [exceptionEntries, setExceptionEntries] = useState<ExceptionEntry[]>([]);
  const [pipelineRuns, setPipelineRuns] = useState<PipelineRun[]>([]);
  const [daemonBusy, setDaemonBusy] = useState<"refresh" | "run" | "pause" | "resume" | "resolve" | null>(null);
  const [diagnosticLogs, setDiagnosticLogs] = useState<DiagnosticLogEntry[]>([]);
  const [diagnosticsBusy, setDiagnosticsBusy] = useState<"refresh" | "export" | null>(null);
  const [selfCheckReport, setSelfCheckReport] = useState<SelfCheckReport | null>(null);
  const [selfCheckBusy, setSelfCheckBusy] = useState(false);
  const [inventoryRootsText, setInventoryRootsText] = useState("");
  const [inventoryArchiveRoot, setInventoryArchiveRoot] = useState(archiveRoot);
  const [inventoryBusy, setInventoryBusy] = useState(false);
  const [inventoryExportBusy, setInventoryExportBusy] = useState(false);
  const [inventoryReport, setInventoryReport] = useState<InventoryPreviewReport | null>(null);
  const [inventoryExecuteBusy, setInventoryExecuteBusy] = useState(false);
  const [inventoryExecutionReport, setInventoryExecutionReport] = useState<InventoryExecutionReport | null>(null);
  const [postMigrationBusy, setPostMigrationBusy] = useState(false);
  const [postMigrationExecuteBusy, setPostMigrationExecuteBusy] = useState(false);
  const [postMigrationReport, setPostMigrationReport] = useState<PostMigrationReviewReport | null>(null);
  const [postMigrationExecutionReport, setPostMigrationExecutionReport] = useState<PostMigrationExecutionReport | null>(null);
  const [inventoryStatusFilter, setInventoryStatusFilter] = useState<InventoryFilter>("all");
  const [selectedInventoryCode, setSelectedInventoryCode] = useState<string | null>(null);
  const [aria2Settings, setAria2Settings] = useState<Aria2Settings>(defaultAria2Settings);
  const [aria2GidsText, setAria2GidsText] = useState("");
  const [aria2Busy, setAria2Busy] = useState(false);
  const [remoteScraperSettings, setRemoteScraperSettings] = useState<RemoteScraperSettings>(defaultRemoteScraperSettings);
  const [remoteScraperBusy, setRemoteScraperBusy] = useState(false);
  const [libraryWorkDetail, setLibraryWorkDetail] = useState<WorkDetail | null>(null);
  const [nonStandardCollapsed, setNonStandardCollapsed] = useState(true);
  const [busy, setBusy] = useState(false);
  const [lightboxSrc, setLightboxSrc] = useState<string | null>(null);
  const [posterDir, setPosterDir] = useState("");
  const [screenshotDir, setScreenshotDir] = useState("");
  const [gifDir, setGifDir] = useState("");
  const [selectedFileVersionIds, setSelectedFileVersionIds] = useState<Set<number>>(() => new Set());
  const [mergeVersionTargetWorkId, setMergeVersionTargetWorkId] = useState("");
  const [libraryQuery, setLibraryQuery] = useState("");
  const [libraryStatusFilter, setLibraryStatusFilter] = useState<WorkStatusFilter>("All");
  const [selectedLibraryWorkId, setSelectedLibraryWorkId] = useState<number | null>(null);
  const [libraryFileVersions, setLibraryFileVersions] = useState<FileVersion[]>([]);
  const [libraryWorkActors, setLibraryWorkActors] = useState<Actor[]>([]);
  async function runBusy<T>(label: string, action: () => Promise<T>): Promise<T | null> {
    setBusy(true);
    setStatus(label);
    try {
      return await action();
    } catch (error) {
      setStatus(`${label}失败：${String(error)}`);
      return null;
    } finally {
      setBusy(false);
    }
  }

  const hasBackend = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const [status, setStatus] = useState("演示数据已载入；Tauri 桌面环境可以扫描真实目录并写入 SQLite。");

  const stats = useMemo(() => buildDashboardStats(items), [items]);
  const reviewSummary = useMemo(() => buildReviewQueueSummary(items), [items]);
  const activeFilters = useMemo(
    () => buildIngestItemFilters(filter, reviewReasonFilter, codePresenceFilter),
    [filter, reviewReasonFilter, codePresenceFilter]
  );
  const tableItems = useMemo(() => viewItemsForMode(visibleItems, activeView), [activeView, visibleItems]);
  const selectedBatchIds = useMemo(() => selectedItemIds(tableItems, selectedIds), [tableItems, selectedIds]);
  const resolvableBatchItems = useMemo(() => resolvableSelectedItems(tableItems, selectedIds), [tableItems, selectedIds]);
  const revalidatableBatchItems = useMemo(
    () => revalidatableMoveFailedItems(tableItems, selectedIds),
    [tableItems, selectedIds]
  );
  const ignorableBatchItems = useMemo(
    () => ignorableDuplicateItems(tableItems, selectedIds),
    [tableItems, selectedIds]
  );
  const selectedItem = tableItems.find((item) => item.id === selectedItemId) ?? tableItems[0] ?? null;
  const selectedWork = works.find((work) => work.id != null && work.id === selectedItem?.candidate_work_id) ?? null;
  const coverPreviewPath = useMemo(() => coverPreviewPathForItem(selectedItem, selectedWork), [selectedItem, selectedWork]);
  const posterPreviewPath = coverPreviewPath ?? thumbnailPath;
  const posterPreviewSrc = useMemo(() => {
    if (!posterPreviewPath) {
      return null;
    }
    if (/^(https?:|data:|blob:|asset:)/i.test(posterPreviewPath)) {
      return posterPreviewPath;
    }
    try {
      return convertFileSrc(posterPreviewPath);
    } catch {
      return posterPreviewPath;
    }
  }, [posterPreviewPath]);
  const mergeableWorks = useMemo(() => mergeableWorksForItem(works, selectedItem), [selectedItem, works]);
  const versionMergeTargets = useMemo(() => mergeVersionTargetWorks(works, selectedWork), [selectedWork, works]);
  const duplicateCandidates = useMemo(() => duplicateCandidatesForItem(items, selectedItem), [items, selectedItem]);
  const codeConflictText = useMemo(
    () => formatCodeConflictEvidence(selectedItem?.code_conflict),
    [selectedItem?.code_conflict]
  );
  const libraryWorks = useMemo(
    () => filterWorksForLibrary(works, libraryQuery, libraryStatusFilter),
    [libraryQuery, libraryStatusFilter, works]
  );
  const libraryTags = useMemo(() => buildWorkTagSummary(works), [works]);
  const libraryPartition = useMemo(() => partitionWorksByKind(libraryWorks), [libraryWorks]);
  const selectedLibraryWork = libraryWorks.find((work) => work.id === selectedLibraryWorkId) ?? libraryWorks[0] ?? null;
  const selectedLibraryArtwork = selectedLibraryWork ? libraryCardArtwork(selectedLibraryWork) : null;
  const selectedLibraryArtworkSrc = useMemo(() => {
    if (!selectedLibraryArtwork) return null;
    if (/^(https?:|data:|blob:|asset:)/i.test(selectedLibraryArtwork)) return selectedLibraryArtwork;
    try { return convertFileSrc(selectedLibraryArtwork); } catch { return selectedLibraryArtwork; }
  }, [selectedLibraryArtwork]);
  const selectedLibraryIngestItem = useMemo(
    () => findIngestItemForWork(items, selectedLibraryWork),
    [items, selectedLibraryWork]
  );
  const inventoryWorkSource = inventoryReport
    ? inventoryStatusFilter === "review:asset_candidate"
      ? inventoryReport.asset_candidates
      : inventoryReport.works
    : [];
  const filteredInventoryWorks = filterInventoryWorks(inventoryWorkSource, inventoryStatusFilter);
  const visibleInventoryOrphans = inventoryOrphansForFilter(inventoryReport, inventoryStatusFilter);
  const inventoryListItemCount = filteredInventoryWorks.length + visibleInventoryOrphans.length;
  const selectedInventoryWork =
    filteredInventoryWorks.find((work) => work.code === selectedInventoryCode) ?? filteredInventoryWorks[0] ?? null;
  const inventoryExecutableCount =
    inventoryReport?.works.filter((work) => work.resolution.bucket === "auto_ready" && work.resolution.execution_plan.ready).length ?? 0;
  const inventoryExecutionBlockedByTruncation =
    Boolean(inventoryReport && inventoryReport.summary.works > inventoryReport.works.length);
  const postMigrationReadyActionCount = postMigrationReport?.summary.ready_actions ?? 0;
  const selectedInventoryRoleByPath = useMemo(
    () => new Map(selectedInventoryWork?.resource_roles.map((role) => [role.path, role]) ?? []),
    [selectedInventoryWork]
  );
  const showArchiveControls = activeView === "ingest" || activeView === "review" || activeView === "archive";

  useEffect(() => {
    let cancelled = false;
    async function loadPersistedStartupState() {
      try {
        const [
          storedSourceRoots,
          storedArchiveRoot,
          storedMetadataProviderEnabled,
          storedArchiveLogs,
          latestJob,
          storedWorks
        ] = await Promise.all([
          api.getSourceRoots(),
          api.getArchiveRoot(),
          api.getMetadataProviderEnabled(),
          api.listArchiveActionLogs(),
          api.getLatestIngestJob(),
          api.listWorks()
        ]);
        if (cancelled) {
          return;
        }
        if (storedSourceRoots.length > 0) {
          const sourceRootText = storedSourceRoots.join("\n");
          setSourceRoots(sourceRootText);
          setInventoryRootsText((current) => (current.trim() ? current : sourceRootText));
        }
        if (storedArchiveRoot) {
          setArchiveRoot(storedArchiveRoot);
          setInventoryArchiveRoot((current) => (current.trim() && current !== "H:/Archive" ? current : storedArchiveRoot));
        }
        setMetadataProviderEnabled(storedMetadataProviderEnabled);
        try {
          const dirs = await api.getPosterDirs();
          setPosterDir(dirs.poster_dir ?? "");
          setScreenshotDir(dirs.screenshot_dir ?? "");
          setGifDir(dirs.gif_dir ?? "");
        } catch {
          // poster dirs are optional
        }
        try {
          const poolDirs = await api.getResourcePoolDirs();
          if (poolDirs.length > 0) {
            setResourcePoolDirs(poolDirs.join("\n"));
          }
        } catch {
          // resource pool dirs are optional
        }
        try {
          const primary = await api.getPrimaryLibraryDir();
          if (primary) {
            setPrimaryLibraryDir(primary);
          }
        } catch {
          // primary library dir is optional
        }
        setArchiveLogs(storedArchiveLogs);
        setWorks(storedWorks);
        if (latestJob) {
          const latestItems = await api.listIngestItems(latestJob.id, activeFilters);
          if (cancelled) {
            return;
          }
          setJob(latestJob);
          setItems(latestItems);
          setVisibleItems(latestItems);
          setSelectedItemId(latestItems[0]?.id ?? null);
          setStatus(`已从 SQLite 恢复最近扫描任务 #${latestJob.id}：${latestJob.total_items} 个视频，${latestJob.review_count} 个待处理。`);
        }
      } catch {
        // Demo/browser mode keeps the default paths until Tauri is available.
      }
    }
    loadPersistedStartupState();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    setManualCode(selectedItem?.normalized_code ?? "");
    setMergeWorkId("");
  }, [selectedItem?.id, selectedItem?.normalized_code]);

  useEffect(() => {
    let cancelled = false;
    if (!selectedItem || coverPreviewPath) {
      setThumbnailPath(null);
      return () => {
        cancelled = true;
      };
    }
    setThumbnailPath(null);
    api
      .getOrCreateThumbnail(selectedItem.path)
      .then((path) => {
        if (!cancelled) {
          setThumbnailPath(path);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setThumbnailPath(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedItem?.path, coverPreviewPath]);

  useEffect(() => {
    setProfileTags(selectedWork?.tags.join(", ") ?? "");
    setProfileLists(selectedWork?.lists.join(", ") ?? "");
    setProfileRating(selectedWork?.rating != null ? String(selectedWork.rating) : "");
    setProfileStatus(selectedWork?.watch_status ?? "Unwatched");
  }, [selectedWork?.id, selectedWork?.tags, selectedWork?.lists, selectedWork?.rating, selectedWork?.watch_status]);

  useEffect(() => {
    setSelectedFileVersionIds(new Set());
    setMergeVersionTargetWorkId("");
  }, [selectedWork?.id]);

  useEffect(() => {
    let cancelled = false;
    async function loadVisibleItems() {
      if (!job) {
        setVisibleItems(applyIngestItemFilters(items, activeFilters));
        return;
      }
      try {
        const filteredItems = await api.listIngestItems(job.id, activeFilters);
        if (!cancelled) {
          setVisibleItems(filteredItems);
        }
      } catch {
        if (!cancelled) {
          setVisibleItems(applyIngestItemFilters(items, activeFilters));
        }
      }
    }
    loadVisibleItems();
    return () => {
      cancelled = true;
    };
  }, [job?.id, items, activeFilters]);

  useEffect(() => {
    setSelectedIds(new Set());
  }, [filter, reviewReasonFilter, codePresenceFilter]);

  useEffect(() => {
    let cancelled = false;
   async function loadFileVersions() {
  if (!selectedWork?.id) {
     setFileVersions([]);
     return;
   }
    try {
      const [versions, actors] = await Promise.all([
        api.listFileVersionsForWork(selectedWork.id),
        api.listWorkActors(selectedWork.id),
      ]);
      if (!cancelled) {
        setFileVersions(versions);
        setWorkActors(actors);
      }
    } catch {
      if (!cancelled) {
        setFileVersions([]);
        setWorkActors([]);
      }
    }
  }
    loadFileVersions();
    return () => {
      cancelled = true;
    };
  }, [selectedWork?.id]);

  useEffect(() => {
    if (activeView !== "settings") {
      return;
    }
    refreshThumbnailCache();
  }, [activeView]);

  useEffect(() => {
    if (activeView !== "settings" || settingsTab !== "daemon") {
      return;
    }
    if (!hasBackend) {
      setStatus("自动管线需要 Tauri 后端；普通浏览器只能查看前端壳。");
      return;
    }
    refreshDaemonPanel();
  }, [activeView, settingsTab]);

  useEffect(() => {
    if (activeView !== "library") {
      return;
    }
    refreshWorks();
  }, [activeView]);

  useEffect(() => {
    let cancelled = false;
    async function loadLibraryWorkDetail() {
      if (!selectedLibraryWork?.id) {
        setLibraryFileVersions([]);
        setLibraryWorkDetail(null);
        return;
      }
      try {
        const detail = await api.listWorkDetail(selectedLibraryWork.id);
        if (!cancelled) {
          setLibraryWorkDetail(detail);
          setLibraryFileVersions(detail?.file_versions ?? []);
        }
      } catch {
        if (!cancelled) {
          try {
            const versions = await api.listFileVersionsForWork(selectedLibraryWork.id);
            if (!cancelled) {
              setLibraryFileVersions(versions);
              setLibraryWorkDetail(null);
            }
          } catch {
            if (!cancelled) {
              setLibraryFileVersions([]);
              setLibraryWorkDetail(null);
            }
          }
        }
      }
    }
    loadLibraryWorkDetail();
    return () => {
      cancelled = true;
    };
  }, [selectedLibraryWork?.id]);

  function unavailable(action: string) {
    setStatus(`${action} 还没有接入；当前版本先验证入库扫描、匹配确认和迁移计划。`);
  }

  async function refreshPersistedState(jobId: number, preferredItemId?: number | null) {
    const [nextJob, nextItems, nextWorks] = await Promise.all([
      api.getIngestJob(jobId),
      api.listIngestItems(jobId),
      api.listWorks()
    ]);
    setJob(nextJob);
    setItems(nextItems);
    setVisibleItems(applyIngestItemFilters(nextItems, activeFilters));
    setWorks(nextWorks);
    setSelectedItemId(preferredItemId ?? nextItems[0]?.id ?? null);
    setSelectedIds(new Set());
    setMergeWorkId("");
    setFileVersions([]);
    setSelectedFileVersionIds(new Set());
    setMergeVersionTargetWorkId("");
  }

  function toggleSelectedItem(item: IngestItem) {
    if (item.id == null) {
      return;
    }
    setSelectedIds((current) => {
      const next = new Set(current);
      if (next.has(item.id as number)) {
        next.delete(item.id as number);
      } else {
        next.add(item.id as number);
      }
      return next;
    });
  }

  function clearBatchSelection() {
    setSelectedIds(new Set());
  }

  function toggleAllVisibleItems() {
    const selectableIds = tableItems.filter((item) => item.id != null).map((item) => item.id as number);
    const allSelected = selectableIds.length > 0 && selectableIds.every((id) => selectedIds.has(id));
    setSelectedIds(allSelected ? new Set() : new Set(selectableIds));
  }

  function toggleFileVersion(version: FileVersion) {
    if (version.id == null) {
      return;
    }
    setSelectedFileVersionIds((current) => {
      const next = new Set(current);
      if (next.has(version.id as number)) {
        next.delete(version.id as number);
      } else {
        next.add(version.id as number);
      }
      return next;
    });
  }

  function clearFileVersionSelection() {
    setSelectedFileVersionIds(new Set());
  }

  function toggleAllFileVersions() {
    const selectableIds = fileVersions.filter((v) => v.id != null).map((v) => v.id as number);
    const allSelected = selectableIds.length > 0 && selectableIds.every((id) => selectedFileVersionIds.has(id));
    setSelectedFileVersionIds(allSelected ? new Set() : new Set(selectableIds));
  }

  function parsedSourceRoots() {
    return sourceRoots
      .split(/\r?\n/)
      .map((value) => value.trim())
      .filter(Boolean);
  }

  /** 将存量扫描输入拆成去空白的根目录列表。 */
  function inventoryRootsFromText(): string[] {
    return parseInventoryRootsInput(inventoryRootsText);
  }

  /** 通过系统目录选择器追加盘点入口目录，普通浏览器不可用时保留手动输入路径。 */
  async function pickInventoryRoots() {
    try {
      const selected = await openDialog({ directory: true, multiple: true });
      const pickedRoots = Array.isArray(selected) ? selected : typeof selected === "string" ? [selected] : [];
      if (pickedRoots.length === 0) {
        return;
      }
      const nextRoots = parseInventoryRootsInput([...inventoryRootsFromText(), ...pickedRoots].join("\n"));
      setInventoryRootsText(nextRoots.join("\n"));
      setActiveView("inventory");
      setStatus(`已加入 ${pickedRoots.length} 个盘点入口。`);
    } catch (error) {
      setStatus(`选择盘点入口失败：${String(error)}`);
    }
  }

  /** 选择存量整理预览使用的目标根目录，只影响盘点预览和导出结果。 */
  async function pickInventoryArchiveRoot() {
    try {
      const selected = await openDialog({ directory: true, multiple: false });
      const pickedRoot = Array.isArray(selected) ? selected[0] : selected;
      if (typeof pickedRoot !== "string" || pickedRoot.trim().length === 0) {
        return;
      }
      setInventoryArchiveRoot(pickedRoot);
      setActiveView("inventory");
      setStatus(`已设置盘点整理目标：${pickedRoot}`);
    } catch (error) {
      setStatus(`选择盘点整理目标失败：${String(error)}`);
    }
  }

  function switchView(view: WorkbenchView) {
    setActiveView(view);
    if (view === "ingest" || view === "review") {
      setFilter("All");
      setReviewReasonFilter("All");
      setCodePresenceFilter("All");
    }
    setSelectedIds(new Set());
    setStatus(`${workbenchViewTitle(view)}已打开。`);
  }

  function focusReviewReason(reason: ReviewReason | "All") {
    setActiveView("review");
    setFilter("All");
    setReviewReasonFilter(reason);
    setCodePresenceFilter("All");
    setSelectedIds(new Set());
    setStatus(reason === "All" ? "已显示全部待处理条目。" : `已按${reviewReasonLabels[reason]}筛选待处理条目。`);
  }

  function openLibraryWorkInIngestDetail() {
    if (!selectedLibraryWork) {
      setStatus("请先选择一个作品。");
      return;
    }
    if (!selectedLibraryIngestItem?.id) {
      setStatus(`${selectedLibraryWork.normalized_code} 当前不在已恢复的入库队列中，只能在作品库里只读查看。`);
      return;
    }
    setActiveView("ingest");
    setFilter("All");
    setReviewReasonFilter("All");
    setCodePresenceFilter("All");
    setSelectedItemId(selectedLibraryIngestItem.id);
    setSelectedIds(new Set());
    setStatus(`${selectedLibraryWork.normalized_code} 已在入库详情中打开，可编辑作品资料或查看版本。`);
  }

  async function saveConfiguration() {
    try {
      await api.configureSourceRoots(parsedSourceRoots());
      await api.configureArchiveRoot(archiveRoot.trim());
      await api.configureMetadataProviderEnabled(metadataProviderEnabled);
      await api.configurePosterDirs(posterDir.trim(), screenshotDir.trim(), gifDir.trim());
      setStatus("来源目录、归档根目录和元数据源开关已保存到 SQLite。");
    } catch (error) {
      setStatus(`保存设置失败：${String(error)}`);
    }
  }

  async function refreshThumbnailCache() {
    try {
      const summary = await api.getThumbnailCacheSummary();
      setThumbnailCache(summary);
      setStatus(`缩略图缓存：${summary.file_count} 个文件，占用 ${formatBytes(summary.total_bytes)}。`);
    } catch (error) {
      setStatus(`读取缩略图缓存失败：${String(error)}`);
    }
  }

  async function refreshWorks() {
    try {
      const nextWorks = await api.listWorks();
      setWorks(nextWorks);
      if (!selectedLibraryWorkId && nextWorks[0]?.id) {
        setSelectedLibraryWorkId(nextWorks[0].id);
      }
      setStatus(`作品库已刷新：${nextWorks.length} 个作品。`);
    } catch (error) {
      setStatus(`刷新作品库失败：${String(error)}`);
    }
  }

  function applyAria2Settings(settings: Aria2Settings) {
    setAria2Settings({ ...settings, secret: settings.secret ?? "" });
    setAria2GidsText(settings.tracked_gids.join("\n"));
  }

  function applyRemoteScraperSettings(settings: RemoteScraperSettings) {
    setRemoteScraperSettings({ ...settings, proxy_url: settings.proxy_url ?? "" });
  }

  async function loadDaemonPanelData() {
    const [
      nextStatus,
      nextHolding,
      nextExceptions,
      nextRuns,
      nextAria2Settings,
      nextRemoteScraperSettings,
      nextDiagnosticLogs
    ] = await Promise.all([
      api.getDaemonStatus(),
      api.listHoldingEntries(),
      api.listExceptionEntries(),
      api.listPipelineRuns(),
      api.getAria2Settings(),
      api.getRemoteScraperSettings(),
      api.getDiagnosticLogTail(80)
    ]);
    setDaemonStatus(nextStatus);
    setHoldingEntries(nextHolding);
    setExceptionEntries(nextExceptions);
    setPipelineRuns(nextRuns);
    setDaemonChannel(api.getDaemonControlChannel());
    applyAria2Settings(nextAria2Settings);
    applyRemoteScraperSettings(nextRemoteScraperSettings);
    setDiagnosticLogs(nextDiagnosticLogs);
    return nextStatus;
  }

  async function refreshDaemonPanel() {
    if (daemonBusy) return;
    setDaemonBusy("refresh");
    setStatus("正在刷新自动管线状态...");
    try {
      const nextStatus = await loadDaemonPanelData();
      setStatus(`自动管线状态：${formatDaemonState(nextStatus.state)}，异常 ${nextStatus.open_exceptions}，搁置 ${nextStatus.holding_items}。`);
    } catch (error) {
      setStatus(`刷新自动管线失败：${String(error)}`);
    } finally {
      setDaemonBusy(null);
    }
  }

  /** 生成只读存量整理预览，不执行任何文件移动。 */
  async function generateInventoryPreview() {
    if (inventoryBusy) return;
    const roots = inventoryRootsFromText();
    const targetRoot = inventoryArchiveRoot.trim();
    if (roots.length === 0) {
      setStatus("请先填写至少一个存量扫描根目录。");
      return;
    }
    const startedAt = Date.now();
    setInventoryBusy(true);
    setInventoryReport(null);
    setInventoryExecutionReport(null);
    setPostMigrationReport(null);
    setPostMigrationExecutionReport(null);
    setInventoryStatusFilter("all");
    setSelectedInventoryCode(null);
    setStatus(`正在扫描 ${roots.length} 个存量根目录...`);
    try {
      const report = await api.previewInventory(roots, targetRoot.length > 0 ? targetRoot : null);
      const elapsedSeconds = ((Date.now() - startedAt) / 1000).toFixed(1);
      setInventoryReport(report);
      setInventoryStatusFilter("all");
      setSelectedInventoryCode(report.works[0]?.code ?? null);
      setStatus(`${formatInventorySummary(report)} 扫描 ${roots.length} 个根目录，用时 ${elapsedSeconds} 秒。`);
    } catch (error) {
      setInventoryReport(null);
      setInventoryExecutionReport(null);
      setPostMigrationReport(null);
      setPostMigrationExecutionReport(null);
      setSelectedInventoryCode(null);
      setStatus(`存量整理预览失败：${String(error)}`);
    } finally {
      setInventoryBusy(false);
    }
  }

  /** 导出当前盘点预览 JSON，便于人工复核或发给协作者排查。 */
  async function exportInventoryPreview() {
    if (!inventoryReport || inventoryExportBusy) {
      return;
    }
    setInventoryExportBusy(true);
    setStatus("正在导出盘点结果...");
    try {
      const result = await api.exportInventoryReport(inventoryReport);
      setStatus(formatInventoryExportSummary(result));
    } catch (error) {
      setStatus(`导出盘点结果失败：${String(error)}`);
    } finally {
      setInventoryExportBusy(false);
    }
  }

  /** 集中迁移当前盘点报告中的安全执行计划，成功后源路径不再保留。 */
  async function executeInventoryPreview() {
    if (!inventoryReport || inventoryExecuteBusy) {
      return;
    }
    if (inventoryExecutableCount === 0) {
      setStatus("当前盘点结果没有可集中迁移的安全计划。");
      return;
    }
    if (inventoryExecutionBlockedByTruncation) {
      setStatus("报告明细已截断，不能集中迁移全部作品；请缩小入口目录后重新盘点。");
      return;
    }
    const targetRoot = inventoryReport.archive_root ?? inventoryArchiveRoot.trim();
    const confirmed = window.confirm(`将集中迁移 ${inventoryExecutableCount} 部作品到 ${targetRoot || "未设置目标"}。这会移动文件，成功后源路径不再保留；同盘直接移动；跨盘会逐文件复制校验后删除源文件；目标已存在不会覆盖；失败会停止后续队列。是否继续？`);
    if (!confirmed) {
      setStatus("已取消集中迁移。");
      return;
    }
    setInventoryExecuteBusy(true);
    setInventoryExecutionReport(null);
    setStatus(`正在集中迁移 ${inventoryExecutableCount} 部作品...`);
    try {
      const result = await api.executeInventoryPlan(inventoryReport, [], "move");
      setInventoryExecutionReport(result);
      setStatus(`${formatInventoryExecutionSummary(result)} 建议重新盘点验证目标状态。`);
    } catch (error) {
      setStatus(`集中迁移失败：${String(error)}`);
    } finally {
      setInventoryExecuteBusy(false);
    }
  }

  /** 生成迁移后复盘报告，用于补迁素材、多视频残留和隔离临时文件。 */
  async function generatePostMigrationReview() {
    if (postMigrationBusy || postMigrationExecuteBusy) {
      return;
    }
    const roots = inventoryRootsFromText();
    const targetRoot = inventoryArchiveRoot.trim();
    if (roots.length === 0) {
      setStatus("请先填写至少一个源目录，再进行迁移后复盘。");
      return;
    }
    if (targetRoot.length === 0) {
      setStatus("请先填写整理目标目录，再进行迁移后复盘。");
      return;
    }
    const startedAt = Date.now();
    setPostMigrationBusy(true);
    setPostMigrationReport(null);
    setPostMigrationExecutionReport(null);
    setStatus(`正在复盘 ${roots.length} 个源目录的迁移残留...`);
    try {
      const report = await api.previewPostMigrationReview(roots, targetRoot);
      const elapsedSeconds = ((Date.now() - startedAt) / 1000).toFixed(1);
      setPostMigrationReport(report);
      setStatus(`${formatPostMigrationSummary(report)} 用时 ${elapsedSeconds} 秒。`);
    } catch (error) {
      setPostMigrationReport(null);
      setPostMigrationExecutionReport(null);
      setStatus(`迁移后复盘失败：${String(error)}`);
    } finally {
      setPostMigrationBusy(false);
    }
  }

  /** 执行迁移后复盘报告中的无冲突补迁/清理动作。 */
  async function executePostMigrationReview() {
    if (!postMigrationReport || postMigrationExecuteBusy) {
      return;
    }
    if (postMigrationReadyActionCount === 0) {
      setStatus("当前复盘报告没有可执行的补迁/清理动作。");
      return;
    }
    if (postMigrationReport.truncated) {
      setStatus("复盘报告明细已截断，不能执行补迁；请缩小入口目录后重新复盘。");
      return;
    }
    const confirmed = window.confirm(`将执行 ${postMigrationReadyActionCount} 个补迁/清理/恢复动作。会移动素材/多视频残留，删除已被目标文件验证覆盖的隔离临时文件，或把目标缺失的隔离文件恢复回源目录；目标已存在不会覆盖。是否继续？`);
    if (!confirmed) {
      setStatus("已取消补迁执行。");
      return;
    }
    setPostMigrationExecuteBusy(true);
    setPostMigrationExecutionReport(null);
    setStatus(`正在执行 ${postMigrationReadyActionCount} 个补迁/清理动作...`);
    try {
      const result = await api.executePostMigrationPlan(postMigrationReport.roots, postMigrationReport.archive_root, []);
      setPostMigrationExecutionReport(result);
      setStatus(formatPostMigrationExecutionSummary(result));
    } catch (error) {
      setStatus(`补迁执行失败：${String(error)}`);
    } finally {
      setPostMigrationExecuteBusy(false);
    }
  }

  async function runDaemonOnce() {
    if (daemonBusy) return;
    setDaemonBusy("run");
    setStatus("自动管线正在运行一轮...");
    try {
      const report = await api.runDaemonOnce();
      setDaemonReport(report);
      await loadDaemonPanelData();
      setStatus(summarizeRunOnceReport(report));
    } catch (error) {
      setStatus(`自动管线运行失败：${String(error)}`);
      try {
        await loadDaemonPanelData();
      } catch {
        // Preserve the operation failure as the visible status.
      }
    } finally {
      setDaemonBusy(null);
    }
  }

  async function saveAria2Settings() {
    if (aria2Busy) return;
    setAria2Busy(true);
    setStatus("正在保存 aria2 配置...");
    try {
      const saved = await api.configureAria2Settings({
        ...aria2Settings,
        secret: aria2Settings.secret?.trim() ? aria2Settings.secret : null,
        tracked_gids: aria2GidsText
          .split(/\r?\n|,/)
          .map((gid) => gid.trim())
          .filter(Boolean)
      });
      applyAria2Settings(saved);
      setStatus(`aria2 配置已保存：${saved.enabled ? "已启用" : "已停用"}，跟踪 ${saved.tracked_gids.length} 个 GID。`);
    } catch (error) {
      setStatus(`保存 aria2 配置失败：${String(error)}`);
    } finally {
      setAria2Busy(false);
    }
  }

  async function saveRemoteScraperSettings() {
    if (remoteScraperBusy) return;
    setRemoteScraperBusy(true);
    setStatus("正在保存远程刮削器配置...");
    try {
      const saved = await api.configureRemoteScraperSettings({
        ...remoteScraperSettings,
        proxy_url: remoteScraperSettings.proxy_url?.trim() ? remoteScraperSettings.proxy_url : null
      });
      applyRemoteScraperSettings(saved);
      setStatus(`远程刮削器配置已保存：${formatRemoteScraperSettingsSummary(saved)}。`);
    } catch (error) {
      setStatus(`保存远程刮削器配置失败：${String(error)}`);
    } finally {
      setRemoteScraperBusy(false);
    }
  }

  async function refreshDiagnosticLogs() {
    if (diagnosticsBusy) return;
    setDiagnosticsBusy("refresh");
    setStatus("正在刷新诊断日志...");
    try {
      const logs = await api.getDiagnosticLogTail(80);
      setDiagnosticLogs(logs);
      setStatus(`已刷新诊断日志：${logs.length} 条。`);
    } catch (error) {
      setStatus(`刷新诊断日志失败：${String(error)}`);
    } finally {
      setDiagnosticsBusy(null);
    }
  }

  async function exportDiagnosticsSnapshot() {
    if (diagnosticsBusy) return;
    setDiagnosticsBusy("export");
    setStatus("正在导出诊断快照...");
    try {
      const result = await api.exportDiagnosticsSnapshot();
      setStatus(formatDiagnosticExportSummary(result));
      const logs = await api.getDiagnosticLogTail(80);
      setDiagnosticLogs(logs);
    } catch (error) {
      setStatus(`导出诊断快照失败：${String(error)}`);
    } finally {
      setDiagnosticsBusy(null);
    }
  }

  async function runPipelineSelfCheck() {
    if (selfCheckBusy) return;
    setSelfCheckBusy(true);
    setStatus("正在执行自动管线自检...");
    try {
      const report = await api.runPipelineSelfCheck();
      setSelfCheckReport(report);
      setStatus(formatSelfCheckSummary(report));
      await loadDaemonPanelData();
    } catch (error) {
      setStatus(`自动管线自检失败：${String(error)}`);
    } finally {
      setSelfCheckBusy(false);
    }
  }

  async function pauseDaemon() {
    if (daemonBusy) return;
    setDaemonBusy("pause");
    setStatus("正在暂停自动管线...");
    try {
      const nextStatus = await api.pauseDaemon();
      await loadDaemonPanelData();
      setDaemonStatus(nextStatus);
      setStatus("自动管线已暂停。");
    } catch (error) {
      setStatus(`暂停自动管线失败：${String(error)}`);
    } finally {
      setDaemonBusy(null);
    }
  }

  async function resumeDaemon() {
    if (daemonBusy) return;
    setDaemonBusy("resume");
    setStatus("正在恢复自动管线...");
    try {
      const nextStatus = await api.resumeDaemon();
      await loadDaemonPanelData();
      setDaemonStatus(nextStatus);
      setStatus("自动管线已恢复。");
    } catch (error) {
      setStatus(`恢复自动管线失败：${String(error)}`);
    } finally {
      setDaemonBusy(null);
    }
  }

  async function resolveDaemonException(id: number | null | undefined, nextStatus: "Ignored" | "Resolved") {
    if (daemonBusy || id == null) return;
    setDaemonBusy("resolve");
    setStatus(nextStatus === "Resolved" ? "正在标记异常为已解决..." : "正在忽略异常...");
    try {
      await api.resolveExceptionEntry(id, nextStatus);
      await loadDaemonPanelData();
      setStatus(nextStatus === "Resolved" ? "异常已标记为已解决。" : "异常已忽略。");
    } catch (error) {
      setStatus(`更新异常状态失败：${String(error)}`);
    } finally {
      setDaemonBusy(null);
    }
  }

  async function clearThumbnailCache() {
    try {
      const cleared = await api.clearThumbnailCache();
      setThumbnailCache({ file_count: 0, total_bytes: 0 });
      setThumbnailPath(null);
      setStatus(`已清理缩略图缓存：${cleared.file_count} 个文件，释放 ${formatBytes(cleared.total_bytes)}。`);
    } catch (error) {
      setStatus(`清理缩略图缓存失败：${String(error)}`);
    }
  }

  async function previewRebuild() {
    if (busy) return;
    setBusy(true);
    setStatus("正在预览重建...");
    try {
      const report = await api.previewRebuild(parsedSourceRoots());
      setRebuildMode("preview");
      setRebuildReport(report);
      setStatus(formatRebuildReport("preview", report));
    } catch (error) {
      setStatus(`预览重建失败：${String(error)}`);
    } finally {
      setBusy(false);
    }
  }

  async function runRebuildLibrary() {
    if (busy) return;
    if (!window.confirm("重建将清空现有作品数据并从 NFO 重新解析，确定继续？")) {
      return;
    }
    try {
      setRebuildReport(null);
      setStatus("正在重建作品库，请稍候…");
      const report = await api.rebuildLibraryFromNfo(parsedSourceRoots());
      setRebuildMode("rebuild");
      setRebuildReport(report);
      setStatus(formatRebuildReport("rebuild", report));
      await refreshWorks();
    } catch (error) {
      setStatus(`重建作品库失败：${String(error)}`);
    }
  }

  function parsedResourcePoolDirs(): string[] {
    return resourcePoolDirs.split("\n").map((line) => line.trim()).filter((line) => line.length > 0);
  }

  async function saveResourcePool() {
    const dirs = parsedResourcePoolDirs();
    try {
      await api.configureResourcePoolDirs(dirs);
      setStatus(`已保存索引来源目录（${dirs.length} 个）`);
    } catch (error) {
      setStatus(`保存索引来源失败：${String(error)}`);
    }
  }

  async function runRebuildFromPool() {
    if (busy) return;
    const dirs = parsedResourcePoolDirs();
    if (dirs.length === 0) {
      setStatus("请填写至少一个索引来源目录");
      return;
    }
    if (!window.confirm("将从索引来源目录重新解析作品库，确定继续？")) {
      return;
    }
    const report = await runBusy<RebuildReport>("正在读取索引来源并重建作品库，请稍候…", () => api.rebuildLibraryFromPool(dirs));
    if (!report) return;
    setRebuildMode("rebuild");
    setRebuildReport(report);
    setStatus(formatRebuildReport("rebuild", report));
    await refreshWorks();
  }

  async function savePrimaryLibrary() {
    const dir = primaryLibraryDir.trim();
    if (!dir) {
      setStatus("请填写主库目录");
      return;
    }
    try {
      await api.configurePrimaryLibraryDir(dir);
      setStatus(`已设主库目录：${dir}`);
    } catch (error) {
      setStatus(`保存主库失败：${String(error)}`);
    }
  }

  async function runIncrementalSync() {
    if (busy) return;
    const dirs = parsedResourcePoolDirs();
    if (dirs.length === 0) {
      setStatus("请填写至少一个索引来源目录");
      return;
    }
    const primary = primaryLibraryDir.trim();
    if (!primary) {
      setStatus("请先设置主库目录");
      return;
    }
    const report = await runBusy<RebuildReport>("正在读取索引来源并增量同步到主库，请稍候…", () => api.incrementalSync(dirs, primary));
    if (!report) return;
    setRebuildMode("rebuild");
    setRebuildReport(report);
    setStatus(`增量同步完成：${formatRebuildReport("rebuild", report)}`);
    await refreshWorks();
  }

  async function runScan() {
    const roots = parsedSourceRoots();
    try {
      setStatus("正在配置目录并启动扫描...");
      await api.configureSourceRoots(roots);
      await api.configureArchiveRoot(archiveRoot.trim());
      await api.configureMetadataProviderEnabled(metadataProviderEnabled);
      const nextJob = await api.startScan([]);
      const [nextItems, nextWorks] = await Promise.all([api.listIngestItems(nextJob.id), api.listWorks()]);
      setJob(nextJob);
      setItems(nextItems);
      setVisibleItems(applyIngestItemFilters(nextItems, activeFilters));
      setWorks(nextWorks);
      setPlan(null);
      setArchiveLogs([]);
      setMergeWorkId("");
      setFileVersions([]);
      clearBatchSelection();
      setSelectedItemId(nextItems[0]?.id ?? null);
      setStatus(`扫描完成并已入库：${nextJob.total_items} 个视频，${nextJob.review_count} 个待处理。`);
    } catch (error) {
      setStatus(`未连接到 Tauri 后端，保留演示数据：${String(error)}`);
    }
  }

  async function previewPlan() {
    const ids = archivePreviewIds(items, selectedIds);
    if (selectedIds.size > 0 && ids.length === 0) {
      setStatus("当前勾选条目里没有可自动归档项；待处理或失败条目不会进入迁移计划。");
      return;
    }
    try {
      const nextPlan = await api.previewArchivePlan(ids);
      setPlan(nextPlan);
      setArchiveLogs([]);
      const scope = selectedIds.size > 0 ? `已选 ${ids.length} 项` : "全部自动归档项";
      setStatus(`已按${scope}生成迁移计划：${nextPlan.actions.length} 个动作，${nextPlan.conflicts.length} 个冲突。`);
    } catch (error) {
      setPlan(demoPlan);
      setStatus(`后端计划不可用，显示演示计划：${String(error)}`);
    }
  }

  async function executePlan() {
    if (!canExecuteArchivePlan(plan) || !plan?.id) {
      setStatus("请先生成无冲突的迁移计划。");
      return;
    }
    try {
      const logs = await api.executeArchivePlan(plan.id);
      const storedLogs = await api.listArchiveActionLogs();
      setArchiveLogs(storedLogs);
      if (job) {
        await refreshPersistedState(job.id, selectedItemId);
      }
      setStatus(archiveExecutionSummary(logs, storedLogs.length).message);
    } catch (error) {
      setStatus(`迁移执行失败：${String(error)}`);
    }
  }

  async function quickResolve(item: IngestItem) {
    const code = normalizeManualCodeInput(manualCode);
    if (!item.id) {
      return;
    }
    if (!code) {
      setStatus("请先填写要确认的番号。");
      return;
    }
    try {
      await api.resolveMatch(item.id, code);
      if (job) {
        await refreshPersistedState(job.id, item.id);
      } else {
        setItems((current) =>
          replaceIngestItem(current, {
            ...item,
            normalized_code: code,
            confidence: 1,
            decision: "AutoArchive",
            review_reasons: []
          })
        );
      }
      setStatus(`${item.file_name} 已确认进入自动归档。`);
    } catch (error) {
      setStatus(`确认匹配失败：${String(error)}`);
    }
  }

  async function mergeIntoWork(item: IngestItem) {
    const workId = Number(mergeWorkId);
    if (!item.id || !job || !Number.isInteger(workId) || workId <= 0) {
      setStatus("请选择一个已有作品后再合并。");
      return;
    }
    try {
      await api.resolveMatch(item.id, item.normalized_code ?? "", workId);
      await refreshPersistedState(job.id, item.id);
      setStatus(`${item.file_name} 已合并到作品 #${workId}。`);
    } catch (error) {
      setStatus(`合并到已有作品失败：${String(error)}`);
    }
  }

  async function mergeSelectedFileVersions() {
    const targetWorkId = Number(mergeVersionTargetWorkId);
    const versionIds = fileVersions
      .filter((version) => version.id != null && selectedFileVersionIds.has(version.id))
      .map((version) => version.id as number);
    if (!selectedWork?.id || !Number.isInteger(targetWorkId) || targetWorkId <= 0 || versionIds.length === 0) {
      setStatus("请选择要合并的文件版本和目标作品。");
      return;
    }
    try {
      await api.mergeVersions(targetWorkId, versionIds);
      const [nextWorks, remainingVersions] = await Promise.all([
        api.listWorks(),
        api.listFileVersionsForWork(selectedWork.id)
      ]);
      setWorks(nextWorks);
      setFileVersions(remainingVersions);
      setSelectedFileVersionIds(new Set());
      setMergeVersionTargetWorkId("");
      setStatus(`已将 ${versionIds.length} 个文件版本合并到作品 #${targetWorkId}。`);
    } catch (error) {
      setStatus(`合并文件版本失败：${String(error)}`);
    }
  }

  async function retrySelectedMetadata(item: IngestItem) {
    if (!item.id || !job) {
      return;
    }
    try {
      const updated = await api.retryMetadata([item.id]);
      await refreshPersistedState(job.id, item.id);
      const stored = updated[0];
      setStatus(`${item.file_name} 元数据重试完成：${stored?.decision ?? "未更新"}。`);
    } catch (error) {
      setStatus(`重试元数据失败：${String(error)}`);
    }
  }

  async function retryBatchMetadata() {
    if (!job || selectedBatchIds.length === 0) {
      setStatus("请先选择要重试元数据的已入库条目。");
      return;
    }
    try {
      const updated = await api.retryMetadata(selectedBatchIds);
      await refreshPersistedState(job.id, selectedItemId);
      setStatus(`批量重试元数据完成：${updated.length} 个条目已刷新。`);
    } catch (error) {
      setStatus(`批量重试元数据失败：${String(error)}`);
    }
  }

  async function revalidateSelectedMoveFailure(item: IngestItem) {
    if (!item.id || !job) {
      return;
    }
    if (!item.review_reasons.includes("MoveFailed")) {
      setStatus("当前条目没有移动失败状态，不需要重新检查文件。");
      return;
    }
    try {
      const updated = await api.revalidateMoveFailedItems([item.id]);
      await refreshPersistedState(job.id, item.id);
      setStatus(
        updated.length > 0
          ? `${item.file_name} 文件已重新可用，已恢复为 ${updated[0].decision}。`
          : `${item.file_name} 文件仍不可用，请确认源文件路径存在。`
      );
    } catch (error) {
      setStatus(`重新检查文件失败：${String(error)}`);
    }
  }

  async function revalidateBatchMoveFailures() {
    if (!job || revalidatableBatchItems.length === 0) {
      setStatus("请先选择带有移动失败状态的条目。");
      return;
    }
    try {
      const ids = revalidatableBatchItems.map((item) => item.id as number);
      const updated = await api.revalidateMoveFailedItems(ids);
      await refreshPersistedState(job.id, selectedItemId);
      setStatus(`文件重新检查完成：${updated.length} 个条目已恢复，${ids.length - updated.length} 个仍不可用。`);
    } catch (error) {
      setStatus(`批量重新检查文件失败：${String(error)}`);
    }
  }

  async function ignoreSelectedDuplicate(item: IngestItem) {
    if (!item.id || !job) {
      return;
    }
    if (item.decision !== "DuplicateCandidate" || !item.review_reasons.includes("DuplicateFile")) {
      setStatus("当前条目不是重复候选，不需要忽略。");
      return;
    }
    try {
      const updated = await api.ignoreDuplicateItems([item.id]);
      await refreshPersistedState(job.id, item.id);
      setStatus(
        updated.length > 0
          ? `${item.file_name} 已标记为重复忽略，不会进入归档计划。`
          : `${item.file_name} 未被标记为重复忽略，请确认它仍是重复候选。`
      );
    } catch (error) {
      setStatus(`忽略重复项失败：${String(error)}`);
    }
  }

  async function ignoreBatchDuplicates() {
    if (!job || ignorableBatchItems.length === 0) {
      setStatus("请先选择重复候选条目。");
      return;
    }
    try {
      const ids = ignorableBatchItems.map((item) => item.id as number);
      const updated = await api.ignoreDuplicateItems(ids);
      await refreshPersistedState(job.id, selectedItemId);
      setStatus(`重复项忽略完成：${updated.length} 个条目已从待处理队列移出。`);
    } catch (error) {
      setStatus(`批量忽略重复项失败：${String(error)}`);
    }
  }

  async function deleteSelectedFile(item: IngestItem) {
    if (!item.id || !job) {
      return;
    }
    if (item.decision !== "DuplicateCandidate" && item.decision !== "Ignored") {
      setStatus("只有重复候选或已忽略的条目才能删除文件。");
      return;
    }
    if (!window.confirm(`确定删除源文件吗？此操作不可撤销：\n${item.file_name}\n\n文件将从磁盘永久移除，条目标记为已忽略。`)) {
      return;
    }
    try {
      const updated = await api.deleteItems([item.id]);
      await refreshPersistedState(job.id, item.id);
      setStatus(
        updated.length > 0
          ? `${item.file_name} 源文件已删除，条目标记为已忽略。`
          : `${item.file_name} 未删除，请确认它仍是重复候选或已忽略条目。`
      );
    } catch (error) {
      setStatus(`删除文件失败：${String(error)}`);
    }
  }

  async function deleteBatchFiles() {
    if (!job || ignorableBatchItems.length === 0) {
      setStatus("请先选择重复候选条目。");
      return;
    }
    if (
      !window.confirm(
        `确定删除 ${ignorableBatchItems.length} 个源文件吗？\n\n这些文件将从磁盘永久移除（不可撤销），条目标记为已忽略。`
      )
    ) {
      return;
    }
    try {
      const ids = ignorableBatchItems.map((item) => item.id as number);
      const updated = await api.deleteItems(ids);
      await refreshPersistedState(job.id, selectedItemId);
      setStatus(`文件删除完成：${updated.length} 个源文件已移除并标记为已忽略。`);
    } catch (error) {
      setStatus(`批量删除文件失败：${String(error)}`);
    }
  }

  async function confirmBatchMatches() {
    if (!job || resolvableBatchItems.length === 0) {
      setStatus("请先选择已有番号的待确认条目；缺少番号的条目需要单独编辑。");
      return;
    }
    try {
      for (const item of resolvableBatchItems) {
        await api.resolveMatch(item.id as number, item.normalized_code as string);
      }
      await refreshPersistedState(job.id, selectedItemId);
      setStatus(`批量确认匹配完成：${resolvableBatchItems.length} 个条目已进入自动归档。`);
    } catch (error) {
      setStatus(`批量确认匹配失败：${String(error)}`);
    }
  }

  async function saveWorkProfile() {
    if (!selectedWork?.id) {
      setStatus("请先确认匹配或合并到已有作品，再编辑作品资料。");
      return;
    }
    const rating = parseProfileRatingInput(profileRating);
    if (profileRating.trim() && rating == null) {
      setStatus("评分必须是 0 到 10 的整数，留空表示不评分。");
      return;
    }
    try {
      const updated = await api.updateWorkProfile(
        selectedWork.id,
        parseDelimitedListInput(profileTags),
        parseDelimitedListInput(profileLists),
        rating,
        profileStatus
      );
      setWorks((current) => current.map((work) => (work.id === updated.id ? updated : work)));
      setStatus(`${updated.normalized_code} 的标签、列表、评分和观看状态已保存。`);
    } catch (error) {
      setStatus(`保存作品资料失败：${String(error)}`);
    }
  }

  async function openSelectedFile(item: IngestItem) {
    try {
      await api.openFileInSystem(item.path);
      setStatus(`${item.file_name} 已交给系统默认播放器打开。`);
    } catch (error) {
      setStatus(`打开系统播放器失败：${String(error)}`);
    }
  }

  async function openSelectedDirectory(item: IngestItem) {
    try {
      await api.openPathInFileManager(item.path);
      setStatus(`${item.file_name} 所在目录已打开。`);
    } catch (error) {
      setStatus(`打开所在目录失败：${String(error)}`);
    }
  }

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <Database size={24} />
          <div>
            <strong>Media Manager</strong>
            <span>影片入库归档工作台</span>
          </div>
        </div>
        <nav className="nav-list">
          <button className={`nav-item ${activeView === "ingest" ? "active" : ""}`} type="button" onClick={() => switchView("ingest")}>
            <FolderInput size={18} /> 入库
          </button>
          <button className={`nav-item ${activeView === "inventory" ? "active" : ""}`} type="button" onClick={() => switchView("inventory")}>
            <Search size={18} /> 盘点
          </button>
          <button className={`nav-item ${activeView === "review" ? "active" : ""}`} type="button" onClick={() => switchView("review")}>
            <ListChecks size={18} /> 待处理
          </button>
          <button className={`nav-item ${activeView === "archive" ? "active" : ""}`} type="button" onClick={() => switchView("archive")}>
            <Archive size={18} /> 迁移计划
          </button>
          <button className={`nav-item ${activeView === "library" ? "active" : ""}`} type="button" onClick={() => switchView("library")}>
            <Tags size={18} /> 作品库
          </button>
          <button className={`nav-item ${activeView === "settings" ? "active" : ""}`} type="button" onClick={() => switchView("settings")}>
            <Settings size={18} /> 设置
          </button>
        </nav>
        {activeView !== "inventory" ? (
          <section className="source-panel">
            <label>来源目录</label>
            <textarea value={sourceRoots} onChange={(event) => setSourceRoots(event.target.value)} />
            <button className="primary" type="button" onClick={runScan} disabled={busy}>
              <RefreshCw size={16} /> 扫描
            </button>
          </section>
        ) : null}
      </aside>

      <section className="workspace">
        <header className="topbar">
          {!hasBackend && (
            <div className="backend-warning">未连接到 Tauri 桌面后端：扫描、归档等操作不可用。请在 Tauri 桌面应用中运行（npm run tauri:dev:win），而不是普通浏览器。</div>
          )}
          <div>
            <h1>{workbenchViewTitle(activeView)}</h1>
            <p>{status}</p>
          </div>
          {showArchiveControls ? (
            <>
              <button className="primary" type="button" onClick={previewPlan}>
                <Archive size={16} /> 预览迁移
              </button>
              <button className="primary" type="button" onClick={executePlan} disabled={!canExecuteArchivePlan(plan)}>
                <CheckCircle2 size={16} /> 执行迁移
              </button>
            </>
          ) : null}
        </header>

        {activeView !== "inventory" ? (
          <>
            <section className="capability-strip">
              {availableActions.map((action) => (
                <span key={action}>
                  <CheckCircle2 size={14} /> {action}
                </span>
              ))}
            </section>

            <section className="stats-grid">
              <Stat icon={<Search size={20} />} label="扫描文件" value={stats.total} />
              <Stat icon={<CheckCircle2 size={20} />} label="自动归档" value={stats.auto} />
              <Stat icon={<Clock3 size={20} />} label="待处理" value={stats.review} />
              <Stat icon={<TriangleAlert size={20} />} label="失败" value={stats.failed} />
              <Stat icon={<ListChecks size={20} />} label="已忽略" value={stats.ignored} />
            </section>
          </>
        ) : null}

        {activeView === "review" ? (
          <section className="review-summary-panel">
            <div className="panel-head">
              <div>
                <h2>待处理概览</h2>
                <span>{reviewSummary.total} 个条目需要人工处理</span>
              </div>
            </div>
            <div className="review-reason-grid">
              <button
                className={reviewReasonFilter === "All" ? "selected" : ""}
                type="button"
                onClick={() => focusReviewReason("All")}
              >
                <span>全部待处理</span>
                <strong>{reviewSummary.total}</strong>
              </button>
              {Object.entries(reviewReasonLabels).map(([value, label]) => {
                const reason = value as ReviewReason;
                return (
                  <button
                    className={reviewReasonFilter === reason ? "selected" : ""}
                    key={reason}
                    type="button"
                    onClick={() => focusReviewReason(reason)}
                  >
                    <span>{label}</span>
                    <strong>{reviewSummary.byReason[reason]}</strong>
                  </button>
                );
              })}
            </div>
          </section>
        ) : null}

        {activeView === "inventory" ? (
          <section className="inventory-page">
            <div className="inventory-panel inventory-panel-large">
              <div className="inventory-panel-head">
                <div>
                  <strong>一键盘点</strong>
                  <span>只读扫描，不移动文件</span>
                </div>
                <span>{inventoryReport ? inventoryReport.generated_at : "未生成"}</span>
              </div>
              <label className="inventory-roots-field">
                入口目录
                <textarea
                  rows={4}
                  value={inventoryRootsText}
                  onChange={(event) => setInventoryRootsText(event.target.value)}
                  disabled={inventoryBusy || inventoryExecuteBusy || postMigrationBusy || postMigrationExecuteBusy}
                  placeholder={"H:\\video\nH:\\AV"}
                />
              </label>
              <label className="inventory-roots-field">
                整理目标目录
                <input
                  value={inventoryArchiveRoot}
                  onChange={(event) => setInventoryArchiveRoot(event.target.value)}
                  disabled={inventoryBusy || inventoryExecuteBusy || postMigrationBusy || postMigrationExecuteBusy}
                  placeholder={"D:\\mm-7a-test\\archive"}
                />
              </label>
              <div className="daemon-actions">
                <button type="button" onClick={pickInventoryRoots} disabled={inventoryBusy || inventoryExecuteBusy || postMigrationBusy || postMigrationExecuteBusy || !hasBackend}>
                  <FolderOpen size={16} /> 选择目录
                </button>
                <button type="button" onClick={pickInventoryArchiveRoot} disabled={inventoryBusy || inventoryExecuteBusy || postMigrationBusy || postMigrationExecuteBusy || !hasBackend}>
                  <FolderInput size={16} /> 选择目标
                </button>
                <button className="primary" type="button" onClick={generateInventoryPreview} disabled={inventoryBusy || inventoryExecuteBusy || postMigrationBusy || postMigrationExecuteBusy || !hasBackend}>
                  <Search size={16} /> {inventoryBusy ? "盘点中" : "开始盘点"}
                </button>
                <button type="button" onClick={exportInventoryPreview} disabled={!inventoryReport || inventoryBusy || inventoryExportBusy || inventoryExecuteBusy || postMigrationBusy || postMigrationExecuteBusy || !hasBackend}>
                  <Archive size={16} /> {inventoryExportBusy ? "导出中" : "导出 JSON"}
                </button>
                <button type="button" onClick={executeInventoryPreview} disabled={!inventoryReport || inventoryExecutableCount === 0 || inventoryExecutionBlockedByTruncation || inventoryBusy || inventoryExportBusy || inventoryExecuteBusy || postMigrationBusy || postMigrationExecuteBusy || !hasBackend}>
                  <FolderInput size={16} /> {inventoryExecuteBusy ? "迁移中" : "集中迁移"}
                </button>
                <button type="button" onClick={generatePostMigrationReview} disabled={inventoryBusy || inventoryExecuteBusy || postMigrationBusy || postMigrationExecuteBusy || !hasBackend}>
                  <ListChecks size={16} /> {postMigrationBusy ? "复盘中" : "复盘补迁"}
                </button>
                <button type="button" onClick={executePostMigrationReview} disabled={!postMigrationReport || postMigrationReadyActionCount === 0 || postMigrationReport.truncated || inventoryBusy || inventoryExecuteBusy || postMigrationBusy || postMigrationExecuteBusy || !hasBackend}>
                  <FolderInput size={16} /> {postMigrationExecuteBusy ? "补迁中" : "执行补迁"}
                </button>
              </div>
              <span className="inventory-status-line">
                {inventoryBusy
                  ? "正在递归盘点资源..."
                  : inventoryExecuteBusy
                    ? "正在集中迁移安全执行计划..."
                    : postMigrationBusy
                      ? "正在生成迁移后复盘报告..."
                      : postMigrationExecuteBusy
                        ? "正在执行补迁/清理动作..."
                        : hasBackend
                          ? `主迁移可执行 ${inventoryExecutableCount} 部作品；复盘补迁可处理 ${postMigrationReadyActionCount} 个残留动作。`
                          : "桌面后端不可用，需在 Tauri 环境中盘点真实目录。"}
              </span>

              {postMigrationReport ? (
                <div className="inventory-execution-report">
                  <div className="inventory-section-head">
                    <strong>迁移后复盘</strong>
                    <span>{formatPostMigrationSummary(postMigrationReport)}</span>
                  </div>
                  <div className="inventory-summary">
                    <div>
                      <span>隔离残留</span>
                      <strong>{postMigrationReport.summary.quarantine_files}</strong>
                    </div>
                    <div>
                      <span>多视频</span>
                      <strong>{postMigrationReport.summary.multi_video_groups}</strong>
                    </div>
                    <div>
                      <span>素材补迁</span>
                      <strong>{postMigrationReport.summary.asset_only_groups}</strong>
                    </div>
                    <div>
                      <span>恢复隔离</span>
                      <strong>{postMigrationReport.summary.restore_candidates}</strong>
                    </div>
                    <div>
                      <span>可执行</span>
                      <strong>{postMigrationReport.summary.ready_actions}</strong>
                    </div>
                    <div>
                      <span>阻断</span>
                      <strong>{postMigrationReport.summary.blocked_actions}</strong>
                    </div>
                  </div>
                  {postMigrationExecutionReport ? (
                    <div className="inventory-report-root">
                      <span>补迁报告</span>
                      <strong>{postMigrationExecutionReport.report_path ?? formatPostMigrationExecutionSummary(postMigrationExecutionReport)}</strong>
                    </div>
                  ) : null}
                  <div className="inventory-execution-log">
                    {postMigrationReport.groups.slice(0, 5).map((group) => (
                      <div className="inventory-execution-log-row" key={`${group.kind}-${group.code}-${group.source_dir}`}>
                        <strong>{group.code}</strong>
                        <span>{postMigrationGroupLabels[group.kind]} · {group.actions.filter((action) => !action.conflict).length} 个可执行动作</span>
                        <small>{group.source_dir}</small>
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}

              {inventoryReport ? (
                <>
                  <div className="inventory-summary">
                    <div>
                      <span>作品</span>
                      <strong>{inventoryReport.summary.works}</strong>
                    </div>
                    <div>
                      <span>素材候选</span>
                      <strong>{inventoryReport.summary.asset_candidates}</strong>
                    </div>
                    <div>
                      <span>可自动</span>
                      <strong>{inventoryReport.summary.auto_ready}</strong>
                    </div>
                    <div>
                      <span>需确认</span>
                      <strong>{inventoryReport.summary.needs_review}</strong>
                    </div>
                    <div>
                      <span>阻断</span>
                      <strong>{inventoryReport.summary.blocked}</strong>
                    </div>
                    <div>
                      <span>可整理</span>
                      <strong>{inventoryReport.summary.ready}</strong>
                    </div>
                    <div>
                      <span>缺 NFO</span>
                      <strong>{inventoryReport.summary.missing_nfo}</strong>
                    </div>
                    <div>
                      <span>缺视频</span>
                      <strong>{inventoryReport.summary.missing_video}</strong>
                    </div>
                    <div>
                      <span>多版本</span>
                      <strong>{inventoryReport.summary.multi_video}</strong>
                    </div>
                    <div>
                      <span>冲突</span>
                      <strong>{inventoryReport.summary.code_conflict}</strong>
                    </div>
                    <div>
                      <span>孤儿</span>
                      <strong>{inventoryReport.summary.orphans}</strong>
                    </div>
                  </div>

                  <div className="inventory-report-root">
                    <span>整理目标</span>
                    <strong>{inventoryReport.archive_root ?? "未设置整理目标"}</strong>
                  </div>

                  {inventoryExecutionReport ? (
                    <div className="inventory-execution-report">
                      <div className="inventory-section-head">
                        <strong>最近集中迁移</strong>
                        <span>{formatInventoryExecutionSummary(inventoryExecutionReport)}</span>
                      </div>
                      {inventoryExecutionReport.report_path ? (
                        <div className="inventory-report-root">
                          <span>执行报告</span>
                          <strong>{inventoryExecutionReport.report_path}</strong>
                        </div>
                      ) : null}
                      <div className="inventory-execution-log">
                        {inventoryExecutionReport.logs.slice(0, 5).map((log, index) => (
                          <div className={`inventory-execution-log-row ${log.status}`} key={`${log.to_path}-${index}`}>
                            <strong>{log.code || "回滚清理"}</strong>
                            <span>
                              {log.status === "linked" ? "已硬链接" : log.status === "copied" ? "已复制" : log.status === "moved" ? "已迁移" : log.status === "rolled_back" ? "已回滚" : "失败"} · {log.kind} · {formatBytes(log.bytes)}
                            </span>
                            <small>{log.to_path}</small>
                            {log.message ? <small>{log.message}</small> : null}
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}

                  <div className="inventory-filter">
                    {inventoryFilters.map((filter) => (
                      <button
                        type="button"
                        key={filter.value}
                        className={inventoryStatusFilter === filter.value ? "active" : ""}
                        onClick={() => {
                          setInventoryStatusFilter(filter.value);
                          setSelectedInventoryCode(null);
                        }}
                      >
                        {filter.label}
                      </button>
                    ))}
                  </div>

                  {inventoryReport.warnings.length > 0 ? (
                    <div className="inventory-warnings">
                      {inventoryReport.warnings.slice(0, 5).map((warning, index) => (
                        <span key={`${warning}-${index}`}>{warning}</span>
                      ))}
                      {inventoryReport.warnings.length > 5 ? (
                        <span>另有 {inventoryReport.warnings.length - 5} 条 warning</span>
                      ) : null}
                    </div>
                  ) : null}

                  {inventoryExecutionBlockedByTruncation ? (
                    <div className="inventory-warnings">
                      <span>报告明细已截断，不能集中迁移全部作品；请缩小入口目录后重新盘点。</span>
                    </div>
                  ) : null}

                  <div className="inventory-layout">
                    <div className="inventory-work-list">
                      <div className="inventory-section-head">
                        <strong>
                          {inventoryStatusFilter === "review:asset_candidate"
                            ? "素材候选"
                            : inventoryStatusFilter === "orphan"
                              ? "资源列表"
                              : "作品列表"}
                        </strong>
                        <span>{inventoryListItemCount} 项</span>
                      </div>
                      {inventoryListItemCount === 0 ? (
                        <span className="empty-text">当前筛选没有资源</span>
                      ) : (
                        <>
                          {filteredInventoryWorks.map((work) => (
                            <button
                              type="button"
                              className={`inventory-work-row ${selectedInventoryWork?.code === work.code ? "active" : ""}`}
                              key={work.code}
                              onClick={() => setSelectedInventoryCode(work.code)}
                            >
                              <strong>{work.code}</strong>
                              <span>{formatInventoryResolutionSummary(work)}</span>
                              <small>{work.statuses.map(formatInventoryStatus).join(" · ")}</small>
                              <small>{summarizeInventoryResources(work.resources)}</small>
                              <small>{work.target_dir ?? inventoryReport.archive_root ?? "未设置整理目标"}</small>
                            </button>
                          ))}
                          {visibleInventoryOrphans.length > 0 ? (
                            <div className="inventory-orphan-list">
                              <div className="inventory-section-head">
                                <strong>孤儿资源</strong>
                                <span>{visibleInventoryOrphans.length} 项</span>
                              </div>
                              {visibleInventoryOrphans.map((resource) => (
                                <div className="inventory-orphan-row" key={resource.path}>
                                  <strong>{resource.kind} · {resource.file_name}</strong>
                                  <span>{formatBytes(resource.size_bytes)} · 未识别番号</span>
                                  <small>{resource.path}</small>
                                  {resource.warnings.length > 0 ? <small>{resource.warnings.join("；")}</small> : null}
                                </div>
                              ))}
                            </div>
                          ) : null}
                        </>
                      )}
                    </div>

                    <div className="inventory-detail">
                      {selectedInventoryWork ? (
                        <>
                          <div className="inventory-section-head">
                            <strong>{selectedInventoryWork.code}</strong>
                            <span>{formatInventoryReviewBucket(selectedInventoryWork.resolution.bucket)}</span>
                          </div>
                          <div className="inventory-target">
                            <span>目标目录</span>
                            <strong>{selectedInventoryWork.target_dir ?? inventoryReport.archive_root ?? "未设置整理目标"}</strong>
                          </div>

                          <div className="inventory-resolution">
                            <div>
                              <span>配对建议</span>
                              <strong>{formatInventoryResolutionSummary(selectedInventoryWork)}</strong>
                            </div>
                            {selectedInventoryWork.resolution.primary_video ? (
                              <small>主视频：{selectedInventoryWork.resolution.primary_video}</small>
                            ) : null}
                            {selectedInventoryWork.resolution.primary_nfo ? (
                              <small>主 NFO：{selectedInventoryWork.resolution.primary_nfo}</small>
                            ) : null}
                            {selectedInventoryWork.resolution.reasons.length > 0 ? (
                              <div className="inventory-resolution-notes">
                                {selectedInventoryWork.resolution.reasons.map((reason, index) => (
                                  <span key={`${selectedInventoryWork.code}-reason-${index}`}>{reason}</span>
                                ))}
                              </div>
                            ) : null}
                            {selectedInventoryWork.resolution.warnings.length > 0 ? (
                              <div className="inventory-resolution-notes warn">
                                {selectedInventoryWork.resolution.warnings.map((warning, index) => (
                                  <span key={`${selectedInventoryWork.code}-warning-${index}`}>{warning}</span>
                                ))}
                              </div>
                            ) : null}
                            {selectedInventoryWork.resolution.blockers.length > 0 ? (
                              <div className="inventory-resolution-notes block">
                                {selectedInventoryWork.resolution.blockers.map((blocker, index) => (
                                  <span key={`${selectedInventoryWork.code}-blocker-${index}`}>{blocker}</span>
                                ))}
                              </div>
                            ) : null}
                          </div>

                          <div className={`inventory-execution-plan ${selectedInventoryWork.resolution.execution_plan.ready ? "ready" : "review"}`}>
                            <div className="inventory-section-head">
                              <strong>安全执行计划</strong>
                              <span>{formatInventoryExecutionPlanSummary(selectedInventoryWork.resolution.execution_plan)}</span>
                            </div>
                            {selectedInventoryWork.resolution.execution_plan.conflicts.length > 0 ? (
                              <div className="inventory-resolution-notes block">
                                {selectedInventoryWork.resolution.execution_plan.conflicts.map((conflict, index) => (
                                  <span key={`${selectedInventoryWork.code}-execution-conflict-${index}`}>{conflict}</span>
                                ))}
                              </div>
                            ) : null}
                            {selectedInventoryWork.resolution.execution_plan.notes.length > 0 ? (
                              <div className="inventory-resolution-notes">
                                {selectedInventoryWork.resolution.execution_plan.notes.map((note, index) => (
                                  <span key={`${selectedInventoryWork.code}-execution-note-${index}`}>{note}</span>
                                ))}
                              </div>
                            ) : null}
                          </div>

                          <div className="inventory-subsection">
                            <strong>资源</strong>
                            {selectedInventoryWork.resources.length === 0 ? (
                              <span className="empty-text">暂无资源</span>
                            ) : (
                              selectedInventoryWork.resources.map((resource) => {
                                const role = selectedInventoryRoleByPath.get(resource.path);
                                return (
                                  <div className="inventory-resource-row" key={resource.path}>
                                    <strong>{resource.kind} · {resource.file_name}</strong>
                                    {role ? (
                                      <span>
                                        {formatInventoryResourceRole(role.role)} · {role.reason}
                                        {role.selected ? " · 已选中" : ""}
                                        {role.needs_review ? " · 需确认" : ""}
                                      </span>
                                    ) : null}
                                    <span>{formatBytes(resource.size_bytes)} · {resource.code ?? "未识别番号"}</span>
                                    <small>{resource.path}</small>
                                    {resource.warnings.length > 0 ? (
                                      <div className="inventory-resource-warnings">
                                        {resource.warnings.map((warning, index) => (
                                          <span key={`${resource.path}-warning-${index}`}>{warning}</span>
                                        ))}
                                      </div>
                                    ) : null}
                                  </div>
                                );
                              })
                            )}
                          </div>

                          <div className="inventory-subsection">
                            <strong>候选动作预览</strong>
                            {selectedInventoryWork.actions.length === 0 ? (
                              <span className="empty-text">暂无候选动作预览</span>
                            ) : (
                              selectedInventoryWork.actions.map((action, index) => (
                                <div
                                  className={`inventory-action-row ${action.conflict ? "warn" : ""}`}
                                  key={`${action.from_path}-${index}`}
                                >
                                  <strong>{action.kind}</strong>
                                  <span>{action.from_path}</span>
                                  <small>{formatInventoryActionTarget(action)}</small>
                                </div>
                              ))
                            )}
                          </div>
                        </>
                      ) : (
                        <span className="empty-text">
                          {visibleInventoryOrphans.length > 0 ? "孤儿资源没有作品详情，左侧可查看路径和告警" : "请选择一个作品查看整理预览"}
                        </span>
                      )}
                    </div>
                  </div>
                </>
              ) : (
                <span className="empty-text">尚未生成盘点结果</span>
              )}
            </div>
          </section>
        ) : null}

        {activeView === "settings" ? (
          <section className="settings-panel">
            <div className="settings-tabs">
              <button type="button" className={settingsTab === "pool" ? "active" : ""} onClick={() => setSettingsTab("pool")}>目录配置</button>
              <button type="button" className={settingsTab === "rebuild" ? "active" : ""} onClick={() => setSettingsTab("rebuild")}>作品库</button>
              <button type="button" className={settingsTab === "daemon" ? "active" : ""} onClick={() => setSettingsTab("daemon")}>自动管线</button>
              <button type="button" className={settingsTab === "cache" ? "active" : ""} onClick={() => setSettingsTab("cache")}>缓存</button>
            </div>

            {settingsTab === "pool" ? (
              <>
                <div className="settings-form">
                  <label>来源目录（入库扫描用）</label>
                  <textarea value={sourceRoots} onChange={(event) => setSourceRoots(event.target.value)} />
                  <label>归档根目录</label>
                  <input value={archiveRoot} onChange={(event) => setArchiveRoot(event.target.value)} />
                  <label className="settings-check">
                    <input type="checkbox" checked={metadataProviderEnabled} onChange={(event) => setMetadataProviderEnabled(event.target.checked)} />
                    <span>启用示例元数据源</span>
                  </label>
                  <button className="primary" type="button" onClick={saveConfiguration}>
                    <CheckCircle2 size={16} /> 保存设置
                  </button>
                </div>
                <div className="rebuild-tools">
                  <div>
                    <strong>索引来源目录</strong>
                    <span>每行一个目录。供作品库重建、增量同步和自动管线读取；存量集中迁移请从一键盘点进入。</span>
                  </div>
                  <button type="button" className="primary" onClick={saveResourcePool}>
                    <CheckCircle2 size={16} /> 保存目录
                  </button>
                </div>
                <div className="migration-form">
                  <textarea value={resourcePoolDirs} onChange={(e) => setResourcePoolDirs(e.target.value)} placeholder="例如&#10;H:\CineMingle-1.3.0\JAV_output&#10;H:\bucket&#10;G:\软件\98tang\Img\Poster&#10;G:\软件\98tang\Img\ScreenShot" />
                </div>
              </>
            ) : null}

            {settingsTab === "rebuild" ? (
              <>
                <div className="rebuild-tools">
                  <div>
                    <strong>作品库重建（全量）</strong>
                    <span>清空作品库，从索引来源目录全量重新解析。仅首次建库或元数据大改时用。</span>
                  </div>
                  <button type="button" className="primary" onClick={runRebuildFromPool} disabled={busy}>
                    <Database size={16} /> 执行重建
                  </button>
                </div>
                <div className="rebuild-tools">
                  <div>
                    <strong>增量同步（日常推荐）</strong>
                    <span>主库目录是唯一库；读取索引来源把缺的视频/图片复制进主库对应作品，新番号自动新增，已有作品保留。无需重设目录。</span>
                  </div>
                  <button type="button" onClick={savePrimaryLibrary}>
                    <CheckCircle2 size={16} /> 保存主库
                  </button>
                  <button type="button" className="primary" onClick={runIncrementalSync} disabled={busy}>
                    <RefreshCw size={16} /> 增量同步
                  </button>
                </div>
                <div className="migration-form">
                  <label>主库目录</label>
                  <input value={primaryLibraryDir} onChange={(e) => setPrimaryLibraryDir(e.target.value)} placeholder="例如 H:\consolidated（迁移后的自包含作品库）" />
                </div>
                {rebuildReport ? (
                  <div className="rebuild-report">
                    <span>
                      {rebuildMode === "preview" ? "预览" : "重建"}：{rebuildReport.nfos_scanned} 个 NFO
                      · {rebuildReport.works_created} 个作品 · {rebuildReport.works_merged} 个合并组
                      · 标签 {rebuildReport.tags_extracted} · 系列 {rebuildReport.sets_extracted}
                      · 演员 {rebuildReport.actors_extracted} · 文件版本 {rebuildReport.file_versions_created}
                      {rebuildReport.errors.length > 0 ? ` · ${rebuildReport.errors.length} 个 NFO 解析失败` : ""}
                    </span>
                    {rebuildReport.errors.length > 0 ? (
                      <ul>
                        {rebuildReport.errors.slice(0, 5).map((error, index) => (
                          <li key={index}>{error.nfo_path}：{error.message}</li>
                        ))}
                      </ul>
                    ) : null}
                  </div>
                ) : null}
              </>
            ) : null}

            {settingsTab === "daemon" ? (
              <div className="daemon-panel">
                <div className="rebuild-tools">
                  <div>
                    <strong>自动管线状态</strong>
                    <span>
                      {daemonStatus
                        ? `${formatDaemonState(daemonStatus.state)} · ${daemonStatus.configured ? "配置完整" : "缺少归档根目录"} · 元数据源 ${daemonStatus.metadata_source === "example" ? "示例" : "未启用"} · 控制通道 ${formatDaemonChannel(daemonChannel)}`
                        : "尚未读取"}
                    </span>
                  </div>
                  <button type="button" onClick={refreshDaemonPanel} disabled={daemonBusy !== null}>
                    <RefreshCw size={16} /> {daemonBusy === "refresh" ? "刷新中" : "刷新状态"}
                  </button>
                  <button type="button" className="primary" onClick={runDaemonOnce} disabled={daemonBusy !== null || !daemonStatus?.configured || daemonStatus?.state === "Paused"}>
                    <Play size={16} /> {daemonBusy === "run" ? "运行中" : "运行一轮"}
                  </button>
                  <button type="button" onClick={runPipelineSelfCheck} disabled={selfCheckBusy || !hasBackend}>
                    <ListChecks size={16} /> {selfCheckBusy ? "自检中" : "一键自检"}
                  </button>
                </div>

                <div className="daemon-actions">
                  <button type="button" onClick={pauseDaemon} disabled={daemonBusy !== null || daemonStatus?.state === "Paused"}>
                    <TriangleAlert size={16} /> {daemonBusy === "pause" ? "暂停中" : "暂停"}
                  </button>
                  <button type="button" onClick={resumeDaemon} disabled={daemonBusy !== null || daemonStatus?.state !== "Paused"}>
                    <RefreshCw size={16} /> {daemonBusy === "resume" ? "恢复中" : "恢复"}
                  </button>
                </div>

                <div className="aria2-settings">
                  <div className="aria2-settings-head">
                    <label>
                      <input
                        type="checkbox"
                        checked={aria2Settings.enabled}
                        onChange={(event) => setAria2Settings({ ...aria2Settings, enabled: event.target.checked })}
                      />
                      aria2 轮询
                    </label>
                    <button type="button" onClick={saveAria2Settings} disabled={aria2Busy || !hasBackend}>
                      <Settings size={16} /> {aria2Busy ? "保存中" : "保存 aria2"}
                    </button>
                  </div>
                  <div className="aria2-settings-grid">
                    <label>
                      主机
                      <input
                        value={aria2Settings.host}
                        onChange={(event) => setAria2Settings({ ...aria2Settings, host: event.target.value })}
                      />
                    </label>
                    <label>
                      端口
                      <input
                        type="number"
                        min={1}
                        max={65535}
                        value={aria2Settings.port}
                        onChange={(event) => setAria2Settings({ ...aria2Settings, port: Number(event.target.value) })}
                      />
                    </label>
                    <label>
                      RPC 路径
                      <input
                        value={aria2Settings.path}
                        onChange={(event) => setAria2Settings({ ...aria2Settings, path: event.target.value })}
                      />
                    </label>
                    <label>
                      Secret
                      <input
                        value={aria2Settings.secret ?? ""}
                        onChange={(event) => setAria2Settings({ ...aria2Settings, secret: event.target.value })}
                      />
                    </label>
                    <label>
                      超时 ms
                      <input
                        type="number"
                        min={1}
                        value={aria2Settings.timeout_ms}
                        onChange={(event) => setAria2Settings({ ...aria2Settings, timeout_ms: Number(event.target.value) })}
                      />
                    </label>
                    <label>
                      轮询间隔 s
                      <input
                        type="number"
                        min={1}
                        value={aria2Settings.poll_interval_secs}
                        onChange={(event) => setAria2Settings({ ...aria2Settings, poll_interval_secs: Number(event.target.value) })}
                      />
                    </label>
                  </div>
                  <label className="aria2-gids-field">
                    跟踪 GID
                    <textarea
                      rows={4}
                      value={aria2GidsText}
                      onChange={(event) => setAria2GidsText(event.target.value)}
                    />
                  </label>
                </div>

                <div className="remote-scraper-settings">
                  <div className="remote-scraper-settings-head">
                    <label>
                      <input
                        type="checkbox"
                        checked={remoteScraperSettings.enabled}
                        onChange={(event) => setRemoteScraperSettings({ ...remoteScraperSettings, enabled: event.target.checked })}
                      />
                      远程刮削器
                    </label>
                    <button type="button" onClick={saveRemoteScraperSettings} disabled={remoteScraperBusy || !hasBackend}>
                      <Settings size={16} /> {remoteScraperBusy ? "保存中" : "保存刮削器"}
                    </button>
                  </div>
                  <div className="remote-scraper-settings-grid">
                    <label>
                      User-Agent
                      <input
                        value={remoteScraperSettings.user_agent}
                        onChange={(event) => setRemoteScraperSettings({ ...remoteScraperSettings, user_agent: event.target.value })}
                      />
                    </label>
                    <label>
                      超时 ms
                      <input
                        type="number"
                        min={1}
                        value={remoteScraperSettings.timeout_ms}
                        onChange={(event) => setRemoteScraperSettings({ ...remoteScraperSettings, timeout_ms: Number(event.target.value) })}
                      />
                    </label>
                    <label>
                      代理 URL
                      <input
                        value={remoteScraperSettings.proxy_url ?? ""}
                        onChange={(event) => setRemoteScraperSettings({ ...remoteScraperSettings, proxy_url: event.target.value })}
                      />
                    </label>
                  </div>
                  <label className="remote-scraper-fallback">
                    <input
                      type="checkbox"
                      checked={remoteScraperSettings.include_example_fallback}
                      onChange={(event) => setRemoteScraperSettings({ ...remoteScraperSettings, include_example_fallback: event.target.checked })}
                    />
                    保留示例 fallback
                  </label>
                  <div className="remote-scraper-source-list">
                    {remoteScraperSettings.sources.map((source, index) => (
                      <div className="remote-scraper-source-row" key={source.id}>
                        <label>
                          <input
                            type="checkbox"
                            checked={source.enabled}
                            onChange={(event) => {
                              const sources = remoteScraperSettings.sources.map((candidate, sourceIndex) =>
                                sourceIndex === index ? { ...candidate, enabled: event.target.checked } : candidate
                              );
                              setRemoteScraperSettings({ ...remoteScraperSettings, sources });
                            }}
                          />
                          {source.id}
                        </label>
                        <input
                          value={source.search_url_template}
                          onChange={(event) => {
                            const sources = remoteScraperSettings.sources.map((candidate, sourceIndex) =>
                              sourceIndex === index ? { ...candidate, search_url_template: event.target.value } : candidate
                            );
                            setRemoteScraperSettings({ ...remoteScraperSettings, sources });
                          }}
                        />
                        <input
                          type="number"
                          min={0}
                          max={1}
                          step={0.01}
                          value={source.min_confidence}
                          onChange={(event) => {
                            const sources = remoteScraperSettings.sources.map((candidate, sourceIndex) =>
                              sourceIndex === index ? { ...candidate, min_confidence: Number(event.target.value) } : candidate
                            );
                            setRemoteScraperSettings({ ...remoteScraperSettings, sources });
                          }}
                        />
                      </div>
                    ))}
                  </div>
                </div>

                {daemonStatus ? (
                  <div className="daemon-grid">
                    <div className="metric-card">
                      <span>来源目录</span>
                      <strong>{daemonStatus.source_roots.length}</strong>
                    </div>
                    <div className="metric-card">
                      <span>索引来源目录</span>
                      <strong>{daemonStatus.asset_roots.length}</strong>
                    </div>
                    <div className="metric-card">
                      <span>异常</span>
                      <strong>{daemonStatus.open_exceptions}</strong>
                    </div>
                    <div className="metric-card">
                      <span>搁置</span>
                      <strong>{daemonStatus.holding_items}</strong>
                    </div>
                  </div>
                ) : null}

                {daemonStatus?.archive_root ? (
                  <div className="rebuild-report">
                    <span>归档根目录：{daemonStatus.archive_root}</span>
                    {daemonStatus.last_error ? <span>最近错误：{daemonStatus.last_error}</span> : null}
                  </div>
                ) : null}

                {daemonReport ? (
                  <div className="rebuild-report">
                    <span>{summarizeRunOnceReport(daemonReport)}</span>
                  </div>
                ) : null}

                <div className="daemon-list">
                  <div className="daemon-list-head">
                    <strong>搁置区</strong>
                    <span>{holdingEntries.length} 项</span>
                  </div>
                  {holdingEntries.length === 0 ? (
                    <span className="empty-text">暂无搁置文件</span>
                  ) : (
                    holdingEntries.slice(0, 10).map((entry) => (
                      <div className="daemon-list-row" key={entry.id ?? entry.path}>
                        <strong>{entry.file_name}</strong>
                        <span>{formatHoldingReason(entry.reason)} · {formatBytes(entry.size_bytes)}</span>
                        <small>{entry.path}</small>
                      </div>
                    ))
                  )}
                </div>

                <div className="daemon-list">
                  <div className="daemon-list-head">
                    <strong>异常队列</strong>
                    <span>{exceptionEntries.length} 项</span>
                  </div>
                  {exceptionEntries.length === 0 ? (
                    <span className="empty-text">暂无异常</span>
                  ) : (
                    exceptionEntries.slice(0, 10).map((entry) => (
                      <div className="daemon-list-row" key={entry.id ?? entry.object_path}>
                        <strong>{formatExceptionKind(entry.kind)} · {formatExceptionStatus(entry.status)}</strong>
                        <span>{entry.object_path}</span>
                        <small>{shortEvidence(entry.evidence_json)}</small>
                        {entry.status === "Open" ? (
                          <div className="daemon-row-actions">
                            <button type="button" onClick={() => resolveDaemonException(entry.id, "Resolved")} disabled={daemonBusy !== null}>
                              <CheckCircle2 size={14} /> 已解决
                            </button>
                            <button type="button" onClick={() => resolveDaemonException(entry.id, "Ignored")} disabled={daemonBusy !== null}>
                              <X size={14} /> 忽略
                            </button>
                          </div>
                        ) : null}
                      </div>
                    ))
                  )}
                </div>

                <div className="daemon-list">
                  <div className="daemon-list-head">
                    <strong>最近运行</strong>
                    <span>{pipelineRuns.length} 条</span>
                  </div>
                  {pipelineRuns.length === 0 ? (
                    <span className="empty-text">暂无运行记录</span>
                  ) : (
                    pipelineRuns.slice(0, 10).map((run) => (
                      <div className="daemon-list-row" key={run.id ?? run.file_path}>
                        <strong>{formatPipelineStatus(run.status)}</strong>
                        <span>{run.file_path}</span>
                        <small>{run.error || run.finished_at || run.started_at || "无错误"}</small>
                      </div>
                    ))
                  )}
                </div>

                {selfCheckReport ? (
                  <div className="self-check-panel">
                    <div className="daemon-list-head">
                      <strong>{formatSelfCheckSummary(selfCheckReport)}</strong>
                      <span>{selfCheckReport.generated_at}</span>
                    </div>
                    <div className="self-check-list">
                      {selfCheckReport.checks.map((item) => (
                        <div className={`self-check-row ${item.severity}`} key={item.id}>
                          <strong>{formatSelfCheckSeverity(item.severity)} · {item.title}</strong>
                          <span>{item.message}</span>
                          {item.action ? <small>{item.action}</small> : null}
                        </div>
                      ))}
                    </div>
                    {selfCheckReport.sandbox ? (
                      <div className="rebuild-report">
                        <span>沙盒目录：{selfCheckReport.sandbox.root}</span>
                        {selfCheckReport.sandbox.archived_path ? <span>归档样本：{selfCheckReport.sandbox.archived_path}</span> : null}
                      </div>
                    ) : null}
                  </div>
                ) : null}

                <div className="diagnostics-panel">
                  <div className="daemon-list-head">
                    <strong>诊断日志</strong>
                    <span>{diagnosticLogs.length} 条</span>
                  </div>
                  <div className="daemon-actions">
                    <button type="button" onClick={refreshDiagnosticLogs} disabled={diagnosticsBusy !== null || !hasBackend}>
                      <RefreshCw size={16} /> {diagnosticsBusy === "refresh" ? "刷新中" : "刷新日志"}
                    </button>
                    <button type="button" onClick={exportDiagnosticsSnapshot} disabled={diagnosticsBusy !== null || !hasBackend}>
                      <Settings size={16} /> {diagnosticsBusy === "export" ? "导出中" : "导出诊断"}
                    </button>
                  </div>
                  <div className="diagnostic-log-list">
                    {diagnosticLogs.length === 0 ? (
                      <span className="empty-text">暂无诊断日志</span>
                    ) : (
                      diagnosticLogs.slice(-20).map((entry, index) => (
                        <div className={`diagnostic-log-row ${entry.level.toLowerCase()}`} key={`${entry.timestamp}-${entry.target}-${index}`}>
                          {formatDiagnosticLogLine(entry)}
                        </div>
                      ))
                    )}
                  </div>
                </div>
              </div>
            ) : null}

            {settingsTab === "cache" ? (
              <div className="cache-tools">
                <div>
                  <strong>缩略图缓存</strong>
                  <span>
                    {thumbnailCache
                      ? `${thumbnailCache.file_count} 个文件 · ${formatBytes(thumbnailCache.total_bytes)}`
                      : "未读取"}
                  </span>
                </div>
                <button type="button" onClick={refreshThumbnailCache}>
                  <RefreshCw size={16} /> 刷新
                </button>
                <button type="button" onClick={clearThumbnailCache}>
                  <TriangleAlert size={16} /> 清理缓存
                </button>
              </div>
            ) : null}
          </section>
        ) : null}

        {activeView === "library" ? (
          <section className="library-grid">
            <div className="library-panel">
              <div className="panel-head">
                <div>
                  <h2>作品浏览</h2>
                  <span>{libraryWorks.length} / {works.length} 个作品</span>
                </div>
                <button type="button" onClick={refreshWorks}>
                  <RefreshCw size={16} /> 刷新
                </button>
              </div>
              <div className="library-filters">
                <input value={libraryQuery} placeholder="搜索番号、标题、标签、列表" onChange={(event) => setLibraryQuery(event.target.value)} />
                <select value={libraryStatusFilter} onChange={(event) => setLibraryStatusFilter(event.target.value as WorkStatusFilter)}>
                  <option value="All">全部状态</option>
                  <option value="Unwatched">未观看</option>
                  <option value="Watched">已观看</option>
                  <option value="Favorite">收藏</option>
                </select>
              </div>
              <div className="tag-summary">
                {libraryTags.length === 0 ? <span>暂无标签</span> : libraryTags.slice(0, 12).map((tag) => (
                  <button key={tag.label} type="button" onClick={() => setLibraryQuery(tag.label)}>{tag.label} <strong>{tag.count}</strong></button>
                ))}
              </div>
              <div className="library-section-head"><h3>标准作品</h3><span>{libraryPartition.standard.length} 个</span></div>
              {libraryPartition.standard.length === 0 ? (
                <div className="empty-inline">没有匹配当前筛选的标准作品。</div>
              ) : (
                <div className="work-card-grid">
                  {libraryPartition.standard.map((work) => (
                    <WorkCard key={work.id ?? work.normalized_code ?? work.source_code} work={work} selected={selectedLibraryWork?.id === work.id} onSelect={() => setSelectedLibraryWorkId(work.id ?? null)} />
                  ))}
                </div>
              )}
              {libraryPartition.nonStandard.length > 0 ? (
                <div className="library-section-head collapsible">
                  <button type="button" onClick={() => setNonStandardCollapsed((value) => !value)}>
                    {nonStandardCollapsed ? <ChevronRight size={18} /> : <ChevronDown size={18} />}
                    <h3>非标准作品</h3><span>{libraryPartition.nonStandard.length} 个</span>
                  </button>
                </div>
              ) : null}
              {!nonStandardCollapsed && libraryPartition.nonStandard.length > 0 ? (
                <div className="work-card-grid">
                  {libraryPartition.nonStandard.map((work) => (
                    <WorkCard key={work.id ?? work.source_code ?? work.normalized_code} work={work} selected={selectedLibraryWork?.id === work.id} onSelect={() => setSelectedLibraryWorkId(work.id ?? null)} />
                  ))}
                </div>
              ) : null}
            </div>
            <aside className="library-detail">
              {selectedLibraryWork ? (
                <WorkDetailPanel work={selectedLibraryWork} detail={libraryWorkDetail} artworkSrc={selectedLibraryArtworkSrc} actors={libraryWorkDetail?.actors ?? libraryWorkActors} fileVersions={libraryFileVersions} onOpenInIngest={openLibraryWorkInIngestDetail} onImageClick={setLightboxSrc} onSearch={(query) => { setActiveView("library"); setLibraryQuery(query); setLibraryStatusFilter("All"); }} />
              ) : (
                <div className="empty">暂无作品</div>
              )}
            </aside>
            {lightboxSrc ? <Lightbox src={lightboxSrc} onClose={() => setLightboxSrc(null)} /> : null}
          </section>
        ) : null}

        {activeView === "ingest" || activeView === "review" ? (
        <section className="content-grid">
          <div className="queue-panel">
            <div className="panel-head">
              <div>
                <h2>扫描结果</h2>
                <span>{job ? `任务 #${job.id} · ${job.status}` : "未扫描"}</span>
              </div>
              <div className="segmented">
                {(["All", "AutoArchive", "NeedsReview", "DuplicateCandidate", "Failed", "Ignored"] as const).map((value) => (
                  <button key={value} className={filter === value ? "selected" : ""} type="button" onClick={() => setFilter(value)}>
                    {value === "All" ? "全部" : decisionLabels[value]}
                  </button>
                ))}
              </div>
            </div>
            <div className="filter-toolbar">
              <select value={reviewReasonFilter} onChange={(event) => setReviewReasonFilter(event.target.value as ReviewReasonFilter)}>
                <option value="All">全部问题</option>
                {Object.entries(reviewReasonLabels).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
              <select value={codePresenceFilter} onChange={(event) => setCodePresenceFilter(event.target.value as CodePresenceFilter)}>
                <option value="All">全部番号状态</option>
                <option value="HasCode">已有番号</option>
                <option value="MissingCode">缺少番号</option>
              </select>
            </div>
            <div className="batch-toolbar">
              <span>已选择 {selectedBatchIds.length} 项，可批量确认 {resolvableBatchItems.length} 项</span>
              <div>
                <button type="button" onClick={retryBatchMetadata} disabled={selectedBatchIds.length === 0}>
                  <RefreshCw size={15} /> 批量重试元数据
                </button>
                <button type="button" onClick={revalidateBatchMoveFailures} disabled={revalidatableBatchItems.length === 0}>
                  <RefreshCw size={15} /> 重新检查文件
                </button>
                <button type="button" onClick={ignoreBatchDuplicates} disabled={ignorableBatchItems.length === 0}>
                  <ListChecks size={15} /> 忽略重复项
                </button>
                <button type="button" onClick={deleteBatchFiles} disabled={ignorableBatchItems.length === 0}>
                  <Trash2 size={15} /> 删除源文件
                </button>
                <button type="button" onClick={confirmBatchMatches} disabled={resolvableBatchItems.length === 0}>
                  <CheckCircle2 size={15} /> 批量确认匹配
                </button>
                <button type="button" onClick={toggleAllVisibleItems} disabled={tableItems.every((item) => item.id == null)}>
                  {tableItems.filter((item) => item.id != null).every((item) => selectedIds.has(item.id as number)) ? "取消全选" : "全选"}
                </button>
                <button type="button" onClick={clearBatchSelection} disabled={selectedBatchIds.length === 0}>
                  清空选择
                </button>
              </div>
            </div>
            <div className="table">
              {tableItems.length === 0 ? (
                <div className="empty-inline">没有匹配当前筛选的条目。确认来源目录存在并包含 mp4/mkv/avi/mov 等视频文件后再扫描。</div>
              ) : (
                tableItems.map((item) => (
                  <div
                    key={item.id ?? item.path}
                    className={`row ${selectedItem?.id === item.id ? "active" : ""}`}
                  >
                    <label className="row-check" title={item.id == null ? "未入库条目不能批量操作" : "选择条目"}>
                      <input
                        type="checkbox"
                        checked={item.id != null && selectedIds.has(item.id)}
                        disabled={item.id == null}
                        onChange={() => toggleSelectedItem(item)}
                      />
                    </label>
                    <button className="row-main" type="button" onClick={() => setSelectedItemId(item.id ?? null)}>
                    <span className="code">{item.normalized_code ?? "未识别"}</span>
                    <span className="file">{item.file_name}</span>
                    <span>{formatBytes(item.size_bytes)}</span>
                    <span className={`badge ${item.decision}`}>{decisionLabels[item.decision]}</span>
                    </button>
                  </div>
                ))
              )}
            </div>
          </div>

          <aside className="detail-panel">
            {selectedItem ? (
              <>
                <button
                  className={`poster ${posterPreviewSrc ? "has-cover" : ""}`}
                  type="button"
                  onClick={() => openSelectedFile(selectedItem)}
                  title="打开系统播放器"
                >
                  {posterPreviewSrc ? (
                    <>
                      <img
                        src={posterPreviewSrc}
                        alt={`${selectedItem.normalized_code ?? selectedItem.file_name} 封面`}
                      />
                      <span className="poster-play">
                        <Play size={18} />
                      </span>
                    </>
                  ) : (
                    <Play size={42} />
                  )}
                </button>
                <div className="detail-title">
                  <h2>{selectedItem.metadata?.title_zh ?? selectedItem.normalized_code ?? "待识别影片"}</h2>
                  <span>{selectedItem.file_name}</span>
                </div>
                <div className="meta-list">
                  <div><span>番号</span><strong>{selectedItem.normalized_code ?? "待补充"}</strong></div>
                  <div><span>置信度</span><strong>{Math.round(selectedItem.confidence * 100)}%</strong></div>
                  <div><span>来源</span><strong>{selectedItem.source_root}</strong></div>
                  <div><span>问题</span><strong>{selectedItem.review_reasons.join(", ") || "无"}</strong></div>
                  <div><span>时长</span><strong>{formatDuration(selectedItem.duration_seconds)}</strong></div>
                  <div><span>媒体</span><strong>{formatMediaInfo(selectedItem)}</strong></div>
                  <div><span>哈希</span><strong>{selectedItem.file_hash?.slice(0, 12) ?? "未计算"}</strong></div>
                </div>
                {codeConflictText ? (
                  <div className="conflict-panel">
                    <div className="editor-head">
                      <TriangleAlert size={16} />
                      <strong>番号冲突</strong>
                    </div>
                    <p>{codeConflictText}</p>
                    <span>{selectedItem.code_conflict?.nfo_path}</span>
                  </div>
                ) : null}
                {duplicateCandidates.length > 0 ? (
                  <div className="duplicate-panel">
                    <div className="editor-head">
                      <TriangleAlert size={16} />
                      <strong>重复候选</strong>
                      <span>{duplicateCandidates.length} 个同哈希文件</span>
                    </div>
                    <div className="duplicate-list">
                      {duplicateCandidates.map((candidate) => (
                        <div className="duplicate-row" key={candidate.id ?? candidate.path}>
                          <div>
                            <strong>{candidate.file_name}</strong>
                            <span>{formatBytes(candidate.size_bytes)} · {candidate.path}</span>
                          </div>
                          <button type="button" onClick={() => setSelectedItemId(candidate.id ?? null)}>
                            查看
                          </button>
                        </div>
                      ))}
                    </div>
                  </div>
                ) : null}
                <div className="code-editor">
                  <label htmlFor="manual-code">确认番号</label>
                  <input
                    id="manual-code"
                    value={manualCode}
                    placeholder="例如 ABP-525"
                    onChange={(event) => setManualCode(event.target.value)}
                  />
                </div>
                <div className="profile-tools">
                  <button type="button" onClick={() => openSelectedFile(selectedItem)}>
                    <Play size={16} /> 播放
                  </button>
                  <button type="button" onClick={() => openSelectedDirectory(selectedItem)}>
                    <FolderInput size={16} /> 目录
                  </button>
                  <button type="button" onClick={() => quickResolve(selectedItem)}>
                    <CheckCircle2 size={16} /> 确认匹配
                  </button>
                  <button type="button" onClick={() => retrySelectedMetadata(selectedItem)}>
                    <RefreshCw size={16} /> 重试元数据
                  </button>
                </div>
                <div className="profile-tools">
                  <button
                    type="button"
                    onClick={() => revalidateSelectedMoveFailure(selectedItem)}
                    disabled={!selectedItem.review_reasons.includes("MoveFailed")}
                  >
                    <RefreshCw size={16} /> 重新检查文件
                  </button>
                  <button
                    type="button"
                    onClick={() => ignoreSelectedDuplicate(selectedItem)}
                    disabled={selectedItem.decision !== "DuplicateCandidate" || !selectedItem.review_reasons.includes("DuplicateFile")}
                  >
                    <ListChecks size={16} /> 忽略重复项
                  </button>
                  <button
                    type="button"
                    onClick={() => deleteSelectedFile(selectedItem)}
                    disabled={selectedItem.decision !== "DuplicateCandidate" && selectedItem.decision !== "Ignored"}
                  >
                    <Trash2 size={16} /> 删除源文件
                  </button>
                </div>
                <div className="merge-tools">
                  <label htmlFor="merge-work">合并到已有作品</label>
                  <select id="merge-work" value={mergeWorkId} onChange={(event) => setMergeWorkId(event.target.value)}>
                    <option value="">选择已有作品</option>
                    {mergeableWorks.map((work) => (
                      <option key={work.id ?? work.normalized_code} value={work.id ?? ""}>
                        {formatWorkOption(work)}
                      </option>
                    ))}
                  </select>
                  <button type="button" onClick={() => mergeIntoWork(selectedItem)} disabled={mergeableWorks.length === 0}>
                    <CheckCircle2 size={16} /> 合并到已有作品
                  </button>
                </div>
                <div className="file-version-panel">
                  <div className="editor-head">
                    <ListChecks size={16} />
                    <strong>文件版本</strong>
                    <span>{selectedWork ? `${fileVersions.length} 个` : "未入库"}</span>
                  </div>
                  {!selectedWork ? (
                    <div className="version-empty">确认匹配后显示该作品已入库的文件版本。</div>
                  ) : fileVersions.length === 0 ? (
                    <div className="version-empty">暂无已入库文件版本。</div>
                  ) : (
                    <>
                      <div className="version-merge-tools">
                        <span>已选 {selectedFileVersionIds.size} 个版本</span>
                        <select
                          value={mergeVersionTargetWorkId}
                          onChange={(event) => setMergeVersionTargetWorkId(event.target.value)}
                          disabled={versionMergeTargets.length === 0}
                        >
                          <option value="">目标作品</option>
                          {versionMergeTargets.map((work) => (
                            <option key={work.id ?? work.normalized_code} value={work.id ?? ""}>
                              {formatWorkOption(work)}
                            </option>
                          ))}
                        </select>
                        <button
                          type="button"
                          onClick={mergeSelectedFileVersions}
                          disabled={selectedFileVersionIds.size === 0 || !mergeVersionTargetWorkId}
                        >
                          <CheckCircle2 size={15} /> 合并版本
                        </button>
                        <button
                          type="button"
                          onClick={toggleAllFileVersions}
                          disabled={fileVersions.every((v) => v.id == null)}
                        >
                          {fileVersions.filter((v) => v.id != null).every((v) => selectedFileVersionIds.has(v.id as number)) ? "取消全选" : "全选"}
                        </button>
                        <button type="button" onClick={clearFileVersionSelection} disabled={selectedFileVersionIds.size === 0}>
                          清空
                        </button>
                      </div>
                      <div className="version-list">
                        {fileVersions.map((version) => (
                          <div className="version-row" key={version.id ?? version.original_path}>
                            <label className="version-check" title={version.id == null ? "未入库版本不能选择" : "选择文件版本"}>
                              <input
                                type="checkbox"
                                checked={version.id != null && selectedFileVersionIds.has(version.id)}
                                disabled={version.id == null}
                                onChange={() => toggleFileVersion(version)}
                              />
                            </label>
                            <div className="version-main">
                              <strong>{formatFileVersionSummary(version)}</strong>
                              <span title={version.original_path}>{version.original_path}</span>
                              {version.archived_path ? <span title={version.archived_path}>归档：{version.archived_path}</span> : null}
                            </div>
                          </div>
                        ))}
                      </div>
                    </>
                  )}
                </div>
                <div className="work-profile-editor">
                  <div className="editor-head">
                    <Tags size={16} />
                    <strong>作品资料</strong>
                  </div>
                  <label htmlFor="profile-tags">标签</label>
                  <input
                    id="profile-tags"
                    value={profileTags}
                    placeholder="收藏, 高清, 待整理"
                    onChange={(event) => setProfileTags(event.target.value)}
                    disabled={!selectedWork}
                  />
                  <label htmlFor="profile-lists">列表</label>
                  <input
                    id="profile-lists"
                    value={profileLists}
                    placeholder="待看, 精选"
                    onChange={(event) => setProfileLists(event.target.value)}
                    disabled={!selectedWork}
                  />
                  <div className="profile-grid">
                    <label htmlFor="profile-rating">评分</label>
                    <label htmlFor="profile-status">状态</label>
                    <input
                      id="profile-rating"
                      value={profileRating}
                      placeholder="0-10"
                      inputMode="numeric"
                      onChange={(event) => setProfileRating(event.target.value)}
                      disabled={!selectedWork}
                    />
                    <select
                      id="profile-status"
                      value={profileStatus}
                      onChange={(event) => setProfileStatus(event.target.value as WatchStatus)}
                      disabled={!selectedWork}
                    >
                      <option value="Unwatched">未观看</option>
                      <option value="Watched">已观看</option>
                      <option value="Favorite">收藏</option>
                    </select>
                  </div>
                  <button type="button" onClick={saveWorkProfile} disabled={!selectedWork}>
                    <Star size={16} /> 保存作品资料
                  </button>
                </div>
              </>
            ) : (
              <div className="empty">没有可显示的条目</div>
            )}
          </aside>
        </section>
        ) : null}

        {activeView !== "settings" && activeView !== "library" ? (
        <section className="plan-panel">
          <div className="panel-head">
            <div>
              <h2>迁移计划</h2>
              <span>只展示高置信度自动归档项，待处理项不会移动。</span>
            </div>
          </div>
          <div className="plan-list">
            {(plan?.actions ?? []).map((action) => (
              <div className="plan-row" key={`${action.from_path}-${action.to_path}`}>
                <span>{action.from_path}</span>
                <strong>{action.to_path}</strong>
              </div>
            ))}
            {(plan?.conflicts ?? []).map((conflict) => (
              <div className="plan-row conflict" key={`${conflict.path}-${conflict.message}`}>
                <span>{conflict.message}</span>
                <strong>{conflict.path}</strong>
              </div>
            ))}
            {archiveLogs.map((log) => (
              <div className={`plan-row ${log.status === "moved" ? "" : "conflict"}`} key={`${log.id ?? log.from_path}-${log.to_path}`}>
                <span>
                  {log.status}
                  {log.job_id ? ` · 任务 #${log.job_id}` : ""}
                  {log.created_at ? ` · ${log.created_at}` : ""}
                  {log.message ? `: ${log.message}` : ""}
                </span>
                <strong>{log.to_path}</strong>
              </div>
            ))}
          </div>
        </section>
        ) : null}
      </section>
    </main>
  );
}

function Stat({ icon, label, value }: { icon: React.ReactNode; label: string; value: number }) {
  return (
    <div className="stat">
      {icon}
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function artworkSrc(path: string | null | undefined): string | null {
  if (!path) return null;
  if (/^(https?:|data:|blob:|asset:)/i.test(path)) return path;
  try { return convertFileSrc(path); } catch { return path; }
}

function Lightbox({ src, onClose }: { src: string; onClose: () => void }) {
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="lightbox" onClick={onClose} role="dialog" aria-modal="true">
      <button type="button" className="lightbox-close" onClick={onClose} aria-label="关闭"><X size={22} /></button>
      <img className="lightbox-image" src={src} alt="" onClick={(event) => event.stopPropagation()} />
    </div>
  );
}

// Image with graceful degradation: a broken/missing local file (e.g. a code the
// external image library does not cover) collapses to a placeholder icon instead
// of the browser's torn-image icon. Optional onClick opens the lightbox.
function ArtImage({ src, alt, className, onClick }: { src: string; alt: string; className?: string; onClick?: () => void }) {
  const [broken, setBroken] = useState(false);
  const clickable = onClick ? "clickable" : "";
  if (broken) return <span className={`art-placeholder ${clickable}`.trim()}><ImageIcon size={28} /></span>;
  const cls = [className, clickable].filter(Boolean).join(" ");
  return <img className={cls || undefined} src={src} alt={alt} loading="lazy" onError={() => setBroken(true)} onClick={onClick} />;
}

function WorkCard({ work, selected, onSelect }: { work: Work; selected: boolean; onSelect: () => void }) {
  const title = libraryCardTitle(work);
  const subtitle = libraryCardSubtitle(work);
  const art = artworkSrc(libraryCardArtwork(work));
  return (
    <button type="button" className={`work-card ${selected ? "active" : ""}`} onClick={onSelect}>
      <div className="work-card-art">
        {art ? <ArtImage src={art} alt={title} /> : <ImageIcon size={28} />}
        {work.rating_value ? <span className="work-card-rating"><Star size={11} /> {work.rating_value.toFixed(1)}</span> : null}
      </div>
      <div className="work-card-body">
        <strong className="work-card-title">{title}</strong>
        {subtitle ? <span className="work-card-subtitle">{subtitle}</span> : null}
      </div>
    </button>
  );
}

function MetaRow({ label, value }: { label: string; value: React.ReactNode }) {
  if (value == null || value === "" || (Array.isArray(value) && value.length === 0)) return null;
  return (<div className="meta-row"><span className="meta-label">{label}</span><strong className="meta-value">{value}</strong></div>);
}

function WorkDetailPanel({ work, detail, artworkSrc: artSrc, actors, fileVersions, onOpenInIngest, onImageClick, onSearch }: {
  work: Work; detail: WorkDetail | null; artworkSrc: string | null; actors: Actor[]; fileVersions: FileVersion[]; onOpenInIngest: () => void; onImageClick: (src: string) => void; onSearch: (query: string) => void;
}) {
  const altArtworkRaw = artworkSrc(work.fanart_path) ?? artworkSrc(work.thumb_path);
  // Skip the second artwork when it resolves to the same file as the main image
  // (common with JAV_output NFOs whose cover/poster/thumb/fanart collapse to one <thumb>).
  const altArtwork = altArtworkRaw && altArtworkRaw !== artSrc ? altArtworkRaw : null;
  const screenshotSrc = artworkSrc(work.screenshot_path);
  const gifSrc = artworkSrc(work.gif_path);
  const ratingText = work.rating_value ? `${work.rating_value.toFixed(1)}${work.rating_max ? ` / ${work.rating_max}` : ""}${work.rating_votes ? ` (${work.rating_votes})` : ""}` : (work.rating != null ? String(work.rating) : null);
  const detailTags = detail?.tags.map((t) => t.name) ?? work.tags;
  const detailSets = detail?.sets.map((set) => set.name) ?? work.sets;
  return (
    <>
      <div className="detail-title">
        <h2>{libraryCardTitle(work)}</h2>
        {libraryCardSubtitle(work) ? <span>{libraryCardSubtitle(work)}</span> : null}
      </div>
      {artSrc ? (<div className="library-detail-art"><ArtImage src={artSrc} alt={libraryCardTitle(work)} onClick={() => onImageClick(artSrc)} />{altArtwork ? <ArtImage className="alt-art" src={altArtwork} alt="fanart" onClick={() => onImageClick(altArtwork)} /> : null}</div>) : null}
      {fileVersions.length === 1 && fileVersions[0].size_bytes > 0 && /\.(mp4|mkv|avi|mov|wmv|flv|webm|m4v|ts)$/i.test(fileVersions[0].original_path) ? (
        <div className="profile-tools library-actions">
          <button type="button" onClick={() => api.openFileInSystem(fileVersions[0].original_path)}><Play size={16} /> 播放</button>
        </div>
      ) : null}
      {screenshotSrc || gifSrc ? (
        <div className="library-detail-extras">
          {screenshotSrc ? <div className="extra-shot"><ArtImage src={screenshotSrc} alt="screenshot" onClick={() => onImageClick(screenshotSrc)} /></div> : null}
          {gifSrc ? <div className="extra-shot"><ArtImage src={gifSrc} alt="gif preview" onClick={() => onImageClick(gifSrc)} /></div> : null}
        </div>
      ) : null}
      <div className="profile-tools library-actions">
        <button type="button" onClick={onOpenInIngest}><FolderInput size={16} /> 在入库详情中编辑</button>
      </div>
      <div className="meta-list">
        <MetaRow label="番号" value={work.normalized_code ?? work.source_code} />
        <MetaRow label="类型" value={work.code_kind === "standard" ? "标准番号" : "非标准"} />
        <MetaRow label="状态" value={watchStatusLabels[work.watch_status]} />
        <MetaRow label="评分" value={ratingText} />
        <MetaRow label="时长" value={formatRuntime(work.runtime_minutes)} />
        <MetaRow label="发行" value={work.release_date ?? (work.year ? String(work.year) : null)} />
        <MetaRow label="片商" value={work.studio} />
        <MetaRow label="厂牌" value={work.label} />
        <MetaRow label="导演" value={work.director} />
        <MetaRow label="分级" value={work.mpaa} />
        {work.website ? <div className="meta-row"><span className="meta-label">主页</span><a className="meta-value" href={work.website} target="_blank" rel="noreferrer">{work.website}</a></div> : null}
      </div>
      {actors.length > 0 ? (
        <div className="detail-actors"><div className="editor-head"><Film size={16} /><strong>演员</strong><span>{actors.length} 位</span></div>
          <div className="actor-chips">{actors.map((actor) => (<span key={actor.id ?? actor.primary_name} className="actor-chip">{actor.avatar_path ? <img src={artworkSrc(actor.avatar_path) ?? undefined} alt={actor.primary_name} /> : null}{actor.primary_name}</span>))}</div>
        </div>
      ) : null}
      {detailTags.length > 0 ? (<div className="detail-tags"><div className="editor-head"><Tags size={16} /><strong>标签</strong></div><div className="tag-chips">{detailTags.map((tag) => <button key={tag} type="button" className="tag-chip clickable" onClick={() => onSearch(tag)}>{tag}</button>)}</div></div>) : null}
      {detailSets.length > 0 ? (<div className="detail-tags"><div className="editor-head"><ListChecks size={16} /><strong>系列</strong></div><div className="tag-chips">{detailSets.map((set) => <button key={set} type="button" className="tag-chip clickable" onClick={() => onSearch(set)}>{set}</button>)}</div></div>) : null}
      {work.genres.length > 0 ? (<div className="detail-tags"><div className="editor-head"><Tags size={16} /><strong>类型</strong></div><div className="tag-chips">{work.genres.map((genre) => <button key={genre} type="button" className="tag-chip clickable" onClick={() => onSearch(genre)}>{genre}</button>)}</div></div>) : null}
      {work.outline || work.summary ? <p className="work-summary">{work.outline ?? work.summary}</p> : null}
      <div className="file-version-panel">
        <div className="editor-head"><ListChecks size={16} /><strong>文件版本</strong><span>{fileVersions.length} 个</span></div>
        {fileVersions.length === 0 ? <div className="version-empty">暂无文件版本。</div> : (
          <div className="version-list">{fileVersions.map((version) => {
            const isVideo = version.size_bytes > 0 && /\.(mp4|mkv|avi|mov|wmv|flv|webm|m4v|ts)$/i.test(version.original_path);
            return (
              <div className="version-row library-version-row" key={version.id ?? version.original_path}>
                <div className="version-main"><strong>{formatFileVersionSummary(version)}</strong><span title={version.original_path}>{version.original_path}</span>{version.archived_path ? <span title={version.archived_path}>归档：{version.archived_path}</span> : null}</div>
                {isVideo ? <button type="button" onClick={() => api.openFileInSystem(version.original_path)} title="用系统默认播放器打开"><Play size={14} /></button> : null}
              </div>
            );
          })}</div>
        )}
      </div>
    </>
  );
}

function DirPicker({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  async function pick() {
    try {
      const selected = await openDialog({ directory: true, multiple: false });
      if (typeof selected === "string") onChange(selected);
    } catch { /* dialog unavailable in plain browser */ }
  }
  return (
    <div className="dir-picker">
      <input value={value} placeholder={`${label}路径`} onChange={(event) => onChange(event.target.value)} />
      <button type="button" onClick={pick} title={`选择${label}文件夹`}><FolderOpen size={16} /></button>
    </div>
  );
}
