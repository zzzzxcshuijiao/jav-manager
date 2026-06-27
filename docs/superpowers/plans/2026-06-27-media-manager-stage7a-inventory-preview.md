# Stage 7A Inventory Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a read-only inventory preview that scans scattered existing videos, NFOs, images, and GIFs across multiple roots, groups them by code, and previews a per-code archive layout without touching files.

**Architecture:** Add a focused Rust `inventory` module for DTOs, recursive scanning, code evidence, aggregation, status tags, and target-path preview. Wire it through a Tauri command, then expose a compact “存量整理预览” panel in the existing settings area with loading/status feedback and frontend formatting helpers. Stage 7A does not write SQLite and does not execute file moves.

**Tech Stack:** Rust, `walkdir`, existing `identifier` and `nfo` parsers, Tauri commands, React, TypeScript, Vitest.

---

## File Map

- Create `src-tauri/src/inventory.rs`
  - DTOs: resource kind, resource evidence, work status, preview action, summary, report.
  - Recursive read-only scanner over arbitrary roots.
  - Code extraction from NFO, file stem, parent directory, and image suffix cleanup.
  - Aggregation by code, orphan/warning collection, target-path preview.
- Modify `src-tauri/src/lib.rs`
  - Export `inventory`.
- Create `src-tauri/tests/inventory.rs`
  - Backend TDD coverage for mixed-resource scanning, conflict tagging, missing root handling, target path preview, and truncation.
- Modify `src-tauri/src/commands.rs`
  - Add `preview_inventory` command and a helper that can use the current archive root when the caller passes `None`.
  - Register command in `generate_handler!`.
- Modify `src/api.ts`
  - Add inventory DTOs and API wrapper.
- Modify `src/viewModel.ts`
  - Add inventory status/summary/action formatting helpers.
- Modify `src/viewModel.test.ts`
  - Add inventory formatting tests.
- Modify `src/App.tsx`
  - Add inventory preview state, root text input, scan button, summary cards, filters, work list/detail.
- Modify `src/styles.css`
  - Add inventory preview panel/list/status styles.
- Modify `.ai_state/tasks.md`, `.ai_state/progress.md`, `.ai_state/reviews/sprint-14.md`, `.ai_state/lessons.md`, `HANDOFF.md`
  - Keep state and handoff current at completion.

---

## Task 1: Backend DTOs and Read-Only Scanner

**Files:**
- Create: `src-tauri/src/inventory.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/inventory.rs`

- [ ] **Step 1: Write failing mixed-resource scan test**

Create `src-tauri/tests/inventory.rs`:

```rust
use media_manager::inventory::{
    preview_inventory_roots, InventoryResourceKind, InventoryStatus,
};
use std::path::{Path, PathBuf};

fn write_file(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn inventory_preview_groups_scattered_resources_by_code_without_writing_files() {
    let tmp = tempfile::tempdir().unwrap();
    let videos = tmp.path().join("videos");
    let metadata = tmp.path().join("metadata");
    let images = tmp.path().join("images");
    let archive = tmp.path().join("archive");
    write_file(&videos.join("IPX-159.mp4"), b"video");
    write_file(
        &metadata.join("renamed-info.nfo"),
        br#"<movie><num>IPX-159</num><title>Inventory</title></movie>"#,
    );
    write_file(&images.join("IPX-159-poster.jpg"), b"poster");
    write_file(&images.join("IPX-159-fanart.webp"), b"fanart");
    write_file(&images.join("IPX-159-shot-01.png"), b"shot");
    write_file(&images.join("IPX-159.gif"), b"gif");

    let report = preview_inventory_roots(
        &[videos.clone(), metadata.clone(), images.clone()],
        Some(&archive),
    )
    .unwrap();

    assert_eq!(report.summary.total_files, 6);
    assert_eq!(report.summary.works, 1);
    assert_eq!(report.summary.ready, 1);
    assert!(report.orphans.is_empty());
    let work = report.works.iter().find(|work| work.code == "IPX-159").unwrap();
    assert!(work.statuses.contains(&InventoryStatus::Ready));
    assert_eq!(work.resources.iter().filter(|r| r.kind == InventoryResourceKind::Video).count(), 1);
    assert_eq!(work.resources.iter().filter(|r| r.kind == InventoryResourceKind::Nfo).count(), 1);
    assert_eq!(work.resources.iter().filter(|r| r.kind == InventoryResourceKind::Poster).count(), 1);
    assert_eq!(work.resources.iter().filter(|r| r.kind == InventoryResourceKind::Fanart).count(), 1);
    assert_eq!(work.resources.iter().filter(|r| r.kind == InventoryResourceKind::Screenshot).count(), 1);
    assert_eq!(work.resources.iter().filter(|r| r.kind == InventoryResourceKind::Gif).count(), 1);
    assert!(work.target_dir.as_ref().unwrap().ends_with(PathBuf::from("IPX-159")));
    assert!(!archive.exists(), "stage 7A preview must not create archive directories");
}
```

