# Stage 7B Pairing Rules Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add read-only pairing rules, review buckets, explanations, per-run archive target input, and JSON export for the inventory preview.

**Architecture:** Extend the existing Rust `inventory` module instead of replacing the Stage 7A scanner. The scanner still produces a read-only `InventoryPreviewReport`; Stage 7B enriches each work with `InventoryResolution` and `InventoryResourceRole`, exposes export through a Tauri command, and updates React/viewModel helpers so the UI can show review queues and explanations. No media file writes, SQLite task persistence, or real archive execution happens in this phase.

**Tech Stack:** Rust, serde, chrono, Tauri commands, React, TypeScript, Vitest.

---

## File Map

- Modify `src-tauri/src/inventory.rs`
  - Add `InventoryReviewBucket`, `InventoryConfidence`, `InventoryResourceRoleKind`, `InventoryResolution`, and `InventoryResourceRole`.
  - Select primary video and primary NFO using deterministic rules.
  - Classify each work into `auto_ready`, `needs_review`, `blocked`, or `asset_candidate`.
  - Add `auto_ready`, `needs_review`, and `blocked` summary counts.
- Modify `src-tauri/tests/inventory.rs`
  - Add TDD coverage for primary video/NFO selection, review buckets, blockers, and resource roles.
- Modify `src-tauri/src/commands.rs`
  - Add `InventoryExportResult`.
  - Add `export_inventory_report_command`.
  - Register the command in `generate_handler!`.
  - Add command tests for JSON export path and content.
- Modify `src/api.ts`
  - Add TypeScript DTOs matching the new Rust inventory types.
  - Add `InventoryExportResult` and `exportInventoryReport(report)`.
- Modify `src/viewModel.ts`
  - Add `InventoryFilter`, bucket formatting, confidence formatting, resolution summary, filter helpers, and export summary formatting.
- Modify `src/viewModel.test.ts`
  - Add tests for filter semantics and formatting.
- Modify `src/App.tsx`
  - Add per-run inventory archive root state.
  - Add target directory picker, export button, review-bucket filters, and resolution detail panel.
- Modify `src/styles.css`
  - Add compact styles for resolution panels, bucket badges, and target-directory input.
- Modify `.ai_state/tasks.md`, `.ai_state/progress.md`
  - Track plan and implementation progress. These are gitignored session state.

---

## Task 1: Backend DTOs and Primary Resource Selection

**Files:**
- Modify: `src-tauri/src/inventory.rs`
- Test: `src-tauri/tests/inventory.rs`

- [ ] **Step 1: Write failing primary selection tests**

Append these tests to `src-tauri/tests/inventory.rs`:

```rust
#[test]
fn inventory_resolution_selects_bare_video_and_matching_nfo_as_primary() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    write_file(&root.join("IPX-159-CD2.mkv"), b"part2");
    write_file(&root.join("IPX-159.mp4"), b"main-video");
    write_file(
        &root.join("metadata.nfo"),
        br#"<movie><num>IPX-159</num><title>Main</title></movie>"#,
    );

    let report = preview_inventory_roots(&[root], None).unwrap();
    let work = report.works.iter().find(|work| work.code == "IPX-159").unwrap();

    assert_eq!(work.resolution.primary_video.as_deref(), Some(work.resources.iter().find(|resource| resource.file_name == "IPX-159.mp4").unwrap().path.as_path()));
    assert_eq!(work.resolution.primary_nfo.as_deref(), Some(work.resources.iter().find(|resource| resource.file_name == "metadata.nfo").unwrap().path.as_path()));
    assert_eq!(work.resolution.confidence, media_manager::inventory::InventoryConfidence::High);
    assert!(work.resolution.reasons.iter().any(|reason| reason.contains("裸番号视频")));
    assert!(work.resource_roles.iter().any(|role| role.path.ends_with("IPX-159.mp4") && role.role == media_manager::inventory::InventoryResourceRoleKind::PrimaryVideo && role.selected));
    assert!(work.resource_roles.iter().any(|role| role.path.ends_with("metadata.nfo") && role.role == media_manager::inventory::InventoryResourceRoleKind::PrimaryNfo && role.selected));
}

#[test]
fn inventory_resolution_uses_largest_video_when_no_bare_or_first_part_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    write_file(&root.join("IPX-160-extra-small.mp4"), b"small");
    write_file(&root.join("IPX-160-extra-large.mkv"), b"larger-video");
    write_file(
        &root.join("IPX-160.nfo"),
        br#"<movie><num>IPX-160</num><title>Main</title></movie>"#,
    );

    let report = preview_inventory_roots(&[root], None).unwrap();
    let work = report.works.iter().find(|work| work.code == "IPX-160").unwrap();

    assert!(work.resolution.primary_video.as_ref().unwrap().ends_with("IPX-160-extra-large.mkv"));
    assert!(work.resolution.reasons.iter().any(|reason| reason.contains("体积最大视频")));
}
```

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory inventory_resolution_selects_bare_video_and_matching_nfo_as_primary -j 1
```

Expected: FAIL because `InventoryWorkPreview` does not yet have `resolution` and `resource_roles`, and the new enum types do not exist.

- [ ] **Step 3: Add DTOs and primary selection helpers**

In `src-tauri/src/inventory.rs`, add these public DTOs after `InventoryStatus`:

```rust
/// Review queue assigned to one inventory work after pairing rules run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryReviewBucket {
    AutoReady,
    NeedsReview,
    Blocked,
    AssetCandidate,
}

/// Confidence level for the pairing recommendation shown in the inventory UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryConfidence {
    High,
    Medium,
    Low,
}

/// Role assigned to one scanned resource by the read-only pairing rules.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryResourceRoleKind {
    PrimaryVideo,
    SecondaryVideo,
    DuplicateVideo,
    PrimaryNfo,
    SecondaryNfo,
    Poster,
    Fanart,
    Thumb,
    Screenshot,
    Gif,
    Image,
    Other,
}

/// Explanation for one resource's role in the proposed pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryResourceRole {
    pub path: PathBuf,
    pub role: InventoryResourceRoleKind,
    pub reason: String,
    pub selected: bool,
    pub needs_review: bool,
}

