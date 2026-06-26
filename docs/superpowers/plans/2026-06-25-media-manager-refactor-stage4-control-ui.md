# 阶段 4 控制接口与前端连线 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把阶段 3 的无头 daemon core 暴露成前端可消费的同步控制命令，并在现有设置页连上状态、运行一轮、搁置区、异常队列和最近运行记录。

**Architecture:** 新增 `src-tauri/src/daemon_control.rs` 作为命令层适配，复用 `HeadlessDaemon`、`Repository` 和阶段 2 `AutoPipeline`，不引入 HTTP/WebSocket 或常驻线程。前端只扩展现有 `api.ts`、`viewModel.ts` 和设置页，保持 UI 改动集中、可用 TypeScript/Vitest/build 验证。

**Tech Stack:** Rust 2021、Tauri command、SQLite/rusqlite、React 18、TypeScript、Vitest。

---

## File Structure

- **Create** `src-tauri/src/daemon_control.rs`
  - 命令层 DTO：`DaemonControlRuntime`、`DaemonControlStatus`、`MetadataSource`。
  - 本地示例 scraper：`ExamplePipelineScraper`。
  - 纯 helper：构建状态、运行一轮、列出/解决异常、列出 holding 和 pipeline runs。
- **Create** `src-tauri/tests/daemon_control.rs`
  - 使用 `tempfile` SQLite 和假视频文件验证命令层 helper，不启动 Tauri/WebView2。
- **Modify** `src-tauri/src/lib.rs`
  - 导出 `pub mod daemon_control;`。
- **Modify** `src-tauri/src/commands.rs`
  - `AppState` 增加 daemon runtime。
  - 注册并实现 Tauri commands。
  - 修复命令路径 `parse_watch_status` 新状态。
  - 增加 `#[cfg(test)]` 单元测试覆盖 parser。
- **Modify** `src/api.ts`
  - 扩展 `WatchStatus` union。
  - 增加 daemon DTO 和 API wrapper。
- **Modify** `src/viewModel.ts`
  - 增加 daemon/holding/exception/pipeline run 的中文格式化 helper。
- **Modify** `src/viewModel.test.ts`
  - 先写新增 helper 的红灯测试。
- **Modify** `src/App.tsx`
  - 设置页增加“自动管线”页签和 loading 状态。
- **Modify** `HANDOFF.md`
  - 阶段 4 完成后更新当前进度、验证命令、下一步。

---

### Task 1: 后端命令层核心

**Files:**
- Create: `src-tauri/src/daemon_control.rs`
- Create: `src-tauri/tests/daemon_control.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/tests/daemon_control.rs`:

```rust
use media_manager::daemon_control::{
    build_daemon_status, list_exception_entries, list_holding_entries, list_recent_pipeline_runs,
    resolve_exception_entry, run_daemon_once, DaemonControlRuntime, ExamplePipelineScraper,
    MetadataSource,
};
use media_manager::domain::{
    Exception, ExceptionKind, ExceptionStatus, HoldingEntry, HoldingReason, PipelineRun,
};
use media_manager::pipeline::ScraperSource;
use media_manager::storage::Repository;
use std::path::{Path, PathBuf};

fn open_repo(path: &Path) -> Repository {
    let repo = Repository::open(path).unwrap();
    repo.migrate().unwrap();
    repo
}

fn configured_repo(tmp: &tempfile::TempDir) -> (Repository, PathBuf, PathBuf, PathBuf) {
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    let inbox = tmp.path().join("inbox");
    let archive = tmp.path().join("archive");
    let assets = tmp.path().join("assets");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    std::fs::create_dir_all(&assets).unwrap();
    repo.set_source_roots(&[inbox.clone()]).unwrap();
    repo.set_archive_root(&archive).unwrap();
    repo.set_resource_pool_dirs(&[assets.clone()]).unwrap();
    (repo, inbox, archive, assets)
}

#[test]
fn example_pipeline_scraper_generates_local_metadata_without_network() {
    let scraper = ExamplePipelineScraper;

    let metadata = scraper.lookup("ABP-404").unwrap().unwrap();

    assert_eq!(scraper.name(), "example");
    assert_eq!(metadata.normalized_code, "ABP-404");
    assert_eq!(metadata.source, "example");
    assert!(metadata.title.contains("ABP-404"));
    assert!(metadata.cover_path.is_none());
}

#[test]
fn daemon_status_reports_configuration_and_queue_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, _inbox, archive, assets) = configured_repo(&tmp);
    repo.record_exception(&Exception {
        id: None,
        object_path: "H:/Inbox/ABP-001.mp4".to_string(),
        kind: ExceptionKind::ScrapeFailed,
        evidence_json: "{}".to_string(),
        status: ExceptionStatus::Open,
        created_at: None,
        resolved_at: None,
    })
    .unwrap();
    repo.add_holding(&HoldingEntry {
        id: None,
        path: "H:/Inbox/no-code.mp4".to_string(),
        file_name: "no-code.mp4".to_string(),
        size_bytes: 10,
        reason: HoldingReason::NoCode,
        created_at: None,
    })
    .unwrap();
    repo.record_pipeline_run(&PipelineRun {
        id: None,
        file_path: "H:/Inbox/ABP-001.mp4".to_string(),
        started_at: None,
        finished_at: None,
        steps_json: "[]".to_string(),
        status: "exception".to_string(),
        error: None,
    })
    .unwrap();
    let runtime = DaemonControlRuntime {
        paused: false,
        processed: 7,
        last_error: Some("previous error".to_string()),
    };

    let status = build_daemon_status(&repo, &runtime, true).unwrap();

    assert!(status.configured);
    assert_eq!(status.state, "Idle");
    assert_eq!(status.archive_root, Some(archive.to_string_lossy().to_string()));
    assert_eq!(status.asset_roots, vec![assets.to_string_lossy().to_string()]);
    assert_eq!(status.processed, 7);
    assert_eq!(status.open_exceptions, 1);
    assert_eq!(status.holding_items, 1);
    assert_eq!(status.recent_runs, 1);
    assert_eq!(status.metadata_source, MetadataSource::Example);
    assert_eq!(status.last_error.as_deref(), Some("previous error"));
}

#[test]
fn run_once_requires_example_metadata_source_before_touching_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _archive, _assets) = configured_repo(&tmp);
    let video = inbox.join("ABP-404.mp4");
    std::fs::write(&video, b"stable video bytes").unwrap();
    let mut runtime = DaemonControlRuntime::default();

    let error = run_daemon_once(&repo, &mut runtime, false).unwrap_err();

    assert!(error.to_string().contains("示例元数据源未开启"));
    assert!(video.exists());
    assert_eq!(repo.list_pipeline_runs().unwrap().len(), 0);
}

#[test]
fn run_once_archives_with_example_scraper_when_enabled() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, archive, _assets) = configured_repo(&tmp);
    let video = inbox.join("ABP-404.mp4");
    std::fs::write(&video, b"stable video bytes").unwrap();
    let mut runtime = DaemonControlRuntime::default();

    let report = run_daemon_once(&repo, &mut runtime, true).unwrap();

    assert_eq!(report.scan.queued_files, 1);
    assert_eq!(report.process.archived, 1);
    assert_eq!(runtime.processed, 1);
    assert!(archive.join("ABP-404").join("ABP-404.mp4").exists());
    assert_eq!(repo.list_pipeline_runs().unwrap()[0].status, "archived");
}

#[test]
fn queue_list_helpers_return_repository_rows_and_resolve_exceptions() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, _inbox, _archive, _assets) = configured_repo(&tmp);
    let exception_id = repo.record_exception(&Exception {
        id: None,
        object_path: "H:/Inbox/ABP-404.mp4".to_string(),
        kind: ExceptionKind::ScrapeFailed,
        evidence_json: "{\"source\":\"example\"}".to_string(),
        status: ExceptionStatus::Open,
        created_at: None,
        resolved_at: None,
    }).unwrap();
    repo.add_holding(&HoldingEntry {
        id: None,
        path: "H:/Inbox/manual.mp4".to_string(),
        file_name: "manual.mp4".to_string(),
        size_bytes: 42,
        reason: HoldingReason::Unrecognizable,
        created_at: None,
    }).unwrap();
    repo.record_pipeline_run(&PipelineRun {
        id: None,
        file_path: "H:/Inbox/ABP-404.mp4".to_string(),
        started_at: None,
        finished_at: None,
        steps_json: "[]".to_string(),
        status: "exception".to_string(),
        error: Some("not found".to_string()),
    }).unwrap();

    assert_eq!(list_exception_entries(&repo).unwrap().len(), 1);
    assert_eq!(list_holding_entries(&repo).unwrap().len(), 1);
    assert_eq!(list_recent_pipeline_runs(&repo).unwrap().len(), 1);

    resolve_exception_entry(&repo, exception_id, ExceptionStatus::Resolved).unwrap();

    assert_eq!(
        list_exception_entries(&repo).unwrap()[0].status,
        ExceptionStatus::Resolved
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test daemon_control -j 1
```

