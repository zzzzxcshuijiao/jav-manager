import { convertFileSrc } from "@tauri-apps/api/core";
import {
  Archive,
  CheckCircle2,
  Clock3,
  Database,
  FolderInput,
  ListChecks,
  Play,
  RefreshCw,
  Search,
  Settings,
  Star,
 Tags,
  TriangleAlert,
  Trash2
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
  ReviewReason,
  ThumbnailCacheSummary,
  RebuildReport,
  WatchStatus,
  Work
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
  Watched: "已观看",
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
  const [selectedFileVersionIds, setSelectedFileVersionIds] = useState<Set<number>>(() => new Set());
  const [mergeVersionTargetWorkId, setMergeVersionTargetWorkId] = useState("");
  const [libraryQuery, setLibraryQuery] = useState("");
  const [libraryStatusFilter, setLibraryStatusFilter] = useState<WorkStatusFilter>("All");
  const [selectedLibraryWorkId, setSelectedLibraryWorkId] = useState<number | null>(null);
  const [libraryFileVersions, setLibraryFileVersions] = useState<FileVersion[]>([]);
  const [libraryWorkActors, setLibraryWorkActors] = useState<Actor[]>([]);
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
  const selectedLibraryWork = libraryWorks.find((work) => work.id === selectedLibraryWorkId) ?? libraryWorks[0] ?? null;
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
    async function loadLibraryFileVersions() {
      if (!selectedLibraryWork?.id) {
        setLibraryFileVersions([]);
        return;
      }
      try {
        const versions = await api.listFileVersionsForWork(selectedLibraryWork.id);
        if (!cancelled) {
          setLibraryFileVersions(versions);
        }
      } catch {
        if (!cancelled) {
          setLibraryFileVersions([]);
        }
      }
    }
    loadLibraryFileVersions();
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
    try {
      const report = await api.previewRebuild(parsedSourceRoots());
      setRebuildMode("preview");
      setRebuildReport(report);
      setStatus(formatRebuildReport("preview", report));
    } catch (error) {
      setStatus(`预览重建失败：${String(error)}`);
    }
  }

  async function runRebuildLibrary() {
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
          <button className="primary" type="button" onClick={runScan}>
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
            <div className="panel-head">
              <div>
                <h2>目录设置</h2>
                <span>保存后会写入 SQLite，扫描时使用这些来源目录和归档根目录。</span>
              </div>
            </div>
            <div className="settings-form">
              <label>来源目录</label>
              <textarea value={sourceRoots} onChange={(event) => setSourceRoots(event.target.value)} />
              <label>归档根目录</label>
              <input value={archiveRoot} onChange={(event) => setArchiveRoot(event.target.value)} />
              <label className="settings-check">
                <input
                  type="checkbox"
                  checked={metadataProviderEnabled}
                  onChange={(event) => setMetadataProviderEnabled(event.target.checked)}
                />
                <span>启用示例元数据源</span>
              </label>
              <p className="settings-note">
                默认关闭。关闭时仍读取本地 NFO、封面和媒体信息，但不会调用示例 Provider 自动补全元数据。
              </p>
              <button className="primary" type="button" onClick={saveConfiguration}>
                <CheckCircle2 size={16} /> 保存设置
              </button>
            </div>
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
            <div className="rebuild-tools">
              <div>
                <strong>作品库重建</strong>
                <span>从来源目录的 NFO 重新解析作品、标签与演员等元数据。</span>
              </div>
              <button type="button" onClick={previewRebuild}>
                <Search size={16} /> 预览重建
              </button>
              <button type="button" className="primary" onClick={runRebuildLibrary}>
                <Database size={16} /> 执行重建
              </button>
            </div>
            {rebuildReport ? (
              <div className="rebuild-report">
                <span>
                  {rebuildMode === "preview" ? "预览" : "重建"}：{rebuildReport.nfos_scanned} 个 NFO
                  · {rebuildReport.works_created} 个作品 · {rebuildReport.works_merged} 个合并组
                  · 标签 {rebuildReport.tags_extracted} · 系列 {rebuildReport.sets_extracted}
                  · 演员 {rebuildReport.actors_extracted} · 文件版本 {rebuildReport.file_versions_created}
                  {rebuildReport.errors.length > 0
                    ? ` · ${rebuildReport.errors.length} 个 NFO 解析失败`
                    : ""}
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
                <input
                  value={libraryQuery}
                  placeholder="搜索番号、标题、标签、列表"
                  onChange={(event) => setLibraryQuery(event.target.value)}
                />
                <select
                  value={libraryStatusFilter}
                  onChange={(event) => setLibraryStatusFilter(event.target.value as WorkStatusFilter)}
                >
                  <option value="All">全部状态</option>
                  <option value="Unwatched">未观看</option>
                  <option value="Watched">已观看</option>
                  <option value="Favorite">收藏</option>
                </select>
              </div>
              <div className="tag-summary">
                {libraryTags.length === 0 ? (
                  <span>暂无标签</span>
                ) : (
                  libraryTags.slice(0, 8).map((tag) => (
                    <button key={tag.label} type="button" onClick={() => setLibraryQuery(tag.label)}>
                      {tag.label} <strong>{tag.count}</strong>
                    </button>
                  ))
                )}
              </div>
              <div className="work-list">
                {libraryWorks.length === 0 ? (
                  <div className="empty-inline">没有匹配当前筛选的作品。先确认匹配或修改作品资料后再查看。</div>
                ) : (
                  libraryWorks.map((work) => (
                    <button
                      key={work.id ?? work.normalized_code}
                      className={`work-row ${selectedLibraryWork?.id === work.id ? "active" : ""}`}
                      type="button"
                      onClick={() => setSelectedLibraryWorkId(work.id ?? null)}
                    >
                      <strong>{work.normalized_code}</strong>
                      <span>{work.title_zh ?? work.original_title ?? "未命名作品"}</span>
                      <small>{watchStatusLabels[work.watch_status]} · {work.tags.join(", ") || "无标签"}</small>
                    </button>
                  ))
                )}
              </div>
            </div>

            <aside className="library-detail">
              {selectedLibraryWork ? (
                <>
                  <div className="detail-title">
                    <h2>{selectedLibraryWork.title_zh ?? selectedLibraryWork.normalized_code}</h2>
                    <span>{selectedLibraryWork.original_title ?? selectedLibraryWork.normalized_code}</span>
                  </div>
                  <div className="profile-tools library-actions">
                    <button type="button" onClick={openLibraryWorkInIngestDetail}>
                      <FolderInput size={16} /> 在入库详情中编辑
                    </button>
                  </div>
                  <div className="meta-list">
                    <div><span>番号</span><strong>{selectedLibraryWork.normalized_code}</strong></div>
                    <div><span>状态</span><strong>{watchStatusLabels[selectedLibraryWork.watch_status]}</strong></div>
                    <div><span>评分</span><strong>{selectedLibraryWork.rating ?? "未评分"}</strong></div>
                    <div><span>标签</span><strong>{selectedLibraryWork.tags.join(", ") || "无"}</strong></div>
                    <div><span>列表</span><strong>{selectedLibraryWork.lists.join(", ") || "无"}</strong></div>
                  </div>
                  {selectedLibraryWork.summary ? (
                    <p className="work-summary">{selectedLibraryWork.summary}</p>
                  ) : null}
                  <div className="file-version-panel">
                    <div className="editor-head">
                      <ListChecks size={16} />
                      <strong>文件版本</strong>
                      <span>{libraryFileVersions.length} 个</span>
                    </div>
                    {libraryFileVersions.length === 0 ? (
                      <div className="version-empty">暂无文件版本。</div>
                    ) : (
                      <div className="version-list">
                        {libraryFileVersions.map((version) => (
                          <div className="version-row library-version-row" key={version.id ?? version.original_path}>
                            <div className="version-main">
                              <strong>{formatFileVersionSummary(version)}</strong>
                              <span title={version.original_path}>{version.original_path}</span>
                              {version.archived_path ? <span title={version.archived_path}>归档：{version.archived_path}</span> : null}
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                </>
              ) : (
                <div className="empty">暂无作品</div>
              )}
            </aside>
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