- [ ] **Step 2: Run focused test and verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory inventory_preview_groups_scattered_resources_by_code_without_writing_files -j 1
```

Expected: FAIL because `media_manager::inventory` does not exist.

- [ ] **Step 3: Implement DTOs and scanner skeleton**

Add `pub mod inventory;` to `src-tauri/src/lib.rs`.

Create `src-tauri/src/inventory.rs` with these public DTOs. Every new public type/function must keep the comments:

```rust
use crate::identifier::normalize_code;
use crate::nfo::parse_nfo_document;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const INVENTORY_DETAIL_LIMIT: usize = 1000;
const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts"];
const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// Resource categories surfaced to the frontend inventory preview.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryResourceKind {
    Video,
    Nfo,
    Poster,
    Fanart,
    Thumb,
    Screenshot,
    Gif,
    Image,
    Other,
}

/// Evidence that explains where a normalized code came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryCodeEvidence {
    pub source: String,
    pub code: String,
    pub value: String,
}

/// One scanned file that may belong to an inventory work preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryResource {
    pub path: PathBuf,
    pub file_name: String,
    pub kind: InventoryResourceKind,
    pub size_bytes: u64,
    pub code: Option<String>,
    pub evidence: Vec<InventoryCodeEvidence>,
    pub warnings: Vec<String>,
}

/// Work-level status tags used for filtering and risk review.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryStatus {
    Ready,
    MissingNfo,
    MissingVideo,
    MultiVideo,
    MultiNfo,
    CodeConflict,
    DuplicateCandidate,
    NfoParseError,
    Orphan,
}

/// A planned target path. Stage 7A describes actions but never executes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryPreviewAction {
    pub from_path: PathBuf,
    pub to_path: Option<PathBuf>,
    pub kind: InventoryResourceKind,
    pub conflict: Option<String>,
}

/// One code-grouped preview entry in the inventory report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryWorkPreview {
    pub code: String,
    pub statuses: Vec<InventoryStatus>,
    pub resources: Vec<InventoryResource>,
    pub target_dir: Option<PathBuf>,
    pub actions: Vec<InventoryPreviewAction>,
}

/// Aggregate counts for a read-only inventory preview.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventorySummary {
    pub total_files: usize,
    pub works: usize,
    pub ready: usize,
    pub missing_nfo: usize,
    pub missing_video: usize,
    pub multi_video: usize,
    pub multi_nfo: usize,
    pub code_conflict: usize,
    pub duplicate_candidate: usize,
    pub orphans: usize,
}

/// Read-only inventory preview report returned by the command layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryPreviewReport {
    pub generated_at: String,
    pub roots: Vec<PathBuf>,
    pub archive_root: Option<PathBuf>,
    pub summary: InventorySummary,
    pub works: Vec<InventoryWorkPreview>,
    pub orphans: Vec<InventoryResource>,
    pub warnings: Vec<String>,
    pub truncated: bool,
}
```

Add this public entrypoint and keep it read-only:

```rust
/// Scan arbitrary roots and return a code-grouped inventory preview without writing files.
pub fn preview_inventory_roots(
    roots: &[PathBuf],
    archive_root: Option<&Path>,
) -> Result<InventoryPreviewReport> {
    let mut warnings = Vec::new();
    let mut resources = Vec::new();
    for root in roots {
        if !root.exists() {
            warnings.push(format!("扫描根目录不存在：{}", root.to_string_lossy()));
            continue;
        }
        for entry in WalkDir::new(root).follow_links(false) {
            let Ok(entry) = entry else {
                warnings.push(format!("读取目录项失败：{}", root.to_string_lossy()));
                continue;
            };
            let path = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            resources.push(classify_resource(path)?);
        }
    }
    Ok(build_report(roots, archive_root, resources, warnings))
}
```

Add private helpers with these exact responsibilities:

- `classify_resource(path: &Path) -> Result<InventoryResource>`:
  - Reads metadata length with `fs::metadata`.
  - Classifies `.nfo` as `Nfo`, video extensions as `Video`, `.gif` as `Gif`, image extensions by `image_kind_from_stem`, everything else as `Other`.
  - Adds code evidence from file stem and parent directory.
  - For NFO, reads text, parses `parse_nfo_document`, and pushes `source = "nfo_num"` when `source_code` normalizes.
  - If NFO read/parse fails, keeps the resource and adds warning `"NFO 解析失败"`.
  - Sets `code` to NFO code if present, otherwise file-stem code, otherwise parent-dir code.
- `image_kind_from_stem(stem: &str) -> InventoryResourceKind`:
  - Poster for suffixes `poster`, `cover`, `ps`, or bare code.
  - Fanart for `fanart`, `background`, `pl`.
  - Thumb for `thumb`, `thumbnail`.
  - Screenshot for `shot`, `screenshot`, `sample`, or `-01` / `_01` style suffix.
  - Image otherwise.
- `build_report(roots: &[PathBuf], archive_root: Option<&Path>, resources: Vec<InventoryResource>, warnings: Vec<String>) -> InventoryPreviewReport`:
  - Groups resources with `code.is_some()` into a `BTreeMap<String, Vec<InventoryResource>>`.
  - Puts resources without `code` into `orphans`.
  - Builds `InventoryWorkPreview` using `build_work_preview`.
  - Computes summary with `summarize_works`.
  - Truncates `works` to `INVENTORY_DETAIL_LIMIT` after summary and sets `truncated`.

- [ ] **Step 4: Run focused test and verify GREEN**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory inventory_preview_groups_scattered_resources_by_code_without_writing_files -j 1
```

