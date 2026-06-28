import { describe, expect, it } from "vitest";
import type { ArchiveActionLog } from "./api";
import { demoItems } from "./demoData";
import {
  autoArchiveIds,
  applyIngestItemFilters,
  archiveExecutionSummary,
  archivePreviewIds,
  buildReviewQueueSummary,
  buildIngestItemFilters,
  buildDashboardStats,
  buildWorkTagSummary,
  canExecuteArchivePlan,
  coverPreviewPathForItem,
  duplicateCandidatesForItem,
  filterItems,
  filterWorksForLibrary,
  findIngestItemForWork,
  formatDaemonChannel,
  formatDaemonState,
  formatDiagnosticExportSummary,
  formatDiagnosticLogLine,
  formatRemoteScraperSettingsSummary,
  formatSelfCheckSeverity,
  formatSelfCheckSummary,
  formatBytes,
  formatCodeConflictEvidence,
  formatDuration,
  formatExceptionKind,
  formatExceptionStatus,
  formatFileVersionSummary,
  filterInventoryWorks,
  formatInventoryConfidence,
  formatInventoryExportSummary,
  formatInventoryResourceRole,
  formatInventoryReviewBucket,
  formatInventoryResolutionSummary,
  formatHoldingReason,
  formatInventoryActionTarget,
  formatInventoryStatus,
  formatInventorySummary,
  inventoryOrphansForFilter,
  formatMediaInfo,
  formatPipelineStatus,
  formatWorkOption,
  mergeVersionTargetWorks,
  normalizeManualCodeInput,
  parseInventoryRootsInput,
  mergeableWorksForItem,
  parseDelimitedListInput,
  parseProfileRatingInput,
  replaceIngestItem,
  revalidatableMoveFailedItems,
  ignorableDuplicateItems,
  resolvableSelectedItems,
  selectedItemIds,
  viewItemsForMode,
  workbenchViewTitle,
  formatRebuildReport,
  shortEvidence,
  summarizeRunOnceReport,
  libraryCardArtwork,
  libraryCardSubtitle,
  libraryCardTitle,
  formatRuntime,
  partitionWorksByKind,
  type InventoryFilter
} from "./viewModel";

import type { Work } from "./api";

/** Builds a Work fixture with sensible defaults so each test only spells out
 *  the fields it cares about. Keeps inline objects in sync as Work grows. */
function makeWork(overrides: Partial<Work> = {}): Work {
  return {
    code_kind: "standard",
    normalized_code: "ABP-525",
    title_zh: null,
    original_title: null,
    source_code: null,
    aliases: [],
    summary: null,
    outline: null,
    cover_path: null,
    poster_path: null,
    thumb_path: null,
    fanart_path: null,
    tags: [],
    genres: [],
    sets: [],
    lists: [],
    rating: null,
    rating_value: null,
    rating_max: null,
    rating_votes: null,
    criticrating: null,
    watch_status: "Unwatched",
    studio: null,
    label: null,
    director: null,
    release_date: null,
    runtime_minutes: null,
    year: null,
    website: null,
    mpaa: null,
    has_video: true,
    ...overrides,
  };
}

describe("buildDashboardStats", () => {
  it("counts ingest decisions and duplicate codes", () => {
    const stats = buildDashboardStats(demoItems);

    expect(stats.total).toBe(6);
    expect(stats.auto).toBe(3);
    expect(stats.review).toBe(2);
    expect(stats.failed).toBe(1);
    expect(stats.duplicateCodes).toEqual(["ABP-001"]);
  });
});

describe("filterItems", () => {
  it("returns only the selected decision", () => {
    const reviewItems = filterItems(demoItems, "NeedsReview");

    expect(reviewItems.map((item) => item.id)).toEqual([3, 4]);
  });

  it("builds backend filters and applies the same semantics in browser fallback", () => {
    expect(buildIngestItemFilters("All", "All", "All")).toBeUndefined();
    expect(buildIngestItemFilters("NeedsReview", "LowConfidence", "HasCode")).toEqual({
      decision: "NeedsReview",
      review_reason: "LowConfidence",
      has_code: true
    });

    const uncodedReviewItems = applyIngestItemFilters(
      demoItems,
      buildIngestItemFilters("NeedsReview", "MissingCode", "MissingCode")
    );

    expect(uncodedReviewItems.map((item) => item.id)).toEqual([3]);
  });
});

