# Stage 7B.1 Safe Execution Plan Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Separate raw inventory candidate actions from a safe, selected execution plan so Stage 7C cannot accidentally execute duplicate target actions.

**Architecture:** Extend the existing Rust inventory resolver with an `InventoryExecutionPlan` nested under `InventoryResolution`. Raw `InventoryWorkPreview.actions` remains a diagnostic candidate list; `resolution.execution_plan.actions` becomes the only future execution-facing list. React and TypeScript only display and format the new plan; no real file operation or SQLite persistence is added.

**Tech Stack:** Rust, serde, React, TypeScript, Vitest.

---

## File Map

- Modify `src-tauri/src/inventory.rs`
  - Add `InventoryExecutionPlan`.
  - Build selected actions from primary video, primary NFO, and first unique target per asset.
  - Make bucket/summary depend on execution plan safety instead of raw candidate action presence.
- Modify `src-tauri/tests/inventory.rs`
  - Add regression tests for duplicate NFO/poster target safety, multi-video review, and target-exists blocking.
- Modify `src/api.ts`
  - Add `InventoryExecutionPlan` DTO and nest it in `InventoryResolution`.
- Modify `src/viewModel.ts`
  - Add `formatInventoryExecutionPlanSummary`.
- Modify `src/viewModel.test.ts`
  - Add formatting and fixture coverage for `execution_plan`.
- Modify `src/App.tsx`
  - Display a safe execution plan panel in selected inventory details.
  - Rename raw action section to candidate action preview.
- Modify `.ai_state/project.json`, `.ai_state/tasks.md`, `.ai_state/design.md`, `.ai_state/progress.md`
  - Track Sprint 16 / Stage 7B.1 state.
- Modify `HANDOFF.md`
  - Record 7B.1 status and verification when complete.

---

## Task 1: Backend Safe Execution Plan

**Files:**
- Modify: `src-tauri/src/inventory.rs`
- Test: `src-tauri/tests/inventory.rs`

- [ ] **Step 1: Write failing regression tests**

Add tests proving:

- A work with one video, two NFOs, and two posters has raw `target_duplicate` candidate actions but an execution plan with no `target_duplicate` conflicts.
- The same work remains `auto_ready` only because the selected execution plan is safe.
- A multi-video work is `needs_review` and `execution_plan.ready == false`.
- A target-existing work is `blocked` and the execution plan carries the target conflict.

- [ ] **Step 2: Verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory execution_plan -j 1
```

Expected: fail because `execution_plan` does not exist yet.

- [ ] **Step 3: Implement `InventoryExecutionPlan`**

Add the DTO with public comments. Build it before bucket selection:

- Use selected role paths for primary video and primary NFO.
- Select only one asset action per target key.
- Strip `target_duplicate` from selected actions when the duplicate came from raw candidates not selected for execution.
- Preserve `target_exists`.
- Add conflicts for missing target path, missing primary video, missing primary NFO, code conflict, NFO parse error, multi-video, duplicate video candidate, and target exists.

- [ ] **Step 4: Make bucket use execution plan**

Rules:

- `asset_candidate` keeps precedence.
- `blocked` for code conflict, NFO parse error, or target exists.
- `auto_ready` only when `execution_plan.ready` is true.
- otherwise `needs_review`.

- [ ] **Step 5: Verify GREEN**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1
```

Expected: all inventory tests pass.

---

## Task 2: Frontend DTO and Formatting

**Files:**
- Modify: `src/api.ts`
- Modify: `src/viewModel.ts`
- Test: `src/viewModel.test.ts`

- [ ] **Step 1: Write failing viewModel tests**

Add tests for `formatInventoryExecutionPlanSummary`:

- ready plan with N actions.
- not ready with conflict count.
- not ready with no actions.

- [ ] **Step 2: Verify RED**

Run:

```powershell
npm test -- src/viewModel.test.ts
```

Expected: fail because DTO/helper is missing.

- [ ] **Step 3: Implement DTO/helper**

Add `InventoryExecutionPlan` to `src/api.ts`, add `execution_plan` to `InventoryResolution`, and implement a Chinese formatter in `src/viewModel.ts`.

- [ ] **Step 4: Verify GREEN**

Run:

```powershell
npm test -- src/viewModel.test.ts
npx tsc --noEmit
```

Expected: both pass.

---

## Task 3: Inventory Detail UI

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/styles.css`
- Test: `src/App.inventory.test.tsx`

- [ ] **Step 1: Write/extend App wiring test**

Assert that selected work details show the safe execution plan summary and the raw action section label is candidate action preview.

- [ ] **Step 2: Verify RED**

Run:

```powershell
npm test -- src/App.inventory.test.tsx
```

Expected: fail because the UI does not render the new labels yet.

- [ ] **Step 3: Implement UI**

Import `formatInventoryExecutionPlanSummary`, render the safe execution plan panel, show conflicts/notes, and rename the raw actions heading to “候选动作预览”.

- [ ] **Step 4: Verify GREEN**

Run:

```powershell
npm test -- src/App.inventory.test.tsx
npm test -- src/viewModel.test.ts src/App.inventory.test.tsx
npx tsc --noEmit
npm run build
```

Expected: all pass.

---

## Task 4: State, Verification, Handoff, Commit

**Files:**
- Modify: `.ai_state/project.json`
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/design.md`
- Modify: `.ai_state/progress.md`
- Modify: `.ai_state/reviews/sprint-16.md`
- Modify: `.ai_state/lessons.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Run verification gate**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1
npm test -- src/viewModel.test.ts src/App.inventory.test.tsx
npx tsc --noEmit
npm run build
```

If environment allows:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
```

- [ ] **Step 2: Self-review diff**

Check that:

- No real media operation was added.
- `actions` remains raw candidate preview.
- `execution_plan.actions` is the future execution-facing list.
- `auto_ready` cannot be true while `execution_plan.ready` is false.
- Chinese files were edited via `apply_patch`.

- [ ] **Step 3: Update handoff and state**

Record verification evidence and the new Stage 7C prerequisite: only consume `resolution.execution_plan.actions`.

- [ ] **Step 4: Commit and push**

Use Chinese commit messages:

```powershell
git add docs/superpowers/specs/2026-06-28-media-manager-stage7b1-safe-execution-plan-design.md docs/superpowers/plans/2026-06-28-media-manager-stage7b1-safe-execution-plan.md src-tauri/src/inventory.rs src-tauri/tests/inventory.rs src/api.ts src/viewModel.ts src/viewModel.test.ts src/App.tsx src/styles.css HANDOFF.md
git commit -m "实现阶段7B1安全执行计划"
git push -u origin codex/stage7b1-execution-plan
```

---

## Self-Review Checklist

- Spec coverage:
  - Candidate vs execution action separation: Task 1.
  - Auto-ready safety: Task 1.
  - Frontend visibility: Task 2 and Task 3.
  - Verification and handoff: Task 4.
- No placeholders remain.
- No task includes real file movement, copy, delete, rename, or SQLite inventory persistence.