Expected: PASS.

- [ ] **Step 5: Commit Task 1**

```powershell
git add src-tauri/src/lib.rs src-tauri/src/inventory.rs src-tauri/tests/inventory.rs
git commit -m "实现阶段7A存量扫描核心"
```

---

## Task 2: Status Tags, Target Paths, and Edge Cases

**Files:**
- Modify: `src-tauri/src/inventory.rs`
- Test: `src-tauri/tests/inventory.rs`

- [ ] **Step 1: Add failing status and target-path tests**

Append to `src-tauri/tests/inventory.rs`:

```rust
#[test]
fn inventory_preview_marks_missing_and_conflict_states() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("ABP-001.mp4"), b"video");
    write_file(&root.join("SSIS-777.nfo"), br#"<movie><num>SSIS-777</num></movie>"#);
    write_file(&root.join("IPX-159.mp4"), b"v1");
    write_file(&root.join("IPX-159-CD2.mkv"), b"v2");
    write_file(&root.join("IPX-159.nfo"), br#"<movie><num>IPX-160</num></movie>"#);

    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();

    let abp = report.works.iter().find(|work| work.code == "ABP-001").unwrap();
    assert!(abp.statuses.contains(&InventoryStatus::MissingNfo));
    let ssis = report.works.iter().find(|work| work.code == "SSIS-777").unwrap();
    assert!(ssis.statuses.contains(&InventoryStatus::MissingVideo));
    let ipx159 = report.works.iter().find(|work| work.code == "IPX-159").unwrap();
    assert!(ipx159.statuses.contains(&InventoryStatus::MultiVideo));
    let ipx160 = report.works.iter().find(|work| work.code == "IPX-160").unwrap();
    assert!(ipx160.statuses.contains(&InventoryStatus::CodeConflict));
}

#[test]
fn inventory_preview_builds_target_actions_and_marks_existing_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    write_file(&root.join("IPX-200.mp4"), b"video");
    write_file(&root.join("IPX-200.nfo"), br#"<movie><num>IPX-200</num></movie>"#);
    write_file(&root.join("IPX-200-cover.jpg"), b"poster");
    write_file(&archive.join("IPX-200").join("IPX-200.mp4"), b"existing");

    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();

    let work = report.works.iter().find(|work| work.code == "IPX-200").unwrap();
    let video_action = work
        .actions
        .iter()
        .find(|action| action.kind == InventoryResourceKind::Video)
        .unwrap();
    assert!(video_action.to_path.as_ref().unwrap().ends_with(PathBuf::from("IPX-200/IPX-200.mp4")));
    assert_eq!(video_action.conflict.as_deref(), Some("target_exists"));
    let nfo_action = work
        .actions
        .iter()
        .find(|action| action.kind == InventoryResourceKind::Nfo)
        .unwrap();
    assert!(nfo_action.to_path.as_ref().unwrap().ends_with(PathBuf::from("IPX-200/IPX-200.nfo")));
    let poster_action = work
        .actions
        .iter()
        .find(|action| action.kind == InventoryResourceKind::Poster)
        .unwrap();
    assert!(poster_action.to_path.as_ref().unwrap().ends_with(PathBuf::from("IPX-200/poster.jpg")));
}

#[test]
fn inventory_preview_keeps_missing_roots_as_warnings() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing");

    let report = preview_inventory_roots(&[missing.clone()], None).unwrap();

    assert_eq!(report.summary.total_files, 0);
    assert!(report.warnings.iter().any(|warning| warning.contains("扫描根目录不存在")));
}
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1
```