Expected: FAIL because `daemon_control` module and its exported symbols do not exist.

- [ ] **Step 3: Implement minimal backend module**

Create `src-tauri/src/daemon_control.rs` with documented public types/functions. Export it from `src-tauri/src/lib.rs` with:

```rust
pub mod daemon_control;
```

Implementation requirements:

- `DaemonControlRuntime` stores `paused`, `processed`, `last_error`.
- `DaemonControlStatus` serializes the fields from the stage 4 design.
- `MetadataSource` serializes as `"example"` / `"disabled"` using serde rename.
- `ExamplePipelineScraper` implements `ScraperSource` and returns deterministic `ScrapedWorkMetadata`.
- `run_daemon_once` refuses disabled metadata source before scanning.
- `run_daemon_once` uses `CompletionPolicy { sample_delay: Duration::ZERO }` so tests and command calls do not wait one second per file.
- List/resolve helpers delegate to existing Repository methods.

- [ ] **Step 4: Run focused Rust test to verify it passes**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test daemon_control -j 1
```

Expected: all `daemon_control` tests PASS.

- [ ] **Step 5: Commit backend core**

Run:

```powershell
git add src-tauri/src/lib.rs src-tauri/src/daemon_control.rs src-tauri/tests/daemon_control.rs
git commit -m "新增阶段4守护控制核心"
```

---

### Task 2: Tauri 命令桥与观看状态修复

**Files:**
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Write parser regression test first**

Add a `#[cfg(test)]` module near the end of `src-tauri/src/commands.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_watch_status_parser_accepts_stage1_statuses() {
        assert_eq!(parse_watch_status("WantToWatch"), WatchStatus::WantToWatch);
        assert_eq!(parse_watch_status("Watching"), WatchStatus::Watching);
        assert_eq!(parse_watch_status("OnHold"), WatchStatus::OnHold);
        assert_eq!(parse_watch_status("watched"), WatchStatus::Watched);
        assert_eq!(parse_watch_status("favorite"), WatchStatus::Favorite);
        assert_eq!(parse_watch_status("unknown"), WatchStatus::Unwatched);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml commands::tests::command_watch_status_parser_accepts_stage1_statuses -j 1
```

Expected: FAIL because command parser maps `WantToWatch` / `Watching` / `OnHold` to `Unwatched`.

- [ ] **Step 3: Implement command parser and daemon commands**

Modify `src-tauri/src/commands.rs`:

- Import daemon control helpers and domain queue types:

```rust
use crate::daemon_control::{
    build_daemon_status, list_exception_entries, list_holding_entries, list_recent_pipeline_runs,
    resolve_exception_entry, run_daemon_once, DaemonControlRuntime, DaemonControlStatus,
};
use crate::domain::{Exception, ExceptionStatus, HoldingEntry, PipelineRun, ...};
```

- Add `pub daemon_runtime: Mutex<DaemonControlRuntime>` to `AppState` and initialize it in `Default`.
- Add Tauri commands:
  - `get_daemon_status`
  - `pause_daemon`
  - `resume_daemon`
  - `run_daemon_once_command`
  - `list_holding_entries`
  - `list_exception_entries`
  - `resolve_exception_entry_command`
  - `list_pipeline_runs`
- Register these commands in `tauri::generate_handler!`.
- Update `parse_watch_status` arms:

```rust
fn parse_watch_status(value: &str) -> WatchStatus {
    match value {
        "Watched" | "watched" => WatchStatus::Watched,
        "Favorite" | "favorite" => WatchStatus::Favorite,
        "WantToWatch" | "want_to_watch" | "wanttowatch" => WatchStatus::WantToWatch,
        "Watching" | "watching" => WatchStatus::Watching,
        "OnHold" | "on_hold" | "onhold" => WatchStatus::OnHold,
        _ => WatchStatus::Unwatched,
    }
}
```

For command names, expose snake_case names to frontend:

- `run_daemon_once_command` is registered as command function name `run_daemon_once_command`; frontend wrapper calls `"run_daemon_once_command"`.
- `resolve_exception_entry_command` takes `id: i64` and `status: String`, only accepts `"Ignored"` or `"Resolved"`.

- [ ] **Step 4: Run command-focused Rust checks**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml commands::tests::command_watch_status_parser_accepts_stage1_statuses -j 1
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test daemon_control -j 1
```

Expected: both commands exit 0.

- [ ] **Step 5: Commit command bridge**

Run:

```powershell
git add src-tauri/src/commands.rs
git commit -m "接入阶段4守护控制命令"
```

---

### Task 3: 前端类型与格式化 helper