/// Work-level read-only pairing decision used by the review UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryResolution {
    pub bucket: InventoryReviewBucket,
    pub primary_video: Option<PathBuf>,
    pub primary_nfo: Option<PathBuf>,
    pub recommended: String,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
    pub confidence: InventoryConfidence,
}
```

Extend `InventoryWorkPreview`:

```rust
pub struct InventoryWorkPreview {
    pub code: String,
    pub statuses: Vec<InventoryStatus>,
    pub resources: Vec<InventoryResource>,
    pub target_dir: Option<PathBuf>,
    pub actions: Vec<InventoryPreviewAction>,
    pub resolution: InventoryResolution,
    pub resource_roles: Vec<InventoryResourceRole>,
}
```

Add helper functions near `build_work_preview`:

```rust
// Return videos in primary-selection order while keeping deterministic path fallback.
fn sorted_videos_for_resolution<'a>(
    code: &str,
    resources: &'a [InventoryResource],
) -> Vec<&'a InventoryResource> {
    let mut videos: Vec<&InventoryResource> = resources
        .iter()
        .filter(|resource| resource.kind == InventoryResourceKind::Video)
        .collect();
    videos.sort_by_key(|resource| {
        let (video_group, part_index, path) = video_action_sort_key(code, &resource.path);
        let size_rank = std::cmp::Reverse(resource.size_bytes);
        (video_group, part_index, size_rank, path)
    });
    videos
}

// Choose the primary video and return a human-readable reason for the UI.
fn select_primary_video<'a>(
    code: &str,
    resources: &'a [InventoryResource],
) -> (Option<&'a InventoryResource>, Option<String>) {
    let videos = sorted_videos_for_resolution(code, resources);
    let Some(selected) = videos.first().copied() else {
        return (None, None);
    };
    let stem = selected
        .path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let reason = if normalize_code(&stem).as_deref() == Some(code) && whole_stem_is_single_code(&stem) {
        "推荐主视频：文件名是裸番号视频".to_string()
    } else if explicit_video_part_index(&stem) == Some(1) {
        "推荐主视频：文件名标记为第一段视频".to_string()
    } else if resources.iter().filter(|resource| resource.kind == InventoryResourceKind::Video).count() > 1 {
        "推荐主视频：未找到裸番号或第一段，选择体积最大视频".to_string()
    } else {
        "推荐主视频：唯一视频资源".to_string()
    };
    (Some(selected), Some(reason))
}

// Rank NFO files by code agreement, bare-code filename, same directory as primary video, then path.
fn nfo_resolution_sort_key(
    code: &str,
    resource: &InventoryResource,
    primary_video: Option<&InventoryResource>,
) -> (u8, u8, u8, PathBuf) {
    let nfo_matches_code = resource
        .evidence
        .iter()
        .any(|evidence| evidence.source == "nfo_num" && evidence.code == code);
    let stem = resource
        .path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let bare_stem = normalize_code(&stem).as_deref() == Some(code) && whole_stem_is_single_code(&stem);
    let same_dir = primary_video
        .and_then(|video| video.path.parent())
        .zip(resource.path.parent())
        .map(|(left, right)| left == right)
        .unwrap_or(false);
    (
        if nfo_matches_code { 0 } else { 1 },
        if bare_stem { 0 } else { 1 },
        if same_dir { 0 } else { 1 },
        resource.path.clone(),
    )
}

// Choose the primary NFO and return a human-readable reason for the UI.
fn select_primary_nfo<'a>(
    code: &str,
    resources: &'a [InventoryResource],
    primary_video: Option<&InventoryResource>,
) -> (Option<&'a InventoryResource>, Option<String>) {
    let mut nfos: Vec<&InventoryResource> = resources
        .iter()
        .filter(|resource| resource.kind == InventoryResourceKind::Nfo)
        .collect();
    nfos.sort_by_key(|resource| nfo_resolution_sort_key(code, resource, primary_video));
    let Some(selected) = nfos.first().copied() else {
        return (None, None);
    };
    let key = nfo_resolution_sort_key(code, selected, primary_video);
    let reason = if key.0 == 0 {
        "推荐主 NFO：NFO <num> 与番号一致".to_string()
    } else if key.1 == 0 {
        "推荐主 NFO：文件名是裸番号".to_string()
    } else if key.2 == 0 {
        "推荐主 NFO：与主视频位于同一目录".to_string()
    } else {
        "推荐主 NFO：按路径排序选择稳定候选".to_string()
    };
    (Some(selected), Some(reason))
}
```

- [ ] **Step 4: Build roles and attach resolution to work previews**

Still in `src-tauri/src/inventory.rs`, replace the tail of `build_work_preview` with this shape:

```rust
let (resolution, resource_roles) = build_resolution(&code, &resources, &statuses, &actions);

InventoryWorkPreview {
    code,
    statuses,
    resources,
    target_dir,
    actions,
    resolution,
    resource_roles,
}
```

Add these helpers:

```rust
// Build the first read-only pairing explanation for one code group.
fn build_resolution(
    code: &str,
    resources: &[InventoryResource],
    statuses: &[InventoryStatus],
    actions: &[InventoryPreviewAction],
) -> (InventoryResolution, Vec<InventoryResourceRole>) {
    let (primary_video, video_reason) = select_primary_video(code, resources);
    let (primary_nfo, nfo_reason) = select_primary_nfo(code, resources, primary_video);
    let mut reasons = Vec::new();
    if let Some(reason) = video_reason {
        reasons.push(reason);
    }
    if let Some(reason) = nfo_reason {
        reasons.push(reason);
    }
    let mut warnings = resolution_warnings(statuses, resources);
    let blockers = resolution_blockers(statuses, actions);
    let bucket = resolution_bucket(statuses, primary_video, primary_nfo, &warnings, &blockers);
    let confidence = resolution_confidence(&bucket, &warnings, &blockers);
    let recommended = resolution_recommendation(&bucket, primary_video, primary_nfo);
    let roles = build_resource_roles(resources, primary_video, primary_nfo);
    if primary_video.is_none() && !statuses.contains(&InventoryStatus::AssetOnly) {
        warnings.push("未找到可作为主视频的资源".to_string());
    }
    (
        InventoryResolution {
            bucket,
            primary_video: primary_video.map(|resource| resource.path.clone()),
            primary_nfo: primary_nfo.map(|resource| resource.path.clone()),
            recommended,
            reasons,
            warnings,
            blockers,
            confidence,
        },
        roles,
    )
}

