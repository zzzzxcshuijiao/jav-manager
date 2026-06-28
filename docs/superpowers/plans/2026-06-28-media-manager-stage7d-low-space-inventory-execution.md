# Stage 7D Low-Space Inventory Execution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make inventory execution usable on a real large media library by hard-linking video files in low-space mode while continuing to copy small metadata and image assets.

**Architecture:** Extend the existing Stage 7C `inventory_execution` module instead of adding a second executor. `low_space` mode branches per action kind: videos use no-clobber hard links from source to target, while non-video resources reuse the existing temp-file copy path. The frontend renames the action to low-space organization and sends `mode: "low_space"`.

**Tech Stack:** Rust, serde, Tauri commands, React, TypeScript, Vitest.

---

## File Map

- Modify `src-tauri/src/inventory_execution.rs`
  - Add `LowSpace` mode, `Linked` status, linked counters, bytes-linked accounting, hard-link execution branch, and rollback messages.
- Modify `src-tauri/tests/inventory_execution.rs`
  - Add low-space hard-link tests and update compatible copy-mode expectations.
- Modify `src/api.ts`
  - Add `low_space` mode, `linked` status, and report fields.
- Modify `src/viewModel.ts`
  - Update `formatInventoryExecutionSummary` for low-space output.
- Modify `src/viewModel.test.ts`
  - Cover low-space summary formatting and copy-mode compatibility.
- Modify `src/App.tsx`
  - Rename UI copy action to low-space organization, update confirm/status/log text, and call `low_space`.
- Modify `src/App.inventory.test.tsx`
  - Update execution fixture and assertions for low-space mode.
- Modify `HANDOFF.md`
  - Record 7D deliverables and user testing expectations.
- Add `.ai_state/*`
  - Recreate session-local stage tracking for 7D.

---

## Task 1: Backend Low-Space Execution

**Files:**
- Modify: `src-tauri/src/inventory_execution.rs`
- Modify: `src-tauri/tests/inventory_execution.rs`

- [ ] **Step 1: Write failing low-space tests**

Add a helper:

```rust
/// Build a low-space inventory execution request for integration tests.
fn low_space_all_request() -> InventoryExecutionRequest {
    InventoryExecutionRequest {
        mode: InventoryExecutionMode::LowSpace,
        selected_codes: Vec::new(),
    }
}
```

Add a test that creates one video, one NFO, and one poster, runs low-space mode, then asserts:

```rust
assert_eq!(execution.mode, InventoryExecutionMode::LowSpace);
assert_eq!(execution.linked_actions, 1);
assert_eq!(execution.copied_actions, 2);
assert_eq!(execution.bytes_copied, nfo_size + poster_size);
assert_eq!(execution.bytes_linked, video_size);
assert_eq!(fs::read(&archive_video).unwrap(), b"video");
write_file(&video, b"changed-video");
assert_eq!(fs::read(&archive_video).unwrap(), b"changed-video");
write_file(&nfo, b"<movie><num>IPX-501</num><title>Changed</title></movie>");
assert_ne!(fs::read(&archive_nfo).unwrap(), fs::read(&nfo).unwrap());
```