Expected: FAIL until status/action logic is complete.

- [ ] **Step 3: Implement work preview status and actions**

In `src-tauri/src/inventory.rs`, add these helpers:

```rust
fn build_work_preview(
    code: String,
    resources: Vec<InventoryResource>,
    archive_root: Option<&Path>,
) -> InventoryWorkPreview {
    let target_dir = archive_root.map(|root| root.join(&code));
    let statuses = work_statuses(&resources);
    let actions = resources
        .iter()
        .enumerate()
        .map(|(index, resource)| preview_action(&code, resource, index, target_dir.as_deref()))
        .collect();
    InventoryWorkPreview {
        code,
        statuses,
        resources,
        target_dir,
        actions,
    }
}

fn work_statuses(resources: &[InventoryResource]) -> Vec<InventoryStatus> {
    let video_count = resources.iter().filter(|r| r.kind == InventoryResourceKind::Video).count();
    let nfo_count = resources.iter().filter(|r| r.kind == InventoryResourceKind::Nfo).count();
    let mut statuses = BTreeSet::new();
    if video_count > 0 && nfo_count > 0 {
        statuses.insert(InventoryStatus::Ready);
    }
    if video_count > 0 && nfo_count == 0 {
        statuses.insert(InventoryStatus::MissingNfo);
    }
    if video_count == 0 {
        statuses.insert(InventoryStatus::MissingVideo);
    }
    if video_count > 1 {
        statuses.insert(InventoryStatus::MultiVideo);
    }
    if nfo_count > 1 {
        statuses.insert(InventoryStatus::MultiNfo);
    }
    if resources.iter().any(resource_has_conflicting_evidence) {
        statuses.insert(InventoryStatus::CodeConflict);
    }
    if resources
        .iter()
        .any(|resource| resource.warnings.iter().any(|warning| warning.contains("NFO 解析失败")))
    {
        statuses.insert(InventoryStatus::NfoParseError);
    }
    statuses.into_iter().collect()
}
```

Implement:

- `resource_has_conflicting_evidence(resource)`:
  - Collects unique `evidence.code`.
  - Returns true when there is more than one unique code.
- `preview_action(code, resource, index, target_dir)`:
  - Returns `to_path = None` when no archive root exists.
  - For video: first video gets `CODE.ext`; subsequent video resources get `CODE-v{index + 1}.ext`.
  - For NFO: `CODE.nfo`.
  - For poster/fanart/thumb: `poster.ext`, `fanart.ext`, `thumb.ext`.
  - For screenshot: `screenshots/<original-name>`.
  - For gif: `gifs/<original-name>`.
  - For image/other: `images/<original-name>`.
  - Sets `conflict = Some("target_exists")` when `to_path.exists()`.
- `summarize_works(works, orphans, total_files)`:
  - Counts each status once per work.
  - `orphans = orphans.len()`.

- [ ] **Step 4: Run inventory suite and verify GREEN**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1
```

Expected: PASS.

- [ ] **Step 5: Commit Task 2**

```powershell
git add src-tauri/src/inventory.rs src-tauri/tests/inventory.rs
git commit -m "完善阶段7A整理预览状态"
```

---

## Task 3: Tauri Command Bridge

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Test: `src-tauri/src/commands.rs` unit tests

- [ ] **Step 1: Write failing command helper test**

Inside `#[cfg(test)] mod tests` in `src-tauri/src/commands.rs`, add:

```rust
#[test]
fn inventory_preview_command_uses_state_archive_root_when_argument_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState::default();
    let root = tmp.path().join("inventory-root");
    let archive = tmp.path().join("archive");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("IPX-301.mp4"), b"video").unwrap();
    *state.archive_root.lock().unwrap() = Some(archive.to_string_lossy().to_string());

    let response = preview_inventory_in_state(
        vec![root.to_string_lossy().to_string()],
        None,
        &state,
    )
    .unwrap();

    let work = response.data.works.iter().find(|work| work.code == "IPX-301").unwrap();
    assert!(work.target_dir.as_ref().unwrap().ends_with(std::path::PathBuf::from("IPX-301")));
    assert!(!archive.exists(), "inventory preview must not create target dirs");
}
```