describe("workbench view helpers", () => {
  it("keeps review mode focused on actionable queue items", () => {
    expect(viewItemsForMode(demoItems, "review").map((item) => item.id)).toEqual([3, 4, 6]);
    expect(viewItemsForMode(demoItems, "ingest").length).toBe(demoItems.length);
  });

  it("labels primary workbench views", () => {
    expect(workbenchViewTitle("ingest")).toBe("入库队列");
    expect(workbenchViewTitle("review")).toBe("待处理队列");
    expect(workbenchViewTitle("archive")).toBe("迁移计划");
    expect(workbenchViewTitle("inventory")).toBe("一键盘点");
    expect(workbenchViewTitle("settings")).toBe("设置");
    expect(workbenchViewTitle("library")).toBe("作品库");
  });
});

describe("buildReviewQueueSummary", () => {
  it("counts actionable items and review reasons for the review page", () => {
    const summary = buildReviewQueueSummary(demoItems);

    expect(summary.total).toBe(3);
    expect(summary.byReason.MissingCode).toBe(1);
    expect(summary.byReason.LowConfidence).toBe(1);
    expect(summary.byReason.ProviderFailed).toBe(1);
    expect(summary.byReason.MoveFailed).toBe(1);
    expect(summary.byReason.DuplicateFile).toBe(0);
  });
});

describe("formatCodeConflictEvidence", () => {
  it("shows both path and NFO codes for conflict review", () => {
    expect(formatCodeConflictEvidence({
      path_code: "ABP-525",
      nfo_code: "ABS-204",
      nfo_path: "H:/Inbox/ABP-525/ABP-525.nfo"
    })).toBe("路径 ABP-525 / NFO ABS-204");
    expect(formatCodeConflictEvidence(null)).toBeNull();
  });
});

describe("duplicateCandidatesForItem", () => {
  it("returns same-hash items while excluding the selected item", () => {
    const duplicateA = {
      ...demoItems[0],
      id: 21,
      file_name: "ABP-001-a.mp4",
      file_hash: "same-hash",
      decision: "DuplicateCandidate" as const,
      review_reasons: ["DuplicateFile" as const]
    };
    const duplicateB = {
      ...demoItems[1],
      id: 22,
      file_name: "ABP-001-b.mp4",
      file_hash: "same-hash",
      decision: "DuplicateCandidate" as const,
      review_reasons: ["DuplicateFile" as const]
    };

    expect(duplicateCandidatesForItem([duplicateA, duplicateB, ...demoItems], duplicateA).map((item) => item.id)).toEqual([22]);
    expect(duplicateCandidatesForItem(demoItems, demoItems[0])).toEqual([]);
  });
});

describe("coverPreviewPathForItem", () => {
  it("prefers the persisted work cover and falls back to scanned item metadata", () => {
    const item = {
      ...demoItems[0],
      metadata: {
        ...demoItems[0].metadata!,
        cover_url: "H:/Inbox/ABP-001-poster.jpg"
      }
    };
    const work = makeWork({
      id: 7,
      normalized_code: "ABP-001",
      title_zh: "已有作品",
      cover_path: "H:/Archive/ABP-001/poster.jpg",
      watch_status: "Unwatched"
    });

    expect(coverPreviewPathForItem(item, work)).toBe("H:/Archive/ABP-001/poster.jpg");
    expect(coverPreviewPathForItem(item, null)).toBe("H:/Inbox/ABP-001-poster.jpg");
    expect(coverPreviewPathForItem(null, work)).toBe("H:/Archive/ABP-001/poster.jpg");
    expect(coverPreviewPathForItem(demoItems[3], null)).toBeNull();
  });
});

describe("formatBytes", () => {
  it("formats large video file sizes for scan tables", () => {
    expect(formatBytes(1_420_000_000)).toBe("1.32 GB");
  });
});

