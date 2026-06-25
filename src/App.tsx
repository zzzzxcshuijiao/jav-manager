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
  FileVersion,
  IngestDecision,
  IngestItem,
  IngestJobSummary,
  MigrationPlan,
  PooledWork,
  ResourcePool,
  ReviewReason,
  ThumbnailCacheSummary,
  RebuildReport,
  Tag,
  UnifiedMigrationPlan,
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
  formatFileVersionSummary,
  formatMediaInfo,
  formatRebuildReport,
  formatWorkOption,
  ignorableDuplicateItems,
  mergeVersionTargetWorks,
  mergeableWorksForItem,
  normalizeManualCodeInput,
  parseDelimitedListInput,
  parseProfileRatingInput,
  replaceIngestItem,
  revalidatableMoveFailedItems,
  resolvableSelectedItems,
  selectedItemIds,
  type ReviewReasonFilter,
  viewItemsForMode,
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
  const [migrationNfoDir, setMigrationNfoDir] = useState("");
  const [migrationVideoDir, setMigrationVideoDir] = useState("");
  const [migrationTargetDir, setMigrationTargetDir] = useState("");
  const [migrationPlan, setMigrationPlan] = useState<MigrationPlan | null>(null);
  const [resourcePoolDirs, setResourcePoolDirs] = useState("");
  const [unifiedTargetDir, setUnifiedTargetDir] = useState("");
  const [unifiedPlan, setUnifiedPlan] = useState<UnifiedMigrationPlan | null>(null);
  const [resourcePool, setResourcePool] = useState<ResourcePool | null>(null);
  const [primaryLibraryDir, setPrimaryLibraryDir] = useState("");
  const [settingsTab, setSettingsTab] = useState<"pool" | "rebuild" | "migrate" | "cache">("pool");
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
          setSourceRoots(storedSourceRoots.join("\n"));
        }
        if (storedArchiveRoot) {
          setArchiveRoot(storedArchiveRoot);
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

  async function planMigration() {
    if (!migrationNfoDir || !migrationVideoDir || !migrationTargetDir) {
      setStatus("请填写 NFO 目录、视频目录和目标目录");
      return;
    }
    try {
      setStatus("正在扫描文件并生成迁移计划...");
      const plan = await api.planCentralizedMigration(migrationNfoDir, migrationVideoDir, migrationTargetDir);
      setMigrationPlan(plan);
      setStatus(`迁移计划：${plan.total_nfos} 个 NFO · ${plan.matched_videos} 个视频匹配 · ${plan.unmatched_nfos} 个未匹配`);
    } catch (error) {
      setStatus(`生成迁移计划失败：${String(error)}`);
    }
  }

  async function executeMigration() {
    if (!migrationPlan) {
      setStatus("请先生成迁移计划");
      return;
    }
    if (!window.confirm(`即将迁移 ${migrationPlan.works.length} 个作品（${migrationPlan.matched_videos} 个视频）到目标目录。视频文件将被移动（不是复制），确定继续？`)) {
      return;
    }
    try {
      setStatus("正在执行迁移，请勿关闭窗口...");
      const migrated = await api.executeCentralizedMigration(migrationPlan);
      setStatus(`迁移完成：${migrated} 个作品`);
      setMigrationPlan(null);
    } catch (error) {
      setStatus(`迁移失败：${String(error)}`);
    }
  }

  function parsedResourcePoolDirs(): string[] {
    return resourcePoolDirs.split("\n").map((line) => line.trim()).filter((line) => line.length > 0);
  }

  async function scanPool() {
    const dirs = parsedResourcePoolDirs();
    if (dirs.length === 0) {
      setStatus("请填写至少一个资源池目录");
      return;
    }
    const pool = await runBusy<ResourcePool>("正在扫描资源池...", () => api.scanResourcePool(dirs));
    if (pool) {
      setResourcePool(pool);
      setStatus(`资源池：${pool.total_nfos} 个 NFO · ${pool.total_videos} 个视频 · ${pool.total_images} 张图片 · ${pool.orphan_videos + pool.orphan_images} 个未匹配`);
    }
  }

  async function planUnifiedMigration() {
    const dirs = parsedResourcePoolDirs();
    if (dirs.length === 0) {
      setStatus("请填写至少一个资源池目录");
      return;
    }
    if (!unifiedTargetDir) {
      setStatus("请填写目标目录");
      return;
    }
    const plan = await runBusy<UnifiedMigrationPlan>("正在扫描资源池并生成迁移计划...", () => api.planUnifiedMigration(dirs, unifiedTargetDir));
    if (plan) {
      setUnifiedPlan(plan);
      setStatus(`迁移计划：${plan.total_works} 个作品 · ${plan.total_videos} 个视频 · ${plan.total_images} 张图片`);
    }
  }

  async function executeUnifiedMigration() {
    if (!unifiedPlan) {
      setStatus("请先生成迁移计划");
      return;
    }
    if (!window.confirm(`即将迁移 ${unifiedPlan.total_works} 个作品（${unifiedPlan.total_videos} 个视频将被移动，${unifiedPlan.total_images} 张图片将被复制）到目标目录，确定继续？`)) {
      return;
    }
    const migrated = await runBusy<number>("正在执行智能迁移，请勿关闭窗口...", () => api.executeUnifiedMigration(unifiedPlan));
    if (migrated === null) return;
    setUnifiedPlan(null);
    setStatus(`智能迁移完成：${migrated} 个作品`);
    // Auto-add the migration target to the resource pool so the next scan/ rebuild
    // picks up the newly consolidated self-contained work directories.
    const current = parsedResourcePoolDirs();
    const target = unifiedTargetDir.trim();
    if (target && !current.includes(target)) {
      const next = [...current, target];
      setResourcePoolDirs(next.join("\n"));
      try {
        await api.configureResourcePoolDirs(next);
      } catch {
        // best-effort persist; the textarea is already updated
      }
    }
  }

  async function saveResourcePool() {
    const dirs = parsedResourcePoolDirs();
    try {
      await api.configureResourcePoolDirs(dirs);
      setStatus(`已保存资源池目录（${dirs.length} 个）`);
    } catch (error) {
      setStatus(`保存资源池失败：${String(error)}`);
    }
  }

  async function runRebuildFromPool() {
    if (busy) return;
    const dirs = parsedResourcePoolDirs();
    if (dirs.length === 0) {
      setStatus("请填写至少一个资源池目录");
      return;
    }
    if (!window.confirm("将从资源池重新解析作品库，确定继续？")) {
      return;
    }
    const report = await runBusy<RebuildReport>("正在扫描资源池并重建作品库，请稍候…", () => api.rebuildLibraryFromPool(dirs));
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
      setStatus("请填写至少一个资源池目录");
      return;
    }
    const primary = primaryLibraryDir.trim();
    if (!primary) {
      setStatus("请先设置主库目录");
      return;
    }
    const report = await runBusy<RebuildReport>("正在扫描资源池并增量同步到主库，请稍候…", () => api.incrementalSync(dirs, primary));
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
        <section className="source-panel">
          <label>来源目录</label>
          <textarea value={sourceRoots} onChange={(event) => setSourceRoots(event.target.value)} />
          <button className="primary" type="button" onClick={runScan} disabled={busy}>
            <RefreshCw size={16} /> 扫描
          </button>
        </section>
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
          <button className="primary" type="button" onClick={previewPlan}>
            <Archive size={16} /> 预览迁移
          </button>
          <button className="primary" type="button" onClick={executePlan} disabled={!canExecuteArchivePlan(plan)}>
            <CheckCircle2 size={16} /> 执行迁移
          </button>
        </header>

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

        {activeView === "settings" ? (
          <section className="settings-panel">
            <div className="settings-tabs">
              <button type="button" className={settingsTab === "pool" ? "active" : ""} onClick={() => setSettingsTab("pool")}>目录与资源池</button>
              <button type="button" className={settingsTab === "rebuild" ? "active" : ""} onClick={() => setSettingsTab("rebuild")}>作品库</button>
              <button type="button" className={settingsTab === "migrate" ? "active" : ""} onClick={() => setSettingsTab("migrate")}>迁移</button>
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
                    <strong>资源池目录</strong>
                    <span>每行一个目录。所有目录视为统一资源池，自动按番号匹配 NFO、视频、封面、剧照、GIF。</span>
                  </div>
                  <button type="button" className="primary" onClick={saveResourcePool}>
                    <CheckCircle2 size={16} /> 保存资源池
                  </button>
                  <button type="button" onClick={scanPool}>
                    <Search size={16} /> 扫描资源池
                  </button>
                </div>
                <div className="migration-form">
                  <textarea value={resourcePoolDirs} onChange={(e) => setResourcePoolDirs(e.target.value)} placeholder="例如&#10;H:\CineMingle-1.3.0\JAV_output&#10;H:\bucket&#10;G:\软件\98tang\Img\Poster&#10;G:\软件\98tang\Img\ScreenShot" />
                </div>
                {resourcePool ? (
                  <div className="rebuild-report">
                    <span>
                      资源池扫描：{resourcePool.total_nfos} 个 NFO · {resourcePool.total_videos} 个视频 · {resourcePool.total_images} 张图片 · 作品 {resourcePool.works.length} 个
                      · 孤儿视频 {resourcePool.orphan_videos} · 孤儿图片 {resourcePool.orphan_images}
                    </span>
                  </div>
                ) : null}
              </>
            ) : null}

            {settingsTab === "rebuild" ? (
              <>
                <div className="rebuild-tools">
                  <div>
                    <strong>作品库重建（全量）</strong>
                    <span>清空作品库，从资源池全量重新解析。仅首次建库或元数据大改时用。</span>
                  </div>
                  <button type="button" className="primary" onClick={runRebuildFromPool} disabled={busy}>
                    <Database size={16} /> 执行重建
                  </button>
                </div>
                <div className="rebuild-tools">
                  <div>
                    <strong>增量同步（日常推荐）</strong>
                    <span>主库目录是唯一库；扫描资源池把缺的视频/图片复制进主库对应作品，新番号自动新增，已有作品保留。无需重设目录。</span>
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

            {settingsTab === "migrate" ? (
              <>
                <div className="migration-tools">
                  <div>
                    <strong>智能迁移（自包含作品目录）</strong>
                    <span>从资源池扫描，按番号匹配 NFO+视频+图片，整合到作品目录（番号/番号.nfo + 番号.mp4 + 番号-v2.mp4 + poster.jpg + screenshots/）。视频移动、图片复制。</span>
                  </div>
                  <div className="migration-form">
                    <label>目标目录</label>
                    <input value={unifiedTargetDir} onChange={(e) => setUnifiedTargetDir(e.target.value)} placeholder="例如 H:\consolidated" />
                  </div>
                  <button type="button" onClick={planUnifiedMigration}>
                    <Search size={16} /> 生成迁移计划
                  </button>
                  <button type="button" className="primary" onClick={executeUnifiedMigration} disabled={!unifiedPlan}>
                    <Archive size={16} /> 执行迁移
                  </button>
                </div>
                {unifiedPlan ? (
                  <div className="rebuild-report">
                    <span>
                      迁移计划：{unifiedPlan.total_works} 个作品 · {unifiedPlan.total_videos} 个视频 · {unifiedPlan.total_images} 张图片
                    </span>
                    {unifiedPlan.works.length > 0 ? (
                      <ul>
                        {unifiedPlan.works.slice(0, 5).map((work) => (
                          <li key={work.code}>{work.code}：{work.videos.length} 视频 + {(work.poster ? 1 : 0) + (work.fanart ? 1 : 0) + work.screenshots.length + work.gifs.length} 图片 → {work.target_dir}</li>
                        ))}
                        {unifiedPlan.works.length > 5 ? <li>...还有 {unifiedPlan.works.length - 5} 个作品</li> : null}
                      </ul>
                    ) : null}
                  </div>
                ) : null}
              </>
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