- [ ] **Step 2: Run command helper test and verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory_preview_command_uses_state_archive_root_when_argument_is_missing -j 1
```

Expected: FAIL because `preview_inventory_in_state` does not exist.

- [ ] **Step 3: Implement command helper and command**

In `src-tauri/src/commands.rs`, import:

```rust
use crate::inventory::{preview_inventory_roots, InventoryPreviewReport};
```

Add helper near the self-check helper:

```rust
/// Build a read-only inventory preview from plain `AppState` for tests and the Tauri command.
fn preview_inventory_in_state(
    roots: Vec<String>,
    archive_root: Option<String>,
    state: &AppState,
) -> Result<CommandResult<InventoryPreviewReport>, String> {
    let root_paths: Vec<PathBuf> = roots
        .into_iter()
        .map(|root| PathBuf::from(root.trim()))
        .filter(|root| !root.as_os_str().is_empty())
        .collect();
    if root_paths.is_empty() {
        return Err("至少需要一个存量扫描根目录".to_string());
    }
    let archive_root = match archive_root {
        Some(value) if !value.trim().is_empty() => Some(PathBuf::from(value.trim())),
        _ => state
            .archive_root
            .lock()
            .map_err(|error| error.to_string())?
            .as_ref()
            .map(PathBuf::from),
    };
    let report = preview_inventory_roots(&root_paths, archive_root.as_deref())
        .map_err(|error| error.to_string())?;
    Ok(CommandResult { data: report })
}
```

Add Tauri command:

```rust
#[tauri::command]
pub fn preview_inventory(
    roots: Vec<String>,
    archive_root: Option<String>,
    state: State<'_, AppState>,
) -> Result<CommandResult<InventoryPreviewReport>, String> {
    preview_inventory_in_state(roots, archive_root, &state)
}
```

Register `preview_inventory` in `tauri::generate_handler!`.

- [ ] **Step 4: Run focused command test and inventory tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory_preview_command_uses_state_archive_root_when_argument_is_missing -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1
```

Expected: PASS.

- [ ] **Step 5: Commit Task 3**

```powershell
git add src-tauri/src/commands.rs
git commit -m "接入阶段7A存量预览命令"
```

---

## Task 4: Frontend DTOs and Formatting

**Files:**
- Modify: `src/api.ts`
- Modify: `src/viewModel.ts`
- Modify: `src/viewModel.test.ts`

- [ ] **Step 1: Write failing frontend formatting tests**

Append to `src/viewModel.test.ts`:

```ts
describe("inventory preview formatting", () => {
  it("formats inventory statuses and summaries", () => {
    const report = {
      generated_at: "2026-06-27T12:00:00Z",
      roots: ["H:/downloads"],
      archive_root: "H:/AV",
      summary: {
        total_files: 8,
        works: 3,
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
      orphans: [],
      warnings: [],
      truncated: false
    };

    expect(formatInventoryStatus("ready")).toBe("可整理");
    expect(formatInventoryStatus("missing_nfo")).toBe("缺 NFO");
    expect(formatInventorySummary(report)).toBe("识别 3 部作品：可整理 1，缺 NFO 1，缺视频 1，冲突 1，孤儿 2。");
  });

  it("formats inventory action targets", () => {
    expect(formatInventoryActionTarget({ from_path: "H:/x/IPX-001.mp4", to_path: "H:/AV/IPX-001/IPX-001.mp4", kind: "video", conflict: null })).toBe("H:/AV/IPX-001/IPX-001.mp4");
    expect(formatInventoryActionTarget({ from_path: "H:/x/IPX-001.mp4", to_path: null, kind: "video", conflict: null })).toBe("未配置归档根目录");
  });
});
```

Update the import list in `src/viewModel.test.ts` to include:

```ts
formatInventoryActionTarget,
formatInventoryStatus,
formatInventorySummary,
```

- [ ] **Step 2: Run frontend test and verify RED**

Run:

```powershell
npm test -- src/viewModel.test.ts
```

Expected: FAIL because inventory types/helpers do not exist.

- [ ] **Step 3: Add API DTOs and wrapper**

In `src/api.ts`, add near existing `ResourcePool` types:

```ts
export type InventoryResourceKind = "video" | "nfo" | "poster" | "fanart" | "thumb" | "screenshot" | "gif" | "image" | "other";
export type InventoryStatus =
  | "ready"
  | "missing_nfo"
  | "missing_video"
  | "multi_video"
  | "multi_nfo"
  | "code_conflict"
  | "duplicate_candidate"
  | "nfo_parse_error"
  | "orphan";

export interface InventoryCodeEvidence {
  source: string;
  code: string;
  value: string;
}

export interface InventoryResource {
  path: string;
  file_name: string;
  kind: InventoryResourceKind;
  size_bytes: number;
  code?: string | null;
  evidence: InventoryCodeEvidence[];
  warnings: string[];
}

export interface InventoryPreviewAction {
  from_path: string;
  to_path?: string | null;
  kind: InventoryResourceKind;
  conflict?: string | null;
}

export interface InventoryWorkPreview {
  code: string;
  statuses: InventoryStatus[];
  resources: InventoryResource[];
  target_dir?: string | null;
  actions: InventoryPreviewAction[];
}

export interface InventorySummary {
  total_files: number;
  works: number;
  ready: number;
  missing_nfo: number;
  missing_video: number;
  multi_video: number;
  multi_nfo: number;
  code_conflict: number;
  duplicate_candidate: number;
  orphans: number;
}

export interface InventoryPreviewReport {
  generated_at: string;
  roots: string[];
  archive_root?: string | null;
  summary: InventorySummary;
  works: InventoryWorkPreview[];
  orphans: InventoryResource[];
  warnings: string[];
  truncated: boolean;
}
```

Add API wrapper:

```ts
previewInventory(roots: string[], archiveRoot?: string | null) {
  return command<InventoryPreviewReport>("preview_inventory", { roots, archiveRoot });
},
```

- [ ] **Step 4: Add viewModel helpers**

In `src/viewModel.ts`, import inventory types and add:

```ts
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
    orphan: "孤儿资源"
  };
  return labels[status];
}

export function formatInventorySummary(report: InventoryPreviewReport): string {
  const s = report.summary;
  const suffix = report.truncated ? " 结果过多，仅展示前 1000 部。" : "";
  return `识别 ${s.works} 部作品：可整理 ${s.ready}，缺 NFO ${s.missing_nfo}，缺视频 ${s.missing_video}，冲突 ${s.code_conflict}，孤儿 ${s.orphans}。${suffix}`;
}

export function formatInventoryActionTarget(action: InventoryPreviewAction): string {
  if (!action.to_path) {
    return "未配置归档根目录";
  }
  if (action.conflict === "target_exists") {
    return `${action.to_path}（目标已存在）`;
  }
  return action.to_path;
}
```

- [ ] **Step 5: Run frontend focused test and TypeScript**

Run:

```powershell
npm test -- src/viewModel.test.ts
npx tsc --noEmit
```

Expected: PASS.

- [ ] **Step 6: Commit Task 4**

```powershell
git add src/api.ts src/viewModel.ts src/viewModel.test.ts
git commit -m "接入阶段7A前端预览模型"
```

---

## Task 5: Frontend Inventory Preview Panel

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/styles.css`
- Test: `npx tsc --noEmit`, `npm test`

- [ ] **Step 1: Add UI state and handler**

In `src/App.tsx`, extend imports:

```ts
import type { InventoryPreviewReport, InventoryStatus } from "./api";
```

Add viewModel imports:

```ts
formatInventoryActionTarget,
formatInventoryStatus,
formatInventorySummary,
```

Add state near existing settings/daemon state:

```ts
const [inventoryRootsText, setInventoryRootsText] = useState("");
const [inventoryBusy, setInventoryBusy] = useState(false);
const [inventoryReport, setInventoryReport] = useState<InventoryPreviewReport | null>(null);
const [inventoryStatusFilter, setInventoryStatusFilter] = useState<InventoryStatus | "all">("all");
const [selectedInventoryCode, setSelectedInventoryCode] = useState<string | null>(null);
```

Add helper functions inside `App`:

```ts
function inventoryRootsFromText(): string[] {
  return inventoryRootsText
    .split(/\r?\n/)
    .map((root) => root.trim())
    .filter(Boolean);
}

async function generateInventoryPreview() {
  if (inventoryBusy) return;
  const roots = inventoryRootsFromText();
  if (roots.length === 0) {
    setStatus("请先填写至少一个存量扫描根目录。");
    return;
  }
  setInventoryBusy(true);
  setStatus("正在扫描存量资源...");
  try {
    const report = await api.previewInventory(roots, daemonStatus?.archive_root ?? null);
    setInventoryReport(report);
    setSelectedInventoryCode(report.works[0]?.code ?? null);
    setStatus(formatInventorySummary(report));
  } catch (error) {
    setStatus(`存量整理预览失败：${String(error)}`);
  } finally {
    setInventoryBusy(false);
  }
}
```

Add derived values before JSX return:

```ts
const filteredInventoryWorks = inventoryReport
  ? inventoryReport.works.filter((work) =>
      inventoryStatusFilter === "all" ? true : work.statuses.includes(inventoryStatusFilter)
    )
  : [];
const selectedInventoryWork =
  inventoryReport?.works.find((work) => work.code === selectedInventoryCode) ??
  filteredInventoryWorks[0] ??
  null;