describe("media info formatters", () => {
  it("formats duration and stream details for the detail panel", () => {
    expect(formatDuration(3661)).toBe("1:01:01");
    expect(formatDuration(null)).toBe("未探测");
    expect(formatMediaInfo({ width: 1920, height: 1080, codec: "hevc" })).toBe("1920x1080 / hevc");
    expect(formatMediaInfo({ width: null, height: null, codec: null })).toBe("未探测");
  });

  it("formats file version rows with archive state and media fields", () => {
    expect(formatFileVersionSummary({
      id: 11,
      work_id: 7,
      source_root: "H:/Inbox",
      original_path: "H:/Inbox/ABP-525-CD2.mkv",
      archived_path: "H:/Archive/ABP-525/ABP-525-v2.mkv",
      original_file_name: "ABP-525-CD2.mkv",
      normalized_file_name: "ABP-525-v2.mkv",
      size_bytes: 2_310_000_000,
      duration_seconds: 3661,
      width: 1920,
      height: 1080,
      codec: "hevc",
      file_hash: "abc"
    })).toBe("ABP-525-v2.mkv · 2.15 GB · 1:01:01 · 1920x1080 / hevc · 已归档");
  });
});

describe("autoArchiveIds", () => {
  it("selects only high-confidence auto archive items for migration preview", () => {
    expect(autoArchiveIds(demoItems)).toEqual([1, 2, 5]);
  });

  it("uses selected auto-archive items for scoped migration preview when a selection exists", () => {
    expect(archivePreviewIds(demoItems, new Set())).toEqual([1, 2, 5]);
    expect(archivePreviewIds(demoItems, new Set([1, 3, 5]))).toEqual([1, 5]);
    expect(archivePreviewIds(demoItems, new Set([3, 4]))).toEqual([]);
  });
});

describe("replaceIngestItem", () => {
  it("uses the persisted item returned by the backend after resolving a match", () => {
    const storedItem = {
      ...demoItems[3],
      confidence: 1,
      decision: "AutoArchive" as const,
      review_reasons: [],
      candidate_work_id: 12
    };

    const updated = replaceIngestItem(demoItems, storedItem);

    expect(updated.find((item) => item.id === storedItem.id)).toEqual(storedItem);
    expect(buildDashboardStats(updated).auto).toBe(4);
    expect(buildDashboardStats(updated).review).toBe(1);
  });
});

describe("canExecuteArchivePlan", () => {
  it("requires a saved plan id, at least one action, and no conflicts", () => {
    expect(canExecuteArchivePlan(null)).toBe(false);
    expect(canExecuteArchivePlan({ id: 1, actions: [], conflicts: [] })).toBe(false);
    expect(canExecuteArchivePlan({ id: 1, actions: demoItems.slice(0, 1).map((item) => ({
      item_id: item.id,
      work_code: item.normalized_code!,
      from_path: item.path,
      to_path: `H:/Archive/${item.normalized_code}/${item.file_name}`,
      original_file_name: item.file_name,
      normalized_file_name: item.file_name
    })), conflicts: [{ item_id: 1, path: "H:/Archive/ABP-001/ABP-001.mp4", reason: "CodeConflict", message: "exists" }] })).toBe(false);
    expect(canExecuteArchivePlan({ id: 1, actions: demoItems.slice(0, 1).map((item) => ({
      item_id: item.id,
      work_code: item.normalized_code!,
      from_path: item.path,
      to_path: `H:/Archive/${item.normalized_code}/${item.file_name}`,
      original_file_name: item.file_name,
      normalized_file_name: item.file_name
    })), conflicts: [] })).toBe(true);
  });
});

describe("archiveExecutionSummary", () => {
  it("counts moved and failed archive logs for the execution status", () => {
    const logs: ArchiveActionLog[] = [
      {
        id: 1,
        item_id: 10,
        job_id: 2,
        from_path: "H:/Inbox/ABP-525.mp4",
        to_path: "H:/Archive/ABP-525/ABP-525.mp4",
        status: "moved",
        message: null,
        created_at: "2026-06-07 21:00:00"
      },
      {
        id: 2,
        item_id: 11,
        job_id: 2,
        from_path: "H:/Inbox/ABS-204.mp4",
        to_path: "H:/Archive/ABS-204/ABS-204.mp4",
        status: "failed",
        message: "source missing",
        created_at: "2026-06-07 21:01:00"
      }
    ];

    expect(archiveExecutionSummary(logs, 7)).toEqual({
      moved: 1,
      failed: 1,
      message: "迁移执行完成：1 个成功，1 个失败；历史日志 7 条。"
    });
  });
});