// Assign each resource a role label for the frontend detail panel.
fn build_resource_roles(
    resources: &[InventoryResource],
    primary_video: Option<&InventoryResource>,
    primary_nfo: Option<&InventoryResource>,
) -> Vec<InventoryResourceRole> {
    resources
        .iter()
        .map(|resource| {
            let is_primary_video = primary_video
                .map(|selected| selected.path == resource.path)
                .unwrap_or(false);
            let is_primary_nfo = primary_nfo
                .map(|selected| selected.path == resource.path)
                .unwrap_or(false);
            let role = match resource.kind {
                InventoryResourceKind::Video if is_primary_video => InventoryResourceRoleKind::PrimaryVideo,
                InventoryResourceKind::Video => InventoryResourceRoleKind::SecondaryVideo,
                InventoryResourceKind::Nfo if is_primary_nfo => InventoryResourceRoleKind::PrimaryNfo,
                InventoryResourceKind::Nfo => InventoryResourceRoleKind::SecondaryNfo,
                InventoryResourceKind::Poster => InventoryResourceRoleKind::Poster,
                InventoryResourceKind::Fanart => InventoryResourceRoleKind::Fanart,
                InventoryResourceKind::Thumb => InventoryResourceRoleKind::Thumb,
                InventoryResourceKind::Screenshot => InventoryResourceRoleKind::Screenshot,
                InventoryResourceKind::Gif => InventoryResourceRoleKind::Gif,
                InventoryResourceKind::Image => InventoryResourceRoleKind::Image,
                InventoryResourceKind::Other => InventoryResourceRoleKind::Other,
            };
            let selected = matches!(
                role,
                InventoryResourceRoleKind::PrimaryVideo | InventoryResourceRoleKind::PrimaryNfo
            );
            let needs_review = matches!(
                role,
                InventoryResourceRoleKind::SecondaryVideo | InventoryResourceRoleKind::SecondaryNfo
            );
            InventoryResourceRole {
                path: resource.path.clone(),
                role,
                reason: resource_role_reason(resource, selected),
                selected,
                needs_review,
            }
        })
        .collect()
}

// Human-readable reason for a resource role.
fn resource_role_reason(resource: &InventoryResource, selected: bool) -> String {
    match resource.kind {
        InventoryResourceKind::Video if selected => "推荐作为主视频".to_string(),
        InventoryResourceKind::Video => "同番号额外视频，需要复核是分段、版本还是重复".to_string(),
        InventoryResourceKind::Nfo if selected => "推荐作为主 NFO".to_string(),
        InventoryResourceKind::Nfo => "同番号额外 NFO，需要复核是否保留".to_string(),
        InventoryResourceKind::Poster => "按文件名识别为 poster".to_string(),
        InventoryResourceKind::Fanart => "按文件名识别为 fanart".to_string(),
        InventoryResourceKind::Thumb => "按文件名识别为 thumb".to_string(),
        InventoryResourceKind::Screenshot => "按文件名识别为 screenshot".to_string(),
        InventoryResourceKind::Gif => "GIF 素材，随作品归组".to_string(),
        InventoryResourceKind::Image => "其他图片素材，随作品归组".to_string(),
        InventoryResourceKind::Other => "其他可识别番号资源，只做预览".to_string(),
    }
}
```

- [ ] **Step 5: Run focused tests and verify GREEN**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory inventory_resolution_selects_bare_video_and_matching_nfo_as_primary -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test inventory inventory_resolution_uses_largest_video_when_no_bare_or_first_part_exists -j 1
```

Expected: both PASS.

- [ ] **Step 6: Commit Task 1**

```powershell
git add src-tauri/src/inventory.rs src-tauri/tests/inventory.rs
git commit -m "实现阶段7B资源配对判定"
```

---

## Task 2: Review Buckets, Blockers, and Summary Counts

**Files:**
- Modify: `src-tauri/src/inventory.rs`
- Test: `src-tauri/tests/inventory.rs`

- [ ] **Step 1: Add failing bucket and blocker tests**

Append to `src-tauri/tests/inventory.rs`:

```rust
#[test]
fn inventory_resolution_buckets_clean_work_as_auto_ready() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    write_file(&root.join("IPX-170.mp4"), b"video");
    write_file(&root.join("IPX-170.nfo"), br#"<movie><num>IPX-170</num></movie>"#);

    let report = preview_inventory_roots(&[root], None).unwrap();
    let work = report.works.iter().find(|work| work.code == "IPX-170").unwrap();

    assert_eq!(report.summary.auto_ready, 1);
    assert_eq!(report.summary.needs_review, 0);
    assert_eq!(report.summary.blocked, 0);
    assert_eq!(work.resolution.bucket, media_manager::inventory::InventoryReviewBucket::AutoReady);
    assert_eq!(work.resolution.recommended, "可自动整理");
}

#[test]
fn inventory_resolution_blocks_target_conflicts_and_code_conflicts() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-171.mp4"), b"video");
    write_file(&root.join("IPX-171.nfo"), br#"<movie><num>IPX-172</num></movie>"#);
    write_file(&archive.join("IPX-171").join("IPX-171.mp4"), b"existing");

    let report = preview_inventory_roots(&[root], Some(&archive)).unwrap();
    let work = report.works.iter().find(|work| work.code == "IPX-171").unwrap();

    assert_eq!(report.summary.blocked, 1);
    assert_eq!(work.resolution.bucket, media_manager::inventory::InventoryReviewBucket::Blocked);
    assert!(work.resolution.blockers.iter().any(|blocker| blocker.contains("番号证据冲突")));
    assert!(work.resolution.blockers.iter().any(|blocker| blocker.contains("目标路径已存在")));
}

#[test]
fn inventory_resolution_keeps_asset_candidates_in_asset_bucket() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    write_file(&root.join("IPX-173-cover.jpg"), b"poster");

    let report = preview_inventory_roots(&[root], None).unwrap();
    let work = report.asset_candidates.iter().find(|work| work.code == "IPX-173").unwrap();

    assert_eq!(report.summary.asset_candidates, 1);
    assert_eq!(work.resolution.bucket, media_manager::inventory::InventoryReviewBucket::AssetCandidate);
    assert_eq!(work.resolution.recommended, "素材候选，等待人工补配视频或 NFO");
}
```

