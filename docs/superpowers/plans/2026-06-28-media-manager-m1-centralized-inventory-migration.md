# M1 Centralized Inventory Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a real centralized inventory migration mode that moves scattered video/NFO/artwork resources into `archive_root/CODE/`, removes successfully migrated source files, and protects the user from overwrite and low-space failures.

**Architecture:** Keep the existing 7B.1/7C/7D pipeline shape: inventory preview creates `resolution.execution_plan.actions`, and inventory execution consumes only that safe plan. Add a new `move` mode with a small file-operation module for no-clobber same-volume moves, cross-volume copy-verify-delete, disk-space checks, and work-scoped rollback. The frontend switches the one-click inventory primary action from low-space hardlinking to centralized migration while keeping `copy` and `low_space` available in the API for compatibility.

**Tech Stack:** Rust 2021, Tauri command bridge, serde, sha2, fs2, tempfile, React 18, TypeScript, Vitest/happy-dom.

---

## File Structure

- Modify `src-tauri/Cargo.toml`
  - Add `fs2 = "0.4"` for production free-space queries.
- Modify `src-tauri/src/lib.rs`
  - Export the new inventory move helper module.
- Create `src-tauri/src/inventory_move.rs`
  - Own file-level move primitives: move method selection, space provider abstraction, same-volume no-clobber hardlink+unlink move, cross-volume copy-verify-delete, and work-scoped rollback helpers.
- Modify `src-tauri/src/inventory_execution.rs`
  - Add `InventoryExecutionMode::Move`.
  - Add `moved` / `rollback_failed` statuses and migration counters.
  - Group prepared actions by work code so failure rolls back only the current work.
  - Route `move` actions through `inventory_move`.
- Modify `src-tauri/tests/inventory_execution.rs`
  - Add integration tests for same-volume move, forced cross-volume copy-verify-delete, target no-overwrite, low-space stop, current-work rollback, and preserving completed previous works.
- Modify `src/api.ts`
  - Extend frontend types for mode, status, and report counters.
- Modify `src/viewModel.ts`
  - Format centralized migration summaries.
- Modify `src/viewModel.test.ts`
  - Cover the new `move` summary.
- Modify `src/App.tsx`
  - Change the one-click inventory primary action to `move`.
  - Update confirmation, loading, result title, log status labels, and truncation warning text.
- Modify `src/App.inventory.test.tsx`
  - Change the App wiring test from `low_space` to `move`.
  - Add confirmation text assertions and moved-log rendering assertions.
- Modify `HANDOFF.md`
  - Mark the M1 plan as written and point the next agent to implementation.

---

### Task 1: Frontend Report Types And Summary Formatter

**Files:**
- Modify: `src/api.ts`
- Modify: `src/viewModel.ts`
- Test: `src/viewModel.test.ts`

- [ ] **Step 1: Add the failing frontend summary test**

Append this case to the existing `formatInventoryExecutionSummary` describe block in `src/viewModel.test.ts`:

```ts
expect(formatInventoryExecutionSummary({
  mode: "move",
  started_at: "2026-06-28T12:00:00Z",
  finished_at: "2026-06-28T12:01:00Z",
  requested_works: 4,
  executed_works: 3,
  skipped_works: 1,
  planned_actions: 12,
  linked_actions: 0,
  copied_actions: 0,
  moved_actions: 10,
  failed_actions: 1,
  rolled_back_actions: 2,
  rollback_failed_actions: 0,
  same_volume_actions: 7,
  cross_volume_actions: 3,
  space_blocked_actions: 0,
  bytes_linked: 0,
  bytes_copied: 0,
  bytes_moved: 9_000_000_000,
  logs: []
})).toBe("集中迁移完成：作品 3/4，迁移 10/12，失败 1，回滚 2，同盘 7，跨盘 3，迁移 8.38 GB。");
```

- [ ] **Step 2: Run the focused frontend test and verify it fails**

Run:

```powershell
npm test -- src/viewModel.test.ts
```

Expected: fails because `"move"` and the new report counters are not part of `InventoryExecutionReport`.

- [ ] **Step 3: Extend the frontend API types**

In `src/api.ts`, replace the current inventory execution type block with:

```ts
export type InventoryExecutionMode = "copy" | "low_space" | "move";
export type InventoryExecutionActionStatus = "linked" | "copied" | "moved" | "failed" | "rolled_back" | "rollback_failed";

export interface InventoryExecutionActionLog {
  code: string;
  kind: InventoryResourceKind;
  from_path: string;
  to_path: string;
  status: InventoryExecutionActionStatus;
  message?: string | null;
  bytes: number;
}

export interface InventoryExecutionReport {
  mode: InventoryExecutionMode;
  started_at: string;
  finished_at: string;
  requested_works: number;
  executed_works: number;
  skipped_works: number;
  planned_actions: number;
  linked_actions: number;
  copied_actions: number;
  moved_actions: number;
  failed_actions: number;
  rolled_back_actions: number;
  rollback_failed_actions: number;
  same_volume_actions: number;
  cross_volume_actions: number;
  space_blocked_actions: number;
  bytes_linked: number;
  bytes_copied: number;
  bytes_moved: number;
  logs: InventoryExecutionActionLog[];
}
```

- [ ] **Step 4: Format centralized migration reports**

In `src/viewModel.ts`, update `formatInventoryExecutionSummary` to:

```ts
/** Format the inventory execution report for status-line feedback. */
export function formatInventoryExecutionSummary(report: InventoryExecutionReport): string {
  if (report.mode === "move") {
    return `集中迁移完成：作品 ${report.executed_works}/${report.requested_works}，迁移 ${report.moved_actions}/${report.planned_actions}，失败 ${report.failed_actions}，回滚 ${report.rolled_back_actions}，同盘 ${report.same_volume_actions}，跨盘 ${report.cross_volume_actions}，迁移 ${formatBytes(report.bytes_moved)}。`;
  }
  if (report.mode === "low_space") {
    return `低空间整理完成：作品 ${report.executed_works}/${report.requested_works}，硬链接 ${report.linked_actions}，复制 ${report.copied_actions}，失败 ${report.failed_actions}，回滚 ${report.rolled_back_actions}，链接视频 ${formatBytes(report.bytes_linked)}，复制小文件 ${formatBytes(report.bytes_copied)}。`;
  }
  return `复制整理完成：作品 ${report.executed_works}/${report.requested_works}，动作 ${report.copied_actions}/${report.planned_actions}，失败 ${report.failed_actions}，回滚 ${report.rolled_back_actions}，复制 ${formatBytes(report.bytes_copied)}。`;
}
```

- [ ] **Step 5: Run the focused frontend test and verify it passes**

Run:

```powershell
npm test -- src/viewModel.test.ts
```

Expected: PASS for `src/viewModel.test.ts`.

- [ ] **Step 6: Commit Task 1**

Run:

```powershell
git add src/api.ts src/viewModel.ts src/viewModel.test.ts
git commit -m "扩展集中迁移前端报告类型"
```

---

### Task 2: Rust Execution Model And Report Shape

**Files:**
- Modify: `src-tauri/src/inventory_execution.rs`
- Test: `src-tauri/tests/inventory_execution.rs`

- [ ] **Step 1: Add a failing same-volume move test**

In `src-tauri/tests/inventory_execution.rs`, add this helper below `low_space_all_request()`:

```rust
/// Build a centralized move request for integration tests.
fn move_all_request() -> InventoryExecutionRequest {
    InventoryExecutionRequest {
        mode: InventoryExecutionMode::Move,
        selected_codes: Vec::new(),
    }
}
```

Then add this test:

```rust
#[test]
fn inventory_move_execution_moves_sources_into_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    let video = root.join("IPX-601.mp4");
    let nfo = root.join("IPX-601.nfo");
    let poster = root.join("IPX-601-cover.jpg");
    write_file(&video, b"video");
    write_file(
        &nfo,
        br#"<movie><num>IPX-601</num><title>Ready</title></movie>"#,
    );
    write_file(&poster, b"poster");

    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();
    let execution = execute_inventory_report(&report, &move_all_request()).unwrap();

    assert_eq!(execution.mode, InventoryExecutionMode::Move);
    assert_eq!(execution.executed_works, 1);
    assert_eq!(execution.moved_actions, 3);
    assert_eq!(execution.same_volume_actions, 3);
    assert_eq!(execution.cross_volume_actions, 0);
    assert_eq!(execution.failed_actions, 0);
    assert_eq!(execution.bytes_moved, 5 + 53 + 6);
    assert!(!video.exists(), "move mode must remove the source video");
    assert!(!nfo.exists(), "move mode must remove the source NFO");
    assert!(!poster.exists(), "move mode must remove the selected poster");
    assert_eq!(fs::read(archive.join("IPX-601").join("IPX-601.mp4")).unwrap(), b"video");
    assert_eq!(
        fs::read(archive.join("IPX-601").join("IPX-601.nfo")).unwrap(),
        br#"<movie><num>IPX-601</num><title>Ready</title></movie>"#
    );
    assert_eq!(fs::read(archive.join("IPX-601").join("poster.jpg")).unwrap(), b"poster");
    assert!(execution
        .logs
        .iter()
        .all(|log| log.status == InventoryExecutionActionStatus::Moved));
}
```

- [ ] **Step 2: Run the focused Rust test and verify it fails**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution inventory_move_execution_moves_sources_into_archive -j 1
```

Expected: compile failure because `InventoryExecutionMode::Move`, `InventoryExecutionActionStatus::Moved`, and move counters do not exist.

- [ ] **Step 3: Extend Rust enums and report fields**

In `src-tauri/src/inventory_execution.rs`, update the enums and report struct to:

```rust
/// File operation mode supported by the inventory execution stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryExecutionMode {
    Copy,
    LowSpace,
    Move,
}

/// Per-action result status returned to the UI after an inventory execution run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryExecutionActionStatus {
    Linked,
    Copied,
    Moved,
    Failed,
    RolledBack,
    RollbackFailed,
}

/// Summary of an inventory execution run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryExecutionReport {
    pub mode: InventoryExecutionMode,
    pub started_at: String,
    pub finished_at: String,
    pub requested_works: usize,
    pub executed_works: usize,
    pub skipped_works: usize,
    pub planned_actions: usize,
    pub linked_actions: usize,
    pub copied_actions: usize,
    pub moved_actions: usize,
    pub failed_actions: usize,
    pub rolled_back_actions: usize,
    pub rollback_failed_actions: usize,
    pub same_volume_actions: usize,
    pub cross_volume_actions: usize,
    pub space_blocked_actions: usize,
    pub bytes_linked: u64,
    pub bytes_copied: u64,
    pub bytes_moved: u64,
    pub logs: Vec<InventoryExecutionActionLog>,
}
```

In `prepare_inventory_execution`, update the mode match to:

```rust
match request.mode {
    InventoryExecutionMode::Copy | InventoryExecutionMode::LowSpace | InventoryExecutionMode::Move => {}
}
```

In `execute_inventory_report`, initialize the new counters to zero and include them in the returned report. For this task only, route `Move` through the existing copy path so the test compiles but still fails on source-preservation assertions:

```rust
let mut moved_actions = 0;
let mut rollback_failed_actions = 0;
let mut same_volume_actions = 0;
let mut cross_volume_actions = 0;
let mut space_blocked_actions = 0;
let mut bytes_moved = 0;
```

Return those fields in `InventoryExecutionReport`.

- [ ] **Step 4: Run the focused Rust test and verify it fails at runtime**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution inventory_move_execution_moves_sources_into_archive -j 1
```

Expected: test compiles and fails because sources still exist or `moved_actions` remains zero.

- [ ] **Step 5: Commit Task 2**

Run:

```powershell
git add src-tauri/src/inventory_execution.rs src-tauri/tests/inventory_execution.rs
git commit -m "扩展集中迁移执行报告模型"
```

---

### Task 3: Same-Volume No-Copy Move

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/inventory_move.rs`
- Modify: `src-tauri/src/inventory_execution.rs`
- Test: `src-tauri/tests/inventory_execution.rs`

- [ ] **Step 1: Add the move helper module shell**

In `src-tauri/Cargo.toml`, add the dependency:

```toml
fs2 = "0.4"
```

In `src-tauri/src/lib.rs`, add:

```rust
pub mod inventory_move;
```

Create `src-tauri/src/inventory_move.rs` with:

```rust
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

pub const CROSS_VOLUME_SPACE_MARGIN_BYTES: u64 = 512 * 1024 * 1024;

/// Strategy object used by inventory migration to classify moves and query target space.
pub trait InventoryMoveStrategy {
    /// Returns true when a source-target pair should use the same-volume no-copy path.
    fn is_same_volume(&self, from_path: &Path, to_path: &Path) -> Result<bool>;

    /// Returns available bytes for the volume containing the requested path.
    fn available_space(&self, path: &Path) -> Result<u64>;
}

/// Production move strategy backed by filesystem metadata and fs2 space queries.
#[derive(Debug, Default)]
pub struct SystemInventoryMoveStrategy;

impl InventoryMoveStrategy for SystemInventoryMoveStrategy {
    fn is_same_volume(&self, from_path: &Path, to_path: &Path) -> Result<bool> {
        same_volume_by_platform(from_path, to_path)
    }

    fn available_space(&self, path: &Path) -> Result<u64> {
        fs2::available_space(path)
            .with_context(|| format!("查询目标磁盘剩余空间失败：{}", path.to_string_lossy()))
    }
}

/// File-level method used by a successful centralized move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryMoveMethod {
    SameVolume,
    CrossVolume,
}

/// Result of one moved file, including enough identity data for current-work rollback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryMovedFile {
    pub from_path: PathBuf,
    pub to_path: PathBuf,
    pub method: InventoryMoveMethod,
    pub bytes: u64,
    pub sha256: Option<String>,
}