describe("formatWorkOption", () => {
  it("shows work id, code, and preferred title for merge selection", () => {
    expect(formatWorkOption(makeWork({ id: 7, normalized_code: "ABP-525", title_zh: "已有作品", original_title: "Original", watch_status: "Unwatched" as const }))).toBe("#7 · ABP-525 · 已有作品");
  });
});

describe("mergeableWorksForItem", () => {
  it("keeps existing works available while excluding the item current candidate work", () => {
    const works = [
      makeWork({ id: 7, normalized_code: "ABP-525", title_zh: "Existing A", watch_status: "Unwatched" as const }),
      makeWork({ id: 8, normalized_code: "ABS-204", title_zh: "Merge Target", watch_status: "Favorite" as const })
    ];
    const item = { ...demoItems[3], candidate_work_id: 7 };

    expect(mergeableWorksForItem(works, item).map((work) => work.id)).toEqual([8]);
  });
});

describe("mergeVersionTargetWorks", () => {
  it("excludes the currently selected work from version merge targets", () => {
    const works = [
      makeWork({ id: 7, normalized_code: "ABP-525", title_zh: "Current", watch_status: "Unwatched" as const }),
      makeWork({ id: 8, normalized_code: "ABS-204", title_zh: "Target", watch_status: "Favorite" as const })
    ];

    expect(mergeVersionTargetWorks(works, works[0]).map((work) => work.id)).toEqual([8]);
    expect(mergeVersionTargetWorks(works, null).map((work) => work.id)).toEqual([7, 8]);
  });
});

describe("library work helpers", () => {
  const works = [
    makeWork({ id: 7, normalized_code: "ABP-525", title_zh: "本地标题", original_title: "Original A", aliases: ["ABP525"], tags: ["收藏", "高清"], lists: ["待看"], rating: 9, watch_status: "Favorite" as const }),
    makeWork({ id: 8, normalized_code: "ABS-204", title_zh: "另一个标题", tags: ["高清"], lists: ["已整理"], watch_status: "Watched" as const })
  ];

  it("filters works by query text and watch status", () => {
    expect(filterWorksForLibrary(works, "abp525", "All").map((work) => work.id)).toEqual([7]);
    expect(filterWorksForLibrary(works, "待看", "All").map((work) => work.id)).toEqual([7]);
    expect(filterWorksForLibrary(works, "高清", "Watched").map((work) => work.id)).toEqual([8]);
    expect(filterWorksForLibrary(works, "", "Favorite").map((work) => work.id)).toEqual([7]);
  });

  it("builds a tag summary sorted by count and name", () => {
    expect(buildWorkTagSummary(works)).toEqual([
      { label: "高清", count: 2 },
      { label: "收藏", count: 1 }
    ]);
  });

  it("finds the ingest queue item that belongs to a selected library work", () => {
    const linkedItem = { ...demoItems[0], id: 21, candidate_work_id: 7, normalized_code: "OTHER-001" };
    const sameCodeItem = { ...demoItems[1], id: 22, candidate_work_id: null, normalized_code: "ABS-204" };

    expect(findIngestItemForWork([sameCodeItem, linkedItem], works[0])?.id).toBe(21);
    expect(findIngestItemForWork([sameCodeItem, linkedItem], works[1])?.id).toBe(22);
    expect(findIngestItemForWork([sameCodeItem], { ...works[0], id: 99, normalized_code: "NOPE-001" })).toBeNull();
  });
});

describe("normalizeManualCodeInput", () => {
  it("trims user-entered codes and rejects blank values before resolving", () => {
    expect(normalizeManualCodeInput("  abp525  ")).toBe("abp525");
    expect(normalizeManualCodeInput("   ")).toBeNull();
  });
});

describe("selected item helpers", () => {
  it("keeps only selected items with persisted ids and safe codes for batch resolve", () => {
    const selected = new Set<number>([3, 4, 99]);

    expect(selectedItemIds(demoItems, selected)).toEqual([3, 4]);
    expect(resolvableSelectedItems(demoItems, selected).map((item) => item.id)).toEqual([4]);
  });

  it("keeps only selected persisted move-failed items for file revalidation", () => {
    const selected = new Set<number>([1, 6, 99]);

    expect(revalidatableMoveFailedItems(demoItems, selected).map((item) => item.id)).toEqual([6]);
  });

  it("keeps only selected persisted duplicate candidates for duplicate ignore", () => {
    const duplicate = {
      ...demoItems[0],
      id: 23,
      decision: "DuplicateCandidate" as const,
      review_reasons: ["DuplicateFile" as const]
    };
    const selected = new Set<number>([1, 23, 99]);

    expect(ignorableDuplicateItems([...demoItems, duplicate], selected).map((item) => item.id)).toEqual([23]);
  });
});