**Files:**
- Modify: `src/api.ts`
- Modify: `src/viewModel.ts`
- Modify: `src/viewModel.test.ts`

- [ ] **Step 1: Write failing Vitest cases**

In `src/viewModel.test.ts`, extend imports with:

```ts
  formatDaemonState,
  formatExceptionKind,
  formatExceptionStatus,
  formatHoldingReason,
  formatPipelineStatus,
  summarizeRunOnceReport,
  shortEvidence
```

Append tests:

```ts
describe("daemon view helpers", () => {
  it("labels daemon states, holding reasons, exception kinds and run statuses", () => {
    expect(formatDaemonState("Idle")).toBe("空闲");
    expect(formatDaemonState("Paused")).toBe("已暂停");
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
      process: { processed: 2, archived: 1, holding: 1, exceptions: 0, failed: 0 }
    })).toBe("扫描 3 个文件，入队 2 个，跳过 1 个；处理 2 个：归档 1，搁置 1，异常 0，失败 0。");

    expect(shortEvidence("{\"source\":\"example\",\"message\":\"not found\"}", 24)).toBe("{\"source\":\"example\",\"...");
    expect(shortEvidence("", 24)).toBe("无证据");
  });
});
```

- [ ] **Step 2: Run Vitest to verify it fails**

Run:

```powershell
npm test -- src/viewModel.test.ts
```

Expected: FAIL because helper functions do not exist.

- [ ] **Step 3: Implement TypeScript DTOs and helpers**

In `src/api.ts`:

- Extend `Work.watch_status` to include `"WantToWatch" | "Watching" | "OnHold"`.
- Add exported types:
  - `DaemonState`
  - `MetadataSource`
  - `DaemonControlStatus`
  - `DaemonRunOnceReport`
  - `HoldingReason`
  - `HoldingEntry`
  - `ExceptionKind`
  - `ExceptionStatus`
  - `ExceptionEntry`
  - `PipelineRun`
- Add API methods:

```ts
getDaemonStatus() { return command<DaemonControlStatus>("get_daemon_status"); },
pauseDaemon() { return command<DaemonControlStatus>("pause_daemon"); },
resumeDaemon() { return command<DaemonControlStatus>("resume_daemon"); },
runDaemonOnce() { return command<DaemonRunOnceReport>("run_daemon_once_command"); },
listHoldingEntries() { return command<HoldingEntry[]>("list_holding_entries"); },
listExceptionEntries() { return command<ExceptionEntry[]>("list_exception_entries"); },
resolveExceptionEntry(id: number, status: Exclude<ExceptionStatus, "Open">) {
  return command<boolean>("resolve_exception_entry_command", { id, status });
},
listPipelineRuns() { return command<PipelineRun[]>("list_pipeline_runs"); }
```

In `src/viewModel.ts`, import relevant types and add helper functions exactly matching the tests.

- [ ] **Step 4: Run frontend focused tests**

Run:

```powershell
npm test -- src/viewModel.test.ts
npx tsc --noEmit
```

Expected: both commands exit 0.

- [ ] **Step 5: Commit frontend type/helper slice**

Run:

```powershell
git add src/api.ts src/viewModel.ts src/viewModel.test.ts
git commit -m "补齐阶段4前端类型与格式化"
```

---

### Task 4: 设置页自动管线 UI

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Compile first to capture missing UI references**

Run:

```powershell
npx tsc --noEmit
```

Expected before edits: exits 0 from Task 3 baseline.

- [ ] **Step 2: Implement UI state and actions**

Modify `src/App.tsx` using `apply_patch` or node:fs only:

- Import new API types:
  - `DaemonControlStatus`
  - `DaemonRunOnceReport`
  - `ExceptionEntry`
  - `HoldingEntry`
  - `PipelineRun`
- Import view helpers:
  - `formatDaemonState`
  - `formatExceptionKind`
  - `formatExceptionStatus`
  - `formatHoldingReason`
  - `formatPipelineStatus`
  - `shortEvidence`
  - `summarizeRunOnceReport`
- Extend `watchStatusLabels` with `WantToWatch`、`Watching`、`OnHold`。
- Extend `settingsTab` union to `"pool" | "rebuild" | "migrate" | "cache" | "daemon"`。
- Add state:

```ts
const [daemonStatus, setDaemonStatus] = useState<DaemonControlStatus | null>(null);
const [daemonReport, setDaemonReport] = useState<DaemonRunOnceReport | null>(null);
const [holdingEntries, setHoldingEntries] = useState<HoldingEntry[]>([]);
const [exceptionEntries, setExceptionEntries] = useState<ExceptionEntry[]>([]);
const [pipelineRuns, setPipelineRuns] = useState<PipelineRun[]>([]);
const [daemonBusy, setDaemonBusy] = useState<"refresh" | "run" | "pause" | "resume" | "resolve" | null>(null);
```