- [ ] **Step 2: Run bucket tests and verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory inventory_resolution_buckets_clean_work_as_auto_ready -j 1
```

Expected: FAIL because `InventorySummary` does not have `auto_ready`, `needs_review`, and `blocked` yet.

- [ ] **Step 3: Add summary fields and bucket logic**

In `InventorySummary`, add:

```rust
pub auto_ready: usize,
pub needs_review: usize,
pub blocked: usize,
```

In `summarize_works`, add:

```rust
for work in works.iter().chain(asset_candidates.iter()) {
    match work.resolution.bucket {
        InventoryReviewBucket::AutoReady => summary.auto_ready += 1,
        InventoryReviewBucket::NeedsReview => summary.needs_review += 1,
        InventoryReviewBucket::Blocked => summary.blocked += 1,
        InventoryReviewBucket::AssetCandidate => {}
    }
}
```

Add bucket helper functions to `src-tauri/src/inventory.rs`:

```rust
// Warnings make a work reviewable but do not always block it.
fn resolution_warnings(
    statuses: &[InventoryStatus],
    resources: &[InventoryResource],
) -> Vec<String> {
    let mut warnings = Vec::new();
    if statuses.contains(&InventoryStatus::MultiVideo) {
        warnings.push("同番号存在多个视频，需要确认是分段、版本还是重复".to_string());
    }
    if statuses.contains(&InventoryStatus::MultiNfo) {
        warnings.push("同番号存在多个 NFO，需要确认主 NFO".to_string());
    }
    if statuses.contains(&InventoryStatus::DuplicateCandidate) {
        warnings.push("同番号存在同尺寸视频，疑似重复".to_string());
    }
    if statuses.contains(&InventoryStatus::MissingNfo) {
        warnings.push("有视频但缺少 NFO，不能直接作为完整整理项".to_string());
    }
    if statuses.contains(&InventoryStatus::MissingVideo) {
        warnings.push("有 NFO 但缺少视频，不能直接整理为完整作品".to_string());
    }
    if resources.iter().any(|resource| resource.kind == InventoryResourceKind::Other) {
        warnings.push("包含其他类型资源，只做预览归组".to_string());
    }
    warnings
}

// Blockers prevent a work from being auto-ready in this read-only phase.
fn resolution_blockers(
    statuses: &[InventoryStatus],
    actions: &[InventoryPreviewAction],
) -> Vec<String> {
    let mut blockers = Vec::new();
    if statuses.contains(&InventoryStatus::CodeConflict) {
        blockers.push("番号证据冲突".to_string());
    }
    if statuses.contains(&InventoryStatus::NfoParseError) {
        blockers.push("NFO 解析失败".to_string());
    }
    if actions.iter().any(|action| conflict_tokens(action).iter().any(|token| token == "target_exists")) {
        blockers.push("目标路径已存在".to_string());
    }
    if actions.iter().any(|action| conflict_tokens(action).iter().any(|token| token == "target_duplicate")) {
        blockers.push("本次预览存在重复目标路径".to_string());
    }
    blockers
}

// Parse comma-separated preview conflict tokens emitted by Stage 7A.
fn conflict_tokens(action: &InventoryPreviewAction) -> Vec<&str> {
    action
        .conflict
        .as_deref()
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect()
}

// Assign the work to one review bucket.
fn resolution_bucket(
    statuses: &[InventoryStatus],
    primary_video: Option<&InventoryResource>,
    primary_nfo: Option<&InventoryResource>,
    warnings: &[String],
    blockers: &[String],
) -> InventoryReviewBucket {
    if statuses.contains(&InventoryStatus::AssetOnly) {
        return InventoryReviewBucket::AssetCandidate;
    }
    if !blockers.is_empty() {
        return InventoryReviewBucket::Blocked;
    }
    if primary_video.is_some()
        && primary_nfo.is_some()
        && warnings.is_empty()
        && statuses.contains(&InventoryStatus::Ready)
    {
        return InventoryReviewBucket::AutoReady;
    }
    InventoryReviewBucket::NeedsReview
}

// Convert bucket and risks into the compact confidence shown in UI.
fn resolution_confidence(
    bucket: &InventoryReviewBucket,
    warnings: &[String],
    blockers: &[String],
) -> InventoryConfidence {
    match bucket {
        InventoryReviewBucket::AutoReady => InventoryConfidence::High,
        InventoryReviewBucket::AssetCandidate => InventoryConfidence::Low,
        InventoryReviewBucket::Blocked => InventoryConfidence::Low,
        InventoryReviewBucket::NeedsReview if warnings.len() <= 1 && blockers.is_empty() => InventoryConfidence::Medium,
        InventoryReviewBucket::NeedsReview => InventoryConfidence::Low,
    }
}