describe("work profile form helpers", () => {
  it("cleans comma and newline separated list input while preserving order", () => {
    expect(parseDelimitedListInput(" 收藏,高清\n收藏 / 待看 ")).toEqual(["收藏", "高清", "待看"]);
  });

  it("parses blank and bounded ratings for work profile updates", () => {
    expect(parseProfileRatingInput("")).toBeNull();
    expect(parseProfileRatingInput(" 9 ")).toBe(9);
    expect(parseProfileRatingInput("11")).toBeNull();
   expect(parseProfileRatingInput("bad")).toBeNull();
 });
});

describe("formatRebuildReport", () => {
  const baseReport = {
    nfos_scanned: 526,
    works_created: 480,
    works_merged: 18,
    tags_extracted: 900,
    sets_extracted: 200,
    actors_extracted: 320,
    file_versions_created: 550,
    errors: [],
  };

  it("formats a clean preview report for the settings status bar", () => {
    const message = formatRebuildReport("preview", baseReport);
    expect(message).toContain("预览完成");
    expect(message).toContain("526");
    expect(message).toContain("480");
    expect(message).not.toContain("失败");
  });

  it("announces rebuild mode and surfaces parse failures when present", () => {
    const report = { ...baseReport, errors: [{ nfo_path: "x.nfo", message: "boom" }] };
    const message = formatRebuildReport("rebuild", report);
    expect(message).toContain("重建完成");
    expect(message).toContain("1 个 NFO 解析失败");
  });
});

