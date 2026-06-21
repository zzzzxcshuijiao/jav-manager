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
  formatBytes,
  formatCodeConflictEvidence,
  formatDuration,
  formatFileVersionSummary,
  formatMediaInfo,
  formatWorkOption,
  mergeVersionTargetWorks,
  normalizeManualCodeInput,
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
  formatRebuildReport
} from "./viewModel";

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
    const work = {
      id: 7,
      normalized_code: "ABP-001",
      title_zh: "已有作品",
      original_title: null,
      aliases: [],
      summary: null,
      cover_path: "H:/Archive/ABP-001/poster.jpg",
      tags: [],
      lists: [],
      rating: null,
      watch_status: "Unwatched" as const
    };

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
    expect(formatWorkOption({
      id: 7,
      normalized_code: "ABP-525",
      title_zh: "已有作品",
      original_title: "Original",
      aliases: [],
      summary: null,
      cover_path: null,
      tags: [],
      lists: [],
      rating: null,
      watch_status: "Unwatched"
    })).toBe("#7 · ABP-525 · 已有作品");
  });
});

describe("mergeableWorksForItem", () => {
  it("keeps existing works available while excluding the item current candidate work", () => {
    const works = [
      {
        id: 7,
        normalized_code: "ABP-525",
        title_zh: "Existing A",
        original_title: null,
        aliases: [],
        summary: null,
        cover_path: null,
        tags: [],
        lists: [],
        rating: null,
        watch_status: "Unwatched" as const
      },
      {
        id: 8,
        normalized_code: "ABS-204",
        title_zh: "Merge Target",
        original_title: null,
        aliases: [],
        summary: null,
        cover_path: null,
        tags: [],
        lists: [],
        rating: null,
        watch_status: "Favorite" as const
      }
    ];
    const item = { ...demoItems[3], candidate_work_id: 7 };

    expect(mergeableWorksForItem(works, item).map((work) => work.id)).toEqual([8]);
  });
});

describe("mergeVersionTargetWorks", () => {
  it("excludes the currently selected work from version merge targets", () => {
    const works = [
      {
        id: 7,
        normalized_code: "ABP-525",
        title_zh: "Current",
        original_title: null,
        aliases: [],
        summary: null,
        cover_path: null,
        tags: [],
        lists: [],
        rating: null,
        watch_status: "Unwatched" as const
      },
      {
        id: 8,
        normalized_code: "ABS-204",
        title_zh: "Target",
        original_title: null,
        aliases: [],
        summary: null,
        cover_path: null,
        tags: [],
        lists: [],
        rating: null,
        watch_status: "Favorite" as const
      }
    ];

    expect(mergeVersionTargetWorks(works, works[0]).map((work) => work.id)).toEqual([8]);
    expect(mergeVersionTargetWorks(works, null).map((work) => work.id)).toEqual([7, 8]);
  });
});

describe("library work helpers", () => {
  const works = [
    {
      id: 7,
      normalized_code: "ABP-525",
      title_zh: "本地标题",
      original_title: "Original A",
      aliases: ["ABP525"],
      summary: null,
      cover_path: null,
      tags: ["收藏", "高清"],
      lists: ["待看"],
      rating: 9,
      watch_status: "Favorite" as const
    },
    {
      id: 8,
      normalized_code: "ABS-204",
      title_zh: "另一个标题",
      original_title: null,
      aliases: [],
      summary: null,
      cover_path: null,
      tags: ["高清"],
      lists: ["已整理"],
      rating: null,
      watch_status: "Watched" as const
    }
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