// Human-readable recommendation headline for one work.
fn resolution_recommendation(
    bucket: &InventoryReviewBucket,
    primary_video: Option<&InventoryResource>,
    primary_nfo: Option<&InventoryResource>,
) -> String {
    match bucket {
        InventoryReviewBucket::AutoReady => "可自动整理".to_string(),
        InventoryReviewBucket::NeedsReview if primary_video.is_none() => "需人工确认：缺少主视频".to_string(),
        InventoryReviewBucket::NeedsReview if primary_nfo.is_none() => "需人工确认：缺少主 NFO".to_string(),
        InventoryReviewBucket::NeedsReview => "需人工确认后再整理".to_string(),
        InventoryReviewBucket::Blocked => "存在阻断，不能自动整理".to_string(),
        InventoryReviewBucket::AssetCandidate => "素材候选，等待人工补配视频或 NFO".to_string(),
    }
}
```

- [ ] **Step 4: Mark duplicate video resource roles**

Change `build_resource_roles` so same-size non-primary videos become `DuplicateVideo`:

```rust
let duplicate_video_paths = duplicate_video_paths(resources, primary_video);
let role = match resource.kind {
    InventoryResourceKind::Video if is_primary_video => InventoryResourceRoleKind::PrimaryVideo,
    InventoryResourceKind::Video if duplicate_video_paths.contains(&resource.path) => InventoryResourceRoleKind::DuplicateVideo,
    InventoryResourceKind::Video => InventoryResourceRoleKind::SecondaryVideo,
    InventoryResourceKind::Nfo if is_primary_nfo => InventoryResourceRoleKind::PrimaryNfo,
    InventoryResourceKind::Nfo => InventoryResourceRoleKind::SecondaryNfo,
    InventoryResourceKind::Poster => InventoryResourceRoleKind::Poster,
    InventoryResourceKind::Fanart => InventoryResourceRoleKind::Fanart,
    InventoryResourceKind::Thumb => InventoryResourceRoleKind::Thumb,
    InventoryResourceKind::Screenshot => InventoryResourceRoleKind::Screenshot,
    InventoryResourceKind::Gif => InventoryResourceRoleKind::Gif,
    InventoryResourceKind::Image => InventoryResourceRoleKind::Image,
    InventoryResourceKind::Other => InventoryResourceRoleKind::Other,
};
```

Add:

```rust
// Return non-primary videos whose size matches another video in the same work.
fn duplicate_video_paths(
    resources: &[InventoryResource],
    primary_video: Option<&InventoryResource>,
) -> BTreeSet<PathBuf> {
    let mut by_size: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();
    for resource in resources {
        if resource.kind == InventoryResourceKind::Video && resource.size_bytes > 0 {
            by_size.entry(resource.size_bytes).or_default().push(resource.path.clone());
        }
    }
    let primary_path = primary_video.map(|resource| resource.path.clone());
    by_size
        .into_values()
        .filter(|paths| paths.len() > 1)
        .flat_map(|paths| paths.into_iter())
        .filter(|path| Some(path) != primary_path.as_ref())
        .collect()
}
```

Update `resource_role_reason` video branch:

```rust
InventoryResourceKind::Video if selected => "推荐作为主视频".to_string(),
InventoryResourceKind::Video => "同番号额外视频，需要复核是分段、版本还是重复".to_string(),
```

After the role value is known, use:

```rust
let reason = if role == InventoryResourceRoleKind::DuplicateVideo {
    "同番号同尺寸视频，疑似重复".to_string()
} else {
    resource_role_reason(resource, selected)
};
```

- [ ] **Step 5: Run focused tests and full inventory suite**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory inventory_resolution_buckets_clean_work_as_auto_ready -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test inventory inventory_resolution_blocks_target_conflicts_and_code_conflicts -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test inventory inventory_resolution_keeps_asset_candidates_in_asset_bucket -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1
```

Expected: all PASS.

- [ ] **Step 6: Commit Task 2**

```powershell
git add src-tauri/src/inventory.rs src-tauri/tests/inventory.rs
git commit -m "补齐阶段7B复核队列统计"
```

---

## Task 3: Inventory JSON Export Command

**Files:**
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Write failing command export test**

Append this test inside `#[cfg(test)] mod tests` in `src-tauri/src/commands.rs`:

```rust
#[test]
fn inventory_export_command_writes_report_json_under_app_data() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("IPX-180.mp4"), b"video").unwrap();
    std::fs::write(root.join("IPX-180.nfo"), br#"<movie><num>IPX-180</num></movie>"#).unwrap();
    let report = preview_inventory_from_paths(vec![root], Some(archive)).unwrap().data;

    let exported = export_inventory_report_to_dir(tmp.path(), report).unwrap();

    assert!(exported.path.ends_with(".json"));
    assert!(exported.path.contains("inventory-reports"));
    assert!(exported.works >= 1);
    let text = std::fs::read_to_string(&exported.path).unwrap();
    assert!(text.contains("\"IPX-180\""));
    assert!(text.contains("\"resolution\""));
    assert!(!text.contains("video</movie>"));
}
```

- [ ] **Step 2: Run command test and verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory_export_command_writes_report_json_under_app_data -j 1
```

Expected: FAIL because `export_inventory_report_to_dir` and `InventoryExportResult` do not exist.

- [ ] **Step 3: Add export DTO and helper**

In `src-tauri/src/commands.rs`, add near other command DTOs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryExportResult {
    pub path: String,
    pub works: usize,
    pub asset_candidates: usize,
    pub orphans: usize,
}
```

Add helper functions near `preview_inventory_from_paths`:

```rust
/// Write the current inventory report JSON under the app-data inventory report directory.
fn export_inventory_report_to_dir(
    app_data_dir: &Path,
    report: InventoryPreviewReport,
) -> Result<InventoryExportResult, String> {
    let export_dir = app_data_dir.join("inventory-reports");
    fs::create_dir_all(&export_dir).map_err(|error| error.to_string())?;
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let path = export_dir.join(format!("inventory-{timestamp}.json"));
    let text = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    fs::write(&path, text).map_err(|error| error.to_string())?;
    Ok(InventoryExportResult {
        path: path.to_string_lossy().to_string(),
        works: report.summary.works,
        asset_candidates: report.summary.asset_candidates,
        orphans: report.summary.orphans,
    })
}
```

- [ ] **Step 4: Add Tauri command and register it**

Add command:

```rust
/// Export the current read-only inventory report to app data for diagnostics and review.
#[tauri::command]
pub fn export_inventory_report_command(
    app: tauri::AppHandle,
    report: InventoryPreviewReport,
) -> Result<CommandResult<InventoryExportResult>, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let result = export_inventory_report_to_dir(&app_data, report)?;
    Ok(CommandResult { data: result })
}
```

In the `generate_handler!` list, add:

```rust
export_inventory_report_command,
```

- [ ] **Step 5: Run command export tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory_export_command_writes_report_json_under_app_data -j 1
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory_preview -j 1
```

Expected: both PASS.

- [ ] **Step 6: Commit Task 3**

```powershell
git add src-tauri/src/commands.rs
git commit -m "新增阶段7B盘点结果导出"
```

---

## Task 4: Frontend Types and ViewModel Helpers

**Files:**
- Modify: `src/api.ts`
- Modify: `src/viewModel.ts`
- Modify: `src/viewModel.test.ts`

- [ ] **Step 1: Write failing viewModel tests**

In `src/viewModel.test.ts`, add imports:

```ts
  filterInventoryWorks,
  formatInventoryConfidence,
  formatInventoryExportSummary,
  formatInventoryReviewBucket,
  formatInventoryResolutionSummary,
  type InventoryFilter,