describe("inventory preview formatting", () => {
  it("parses one-click inventory roots from pasted or picked directories", () => {
    expect(parseInventoryRootsInput(" H:/video \n\nH:/AV\nH:/video ")).toEqual(["H:/video", "H:/AV"]);
  });

  it("formats inventory statuses and summaries", () => {
    const report = {
      generated_at: "2026-06-27T12:00:00Z",
      roots: ["H:/downloads"],
      archive_root: "H:/AV",
      summary: {
        total_files: 10,
        works: 3,
        asset_candidates: 2,
        auto_ready: 1,
        needs_review: 1,
        blocked: 1,
        ready: 1,
        missing_nfo: 1,
        missing_video: 1,
        multi_video: 1,
        multi_nfo: 0,
        code_conflict: 1,
        duplicate_candidate: 0,
        orphans: 2
      },
      works: [],
      asset_candidates: [],
      orphans: [],
      warnings: [],
      truncated: false
    };

    expect(formatInventoryStatus("ready")).toBe("可整理");
    expect(formatInventoryStatus("missing_nfo")).toBe("缺 NFO");
    expect(formatInventoryStatus("asset_only")).toBe("素材候选");
    expect(formatInventorySummary(report)).toBe("识别 3 部作品，素材候选 2 组：可自动整理 1，需人工确认 1，阻断 1，缺 NFO 1，缺视频 1，冲突 1，孤儿 2。");
    expect(formatInventorySummary({ ...report, truncated: true })).toBe(
      "识别 3 部作品，素材候选 2 组：可自动整理 1，需人工确认 1，阻断 1，缺 NFO 1，缺视频 1，冲突 1，孤儿 2。 结果过多，作品、素材候选和孤儿资源明细各最多展示 1000 项。"
    );
  });

  it("formats inventory action targets", () => {
    expect(formatInventoryActionTarget({ from_path: "H:/x/IPX-001.mp4", to_path: "H:/AV/IPX-001/IPX-001.mp4", kind: "video", conflict: null })).toBe("H:/AV/IPX-001/IPX-001.mp4");
    expect(formatInventoryActionTarget({ from_path: "H:/x/IPX-001.mp4", to_path: "H:/AV/IPX-001/IPX-001.mp4", kind: "video", conflict: "target_exists" })).toBe("H:/AV/IPX-001/IPX-001.mp4（目标已存在）");
    expect(formatInventoryActionTarget({ from_path: "H:/x/IPX-001.mp4", to_path: "H:/AV/IPX-001/IPX-001.mp4", kind: "video", conflict: "target_duplicate" })).toBe("H:/AV/IPX-001/IPX-001.mp4（目标重复）");
    expect(formatInventoryActionTarget({ from_path: "H:/x/IPX-001.mp4", to_path: "H:/AV/IPX-001/IPX-001.mp4", kind: "video", conflict: "target_exists,target_duplicate" })).toBe("H:/AV/IPX-001/IPX-001.mp4（目标已存在，目标重复）");
    expect(formatInventoryActionTarget({ from_path: "H:/x/IPX-001.mp4", to_path: "H:/AV/IPX-001/IPX-001.mp4", kind: "video", conflict: "unexpected_token" })).toBe("H:/AV/IPX-001/IPX-001.mp4（存在冲突：unexpected_token）");
    expect(formatInventoryActionTarget({ from_path: "H:/x/IPX-001.mp4", to_path: null, kind: "video", conflict: null })).toBe("未配置归档根目录");
  });

  it("keeps orphan resources visible only for orphan filters", () => {
    const orphan = {
      path: "H:/loose/readme.txt",
      file_name: "readme.txt",
      kind: "other" as const,
      size_bytes: 12,
      code: null,
      evidence: [],
      warnings: ["无法识别番号"]
    };
    const report = {
      generated_at: "2026-06-27T12:00:00Z",
      roots: ["H:/downloads"],
      archive_root: null,
      summary: {
        total_files: 1,
        works: 0,
        asset_candidates: 0,
        auto_ready: 0,
        needs_review: 0,
        blocked: 0,
        ready: 0,
        missing_nfo: 0,
        missing_video: 0,
        multi_video: 0,
        multi_nfo: 0,
        code_conflict: 0,
        duplicate_candidate: 0,
        orphans: 1
      },
      works: [],
      asset_candidates: [],
      orphans: [orphan],
      warnings: [],
      truncated: false
    };

    expect(inventoryOrphansForFilter(report, "all")).toEqual([]);
    expect(inventoryOrphansForFilter(report, "orphan")).toEqual([orphan]);
    expect(inventoryOrphansForFilter(report, "status:ready")).toEqual([]);
  });

  it("formats inventory review buckets, confidence, and resolution summary", () => {
    const work = {
      code: "IPX-180",
      statuses: ["ready" as const],
      resources: [],
      target_dir: "H:/AV/IPX-180",
      actions: [],
      resource_roles: [],
      resolution: {
        bucket: "auto_ready" as const,
        primary_video: "H:/x/IPX-180.mp4",
        primary_nfo: "H:/x/IPX-180.nfo",
        recommended: "可自动整理",
        reasons: ["推荐主视频：文件名是裸番号视频"],
        warnings: [],
        blockers: [],
        confidence: "high" as const
      }
    };

    expect(formatInventoryReviewBucket("auto_ready")).toBe("可自动整理");
    expect(formatInventoryReviewBucket("needs_review")).toBe("需人工确认");
    expect(formatInventoryConfidence("high")).toBe("高");
    expect(formatInventoryResolutionSummary(work)).toBe("可自动整理 · 置信度 高");
    expect(formatInventoryResourceRole("primary_video")).toBe("主视频");
    expect(formatInventoryResourceRole("duplicate_video")).toBe("疑似重复视频");
    expect(formatInventoryResourceRole("poster")).toBe("封面");
  });

  it("filters inventory works by review bucket and existing status", () => {
    const readyWork = {
      code: "IPX-181",
      statuses: ["ready" as const],
      resources: [],
      target_dir: null,
      actions: [],
      resource_roles: [],
      resolution: {
        bucket: "auto_ready" as const,
        primary_video: "H:/x/IPX-181.mp4",
        primary_nfo: "H:/x/IPX-181.nfo",
        recommended: "可自动整理",
        reasons: [],
        warnings: [],
        blockers: [],
        confidence: "high" as const
      }
    };
    const reviewWork = {
      ...readyWork,
      code: "IPX-182",
      statuses: ["multi_video" as const],
      resolution: {
        ...readyWork.resolution,
        bucket: "needs_review" as const,
        recommended: "需人工确认后再整理",
        confidence: "medium" as const
      }
    };

    const reviewFilter: InventoryFilter = "review:auto_ready";
    const statusFilter: InventoryFilter = "status:multi_video";

    expect(filterInventoryWorks([readyWork, reviewWork], reviewFilter).map((work) => work.code)).toEqual(["IPX-181"]);
    expect(filterInventoryWorks([readyWork, reviewWork], statusFilter).map((work) => work.code)).toEqual(["IPX-182"]);
    expect(filterInventoryWorks([readyWork, reviewWork], "orphan")).toEqual([]);
  });

  it("formats inventory export result", () => {
    expect(formatInventoryExportSummary({
      path: "C:/Users/A/AppData/Roaming/local.media-manager/inventory-reports/inventory-20260628-101010.json",
      works: 10,
      asset_candidates: 3,
      orphans: 2
    })).toBe("已导出盘点结果：C:/Users/A/AppData/Roaming/local.media-manager/inventory-reports/inventory-20260628-101010.json（作品 10，素材候选 3，孤儿 2）。");
  });
});