```

- [ ] **Step 2: Add panel markup**

In the settings `auto` tab, place this panel before the diagnostics panel or directly after self-check:

```tsx
<div className="inventory-panel">
  <div className="daemon-list-head">
    <strong>存量整理预览</strong>
    <span>预览，不会移动文件</span>
  </div>
  <label>
    存量扫描根目录
    <textarea
      value={inventoryRootsText}
      onChange={(event) => setInventoryRootsText(event.target.value)}
      placeholder={"每行一个目录，例如：\nH:\\downloads\nG:\\JAV_output"}
      rows={4}
    />
  </label>
  <div className="daemon-actions">
    <button type="button" onClick={generateInventoryPreview} disabled={inventoryBusy || !hasBackend}>
      <RefreshCw size={16} /> {inventoryBusy ? "扫描中" : "生成整理预览"}
    </button>
  </div>
  {inventoryReport ? (
    <>
      <div className="inventory-summary">
        <div><span>识别作品</span><strong>{inventoryReport.summary.works}</strong></div>
        <div><span>可整理</span><strong>{inventoryReport.summary.ready}</strong></div>
        <div><span>缺 NFO</span><strong>{inventoryReport.summary.missing_nfo}</strong></div>
        <div><span>缺视频</span><strong>{inventoryReport.summary.missing_video}</strong></div>
        <div><span>冲突</span><strong>{inventoryReport.summary.code_conflict}</strong></div>
        <div><span>孤儿</span><strong>{inventoryReport.summary.orphans}</strong></div>
      </div>
      <div className="inventory-filter">
        {(["all", "ready", "missing_nfo", "missing_video", "multi_video", "code_conflict"] as const).map((filter) => (
          <button
            type="button"
            className={inventoryStatusFilter === filter ? "active" : ""}
            onClick={() => setInventoryStatusFilter(filter)}
            key={filter}
          >
            {filter === "all" ? "全部" : formatInventoryStatus(filter)}
          </button>
        ))}
      </div>
      {inventoryReport.warnings.length > 0 ? (
        <div className="rebuild-report">
          {inventoryReport.warnings.slice(0, 5).map((warning) => <span key={warning}>{warning}</span>)}
        </div>
      ) : null}
      <div className="inventory-layout">
        <div className="inventory-work-list">
          {filteredInventoryWorks.length === 0 ? (
            <span className="empty-text">没有匹配当前过滤条件的作品</span>
          ) : (
            filteredInventoryWorks.slice(0, 50).map((work) => (
              <button
                type="button"
                className={selectedInventoryWork?.code === work.code ? "inventory-work-row active" : "inventory-work-row"}
                onClick={() => setSelectedInventoryCode(work.code)}
                key={work.code}
              >
                <strong>{work.code}</strong>
                <span>{work.statuses.map(formatInventoryStatus).join(" / ")}</span>
                <small>{work.resources.length} 个资源</small>
              </button>
            ))
          )}
        </div>
        {selectedInventoryWork ? (
          <div className="inventory-detail">
            <div className="daemon-list-head">
              <strong>{selectedInventoryWork.code}</strong>
              <span>{selectedInventoryWork.target_dir ?? "未配置归档根目录"}</span>
            </div>
            <div className="inventory-resource-list">
              {selectedInventoryWork.resources.map((resource) => (
                <div className="inventory-resource-row" key={resource.path}>
                  <strong>{resource.kind}</strong>
                  <span>{resource.path}</span>
                  {resource.warnings.length > 0 ? <small>{resource.warnings.join("；")}</small> : null}
                </div>
              ))}
            </div>
            <div className="inventory-action-list">
              {selectedInventoryWork.actions.map((action) => (
                <div className={action.conflict ? "inventory-action-row warn" : "inventory-action-row"} key={`${action.from_path}-${action.kind}`}>
                  <strong>{action.kind}</strong>
                  <span>{formatInventoryActionTarget(action)}</span>
                </div>
              ))}
            </div>
          </div>
        ) : null}
      </div>
    </>
  ) : null}
</div>
```

Do not add visible text that suggests execution. Keep all copy explicit that this is preview-only.

- [ ] **Step 3: Add styles**

In `src/styles.css`, add:

```css
.inventory-panel {
  display: grid;
  gap: 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 12px;
}

.inventory-panel textarea {
  width: 100%;
  resize: vertical;
  min-height: 92px;
}

.inventory-summary {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(92px, 1fr));
  gap: 8px;
}

.inventory-summary > div {
  display: grid;
  gap: 4px;
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 8px;
}

