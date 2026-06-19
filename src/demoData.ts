import type { ArchivePlan, IngestItem, IngestJobSummary } from "./api";

export const demoJob: IngestJobSummary = {
  id: 1,
  status: "completed",
  total_items: 6,
  auto_count: 3,
  review_count: 2,
  failed_count: 1
};

export const demoItems: IngestItem[] = [
  {
    id: 1,
    job_id: 1,
    source_root: "H:/Downloads/A",
    path: "H:/Downloads/A/[site] ABP_001 1080p/ABP001.mp4",
    file_name: "ABP001.mp4",
    size_bytes: 1_420_000_000,
    duration_seconds: 5412,
    width: 1920,
    height: 1080,
    codec: "h264",
    normalized_code: "ABP-001",
    confidence: 0.96,
    decision: "AutoArchive",
    review_reasons: [],
    metadata: {
      provider: "example",
      title_zh: "ABP-001 本地示例标题",
      original_title: "ABP-001 Example Title",
      aliases: ["ABP001"],
      summary: null,
      cover_url: null,
      release_date: null,
      confidence: 0.85
    }
  },
  {
    id: 2,
    job_id: 1,
    source_root: "H:/Downloads/B",
    path: "H:/Downloads/B/ABP-001-source.mkv",
    file_name: "ABP-001-source.mkv",
    size_bytes: 2_310_000_000,
    duration_seconds: 5520,
    width: 3840,
    height: 2160,
    codec: "hevc",
    normalized_code: "ABP-001",
    confidence: 0.94,
    decision: "AutoArchive",
    review_reasons: [],
    metadata: null
  },
  {
    id: 3,
    job_id: 1,
    source_root: "H:/Inbox",
    path: "H:/Inbox/noisy-title.mp4",
    file_name: "noisy-title.mp4",
    size_bytes: 980_000_000,
    normalized_code: null,
    confidence: 0.3,
    decision: "NeedsReview",
    review_reasons: ["MissingCode"],
    metadata: null
  },
  {
    id: 4,
    job_id: 1,
    source_root: "H:/Inbox",
    path: "H:/Inbox/xyz002.mp4",
    file_name: "xyz002.mp4",
    size_bytes: 1_110_000_000,
    normalized_code: "XYZ-002",
    confidence: 0.63,
    decision: "NeedsReview",
    review_reasons: ["LowConfidence", "ProviderFailed"],
    metadata: null
  },
  {
    id: 5,
    job_id: 1,
    source_root: "H:/Downloads/A",
    path: "H:/Downloads/A/IPX-888.mp4",
    file_name: "IPX-888.mp4",
    size_bytes: 1_820_000_000,
    normalized_code: "IPX-888",
    confidence: 0.97,
    decision: "AutoArchive",
    review_reasons: [],
    metadata: null
  },
  {
    id: 6,
    job_id: 1,
    source_root: "H:/Downloads/C",
    path: "H:/Downloads/C/locked.mp4",
    file_name: "locked.mp4",
    size_bytes: 500_000_000,
    normalized_code: null,
    confidence: 0,
    decision: "Failed",
    review_reasons: ["MoveFailed"],
    metadata: null
  }
];

export const demoPlan: ArchivePlan = {
  id: 1,
  actions: demoItems
    .filter((item) => item.decision === "AutoArchive" && item.normalized_code)
    .map((item, index) => ({
      item_id: item.id,
      work_code: item.normalized_code!,
      from_path: item.path,
      to_path: `H:/Archive/${item.normalized_code}/${item.normalized_code}${index === 1 ? "-v2" : ""}.${item.file_name.split(".").pop()}`,
      original_file_name: item.file_name,
      normalized_file_name: `${item.normalized_code}${index === 1 ? "-v2" : ""}.${item.file_name.split(".").pop()}`
    })),
  conflicts: []
};