describe("daemon view helpers", () => {
  it("labels daemon states, holding reasons, exception kinds and run statuses", () => {
    expect(formatDaemonState("Idle")).toBe("空闲");
    expect(formatDaemonState("Paused")).toBe("已暂停");
    expect(formatDaemonChannel("service")).toBe("本地服务");
    expect(formatDaemonChannel("command")).toBe("命令桥");
    expect(formatDaemonChannel("none")).toBe("未连接");
    expect(formatHoldingReason("NoCode")).toBe("缺少番号");
    expect(formatHoldingReason("Unrecognizable")).toBe("无法识别");
    expect(formatExceptionKind("ScrapeFailed")).toBe("刮削失败");
    expect(formatExceptionStatus("Resolved")).toBe("已解决");
    expect(formatPipelineStatus("archived")).toBe("已归档");
    expect(formatPipelineStatus("failed")).toBe("失败");
  });

  it("summarizes daemon run reports and trims JSON evidence", () => {
    expect(summarizeRunOnceReport({
      scan: { scanned_files: 3, queued_files: 2, skipped_files: 1 },
      aria2: {
        enabled: true,
        attempted_gids: 2,
        completed_gids: 1,
        queued_files: 1,
        skipped_files: 0,
        failed_gids: 1,
        errors: ["gid-bad: forbidden"]
      },
      process: { processed: 2, archived: 1, holding: 1, exceptions: 0, failed: 0 }
    })).toBe("aria2 尝试 2 个 GID，完成 1 个，入队 1 个，失败 1 个；扫描 3 个文件，入队 2 个，跳过 1 个；处理 2 个：归档 1，搁置 1，异常 0，失败 0。");

    expect(shortEvidence("{\"source\":\"example\",\"message\":\"not found\"}", 24)).toBe("{\"source\":\"example\",\"...");
    expect(shortEvidence("", 24)).toBe("无证据");
  });

  it("formats remote scraper settings summary", () => {
    expect(formatRemoteScraperSettingsSummary({
      enabled: true,
      timeout_ms: 8000,
      user_agent: "media-manager-test",
      proxy_url: null,
      include_example_fallback: true,
      sources: [
        { id: "javdb", enabled: true, search_url_template: "https://example.test/{code}", min_confidence: 0.82 },
        { id: "javbus", enabled: false, search_url_template: "https://example.test/{code}", min_confidence: 0.82 }
      ]
    })).toBe("已启用 · 1 个远程源 · 保留示例 fallback");

    expect(formatRemoteScraperSettingsSummary({
      enabled: false,
      timeout_ms: 8000,
      user_agent: "media-manager-test",
      proxy_url: null,
      include_example_fallback: false,
      sources: []
    })).toBe("已停用");
  });
});