/// Move one file into its target without overwriting an existing file.
pub fn move_file_no_clobber(
    from_path: &Path,
    to_path: &Path,
    strategy: &dyn InventoryMoveStrategy,
    run_id: &str,
    index: usize,
) -> Result<InventoryMovedFile> {
    let parent = to_path
        .parent()
        .ok_or_else(|| anyhow!("目标路径没有父目录：{}", to_path.to_string_lossy()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("创建目标目录失败：{}", parent.to_string_lossy()))?;
    if to_path.exists() {
        bail!("目标路径已存在：{}", to_path.to_string_lossy());
    }
    let source_size = fs::metadata(from_path)
        .with_context(|| format!("读取源文件失败：{}", from_path.to_string_lossy()))?
        .len();
    if strategy.is_same_volume(from_path, to_path)? {
        move_same_volume_no_copy(from_path, to_path, source_size)?;
        return Ok(InventoryMovedFile {
            from_path: from_path.to_path_buf(),
            to_path: to_path.to_path_buf(),
            method: InventoryMoveMethod::SameVolume,
            bytes: source_size,
            sha256: None,
        });
    }
    move_cross_volume_copy_verify_delete(from_path, to_path, source_size, strategy, run_id, index)
}

/// Move within one volume by creating a no-clobber hard link at the target and removing the source path.
pub fn move_same_volume_no_copy(from_path: &Path, to_path: &Path, expected_size: u64) -> Result<()> {
    fs::hard_link(from_path, to_path).with_context(|| {
        format!(
            "同盘迁移创建目标失败：{} -> {}",
            from_path.to_string_lossy(),
            to_path.to_string_lossy()
        )
    })?;
    let target_size = fs::metadata(to_path)
        .with_context(|| format!("读取迁移目标失败：{}", to_path.to_string_lossy()))?
        .len();
    if target_size != expected_size {
        let _ = fs::remove_file(to_path);
        bail!("同盘迁移后大小校验失败：{}", to_path.to_string_lossy());
    }
    fs::remove_file(from_path)
        .with_context(|| format!("删除源文件失败：{}", from_path.to_string_lossy()))?;
    if from_path.exists() {
        bail!("源文件删除后仍存在：{}", from_path.to_string_lossy());
    }
    Ok(())
}

fn same_volume_by_platform(from_path: &Path, to_path: &Path) -> Result<bool> {
    #[cfg(windows)]
    {
        Ok(windows_drive_key(from_path) == windows_drive_key(to_path))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let from_dev = fs::metadata(from_path)
            .with_context(|| format!("读取源文件失败：{}", from_path.to_string_lossy()))?
            .dev();
        let parent = to_path
            .parent()
            .ok_or_else(|| anyhow!("目标路径没有父目录：{}", to_path.to_string_lossy()))?;
        let mut candidate = parent.to_path_buf();
        while !candidate.exists() {
            if !candidate.pop() {
                bail!("目标父目录无法校验：{}", parent.to_string_lossy());
            }
        }
        let target_dev = fs::metadata(&candidate)
            .with_context(|| format!("读取目标目录失败：{}", candidate.to_string_lossy()))?
            .dev();
        Ok(from_dev == target_dev)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = from_path;
        let _ = to_path;
        Ok(false)
    }
}

#[cfg(windows)]
fn windows_drive_key(path: &Path) -> Option<String> {
    use std::path::{Component, Prefix};
    match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                Some((letter as char).to_ascii_uppercase().to_string())
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                Some(format!("{}\\{}", server.to_string_lossy().to_ascii_lowercase(), share.to_string_lossy().to_ascii_lowercase()))
            }
            _ => None,
        },
        _ => None,
    }
}
```

- [ ] **Step 2: Keep cross-volume as an explicit failure for now**

Append this function to `src-tauri/src/inventory_move.rs` so Task 3 stays focused:

```rust
fn move_cross_volume_copy_verify_delete(
    from_path: &Path,
    to_path: &Path,
    source_size: u64,
    strategy: &dyn InventoryMoveStrategy,
    run_id: &str,
    index: usize,
) -> Result<InventoryMovedFile> {
    let _ = from_path;
    let _ = to_path;
    let _ = source_size;
    let _ = strategy;
    let _ = run_id;
    let _ = index;
    bail!("跨盘集中迁移尚未启用");
}
```

- [ ] **Step 3: Route `move` mode through the helper**

In `src-tauri/src/inventory_execution.rs`, add imports:

```rust
use crate::inventory_move::{
    move_file_no_clobber, InventoryMoveMethod, InventoryMoveStrategy, SystemInventoryMoveStrategy,
};
```

Add this public options struct below `InventoryExecutionRequest`:

```rust
/// Optional execution dependencies used by tests to force cross-volume and low-space branches.
pub struct InventoryExecutionOptions<'a> {
    pub move_strategy: &'a dyn InventoryMoveStrategy,
}

impl<'a> InventoryExecutionOptions<'a> {
    /// Build production execution options backed by the real filesystem.
    pub fn system() -> Self {
        static SYSTEM_MOVE_STRATEGY: SystemInventoryMoveStrategy = SystemInventoryMoveStrategy;
        Self {
            move_strategy: &SYSTEM_MOVE_STRATEGY,
        }
    }
}
```

Change `execute_inventory_report` into a wrapper:

```rust
/// Execute a preview report's safe inventory plan.
pub fn execute_inventory_report(
    report: &InventoryPreviewReport,
    request: &InventoryExecutionRequest,
) -> Result<InventoryExecutionReport> {
    execute_inventory_report_with_options(report, request, &InventoryExecutionOptions::system())
}

/// Execute a preview report's safe inventory plan with injectable filesystem options.
pub fn execute_inventory_report_with_options(
    report: &InventoryPreviewReport,
    request: &InventoryExecutionRequest,
    options: &InventoryExecutionOptions<'_>,
) -> Result<InventoryExecutionReport> {
    let prepared = prepare_inventory_execution(report, request)?;
    let started_at = Utc::now().to_rfc3339();
    let run_id = format!("{}-{}", std::process::id(), Utc::now().timestamp_micros());
    let mut logs = Vec::new();
    let mut created_targets = Vec::new();
    let mut successful_counts_by_code: BTreeMap<String, usize> = BTreeMap::new();
    let mut planned_counts_by_code: BTreeMap<String, usize> = BTreeMap::new();
    let mut linked_actions = 0;
    let mut copied_actions = 0;
    let mut moved_actions = 0;
    let mut failed_actions = 0;
    let mut rolled_back_actions = 0;
    let mut rollback_failed_actions = 0;
    let mut same_volume_actions = 0;
    let mut cross_volume_actions = 0;
    let mut space_blocked_actions = 0;
    let mut bytes_linked = 0;
    let mut bytes_copied = 0;
    let mut bytes_moved = 0;

    for action in &prepared.actions {
        *planned_counts_by_code
            .entry(action.code.clone())
            .or_default() += 1;
    }

    for (index, action) in prepared.actions.iter().enumerate() {
        match execute_prepared_action(
            action,
            request.mode,
            &prepared.archive_root_canonical,
            &run_id,
            index,
            options,
        ) {
            Ok(created) => {
                let status = match created.operation {
                    CreatedInventoryOperation::Copied => {
                        copied_actions += 1;
                        bytes_copied += created.bytes;
                        InventoryExecutionActionStatus::Copied
                    }
                    CreatedInventoryOperation::Linked => {
                        linked_actions += 1;
                        bytes_linked += created.bytes;
                        InventoryExecutionActionStatus::Linked
                    }
                    CreatedInventoryOperation::Moved(method) => {
                        moved_actions += 1;
                        bytes_moved += created.bytes;
                        match method {
                            InventoryMoveMethod::SameVolume => same_volume_actions += 1,
                            InventoryMoveMethod::CrossVolume => cross_volume_actions += 1,
                        }
                        InventoryExecutionActionStatus::Moved
                    }
                };
                created_targets.push(created.clone());
                *successful_counts_by_code.entry(action.code.clone()).or_default() += 1;
                logs.push(InventoryExecutionActionLog {
                    code: action.code.clone(),
                    kind: action.kind.clone(),
                    from_path: action.from_path.clone(),
                    to_path: action.to_path.clone(),
                    status,
                    message: created.message.clone(),
                    bytes: created.bytes,
                });
            }
            Err(error) => {
                failed_actions += 1;
                logs.push(InventoryExecutionActionLog {
                    code: action.code.clone(),
                    kind: action.kind.clone(),
                    from_path: action.from_path.clone(),
                    to_path: action.to_path.clone(),
                    status: InventoryExecutionActionStatus::Failed,
                    message: Some(error.to_string()),
                    bytes: 0,
                });
                rolled_back_actions += rollback_created_targets(&created_targets, &mut logs);
                break;
            }
        }
    }

    let executed_works = if failed_actions == 0 {
        planned_counts_by_code
            .iter()
            .filter(|(code, planned_count)| {
                successful_counts_by_code.get(*code).copied().unwrap_or(0) == **planned_count
            })
            .count()
    } else {
        0
    };

    Ok(InventoryExecutionReport {
        mode: request.mode,
        started_at,
        finished_at: Utc::now().to_rfc3339(),
        requested_works: prepared.requested_works,
        executed_works,
        skipped_works: prepared.skipped_works,
        planned_actions: prepared.actions.len(),
        linked_actions,
        copied_actions,
        moved_actions,
        failed_actions,
        rolled_back_actions,
        rollback_failed_actions,
        same_volume_actions,
        cross_volume_actions,
        space_blocked_actions,
        bytes_linked,
        bytes_copied,
        bytes_moved,
        logs,
    })
}
```

Update `CreatedInventoryOperation` and `CreatedInventoryTarget`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreatedInventoryOperation {
    Copied,
    Linked,
    Moved(InventoryMoveMethod),
}

#[derive(Debug, Clone)]
struct CreatedInventoryTarget {
    code: String,
    kind: InventoryResourceKind,
    operation: CreatedInventoryOperation,
    source_path: PathBuf,
    path: PathBuf,
    bytes: u64,
    sha256: String,
    message: Option<String>,
}
```