- Add functions:
  - `refreshDaemonPanel`
  - `runDaemonOnce`
  - `pauseDaemon`
  - `resumeDaemon`
  - `resolveDaemonException`
- Add `useEffect` that calls `refreshDaemonPanel()` when `activeView === "settings" && settingsTab === "daemon"`。

- [ ] **Step 3: Render the 自动管线 tab**

Add a settings tab button:

```tsx
<button type="button" className={settingsTab === "daemon" ? "active" : ""} onClick={() => setSettingsTab("daemon")}>自动管线</button>
```

Add a section under `settings-panel`:

- Status summary: state/config/source roots/archive root/resource pool/metadata source/open counts.
- Buttons: refresh, run once, pause, resume.
- Holding list: top 10 rows.
- Exception list: top 10 rows with resolve/ignore buttons.
- Pipeline run list: top 10 rows.

Every button must be disabled while `daemonBusy` is non-null and must swap label text during its own operation.

- [ ] **Step 4: Run TypeScript/build checks**

Run:

```powershell
npx tsc --noEmit
npm run build
```

Expected: both commands exit 0.

- [ ] **Step 5: Commit UI slice**

Run:

```powershell
git add src/App.tsx
git commit -m "连线阶段4自动管线设置页"
```

---

### Task 5: 全量验证、评审记录、交接提交

**Files:**
- Modify: `HANDOFF.md`
- Modify local ignored state: `.ai_state/tasks.md`, `.ai_state/progress.md`, `.ai_state/reviews/sprint-4.md`, `.ai_state/lessons.md`

- [ ] **Step 1: Run full verification**

Run exactly:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

Expected: all exit 0. Do not run Tauri GUI, WebView2, `tauri dev`, default `cargo run`, or `media-manager.exe`.

- [ ] **Step 2: Self-review the diff**

Run:

```powershell
git diff --stat HEAD~3..HEAD
git diff HEAD~3..HEAD -- src-tauri/src/daemon_control.rs src-tauri/src/commands.rs src/api.ts src/viewModel.ts src/App.tsx
```

Check:

- No command starts WebView2.
- `run_daemon_once` refuses disabled example metadata before touching files.
- All new Rust public structs/functions have doc comments.
- Chinese source edits were done with `apply_patch`/node:fs, not PowerShell string writes.
- UI operations have loading/disabled feedback.

- [ ] **Step 3: Write review record**

Create `.ai_state/reviews/sprint-4.md`:

```markdown
# Sprint 4 Review

## Scope

阶段 4：Tauri 命令桥 + 设置页自动管线连线。

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`: PASS
- `npm test`: PASS
- `npx tsc --noEmit`: PASS
- `npm run build`: PASS

## Findings

- Main self-review: no unresolved findings.
- External reviewer: unavailable in this Codex setup; downgraded to self-review with full diff and full verification.

## Deferred

- Real network scraper remains later-stage work.
- HTTP/WebSocket/tray/autostart remain later-stage work.
```

- [ ] **Step 4: Update handoff and local state**

Update `HANDOFF.md`:

- Current progress says 阶段 4 已实现并验证。
- Add stage 4 deliverables.
- Add verification commands.
- Next step says 阶段 5 is real daemon/service hardening and real scraper/control API, unless user wants UI polish first.

Update `.ai_state/tasks.md` to Sprint 4 tasks all checked. Append `.ai_state/progress.md`. Add `.ai_state/lessons.md` entry only for real lessons discovered during implementation.

- [ ] **Step 5: Final commit**

Run:

```powershell
git add HANDOFF.md docs/superpowers/plans/2026-06-25-media-manager-refactor-stage4-control-ui.md
git commit -m "完成阶段4控制接口与前端连线"
```

If earlier task commits were already made, this final commit should contain only plan/handoff docs. If implementation commits are squashed later, keep Chinese commit messages.

---

## Plan Self-Review

- Spec coverage: covers backend command facade, parser fix, frontend DTO/API, settings UI, loading feedback, tests, no WebView2, no real scraper, no HTTP/WebSocket.
- Unfinished-marker scan: no unfinished-marker instructions; every task has concrete commands and expected result.
- Type consistency: Rust command names match frontend wrappers; `run_daemon_once_command` and `resolve_exception_entry_command` avoid name collision with pure helpers.