- [ ] **Step 2: Verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1
```

Expected: fail because `LowSpace`, `Linked`, `linked_actions`, and `bytes_linked` do not exist.

- [ ] **Step 3: Add execution mode and report fields**

In `InventoryExecutionMode`, add:

```rust
LowSpace,
```

In `InventoryExecutionActionStatus`, add:

```rust
Linked,
```

In `InventoryExecutionReport`, add:

```rust
pub linked_actions: usize,
pub bytes_linked: u64,
```

Initialize both counters in `execute_inventory_report`.

- [ ] **Step 4: Branch execution by resource kind**

Replace the single `copy_prepared_action(...)` call with a method that checks mode and action kind:

```rust
let outcome = execute_prepared_action(action, request.mode, &prepared.archive_root_canonical, &run_id, index)?;
```

Implement:

```rust
fn execute_prepared_action(
    action: &PreparedInventoryAction,
    mode: InventoryExecutionMode,
    archive_root_canonical: &Path,
    run_id: &str,
    index: usize,
) -> Result<CreatedInventoryTarget>
```

Behavior:

- `Copy`: call the existing copy path.
- `LowSpace` + `Video`: call a new `link_prepared_video_action`.
- `LowSpace` + non-video: call the existing copy path.

- [ ] **Step 5: Implement no-fallback video hard link**

Add:

```rust
/// Create a no-clobber hard link for a validated video action without copying video bytes.
fn link_prepared_video_action(
    action: &PreparedInventoryAction,
    archive_root_canonical: &Path,
) -> Result<CreatedInventoryTarget>
```

Implementation requirements:

- create target parent directory;
- re-run `target_existing_parent_is_inside_root`;
- read source size;
- call `fs::hard_link(&action.from_path, &action.to_path)`;
- if it fails, return an error containing `视频硬链接失败`;
- do not call `copy_source_to_new_temp`;
- return `CreatedInventoryTarget` with `operation: CreatedInventoryOperation::Linked`.

- [ ] **Step 6: Track copied vs linked outcomes**

Add an internal enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreatedInventoryOperation {
    Copied,
    Linked,
}
```

Store it on `CreatedInventoryTarget`. Use it to:

- set log status to `linked` or `copied`;
- increment `linked_actions` and `bytes_linked` for video links;
- increment `copied_actions` and `bytes_copied` for copied small files;
- count both linked and copied actions as successful for `executed_works`.

- [ ] **Step 7: Update rollback messages**

Use `CreatedInventoryOperation` when pushing rollback logs:

```rust
let message = match target.operation {
    CreatedInventoryOperation::Linked => "执行失败后已删除本轮生成的硬链接目标",
    CreatedInventoryOperation::Copied => "执行失败后已删除本轮复制目标文件",
};
```

- [ ] **Step 8: Verify backend focused tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1
```

Expected: all inventory execution tests pass.

---

## Task 2: Frontend API and Formatting

**Files:**
- Modify: `src/api.ts`
- Modify: `src/viewModel.ts`
- Modify: `src/viewModel.test.ts`

- [ ] **Step 1: Write failing formatter expectation**

Update the execution summary test with:

```ts
expect(formatInventoryExecutionSummary({
  mode: "low_space",
  started_at: "2026-06-28T10:00:00Z",
  finished_at: "2026-06-28T10:01:00Z",
  requested_works: 4,
  executed_works: 3,
  skipped_works: 2,
  planned_actions: 12,
  linked_actions: 3,
  copied_actions: 8,
  failed_actions: 1,
  rolled_back_actions: 2,
  bytes_linked: 9_000_000_000,
  bytes_copied: 2048,
  logs: []
})).toBe("低空间整理完成：作品 3/4，硬链接 3，复制 8，失败 1，回滚 2，链接视频 8.38 GB，复制小文件 2.00 KB。");
```

- [ ] **Step 2: Verify RED**

Run:

```powershell
npm test -- src/viewModel.test.ts
```

Expected: fail because `low_space` types/fields are missing or formatter still says copy-only.

- [ ] **Step 3: Update TypeScript DTOs**

Change:

```ts
export type InventoryExecutionMode = "copy" | "low_space";
export type InventoryExecutionActionStatus = "linked" | "copied" | "failed" | "rolled_back";
```

Add to `InventoryExecutionReport`:

```ts
linked_actions: number;
bytes_linked: number;
```

- [ ] **Step 4: Update summary formatter**

Use mode-specific text:

```ts
if (report.mode === "low_space") {
  return `低空间整理完成：作品 ${report.executed_works}/${report.requested_works}，硬链接 ${report.linked_actions}，复制 ${report.copied_actions}，失败 ${report.failed_actions}，回滚 ${report.rolled_back_actions}，链接视频 ${formatBytes(report.bytes_linked)}，复制小文件 ${formatBytes(report.bytes_copied)}。`;
}
return `复制整理完成：作品 ${report.executed_works}/${report.requested_works}，动作 ${report.copied_actions}/${report.planned_actions}，失败 ${report.failed_actions}，回滚 ${report.rolled_back_actions}，复制 ${formatBytes(report.bytes_copied)}。`;
```

- [ ] **Step 5: Verify formatter tests**

Run:

```powershell
npm test -- src/viewModel.test.ts
```

Expected: view model tests pass.

---

## Task 3: Frontend Low-Space UX

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/App.inventory.test.tsx`

