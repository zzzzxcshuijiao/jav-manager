# Stage 7C Inventory Copy Execution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Execute Stage 7B.1 safe inventory plans in a copy-only mode so users can materialize an organized archive without deleting or moving original scattered files.

**Architecture:** Add a Rust `inventory_execution` core module that consumes only `resolution.execution_plan.actions`, expose it through a Tauri command, and add React UI wiring for one-click copy execution with status feedback.

**Tech Stack:** Rust, serde, Tauri commands, React, TypeScript, Vitest.

---

## File Map

- Add `src-tauri/src/inventory_execution.rs`
  - Execution DTOs.
  - Preflight validation.
  - Copy-only execution and rollback cleanup.
- Modify `src-tauri/src/lib.rs`
  - Export `inventory_execution`.
- Modify `src-tauri/src/commands.rs`
  - Add `execute_inventory_plan` async command.
  - Register it in the invoke handler.
- Add `src-tauri/tests/inventory_execution.rs`
  - Temporary-dir tests for ready copy, non-ready rejection, target-exists preflight, and target-root escape.
- Modify `src/api.ts`
  - Add execution DTOs and `api.executeInventoryPlan`.
- Modify `src/viewModel.ts`
  - Add `formatInventoryExecutionSummary`.
- Modify `src/viewModel.test.ts`
  - Cover execution summary formatting.
- Modify `src/App.tsx`
  - Add inventory execution state, button, confirmation, status, and latest report panel.
- Modify `src/App.inventory.test.tsx`
  - Cover execute button loading/disabled state and API call.
- Modify `src/styles.css`
  - Style execution report/log rows.
- Modify `.ai_state/*`, `.ai_state/reviews/sprint-17.md`, `HANDOFF.md`
  - Record Stage 7C state, verification, lessons, and next steps.

---

## Task 1: Backend Execution Core

**Files:**
- Add: `src-tauri/src/inventory_execution.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/inventory_execution.rs`

- [ ] **Step 1: Write failing tests**

Add tests proving:

- Auto-ready works copy only `resolution.execution_plan.actions` to the archive root and leave sources in place.
- All-run skips non-auto-ready works.
- Explicitly selecting a non-ready work is rejected before any copy.
- Existing target paths reject the batch before copying earlier actions.
- A target path outside `archive_root` is rejected.

- [ ] **Step 2: Verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1
```

Expected: fail because `inventory_execution` does not exist.

- [ ] **Step 3: Implement DTOs and preflight**

Add:

- `InventoryExecutionMode`
- `InventoryExecutionRequest`
- `InventoryExecutionActionStatus`
- `InventoryExecutionActionLog`
- `InventoryExecutionReport`

Preflight must:

- require `archive_root`;
- reject full execution when work details are truncated;
- select all auto-ready works when `selected_codes` is empty;
- require selected codes to exist and be auto-ready;
- read only `resolution.execution_plan.actions`;
- reject missing target, action conflict, missing source, existing target, duplicate target, and target outside archive root.

- [ ] **Step 4: Implement copy-only execution**

Copy each action through a temp file in the destination directory, verify byte count, rename to the final target, and preserve source files. On runtime failure, clean up temp files and delete targets created earlier in the same run as best effort.

- [ ] **Step 5: Verify GREEN**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1
```

Expected: both pass.

---

## Task 2: Command and API

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/api.ts`

- [ ] **Step 1: Add command wrapper**

Expose `execute_inventory_plan(report, selected_codes, mode)` as an async command using `spawn_blocking`.

- [ ] **Step 2: Register command**

Add it to the Tauri invoke handler without touching WebView-dependent flows.

- [ ] **Step 3: Add TypeScript DTO/API**

Add execution report types and `api.executeInventoryPlan(report, selectedCodes, mode)`.

- [ ] **Step 4: Verify compile**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1
npx tsc --noEmit
```

---

## Task 3: Frontend Execution UX

**Files:**
- Modify: `src/viewModel.ts`
- Modify: `src/viewModel.test.ts`
- Modify: `src/App.tsx`
- Modify: `src/App.inventory.test.tsx`
- Modify: `src/styles.css`

- [ ] **Step 1: Write failing frontend tests**

Add tests for:

- `formatInventoryExecutionSummary`.
- App calls `api.executeInventoryPlan` after preview and disables the execute button while copying.
- App shows the latest execution summary.

- [ ] **Step 2: Verify RED**

Run:

```powershell
npm test -- src/viewModel.test.ts src/App.inventory.test.tsx
```

Expected: fail because helpers/UI/API are missing.

- [ ] **Step 3: Implement UI wiring**

Add:

- `inventoryExecuteBusy`
- `inventoryExecutionReport`
- execute button with spinner label and disabled state
- confirmation before real copy
- status-line summary after completion
- latest execution report panel

- [ ] **Step 4: Verify GREEN**

Run:

```powershell
npm test -- src/viewModel.test.ts src/App.inventory.test.tsx
npx tsc --noEmit
npm run build
```

Expected: all pass.

---

## Task 4: Verification, Review, State, Commit

**Files:**
- Modify: `.ai_state/project.json`
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/design.md`
- Modify: `.ai_state/progress.md`
- Add: `.ai_state/reviews/sprint-17.md`
- Modify: `.ai_state/lessons.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Run backend gate**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1
cargo test --manifest-path src-tauri/Cargo.toml -j 1
```

- [ ] **Step 2: Run frontend gate**

```powershell
npm test
npx tsc --noEmit
npm run build
```

- [ ] **Step 3: Self-review and reviewer pass**

Check:

- no source delete/move code exists in 7C;
- execution consumes `resolution.execution_plan.actions`, never raw `actions`;
- preflight rejects existing target and target escape before writing;
- target path checks resolve junction/symlink parents before writing files;
- target creation uses no-clobber behavior and rollback verifies target identity;
- button has explicit loading/disabled state;
- Chinese files were edited via `apply_patch`.

- [ ] **Step 4: Update state and handoff**

Record verification evidence, the new copy-only execution capability, and remaining Stage 7D work for move/delete cleanup.

- [ ] **Step 5: Commit and push**

Use a Chinese commit message:

```powershell
git add docs/superpowers/specs/2026-06-28-media-manager-stage7c-inventory-copy-execution-design.md docs/superpowers/plans/2026-06-28-media-manager-stage7c-inventory-copy-execution.md src-tauri/src/inventory_execution.rs src-tauri/src/lib.rs src-tauri/src/commands.rs src-tauri/tests/inventory_execution.rs src/api.ts src/viewModel.ts src/viewModel.test.ts src/App.tsx src/App.inventory.test.tsx src/styles.css HANDOFF.md
git commit -m "实现阶段7C存量复制整理"
git push -u origin codex/stage7c-inventory-execution
```

---

## Self-Review Checklist

- Spec coverage:
  - Copy-only execution: Task 1/3.
  - Safety preflight: Task 1.
  - Command/API/UI: Task 2/3.
  - Verification and handoff: Task 4.
- No placeholder text remains.
- No Tauri GUI/WebView2 command was run.
- No real media path is required for tests.