Set `message: None` in existing copy and hardlink constructors.

Update `execute_prepared_action` signature and match:

```rust
fn execute_prepared_action(
    action: &PreparedInventoryAction,
    mode: InventoryExecutionMode,
    archive_root_canonical: &Path,
    run_id: &str,
    index: usize,
    options: &InventoryExecutionOptions<'_>,
) -> Result<CreatedInventoryTarget> {
    match (mode, &action.kind) {
        (InventoryExecutionMode::Move, _) => {
            move_prepared_action(action, archive_root_canonical, run_id, index, options)
        }
        (InventoryExecutionMode::LowSpace, InventoryResourceKind::Video) => {
            link_prepared_video_action(action, archive_root_canonical)
        }
        _ => copy_prepared_action(action, archive_root_canonical, run_id, index),
    }
}
```

Add:

```rust
/// Move one validated action into the archive and remove the source after validation.
fn move_prepared_action(
    action: &PreparedInventoryAction,
    archive_root_canonical: &Path,
    run_id: &str,
    index: usize,
    options: &InventoryExecutionOptions<'_>,
) -> Result<CreatedInventoryTarget> {
    let parent = action
        .to_path
        .parent()
        .ok_or_else(|| anyhow!("目标路径没有父目录：{}", action.to_path.to_string_lossy()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("创建目标目录失败：{}", parent.to_string_lossy()))?;
    if !target_existing_parent_is_inside_root(&action.to_path, archive_root_canonical)? {
        bail!("目标路径位于整理目标目录之外");
    }
    let moved = move_file_no_clobber(
        &action.from_path,
        &action.to_path,
        options.move_strategy,
        run_id,
        index,
    )?;
    let message = match moved.method {
        InventoryMoveMethod::SameVolume => "rename".to_string(),
        InventoryMoveMethod::CrossVolume => "copy_verify_delete".to_string(),
    };
    Ok(CreatedInventoryTarget {
        code: action.code.clone(),
        kind: action.kind.clone(),
        operation: CreatedInventoryOperation::Moved(moved.method),
        source_path: action.from_path.clone(),
        path: action.to_path.clone(),
        bytes: moved.bytes,
        sha256: moved.sha256.unwrap_or_default(),
        message: Some(message),
    })
}
```

Update `created_target_still_matches` so moved same-volume targets can be removed during the old global rollback path:

```rust
fn created_target_still_matches(target: &CreatedInventoryTarget) -> Result<bool> {
    match target.operation {
        CreatedInventoryOperation::Linked => linked_target_still_removable(target),
        CreatedInventoryOperation::Copied | CreatedInventoryOperation::Moved(_) => {
            if target.sha256.is_empty() {
                moved_target_still_removable(target)
            } else {
                copied_target_still_matches(target)
            }
        }
    }
}
```

Add:

```rust
/// Confirm that a same-volume moved target still exists and is not the original source path.
fn moved_target_still_removable(target: &CreatedInventoryTarget) -> Result<bool> {
    let metadata = match fs::metadata(&target.path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(false),
    };
    if !metadata.is_file() || metadata.len() != target.bytes {
        return Ok(false);
    }
    Ok(normalized_path_key(&target.path)? != normalized_path_key(&target.source_path)?)
}
```

- [ ] **Step 4: Run the same-volume move test and verify it passes**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution inventory_move_execution_moves_sources_into_archive -j 1
```

Expected: PASS.

- [ ] **Step 5: Run existing inventory execution tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1
```

Expected: all tests in `inventory_execution` pass.

- [ ] **Step 6: Commit Task 3**

Run:

```powershell
git add src-tauri/Cargo.toml src-tauri/src/lib.rs src-tauri/src/inventory_move.rs src-tauri/src/inventory_execution.rs src-tauri/tests/inventory_execution.rs
git commit -m "实现同盘集中迁移"
```

---

### Task 4: Cross-Volume Copy-Verify-Delete And Space Stop

**Files:**
- Modify: `src-tauri/src/inventory_move.rs`
- Modify: `src-tauri/src/inventory_execution.rs`
- Test: `src-tauri/tests/inventory_execution.rs`

- [ ] **Step 1: Add fake move strategy helpers in the Rust integration test**

In `src-tauri/tests/inventory_execution.rs`, extend the imports:

```rust
use media_manager::inventory_execution::{
    execute_inventory_report, execute_inventory_report_with_options, InventoryExecutionActionStatus,
    InventoryExecutionMode, InventoryExecutionOptions, InventoryExecutionRequest,
};
use media_manager::inventory_move::InventoryMoveStrategy;
use anyhow::Result;
use std::path::{Path, PathBuf};
```

Add this helper near the request helpers:

```rust
/// Test strategy that forces all move actions down one volume branch with deterministic space.
struct FakeMoveStrategy {
    same_volume: bool,
    available_space: u64,
}

impl InventoryMoveStrategy for FakeMoveStrategy {
    fn is_same_volume(&self, _from_path: &Path, _to_path: &Path) -> Result<bool> {
        Ok(self.same_volume)
    }

    fn available_space(&self, _path: &Path) -> Result<u64> {
        Ok(self.available_space)
    }
}
```

- [ ] **Step 2: Add a failing cross-volume copy-verify-delete test**

Add:

```rust
#[test]
fn inventory_move_execution_cross_volume_copies_verifies_and_deletes_source() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    let video = root.join("IPX-602.mp4");
    let nfo = root.join("IPX-602.nfo");
    write_file(&video, b"video-cross");
    write_file(
        &nfo,
        br#"<movie><num>IPX-602</num><title>Ready</title></movie>"#,
    );
    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();
    let strategy = FakeMoveStrategy {
        same_volume: false,
        available_space: 1024 * 1024 * 1024,
    };
    let options = InventoryExecutionOptions {
        move_strategy: &strategy,
    };

    let execution =
        execute_inventory_report_with_options(&report, &move_all_request(), &options).unwrap();

    assert_eq!(execution.moved_actions, 2);
    assert_eq!(execution.same_volume_actions, 0);
    assert_eq!(execution.cross_volume_actions, 2);
    assert_eq!(execution.space_blocked_actions, 0);
    assert_eq!(execution.bytes_moved, 11 + 53);
    assert!(!video.exists());
    assert!(!nfo.exists());
    assert_eq!(fs::read(archive.join("IPX-602").join("IPX-602.mp4")).unwrap(), b"video-cross");
    assert_eq!(
        fs::read(archive.join("IPX-602").join("IPX-602.nfo")).unwrap(),
        br#"<movie><num>IPX-602</num><title>Ready</title></movie>"#
    );
    assert!(execution.logs.iter().all(|log| log.message.as_deref() == Some("copy_verify_delete")));
}
```

- [ ] **Step 3: Add a failing low-space test**

Add:

```rust
#[test]
fn inventory_move_execution_stops_before_cross_volume_copy_when_space_is_low() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    let video = root.join("IPX-603.mp4");
    let nfo = root.join("IPX-603.nfo");
    write_file(&video, b"video-low-space");
    write_file(
        &nfo,
        br#"<movie><num>IPX-603</num><title>Ready</title></movie>"#,
    );
    let report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();
    let strategy = FakeMoveStrategy {
        same_volume: false,
        available_space: 16,
    };
    let options = InventoryExecutionOptions {
        move_strategy: &strategy,
    };

    let execution =
        execute_inventory_report_with_options(&report, &move_all_request(), &options).unwrap();

    assert_eq!(execution.executed_works, 0);
    assert_eq!(execution.moved_actions, 0);
    assert_eq!(execution.failed_actions, 1);
    assert_eq!(execution.space_blocked_actions, 1);
    assert!(video.exists());
    assert!(nfo.exists());
    assert!(!archive.join("IPX-603").join("IPX-603.mp4").exists());
    assert!(execution.logs.iter().any(|log| log
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("目标磁盘剩余空间不足")));
}
```

- [ ] **Step 4: Run both tests and verify they fail**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution inventory_move_execution_cross_volume_copies_verifies_and_deletes_source -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution inventory_move_execution_stops_before_cross_volume_copy_when_space_is_low -j 1
```

Expected: cross-volume test fails with `跨盘集中迁移尚未启用`; low-space test fails until the cross-volume preflight returns a report and increments `space_blocked_actions`.

- [ ] **Step 5: Implement cross-volume migration**

Replace `move_cross_volume_copy_verify_delete` in `src-tauri/src/inventory_move.rs` with:

```rust
fn move_cross_volume_copy_verify_delete(
    from_path: &Path,
    to_path: &Path,
    source_size: u64,
    strategy: &dyn InventoryMoveStrategy,
    run_id: &str,
    index: usize,
) -> Result<InventoryMovedFile> {
    let parent = to_path
        .parent()
        .ok_or_else(|| anyhow!("目标路径没有父目录：{}", to_path.to_string_lossy()))?;
    let required_space = source_size.saturating_add(CROSS_VOLUME_SPACE_MARGIN_BYTES);
    let available = strategy.available_space(parent)?;
    if available < required_space {
        bail!(
            "目标磁盘剩余空间不足：需要至少 {} 字节，可用 {} 字节",
            required_space,
            available
        );
    }
    let temp_path = temporary_move_path(to_path, run_id, index)?;
    let (copied_size, sha256) = copy_source_to_new_temp(from_path, &temp_path)?;
    let temp_size = fs::metadata(&temp_path)
        .with_context(|| format!("读取临时迁移文件失败：{}", temp_path.to_string_lossy()))?
        .len();
    if copied_size != source_size || temp_size != source_size {
        let _ = fs::remove_file(&temp_path);
        bail!("跨盘迁移后大小校验失败：{}", from_path.to_string_lossy());
    }
    if file_sha256(&temp_path)? != sha256 {
        let _ = fs::remove_file(&temp_path);
        bail!("跨盘迁移后哈希校验失败：{}", from_path.to_string_lossy());
    }
    persist_temp_without_clobber(&temp_path, to_path)?;
    if fs::metadata(to_path)
        .with_context(|| format!("读取迁移目标失败：{}", to_path.to_string_lossy()))?
        .len()
        != source_size
    {
        bail!("跨盘迁移目标大小校验失败：{}", to_path.to_string_lossy());
    }
    if file_sha256(to_path)? != sha256 {
        bail!("跨盘迁移目标哈希校验失败：{}", to_path.to_string_lossy());
    }
    fs::remove_file(from_path)
        .with_context(|| format!("删除源文件失败：{}", from_path.to_string_lossy()))?;
    if from_path.exists() {
        bail!("源文件删除后仍存在：{}", from_path.to_string_lossy());
    }
    Ok(InventoryMovedFile {
        from_path: from_path.to_path_buf(),
        to_path: to_path.to_path_buf(),
        method: InventoryMoveMethod::CrossVolume,
        bytes: source_size,
        sha256: Some(sha256),
    })
}
```

Append the helper functions to `src-tauri/src/inventory_move.rs`:

```rust
/// Build a same-directory temp path for cross-volume migration.
fn temporary_move_path(to_path: &Path, run_id: &str, index: usize) -> Result<PathBuf> {
    let parent = to_path
        .parent()
        .ok_or_else(|| anyhow!("目标路径没有父目录：{}", to_path.to_string_lossy()))?;
    let file_name = to_path
        .file_name()
        .ok_or_else(|| anyhow!("目标路径没有文件名：{}", to_path.to_string_lossy()))?
        .to_string_lossy();
    for attempt in 0..100 {
        let candidate = parent.join(format!(
            ".{file_name}.mm-moving-{run_id}-{index}-{attempt}"
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!("无法生成临时迁移文件名：{}", to_path.to_string_lossy());
}

/// Copy a source into a newly created temp file while computing SHA-256.
fn copy_source_to_new_temp(from_path: &Path, temp_path: &Path) -> Result<(u64, String)> {
    let mut input = File::open(from_path)
        .with_context(|| format!("打开源文件失败：{}", from_path.to_string_lossy()))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .with_context(|| format!("创建临时迁移文件失败：{}", temp_path.to_string_lossy()))?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .with_context(|| format!("读取源文件失败：{}", from_path.to_string_lossy()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .with_context(|| format!("写入临时迁移文件失败：{}", temp_path.to_string_lossy()))?;
        total += read as u64;
    }
    output
        .sync_all()
        .with_context(|| format!("同步临时迁移文件失败：{}", temp_path.to_string_lossy()))?;
    Ok((total, hex_digest(hasher.finalize().as_slice())))
}

/// Commit a temp file to the final target without replacing an existing file.
fn persist_temp_without_clobber(temp_path: &Path, to_path: &Path) -> Result<()> {
    if to_path.exists() {
        let _ = fs::remove_file(temp_path);
        bail!("目标路径已存在：{}", to_path.to_string_lossy());
    }
    match fs::hard_link(temp_path, to_path) {
        Ok(()) => {
            let _ = fs::remove_file(temp_path);
            Ok(())
        }
        Err(_) => {
            let mut input = File::open(temp_path)
                .with_context(|| format!("打开临时迁移文件失败：{}", temp_path.to_string_lossy()))?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(to_path)
                .with_context(|| format!("创建迁移目标失败：{}", to_path.to_string_lossy()))?;
            if let Err(error) = std::io::copy(&mut input, &mut output) {
                let _ = fs::remove_file(to_path);
                let _ = fs::remove_file(temp_path);
                return Err(error)
                    .with_context(|| format!("写入迁移目标失败：{}", to_path.to_string_lossy()));
            }
            if let Err(error) = output.sync_all() {
                let _ = fs::remove_file(to_path);
                let _ = fs::remove_file(temp_path);
                return Err(error)
                    .with_context(|| format!("同步迁移目标失败：{}", to_path.to_string_lossy()));
            }
            let _ = fs::remove_file(temp_path);
            Ok(())
        }
    }
}

/// Compute a SHA-256 hash for migration verification.
fn file_sha256(path: &Path) -> Result<String> {
    let mut input =
        File::open(path).with_context(|| format!("打开文件失败：{}", path.to_string_lossy()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .with_context(|| format!("读取文件失败：{}", path.to_string_lossy()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(hasher.finalize().as_slice()))
}

/// Format a digest as lowercase hexadecimal.
fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
```

- [ ] **Step 6: Count space-blocked failures in the report**

In `src-tauri/src/inventory_execution.rs`, in the `Err(error)` branch of `execute_inventory_report_with_options`, increment `space_blocked_actions` when the error message contains the low-space marker:

```rust
let message = error.to_string();
if message.contains("目标磁盘剩余空间不足") {
    space_blocked_actions += 1;
}
logs.push(InventoryExecutionActionLog {
    code: action.code.clone(),
    kind: action.kind.clone(),
    from_path: action.from_path.clone(),
    to_path: action.to_path.clone(),
    status: InventoryExecutionActionStatus::Failed,
    message: Some(message),
    bytes: 0,
});
```

- [ ] **Step 7: Run the focused Rust tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution inventory_move_execution_cross_volume_copies_verifies_and_deletes_source -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution inventory_move_execution_stops_before_cross_volume_copy_when_space_is_low -j 1
```

Expected: both tests pass.

- [ ] **Step 8: Commit Task 4**

Run:

```powershell
git add src-tauri/src/inventory_move.rs src-tauri/src/inventory_execution.rs src-tauri/tests/inventory_execution.rs
git commit -m "支持跨盘集中迁移空间检查"
```

---

### Task 5: Work-Scoped Rollback And Completed Work Preservation

**Files:**
- Modify: `src-tauri/src/inventory_execution.rs`
- Modify: `src-tauri/src/inventory_move.rs`
- Test: `src-tauri/tests/inventory_execution.rs`

- [ ] **Step 1: Add a runtime failure test that rolls back only the current work**

Add this test:

```rust
#[test]
fn inventory_move_execution_rolls_back_current_work_and_keeps_completed_work() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let archive = tmp.path().join("archive");
    let first_video = root.join("IPX-604.mp4");
    let first_nfo = root.join("IPX-604.nfo");
    let second_video = root.join("IPX-605.mp4");
    let second_nfo = root.join("IPX-605.nfo");
    write_file(&first_video, b"first-video");
    write_file(
        &first_nfo,
        br#"<movie><num>IPX-604</num><title>First</title></movie>"#,
    );
    write_file(&second_video, b"second-video");
    write_file(
        &second_nfo,
        br#"<movie><num>IPX-605</num><title>Second</title></movie>"#,
    );
    let mut report = preview_inventory_roots(&[root.clone()], Some(&archive)).unwrap();
    report.works.sort_by(|left, right| left.code.cmp(&right.code));
    let second = report
        .works
        .iter_mut()
        .find(|work| work.code == "IPX-605")
        .unwrap();
    let second_nfo_action = second
        .resolution
        .execution_plan
        .actions
        .iter_mut()
        .find(|action| action.kind == InventoryResourceKind::Nfo)
        .unwrap();
    second_nfo_action.from_path = root.join("IPX-605-missing.nfo");

    let execution = execute_inventory_report(&report, &move_all_request()).unwrap();

    assert_eq!(execution.executed_works, 1);
    assert_eq!(execution.failed_actions, 1);
    assert_eq!(execution.rolled_back_actions, 1);
    assert!(!first_video.exists());
    assert!(!first_nfo.exists());
    assert!(archive.join("IPX-604").join("IPX-604.mp4").exists());
    assert!(archive.join("IPX-604").join("IPX-604.nfo").exists());
    assert!(second_video.exists(), "current work video should be restored after rollback");
    assert!(!archive.join("IPX-605").join("IPX-605.mp4").exists());
    assert!(execution
        .logs
        .iter()
        .any(|log| log.status == InventoryExecutionActionStatus::RolledBack && log.code == "IPX-605"));
}
```

- [ ] **Step 2: Run the rollback test and verify it fails**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution inventory_move_execution_rolls_back_current_work_and_keeps_completed_work -j 1
```

Expected: fails because current code rolls back all created targets or because the modified missing source is rejected during preflight before the first work moves.

- [ ] **Step 3: Change prepared execution to preserve work grouping**

In `src-tauri/src/inventory_execution.rs`, replace `PreparedInventoryExecution.actions` with work groups:

```rust
#[derive(Debug, Clone)]
struct PreparedInventoryWork {
    code: String,
    actions: Vec<PreparedInventoryAction>,
}

#[derive(Debug, Clone)]
struct PreparedInventoryExecution {
    requested_works: usize,
    skipped_works: usize,
    archive_root_canonical: PathBuf,
    works: Vec<PreparedInventoryWork>,
}
```

In `prepare_inventory_execution`, build groups:

```rust
let mut works = Vec::new();
for work in &selected_works {
    let mut work_actions = Vec::new();
    for action in &work.resolution.execution_plan.actions {
        work_actions.push(validate_inventory_action(
            &work.code,
            action,
            archive_root,
            &archive_root_canonical,
            &mut target_keys,
        )?);
    }
    if !work_actions.is_empty() {
        works.push(PreparedInventoryWork {
            code: work.code.clone(),
            actions: work_actions,
        });
    }
}
if works.iter().all(|work| work.actions.is_empty()) {
    bail!("安全执行计划没有可执行动作");
}
```

Return `works`.

- [ ] **Step 4: Stop preflight from rejecting disappeared sources for move mode**

Change `validate_inventory_action` to accept a mode parameter:

```rust
fn validate_inventory_action(
    code: &str,
    action: &InventoryPreviewAction,
    archive_root: &Path,
    archive_root_canonical: &Path,
    target_keys: &mut BTreeSet<String>,
    mode: InventoryExecutionMode,
) -> Result<PreparedInventoryAction> {
```

Keep metadata preflight for `copy` and `low_space`, but for `move` store the action and let runtime fail so current-work rollback is possible:

```rust
if mode != InventoryExecutionMode::Move {
    let from_metadata = fs::metadata(&action.from_path)
        .with_context(|| format!("源文件不存在：{}", action.from_path.to_string_lossy()))?;
    if !from_metadata.is_file() {
        bail!("源路径不是文件：{}", action.from_path.to_string_lossy());
    }
}
```

Pass `request.mode` from `prepare_inventory_execution`.

- [ ] **Step 5: Execute by work and roll back only current work**

In `execute_inventory_report_with_options`, replace the single action loop with nested work/action loops:

```rust
let planned_actions: usize = prepared.works.iter().map(|work| work.actions.len()).sum();
for work in &prepared.works {
    planned_counts_by_code.insert(work.code.clone(), work.actions.len());
}

for work in &prepared.works {
    let mut current_work_targets = Vec::new();
    let mut current_work_failed = false;
    for (index, action) in work.actions.iter().enumerate() {
        let global_index = logs.len();
        match execute_prepared_action(
            action,
            request.mode,
            &prepared.archive_root_canonical,
            &run_id,
            global_index + index,
            options,
        ) {
            Ok(created) => {
                let status = match created.operation {
                    CreatedInventoryOperation::Copied => {
                        copied_actions += 1;
                        bytes_copied += created.bytes;
                        InventoryExecutionActionStatus::Copied
                    }
                    CreatedInventoryOperation::Linked => {
                        linked_actions += 1;
                        bytes_linked += created.bytes;
                        InventoryExecutionActionStatus::Linked
                    }
                    CreatedInventoryOperation::Moved(method) => {
                        moved_actions += 1;
                        bytes_moved += created.bytes;
                        match method {
                            InventoryMoveMethod::SameVolume => same_volume_actions += 1,
                            InventoryMoveMethod::CrossVolume => cross_volume_actions += 1,
                        }
                        InventoryExecutionActionStatus::Moved
                    }
                };
                current_work_targets.push(created.clone());
                *successful_counts_by_code.entry(action.code.clone()).or_default() += 1;
                logs.push(InventoryExecutionActionLog {
                    code: action.code.clone(),
                    kind: action.kind.clone(),
                    from_path: action.from_path.clone(),
                    to_path: action.to_path.clone(),
                    status,
                    message: created.message.clone(),
                    bytes: created.bytes,
                });
            }
            Err(error) => {
                current_work_failed = true;
                failed_actions += 1;
                let message = error.to_string();
                if message.contains("目标磁盘剩余空间不足") {
                    space_blocked_actions += 1;
                }
                logs.push(InventoryExecutionActionLog {
                    code: action.code.clone(),
                    kind: action.kind.clone(),
                    from_path: action.from_path.clone(),
                    to_path: action.to_path.clone(),
                    status: InventoryExecutionActionStatus::Failed,
                    message: Some(message),
                    bytes: 0,
                });
                let rollback = rollback_current_work_targets(&current_work_targets, &mut logs, options);
                rolled_back_actions += rollback.rolled_back;
                rollback_failed_actions += rollback.rollback_failed;
                break;
            }
        }
    }
    if current_work_failed {
        break;
    }
}
```

Use `planned_actions` in the returned report.

- [ ] **Step 6: Add current-work rollback helpers**

In `src-tauri/src/inventory_execution.rs`, add:

```rust
struct InventoryRollbackCounts {
    rolled_back: usize,
    rollback_failed: usize,
}

/// Roll back targets produced by the current work after a move-mode runtime failure.
fn rollback_current_work_targets(
    created_targets: &[CreatedInventoryTarget],
    logs: &mut Vec<InventoryExecutionActionLog>,
    options: &InventoryExecutionOptions<'_>,
) -> InventoryRollbackCounts {
    let mut counts = InventoryRollbackCounts {
        rolled_back: 0,
        rollback_failed: 0,
    };
    for target in created_targets.iter().rev() {
        let rollback_result = match target.operation {
            CreatedInventoryOperation::Moved(_) => crate::inventory_move::rollback_moved_file(
                &target.path,
                &target.source_path,
                target.bytes,
                target.sha256.as_deref(),
                options.move_strategy,
            ),
            CreatedInventoryOperation::Copied | CreatedInventoryOperation::Linked => {
                rollback_created_target_delete_only(target)
            }
        };
        match rollback_result {
            Ok(()) => {
                counts.rolled_back += 1;
                logs.push(InventoryExecutionActionLog {
                    code: target.code.clone(),
                    kind: target.kind.clone(),
                    from_path: target.path.clone(),
                    to_path: target.source_path.clone(),
                    status: InventoryExecutionActionStatus::RolledBack,
                    message: Some("执行失败后已回滚当前作品文件".to_string()),
                    bytes: target.bytes,
                });
            }
            Err(error) => {
                counts.rollback_failed += 1;
                logs.push(InventoryExecutionActionLog {
                    code: target.code.clone(),
                    kind: target.kind.clone(),
                    from_path: target.path.clone(),
                    to_path: target.source_path.clone(),
                    status: InventoryExecutionActionStatus::RollbackFailed,
                    message: Some(format!("回滚失败：{error}")),
                    bytes: 0,
                });
            }
        }
    }
    counts
}

/// Delete a copy or hardlink target created in the current work.
fn rollback_created_target_delete_only(target: &CreatedInventoryTarget) -> Result<()> {
    if created_target_still_matches(target)? {
        fs::remove_file(&target.path)
            .with_context(|| format!("删除回滚目标失败：{}", target.path.to_string_lossy()))?;
    }
    Ok(())
}
```

In `src-tauri/src/inventory_move.rs`, add:

```rust
/// Restore a moved target back to its original source path during current-work rollback.
pub fn rollback_moved_file(
    target_path: &Path,
    source_path: &Path,
    expected_size: u64,
    expected_sha256: Option<&str>,
    strategy: &dyn InventoryMoveStrategy,
) -> Result<()> {
    if source_path.exists() {
        bail!("源路径已存在，不能覆盖：{}", source_path.to_string_lossy());
    }
    let metadata = fs::metadata(target_path)
        .with_context(|| format!("读取回滚目标失败：{}", target_path.to_string_lossy()))?;
    if !metadata.is_file() || metadata.len() != expected_size {
        bail!("回滚目标大小不匹配：{}", target_path.to_string_lossy());
    }
    if let Some(expected) = expected_sha256 {
        if !expected.is_empty() && file_sha256(target_path)? != expected {
            bail!("回滚目标哈希不匹配：{}", target_path.to_string_lossy());
        }
    }
    if strategy.is_same_volume(target_path, source_path).unwrap_or(false) {
        move_same_volume_no_copy(target_path, source_path, expected_size)?;
        return Ok(());
    }
    let parent = source_path
        .parent()
        .ok_or_else(|| anyhow!("源路径没有父目录：{}", source_path.to_string_lossy()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("创建源目录失败：{}", parent.to_string_lossy()))?;
    let available = strategy.available_space(parent)?;
    let required_space = expected_size.saturating_add(CROSS_VOLUME_SPACE_MARGIN_BYTES);
    if available < required_space {
        bail!(
            "回滚空间不足：需要至少 {} 字节，可用 {} 字节",
            required_space,
            available
        );
    }
    let temp_path = temporary_move_path(source_path, "rollback", 0)?;
    let (copied_size, sha256) = copy_source_to_new_temp(target_path, &temp_path)?;
    if copied_size != expected_size {
        let _ = fs::remove_file(&temp_path);
        bail!("回滚复制大小不匹配：{}", target_path.to_string_lossy());
    }
    if let Some(expected) = expected_sha256 {
        if !expected.is_empty() && sha256 != expected {
            let _ = fs::remove_file(&temp_path);
            bail!("回滚复制哈希不匹配：{}", target_path.to_string_lossy());
        }
    }
    persist_temp_without_clobber(&temp_path, source_path)?;
    fs::remove_file(target_path)
        .with_context(|| format!("删除回滚目标失败：{}", target_path.to_string_lossy()))?;
    Ok(())
}
```

- [ ] **Step 7: Run the rollback test and full inventory execution tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution inventory_move_execution_rolls_back_current_work_and_keeps_completed_work -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1
```

Expected: both commands pass.

- [ ] **Step 8: Commit Task 5**

Run:

```powershell
git add src-tauri/src/inventory_execution.rs src-tauri/src/inventory_move.rs src-tauri/tests/inventory_execution.rs
git commit -m "实现集中迁移作品级回滚"
```

---

### Task 6: Frontend Centralized Migration Entry

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/App.inventory.test.tsx`
- Modify: `src/viewModel.ts`
- Test: `src/App.inventory.test.tsx`

- [ ] **Step 1: Update the App execution fixture to move mode**

In `src/App.inventory.test.tsx`, rename the helper comment and return value:

```ts
/** Minimal M1 execution report used to verify centralized migration UI wiring. */
function makeInventoryExecutionReport(): InventoryExecutionReport {
  return {
    mode: "move",
    started_at: "2026-06-28T12:01:00Z",
    finished_at: "2026-06-28T12:02:00Z",
    requested_works: 1,
    executed_works: 1,
    skipped_works: 0,
    planned_actions: 1,
    linked_actions: 0,
    copied_actions: 0,
    moved_actions: 1,
    failed_actions: 0,
    rolled_back_actions: 0,
    rollback_failed_actions: 0,
    same_volume_actions: 1,
    cross_volume_actions: 0,
    space_blocked_actions: 0,
    bytes_linked: 0,
    bytes_copied: 0,
    bytes_moved: 5,
    logs: [
      {
        code: "IPX-201",
        kind: "video",
        from_path: "D:\\inventory-inbox\\IPX-201.mp4",
        to_path: "D:\\inventory-archive\\IPX-201\\IPX-201.mp4",
        status: "moved",
        message: "rename",
        bytes: 5
      }
    ]
  };
}
```

- [ ] **Step 2: Replace the low-space App wiring test**

Replace the existing `executes the current safe inventory plan with low-space loading feedback` test with:

```ts
it("executes the current safe inventory plan with centralized migration feedback", async () => {
  const report = makeInventoryReport();
  const executionReport = makeInventoryExecutionReport();
  vi.spyOn(api, "previewInventory").mockResolvedValue(report);
  const pendingExecution = deferred<InventoryExecutionReport>();
  const executeSpy = vi.spyOn(api, "executeInventoryPlan").mockReturnValue(pendingExecution.promise);
  const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);

  await act(async () => {
    root?.render(<App />);
  });
  await act(async () => {
    buttonContaining("盘点").click();
  });

  const rootsField = document.querySelector(".inventory-roots-field textarea") as HTMLTextAreaElement;
  const targetField = document.querySelector(".inventory-roots-field input") as HTMLInputElement;
  await act(async () => {
    setTextFieldValue(rootsField, "D:\\inventory-inbox");
    setTextFieldValue(targetField, "D:\\inventory-archive");
    buttonContaining("开始盘点").click();
  });

  await act(async () => {
    buttonContaining("集中迁移").click();
  });

  expect(executeSpy).toHaveBeenCalledWith(report, [], "move");
  expect(confirmSpy.mock.calls[0][0]).toContain("成功后源路径不再保留");
  expect(confirmSpy.mock.calls[0][0]).toContain("跨盘会逐文件复制校验后删除源文件");
  expect(buttonContaining("迁移中").disabled).toBe(true);

  await act(async () => {
    pendingExecution.resolve(executionReport);
    await pendingExecution.promise;
  });

  expect(document.body.textContent).toContain("集中迁移完成：作品 1/1，迁移 1/1，失败 0，回滚 0，同盘 1，跨盘 0");
  expect(document.body.textContent).toContain("已迁移");
  expect(document.body.textContent).toContain("IPX-201");
});
```

Update truncation tests:

```ts
expect(buttonContaining("集中迁移").disabled).toBe(true);
expect(document.body.textContent).toContain("报告明细已截断，不能集中迁移全部作品");
```

and:

```ts
expect(buttonContaining("集中迁移").disabled).toBe(false);
expect(document.body.textContent).not.toContain("报告明细已截断，不能集中迁移全部作品");
```

- [ ] **Step 3: Run the App inventory test and verify it fails**

Run:

```powershell
npm test -- src/App.inventory.test.tsx
```

Expected: fails because UI still says low-space and calls `"low_space"`.

- [ ] **Step 4: Update the inventory execution function in App**

In `src/App.tsx`, replace `executeInventoryPreview` with:

```tsx
  /** 集中迁移当前盘点报告中的安全执行计划，成功后源文件不再保留。 */
  async function executeInventoryPreview() {
    if (!inventoryReport || inventoryExecutableCount === 0) {
      setStatus("当前盘点结果没有可集中迁移的安全计划。");
      return;
    }
    if (inventoryExecutionBlockedByTruncation) {
      setStatus("报告明细已截断，不能集中迁移全部作品；请缩小入口目录后重新盘点。");
      return;
    }
    const targetRoot = inventoryReport.archive_root ?? inventoryArchiveRoot.trim();
    const confirmed = window.confirm(`将集中迁移 ${inventoryExecutableCount} 部作品到 ${targetRoot || "未设置目标"}。本操作会移动文件，成功后源路径不再保留；同盘直接移动，跨盘会逐文件复制校验后删除源文件；目标已存在不会覆盖；失败会停止后续队列并显示原因。是否继续？`);
    if (!confirmed) {
      setStatus("已取消集中迁移。");
      return;
    }
    setInventoryExecuteBusy(true);
    setStatus(`正在集中迁移 ${inventoryExecutableCount} 部作品...`);
    try {
      const result = await api.executeInventoryPlan(inventoryReport, [], "move");
      setInventoryExecutionReport(result);
      setStatus(`${formatInventoryExecutionSummary(result)} 建议重新盘点验证源路径和目标状态。`);
    } catch (error) {
      setStatus(`集中迁移失败：${String(error)}`);
    } finally {
      setInventoryExecuteBusy(false);
    }
  }
```

- [ ] **Step 5: Update button text, status text, log title, and truncation copy**

In `src/App.tsx`, update the inventory execution button block:

```tsx
                <button type="button" onClick={executeInventoryPreview} disabled={!inventoryReport || inventoryExecutableCount === 0 || inventoryExecutionBlockedByTruncation || inventoryBusy || inventoryExportBusy || inventoryExecuteBusy || !hasBackend}>
                  <Copy size={16} /> {inventoryExecuteBusy ? "迁移中" : "集中迁移"}
                </button>
```

Update the helper text near the button:

```tsx
                  {inventoryExecuteBusy
                    ? "正在集中迁移安全执行计划..."
                    : `将 ${inventoryExecutableCount} 个可自动整理作品迁移到目标目录，成功后源文件不再保留。`}
```

Update the execution report title:

```tsx
                        <strong>最近集中迁移</strong>
```

Update the log status label expression:

```tsx
                              {log.status === "moved" ? "已迁移" : log.status === "linked" ? "已硬链接" : log.status === "copied" ? "已复制" : log.status === "rolled_back" ? "已回滚" : log.status === "rollback_failed" ? "回滚失败" : "失败"} · {log.kind} · {formatBytes(log.bytes)}
```

Update the truncation warning:

```tsx
                      <span>报告明细已截断，不能集中迁移全部作品；请缩小入口目录后重新盘点。</span>
```

- [ ] **Step 6: Run the focused App test**

Run:

```powershell
npm test -- src/App.inventory.test.tsx
```

Expected: PASS.

- [ ] **Step 7: Commit Task 6**

Run:

```powershell
git add src/App.tsx src/App.inventory.test.tsx src/viewModel.ts src/api.ts
git commit -m "接入集中迁移前端入口"
```

---

### Task 7: Command Boundary And Full Verification

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `HANDOFF.md`
- Test: `src-tauri/tests/inventory_execution.rs`
- Test: `src/App.inventory.test.tsx`
- Test: `src/viewModel.test.ts`

- [ ] **Step 1: Confirm the Tauri command still defaults to copy**

In `src-tauri/src/commands.rs`, keep this behavior unchanged:

```rust
let request = InventoryExecutionRequest {
    mode: mode.unwrap_or(InventoryExecutionMode::Copy),
    selected_codes,
};
```

This preserves backward compatibility for callers that omit `mode`.

- [ ] **Step 2: Run focused backend and frontend tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1
npm test -- src/viewModel.test.ts src/App.inventory.test.tsx
```

Expected: both commands pass.

- [ ] **Step 3: Run full verification gate**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

Expected:

- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`: all Rust tests pass. Historical `resource_pool.rs` warnings may still appear.
- `npm test`: all Vitest tests pass.
- `npx tsc --noEmit`: exits 0.
- `npm run build`: exits 0.

- [ ] **Step 4: Update HANDOFF**

In `HANDOFF.md`, update the M1 section to say implementation is complete and add the exact validation commands from Step 3 with their observed pass/fail status.

- [ ] **Step 5: Commit Task 7**

Run:

```powershell
git add HANDOFF.md
git commit -m "记录M1集中迁移验证结果"
```

- [ ] **Step 6: Push the branch**

Run:

```powershell
git push
```

Expected: branch `codex/m1-inventory-centralized-migration` is updated on GitHub.

---

## Self-Review Checklist

- Spec coverage:
  - Direct centralized migration: Tasks 2, 3, 4, 5, 6.
  - Same-volume no-copy move: Task 3.
  - Cross-volume copy-verify-delete: Task 4.
  - Per-file space check: Task 4.
  - No overwrite: Tasks 3 and 4 keep `exists` / create-new checks; existing 7C target checks remain.
  - Work-scoped rollback: Task 5.
  - UI loading and confirmation: Task 6.
  - Existing `copy` / `low_space` compatibility: Tasks 2, 3, 7.
  - No WebView2 verification: Task 7.
- Type consistency:
  - Rust mode: `InventoryExecutionMode::Move`.
  - JSON/TS mode: `"move"`.
  - Statuses: `moved`, `rollback_failed`.
  - Counters: `moved_actions`, `bytes_moved`, `same_volume_actions`, `cross_volume_actions`, `space_blocked_actions`, `rollback_failed_actions`.
- Risk notes:
  - Same-volume implementation uses no-clobber hardlink+unlink rather than overwrite-prone `rename`. It still avoids content copy and removes the source path.
  - Cross-volume tests use `FakeMoveStrategy`, so they do not require real separate disks.
  - The plan keeps SQLite sync and old migration entry cleanup outside M1 implementation.