describe("diagnostics formatting", () => {
  it("formats diagnostic log lines", () => {
    expect(
      formatDiagnosticLogLine({
        timestamp: "2026-06-26T10:00:00Z",
        level: "Error",
        target: "daemon.run_once",
        message: "run failed",
        context: { error: "boom" }
      })
    ).toBe("2026-06-26T10:00:00Z · 错误 · daemon.run_once · run failed");
  });

  it("formats diagnostic export summaries", () => {
    expect(
      formatDiagnosticExportSummary({
        path: "C:/Users/DELL/AppData/Roaming/local.media-manager/diagnostics/diagnostics-20260626-100000.json",
        logs: 12,
        pipeline_runs: 2,
        scrape_jobs: 3,
        open_exceptions: 1,
        holding_items: 4
      })
    ).toContain("已导出诊断快照");
  });
});

describe("self-check formatting", () => {
  it("formats self-check severity and summary", () => {
    const report = {
      generated_at: "2026-06-27T10:00:00Z",
      overall: "warn" as const,
      sandbox: null,
      checks: [
        { id: "sandbox_archive", title: "沙盒归档", severity: "pass" as const, message: "归档成功", action: null },
        { id: "aria2_settings", title: "aria2", severity: "warn" as const, message: "未配置 GID", action: "填写真实 GID" }
      ]
    };

    expect(formatSelfCheckSeverity("pass")).toBe("通过");
    expect(formatSelfCheckSeverity("warn")).toBe("警告");
    expect(formatSelfCheckSummary(report)).toBe("自检有警告：通过 1 项，警告 1 项，失败 0 项。");
  });
});

describe("library browse layer helpers", () => {
  const standardWork = {
    id: 1,
    code_kind: "standard" as const,
    normalized_code: "ABP-525",
    title_zh: "中文标题",
    original_title: "Original Title",
    source_code: null,
    aliases: [],
    summary: null,
    outline: null,
    cover_path: null,
    poster_path: "H:/posters/abp525.jpg",
    thumb_path: null,
    fanart_path: null,
    tags: ["高清"],
    genres: [],
    sets: [],
    lists: [],
    rating: null,
    rating_value: 8.5,
    rating_max: 10,
    rating_votes: 120,
    criticrating: null,
    watch_status: "Unwatched" as const,
    studio: "S1",
    label: null,
    director: null,
    release_date: "2017-01-01",
    runtime_minutes: 150,
    year: 2017,
    website: null,
    mpaa: null,
    has_video: true
  };
  const nonStandardWork = {
    id: 2,
    code_kind: "non_standard" as const,
    normalized_code: null,
    title_zh: "非番号作品",
    original_title: null,
    source_code: "SOME-TITLE-001",
    aliases: [],
    summary: null,
    outline: null,
    cover_path: "H:/covers/some.jpg",
    poster_path: null,
    thumb_path: null,
    fanart_path: null,
    tags: [],
    genres: [],
    sets: [],
    lists: [],
    rating: null,
    rating_value: null,
    rating_max: null,
    rating_votes: null,
    criticrating: null,
    watch_status: "Unwatched" as const,
    studio: null,
    label: null,
    director: null,
    release_date: null,
    runtime_minutes: null,
    year: null,
    website: null,
    mpaa: null,
    has_video: true
  };

  it("partitions works into standard and non-standard sections", () => {
    const result = partitionWorksByKind([standardWork, nonStandardWork]);
    expect(result.standard.map((w) => w.id)).toEqual([1]);
    expect(result.nonStandard.map((w) => w.id)).toEqual([2]);
  });

  it("uses the normalized code as the card title when present, otherwise the title", () => {
    expect(libraryCardTitle(standardWork)).toBe("ABP-525");
    expect(libraryCardTitle(nonStandardWork)).toBe("非番号作品");
  });

  it("shows the title as subtitle only when it differs from the main label", () => {
    expect(libraryCardSubtitle(standardWork)).toBe("中文标题");
    expect(libraryCardSubtitle(nonStandardWork)).toBe("");
  });

  it("prefers poster, then cover, then thumb, then fanart for the card artwork", () => {
    expect(libraryCardArtwork(standardWork)).toBe("H:/posters/abp525.jpg");
    expect(libraryCardArtwork(nonStandardWork)).toBe("H:/covers/some.jpg");
  });

  it("formats runtime minutes into hour and minute strings", () => {
    expect(formatRuntime(150)).toBe("2时30分");
    expect(formatRuntime(60)).toBe("1小时");
    expect(formatRuntime(45)).toBe("45分钟");
    expect(formatRuntime(null)).toBe("");
    expect(formatRuntime(0)).toBe("");
  });
});