```

Append inside the existing `describe("inventory preview formatting", () => {` block after the orphan filter test:

```ts
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

    expect(filterInventoryWorks([readyWork, reviewWork], "review:auto_ready").map((work) => work.code)).toEqual(["IPX-181"]);
    expect(filterInventoryWorks([readyWork, reviewWork], "status:multi_video").map((work) => work.code)).toEqual(["IPX-182"]);
  });

  it("formats inventory export result", () => {
    expect(formatInventoryExportSummary({
      path: "C:/Users/A/AppData/Roaming/local.media-manager/inventory-reports/inventory-20260628-101010.json",
      works: 10,
      asset_candidates: 3,
      orphans: 2
    })).toBe("已导出盘点结果：C:/Users/A/AppData/Roaming/local.media-manager/inventory-reports/inventory-20260628-101010.json（作品 10，素材候选 3，孤儿 2）。");
  });
```

- [ ] **Step 2: Run frontend test and verify RED**

Run:

```powershell
npm test -- src/viewModel.test.ts
```

Expected: FAIL because the new DTO fields and helper functions do not exist.

- [ ] **Step 3: Add API DTOs and wrapper**

In `src/api.ts`, add:

```ts
export type InventoryReviewBucket = "auto_ready" | "needs_review" | "blocked" | "asset_candidate";
export type InventoryConfidence = "high" | "medium" | "low";
export type InventoryResourceRoleKind =
  | "primary_video"
  | "secondary_video"
  | "duplicate_video"
  | "primary_nfo"
  | "secondary_nfo"
  | "poster"
  | "fanart"
  | "thumb"
  | "screenshot"
  | "gif"
  | "image"
  | "other";

export interface InventoryResolution {
  bucket: InventoryReviewBucket;
  primary_video?: string | null;
  primary_nfo?: string | null;
  recommended: string;
  reasons: string[];
  warnings: string[];
  blockers: string[];
  confidence: InventoryConfidence;
}

export interface InventoryResourceRole {
  path: string;
  role: InventoryResourceRoleKind;
  reason: string;
  selected: boolean;
  needs_review: boolean;
}

export interface InventoryExportResult {
  path: string;
  works: number;
  asset_candidates: number;
  orphans: number;
}
```

Extend `InventoryWorkPreview`:

```ts
  resolution: InventoryResolution;
  resource_roles: InventoryResourceRole[];
```

Extend `InventorySummary`:

```ts
  auto_ready: number;
  needs_review: number;
  blocked: number;
```

Add API wrapper:

```ts
  exportInventoryReport(report: InventoryPreviewReport) {
    return command<InventoryExportResult>("export_inventory_report_command", { report });
  },
```

- [ ] **Step 4: Add viewModel helpers**

In `src/viewModel.ts`, import:

```ts
  InventoryConfidence,
  InventoryExportResult,
  InventoryReviewBucket,
  InventoryWorkPreview,
```

Replace `InventoryStatusFilter` with:

```ts
export type InventoryFilter =
  | "all"
  | `status:${InventoryStatus}`
  | `review:${InventoryReviewBucket}`
  | "orphan";
export type InventoryStatusFilter = InventoryFilter;
```

Add helpers:

```ts
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

/** Summarize one work's read-only pairing recommendation. */
export function formatInventoryResolutionSummary(work: InventoryWorkPreview): string {
  return `${work.resolution.recommended} · 置信度 ${formatInventoryConfidence(work.resolution.confidence)}`;
}