- [ ] **Step 1: Update App inventory test expectations**

Change the execution fixture:

```ts
mode: "low_space",
linked_actions: 1,
copied_actions: 0,
bytes_linked: 5,
bytes_copied: 0,
logs: [{ status: "linked", kind: "video", bytes: 5, ... }]
```

Assert:

```ts
buttonContaining("低空间整理").click();
expect(executeSpy).toHaveBeenCalledWith(report, [], "low_space");
expect(buttonContaining("整理中").disabled).toBe(true);
expect(document.body.textContent).toContain("低空间整理完成：作品 1/1，硬链接 1，复制 0，失败 0，回滚 0");
expect(document.body.textContent).toContain("已硬链接");
```

- [ ] **Step 2: Verify RED**

Run:

```powershell
npm test -- src/App.inventory.test.tsx
```

Expected: fail because UI still says copy and sends `copy`.

- [ ] **Step 3: Update App behavior and text**

In `executeInventoryPreview`:

- change empty-plan status to `当前盘点结果没有可低空间整理的安全计划。`;
- change truncation text to `报告明细已截断，不能低空间整理全部作品；请缩小入口目录后重新盘点。`;
- change confirm text to mention hard links and copied small assets;
- change busy status to `正在低空间整理 ${inventoryExecutableCount} 部作品...`;
- call `api.executeInventoryPlan(inventoryReport, [], "low_space")`;
- change failure prefix to `低空间整理失败：`.

In the button and report:

- label `低空间整理`;
- busy label `整理中`;
- status line `正在低空间整理安全执行计划...`;
- report title `最近低空间整理`;
- log status `linked` renders `已硬链接`.

- [ ] **Step 4: Verify App tests**

Run:

```powershell
npm test -- src/App.inventory.test.tsx
```

Expected: App inventory tests pass.

---

## Task 4: Verification, State, Review, Commit

**Files:**
- Modify: `HANDOFF.md`
- Add/modify: `.ai_state/project.json`
- Add/modify: `.ai_state/tasks.md`
- Add/modify: `.ai_state/design.md`
- Add/modify: `.ai_state/progress.md`
- Add/modify: `.ai_state/lessons.md`
- Add/modify: `.ai_state/reviews/sprint-18.md`

- [ ] **Step 1: Run focused backend/frontend gate**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1
npm test -- src/viewModel.test.ts src/App.inventory.test.tsx
npx tsc --noEmit
npm run build
```

- [ ] **Step 2: Run full quality gate**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

- [ ] **Step 3: Self-review**

Check the diff for:

- video low-space path never calls the temp copy function;
- hard-link failure does not fallback to copying video;
- target no-clobber and archive-root checks still run before writes;
- `copy` mode remains compatible with Stage 7C;
- Chinese source edits were made via `apply_patch`;
- no Tauri GUI/WebView2 command was run.

- [ ] **Step 4: Update handoff and state**

Record:

- 7D design summary;
- verified commands;
- user-facing real environment test steps;
- remaining future work: move/delete cleanup, manual review execution, SQLite sync.

- [ ] **Step 5: Commit and push**

```powershell
git add docs/superpowers/specs/2026-06-28-media-manager-stage7d-low-space-inventory-execution-design.md docs/superpowers/plans/2026-06-28-media-manager-stage7d-low-space-inventory-execution.md src-tauri/src/inventory_execution.rs src-tauri/tests/inventory_execution.rs src/api.ts src/viewModel.ts src/viewModel.test.ts src/App.tsx src/App.inventory.test.tsx HANDOFF.md
git commit -m "实现阶段7D低空间整理执行"
git push -u origin codex/stage7d-low-space-execution
```

---

## Self-Review Checklist

- Spec coverage:
  - hard-link video: Task 1.
  - copy small resources: Task 1.
  - no fallback to video copy: Task 1 and review.
  - UI wording and mode switch: Task 3.
  - verification and handoff: Task 4.
- No placeholder text remains.
- No WebView2/Tauri GUI commands are used.
- Tests remain independent of real media drives.