.inventory-filter {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.inventory-filter button.active,
.inventory-work-row.active {
  border-color: var(--accent);
}

.inventory-layout {
  display: grid;
  grid-template-columns: minmax(220px, 0.8fr) minmax(0, 1.2fr);
  gap: 12px;
}

.inventory-work-list,
.inventory-detail,
.inventory-resource-list,
.inventory-action-list {
  display: grid;
  gap: 8px;
}

.inventory-work-row {
  display: grid;
  gap: 4px;
  text-align: left;
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 8px;
}

.inventory-resource-row,
.inventory-action-row {
  display: grid;
  gap: 4px;
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 8px;
  overflow-wrap: anywhere;
}

.inventory-action-row.warn {
  border-color: rgba(245, 158, 11, 0.65);
}

@media (max-width: 860px) {
  .inventory-layout {
    grid-template-columns: 1fr;
  }
}
```

If the CSS variable names differ, use existing neighboring panel variables from `.self-check-panel` and `.daemon-list-row` instead of inventing a new palette.

- [ ] **Step 4: Run frontend verification**

Run:

```powershell
npm test
npx tsc --noEmit
npm run build
```

Expected: PASS.

- [ ] **Step 5: Commit Task 5**

```powershell
git add src/App.tsx src/styles.css
git commit -m "接入阶段7A存量整理预览面板"
```

---

## Task 6: Full Verification, Review, and Handoff

**Files:**
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/progress.md`
- Modify: `.ai_state/lessons.md`
- Create: `.ai_state/reviews/sprint-14.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Run focused backend suites**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory_preview_command_uses_state_archive_root_when_argument_is_missing -j 1
```

Expected: PASS.

- [ ] **Step 2: Run full Rust tests**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
```

Expected: PASS. Historical `resource_pool.rs` warnings may remain.

- [ ] **Step 3: Run frontend tests and builds**

```powershell
npm test
npx tsc --noEmit
npm run build
```

Expected: PASS.

- [ ] **Step 4: Self-review diff**

Review:

```powershell
git diff --stat
git diff -- src-tauri/src/inventory.rs src-tauri/src/commands.rs src/api.ts src/viewModel.ts src/App.tsx
```

Check:

- No write calls in `inventory.rs` except test helpers.
- No rename/move/copy/delete execution path in Stage 7A.
- No SQLite writes for inventory preview.
- Missing roots become warnings, not hard failures.
- Target path preview marks existing targets and does not create directories.
- Every new Rust type/function has a useful comment.
- Chinese source was edited only via `apply_patch` or node:fs, not PowerShell string replacement.

- [ ] **Step 5: Write review record**

Create `.ai_state/reviews/sprint-14.md`:

```markdown
# Sprint 14 Review - 阶段 7A 存量资源盘点与整理预览

## Scope

- Read-only inventory scanner.
- Code-grouped resource aggregation.
- Per-code archive target preview.
- Tauri command and frontend preview panel.

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1`: PASS.
- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`: PASS.
- `npm test`: PASS.
- `npx tsc --noEmit`: PASS.
- `npm run build`: PASS.

## Findings

- PASS: Stage 7A does not move, copy, delete, rename, or write SQLite inventory rows.
- PASS: Missing roots and per-file failures are warnings, not scan-wide failures.
- PASS: Target paths are preview-only and existing targets are marked.
- PASS: Frontend marks the feature as preview-only.

## Residual Risk

- No persistence or pagination; report details are capped at 1000 works.
- Multi-video and multi-NFO decisions are classified but not resolved until 7B.
- Duplicate candidates are heuristic only; full fingerprint-based duplicate handling remains future work.
```

- [ ] **Step 6: Update handoff and lessons**

Append to `.ai_state/lessons.md`:

```markdown
## [2026-06-27 Sprint 14] 存量整理先做只读盘点

- **Pattern**: 对散乱存量资源，先用只读 inventory preview 聚合视频/NFO/图片/GIF，再讨论移动执行；这样可以把风险前置到可解释预览里。
- **Pitfall**: NFO-first rebuild 会漏掉“有视频但无 NFO”的真实存量痛点；Stage 7A 必须允许视频先定义候选作品，再用 NFO/图片补齐。
- **Constraint**: 7A 不写 SQLite、不创建目录、不移动文件；执行整理必须留到带二次确认和日志的后续阶段。
```

Update `HANDOFF.md` with:

- Stage 7A completion summary.
- New file list.
- Verification commands.
- Remaining 7B/7C/7D boundaries.

- [ ] **Step 7: Commit handoff docs**

```powershell
git add HANDOFF.md
git commit -m "更新阶段7A交接说明"
```

- [ ] **Step 8: Push branch**

```powershell
git push origin codex/stage2-auto-pipeline
```

Expected: push succeeds.