/** Filter inventory work previews by Stage 7B review bucket or existing Stage 7A status. */
export function filterInventoryWorks(works: InventoryWorkPreview[], filter: InventoryFilter): InventoryWorkPreview[] {
  if (filter === "all" || filter === "orphan") {
    return works;
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
```

Update `inventoryOrphansForFilter`:

```ts
  return filter === "orphan" ? report.orphans : [];
```

Update `formatInventorySummary` expected shape:

```ts
return `识别 ${s.works} 部作品，素材候选 ${s.asset_candidates} 组：可自动整理 ${s.auto_ready}，需人工确认 ${s.needs_review}，阻断 ${s.blocked}，缺 NFO ${s.missing_nfo}，缺视频 ${s.missing_video}，冲突 ${s.code_conflict}，孤儿 ${s.orphans}。${suffix}`;
```

- [ ] **Step 5: Update existing test fixtures**

In `src/viewModel.test.ts`, every inline `InventoryPreviewReport.summary` fixture must include:

```ts
auto_ready: 0,
needs_review: 0,
blocked: 0,
```

Every inline `InventoryWorkPreview` fixture must include `resolution` and `resource_roles` as shown in Step 1.

- [ ] **Step 6: Run frontend tests and TypeScript**

Run:

```powershell
npm test -- src/viewModel.test.ts
npx tsc --noEmit
```

Expected: both PASS.

- [ ] **Step 7: Commit Task 4**

```powershell
git add src/api.ts src/viewModel.ts src/viewModel.test.ts
git commit -m "接入阶段7B前端复核模型"
```

---

## Task 5: Inventory Page UI for Target Root, Review Details, and Export

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/styles.css`

- [ ] **Step 1: Add UI state and import helpers**

In `src/App.tsx`, update imports from `viewModel`:

```ts
  filterInventoryWorks,
  formatInventoryExportSummary,
  formatInventoryResolutionSummary,
  formatInventoryReviewBucket,
  type InventoryFilter,
```

Change inventory filter state:

```ts
const [inventoryStatusFilter, setInventoryStatusFilter] = useState<InventoryFilter>("all");
const [inventoryArchiveRoot, setInventoryArchiveRoot] = useState("");
const [inventoryExportBusy, setInventoryExportBusy] = useState(false);
```

When `archiveRoot` loads or changes, set a default without overwriting user input:

```ts
useEffect(() => {
  setInventoryArchiveRoot((current) => (current.trim() ? current : archiveRoot));
}, [archiveRoot]);
```

- [ ] **Step 2: Replace filtered works logic**

Replace current `filteredInventoryWorks` with:

```ts
const inventoryWorksForFilter = inventoryReport
  ? inventoryStatusFilter === "review:asset_candidate"
    ? inventoryReport.asset_candidates
    : inventoryReport.works
  : [];
const filteredInventoryWorks = filterInventoryWorks(inventoryWorksForFilter, inventoryStatusFilter);
```

Keep:

```ts
const visibleInventoryOrphans = inventoryOrphansForFilter(inventoryReport, inventoryStatusFilter);
```

- [ ] **Step 3: Add target picker and export handlers**

Add next to `pickInventoryRoots`:

```ts
/** 通过系统目录选择器设置本次盘点整理目标目录，普通浏览器不可用时保留手动输入路径。 */
async function pickInventoryArchiveRoot() {
  if (!hasBackend) return;
  try {
    const picked = await open({ directory: true, multiple: false });
    const nextPath = Array.isArray(picked) ? picked[0] : picked;
    if (!nextPath) return;
    setInventoryArchiveRoot(nextPath);
    setActiveView("inventory");
    setStatus("已设置本次盘点整理目标目录。");
  } catch (error) {
    setStatus(`选择整理目标目录失败：${String(error)}`);
  }
}

/** 导出当前盘点报告为 JSON，便于真实环境反馈和后续分析。 */
async function exportInventoryReport() {
  if (!inventoryReport || inventoryExportBusy) return;
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
```

In `generateInventoryPreview`, replace:

```ts
const report = await api.previewInventory(roots, null);
```

with:

```ts
const targetRoot = inventoryArchiveRoot.trim() || null;
const report = await api.previewInventory(roots, targetRoot);
```

- [ ] **Step 4: Update filter definitions**

Replace `inventoryStatusFilters` with:

```ts
const inventoryFilters: Array<{ value: InventoryFilter; label: string }> = [
  { value: "all", label: "全部" },
  { value: "review:auto_ready", label: "可自动整理" },
  { value: "review:needs_review", label: "需人工确认" },
  { value: "review:blocked", label: "阻断" },
  { value: "status:multi_video", label: "多视频" },
  { value: "status:multi_nfo", label: "多 NFO" },
  { value: "review:asset_candidate", label: "素材候选" },
  { value: "status:code_conflict", label: "番号冲突" },
  { value: "status:duplicate_candidate", label: "疑似重复" },
  { value: "orphan", label: "孤儿资源" }
];
```

Update the filter rendering:

```tsx
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
```

- [ ] **Step 5: Add target input and export button markup**

In the inventory panel, under the roots field, add:

```tsx
<label className="inventory-roots-field">
  整理目标目录
  <input
    value={inventoryArchiveRoot}
    onChange={(event) => setInventoryArchiveRoot(event.target.value)}
    placeholder="例如 D:\\media-archive"
    disabled={inventoryBusy}
  />
</label>
```

In the button row, add:

```tsx
<button type="button" onClick={pickInventoryArchiveRoot} disabled={inventoryBusy || !hasBackend}>
  <FolderOpen size={16} /> 选择目标
</button>
<button type="button" onClick={exportInventoryReport} disabled={!inventoryReport || inventoryExportBusy || !hasBackend}>
  <Settings size={16} /> {inventoryExportBusy ? "导出中" : "导出 JSON"}
</button>
```

Update summary cards to include:

```tsx
<div>
  <span>可自动整理</span>
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
```

- [ ] **Step 6: Add resolution detail panel**

Inside selected work detail, before the resources subsection, add:

```tsx
<div className={`inventory-resolution inventory-resolution-${selectedInventoryWork.resolution.bucket}`}>
  <div className="inventory-section-head">
    <strong>{formatInventoryReviewBucket(selectedInventoryWork.resolution.bucket)}</strong>
    <span>{formatInventoryResolutionSummary(selectedInventoryWork)}</span>
  </div>
  <div className="inventory-resolution-grid">
    <div>
      <span>主视频</span>
      <strong>{selectedInventoryWork.resolution.primary_video ?? "未选择"}</strong>
    </div>
    <div>
      <span>主 NFO</span>
      <strong>{selectedInventoryWork.resolution.primary_nfo ?? "未选择"}</strong>
    </div>
  </div>
  {selectedInventoryWork.resolution.reasons.length > 0 ? (
    <div className="inventory-resolution-list">
      <strong>判定理由</strong>
      {selectedInventoryWork.resolution.reasons.map((reason, index) => (
        <span key={`${selectedInventoryWork.code}-reason-${index}`}>{reason}</span>
      ))}
    </div>
  ) : null}
  {selectedInventoryWork.resolution.warnings.length > 0 ? (
    <div className="inventory-resolution-list warn">
      <strong>风险提示</strong>
      {selectedInventoryWork.resolution.warnings.map((warning, index) => (
        <span key={`${selectedInventoryWork.code}-warning-${index}`}>{warning}</span>
      ))}
    </div>
  ) : null}
  {selectedInventoryWork.resolution.blockers.length > 0 ? (
    <div className="inventory-resolution-list block">
      <strong>阻断原因</strong>
      {selectedInventoryWork.resolution.blockers.map((blocker, index) => (
        <span key={`${selectedInventoryWork.code}-blocker-${index}`}>{blocker}</span>
      ))}
    </div>
  ) : null}
</div>
```

In each resource row, find the matching role:

```tsx
const role = selectedInventoryWork.resource_roles.find((candidate) => candidate.path === resource.path);
```

Then render:

```tsx
{role ? <small>{role.role} · {role.reason}</small> : null}
```

- [ ] **Step 7: Add CSS**

In `src/styles.css`, add:

```css
.inventory-roots-field input {
  min-height: 34px;
  border: 1px solid #d8e1e7;
  border-radius: 6px;
  background: #ffffff;
  color: #25313c;
  padding: 7px 9px;
  font: inherit;
}

.inventory-resolution {
  margin: 10px 12px;
  border: 1px solid #d8e1e7;
  border-radius: 6px;
  background: #ffffff;
  display: grid;
  gap: 8px;
  overflow: hidden;
}

.inventory-resolution-auto_ready {
  border-color: rgba(22, 163, 74, 0.4);
}

.inventory-resolution-needs_review {
  border-color: rgba(217, 119, 6, 0.45);
}

.inventory-resolution-blocked {
  border-color: rgba(190, 18, 60, 0.45);
}

.inventory-resolution-asset_candidate {
  border-color: rgba(37, 99, 235, 0.35);
}

.inventory-resolution-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
  padding: 0 12px;
}

.inventory-resolution-grid div,
.inventory-resolution-list {
  border: 1px solid #edf2f5;
  border-radius: 6px;
  background: #fbfdfe;
  padding: 8px 10px;
  display: grid;
  gap: 4px;
  min-width: 0;
}

.inventory-resolution-grid span,
.inventory-resolution-list span {
  color: #61717d;
  font-size: 12px;
  overflow-wrap: anywhere;
}

.inventory-resolution-grid strong,
.inventory-resolution-list strong {
  color: #25313c;
  overflow-wrap: anywhere;
}

.inventory-resolution-list.warn {
  background: #fffaf2;
}

.inventory-resolution-list.block {
  background: #fff5f7;
}

@media (max-width: 860px) {
  .inventory-resolution-grid {
    grid-template-columns: 1fr;
  }
}
```

- [ ] **Step 8: Run frontend verification**

Run:

```powershell
npm test -- src/viewModel.test.ts
npx tsc --noEmit
npm run build
```

Expected: all PASS.

- [ ] **Step 9: Commit Task 5**

```powershell
git add src/App.tsx src/styles.css
git commit -m "完善阶段7B盘点复核界面"
```

---

## Task 6: Full Verification, Review, and Handoff

**Files:**
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/progress.md`
- Modify: `.ai_state/lessons.md`
- Create: `.ai_state/reviews/sprint-15.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Run focused backend suites**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory -j 1
```

Expected: PASS. If the command filter does not match all inventory command tests, run the individual inventory preview command tests and `commands::tests::inventory_export_command_writes_report_json_under_app_data` by name.

- [ ] **Step 2: Run full frontend verification**

```powershell
npm test
npx tsc --noEmit
npm run build
```

Expected: PASS.

- [ ] **Step 3: Run full Rust gate when the environment allows**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
```

Expected: PASS. If the current Windows session hits `os error 1455` / rlib mmap pagefile exhaustion before tests execute, record it as an environment failure and keep the focused Rust inventory/command test evidence.

- [ ] **Step 4: Self-review the diff**

Run:

```powershell
git diff --stat
git diff -- src-tauri/src/inventory.rs src-tauri/src/commands.rs src/api.ts src/viewModel.ts src/App.tsx src/styles.css
```

Check:

- `preview_inventory_roots` still does not move, copy, delete, rename, or create media target dirs.
- No SQLite inventory task persistence was added.
- `export_inventory_report_command` only writes JSON under app data and never writes media/NFO/image contents.
- The inventory page passes `inventoryArchiveRoot` to `api.previewInventory`.
- All new Rust public types/functions have useful comments.
- Chinese source files were edited through `apply_patch` or node:fs, not PowerShell string replacement.

- [ ] **Step 5: Request independent review**

Use the requesting-code-review flow or a reviewer subagent if available. Record findings in `.ai_state/reviews/sprint-15.md` with this structure:

```markdown
# Sprint 15 Review - 阶段 7B 配对规则与人工复核预览

## Scope

- Inventory pairing resolution and resource roles.
- Review bucket summary and filters.
- JSON export command.
- Inventory page target root, resolution detail, and export UI.

## Verification

Each verification line must contain the exact command outcome from this run. Use concrete wording such as `PASS 18/18`, `PASS exit 0`, or `BLOCKED by os error 1455 before tests executed`.

## Findings

- PASS/CONCERN entries from self-review and external review.

## Residual Risk

- Report details remain capped at 1000 works, 1000 asset candidates, and 1000 orphan resources.
- JSON export includes local filesystem paths by design.
- 7B still does not persist manual pairing decisions; execution remains 7C.
```

- [ ] **Step 6: Update lessons**

Append to `.ai_state/lessons.md`:

```markdown
## [2026-06-28 Sprint 15] 盘点结果要先解释再执行

- **Pattern**: 存量整理在真实执行前增加 `InventoryResolution` 和 `InventoryResourceRole`，把“主视频/主 NFO/阻断原因/风险提示”放进 DTO；这样 UI 和导出 JSON 使用同一套判定，不会出现页面解释和后端执行预览不一致。
- **Pitfall**: 盘点页如果继续传 `archiveRoot=null`，用户会误以为在当前页面设置了目标目录；Stage 7B 必须把本次整理目标目录作为 preview 参数传入。
- **Constraint**: JSON 导出用于诊断和真实反馈，只能写 report JSON，不能复制媒体、NFO 或图片内容；报告包含本地路径，跨机器分享前需用户判断是否匿名化。
```

- [ ] **Step 7: Update handoff**

Update `HANDOFF.md` with:

```markdown
## Current Status

- Stage 7B completed on branch `codex/stage7b-pairing-rules`.
- Inventory preview remains read-only.
- New pairing fields: `resolution`, `resource_roles`, `summary.auto_ready`, `summary.needs_review`, `summary.blocked`.
- New export command: `export_inventory_report_command`.

## How To Verify

- Do not run Tauri GUI in Codex.
- Run focused Rust and frontend tests listed in Sprint 15 review.
- User can run the desktop app in their own terminal and test:
  1. Open 一键盘点.
  2. Fill entrada roots and 整理目标目录.
  3. Start scan.
  4. Check 可自动整理/需人工确认/阻断 filters.
  5. Open a work detail and inspect 配对判定.
  6. Export JSON and share the report path if analysis is needed.

## Remaining Work

- 7C: real archive execution with move/copy strategy, confirmation, logs, and failure recovery.
- Future: persisted manual pairing decisions and paginated full inventory reports.
```

Keep any existing handoff sections that remain accurate.

- [ ] **Step 8: Final verification before completion claim**

Run:

```powershell
git status --short --branch
git log --oneline -5
```

Confirm:

- Working tree contains only expected untracked or gitignored `.ai_state` changes.
- Implementation commits are present on `codex/stage7b-pairing-rules`.

- [ ] **Step 9: Commit handoff docs**

```powershell
git add HANDOFF.md
git commit -m "更新阶段7B交接说明"
```

`.ai_state` remains gitignored and should not be staged.

---

## Self-Review Checklist For This Plan

- Spec coverage:
  - Pairing DTOs and primary selection: Task 1.
  - Review buckets, blockers, and summary counts: Task 2.
  - JSON export: Task 3.
  - Frontend DTOs and filters: Task 4.
  - Inventory page target root, resolution details, and export button: Task 5.
  - Verification, review, lessons, and handoff: Task 6.
- Scope kept to read-only preview, explanation, filters, and JSON export.
- Real file movement, SQLite inventory tasks, manual decision persistence, remote scraping, and pagination are out of scope.
