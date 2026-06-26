# Stage 6F Self-Check Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a one-click automatic pipeline self-check that validates local sandbox archiving and summarizes configuration health without touching the user's real media library.

**Architecture:** Add a focused Rust `self_check` module that owns DTOs, config checks, and isolated sandbox execution. Wire it through a Tauri command with diagnostic logging, then expose a compact result panel in the existing settings/automatic-pipeline UI. Keep aria2 and remote scraper checks configuration-only in this stage.

**Tech Stack:** Rust, SQLite repository, existing daemon/control pipeline, Tauri commands, React, TypeScript, Vitest.

---

## File Map

- Create `src-tauri/src/self_check.rs`
  - DTOs: severity, item, sandbox summary, report.
  - Pure config checks for control service, roots, metadata source, aria2, remote scraper, diagnostics.
  - Isolated sandbox run using a temporary SQLite under app data.
- Modify `src-tauri/src/lib.rs`
  - Export `self_check`.
- Modify `src-tauri/src/daemon_control.rs`
  - Replace old metadata gate with actual scraper availability derived from remote settings and example fallback.
  - Keep existing public function signatures where possible.
- Modify `src-tauri/src/commands.rs`
  - Add `run_pipeline_self_check_command`.
  - Register the command.
  - Log start/completion/failure diagnostics.
- Create `src-tauri/tests/self_check.rs`
  - Backend TDD coverage for config checks and sandbox archive.
- Modify `src-tauri/tests/daemon_control.rs`
  - Regression coverage for remote/fallback metadata availability.
- Modify `src/api.ts`
  - Add self-check DTOs and API wrapper.
- Modify `src/viewModel.ts`
  - Add self-check formatting helpers.
- Modify `src/viewModel.test.ts`
  - Frontend formatting tests.
- Modify `src/App.tsx`
  - Add button, busy state, report state, result rows.
- Modify `src/styles.css`
  - Add self-check result row styling.
- Modify `.ai_state/tasks.md`, `.ai_state/progress.md`, `HANDOFF.md`
  - Keep local state and handoff current.

---

## Task 1: Backend DTOs and Configuration Checks

**Files:**
- Create: `src-tauri/src/self_check.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/self_check.rs`

- [ ] **Step 1: Write failing tests for config health**

Create `src-tauri/tests/self_check.rs` with:

```rust
use media_manager::aria2::Aria2Settings;
use media_manager::control_service_host::ControlServiceHostStatus;
use media_manager::daemon_control::{DaemonControlRuntime, build_daemon_status};
use media_manager::remote_scraper::{RemoteScraperSettings, RemoteScraperSourceSettings};
use media_manager::self_check::{SelfCheckSeverity, build_config_self_check_items};
use media_manager::storage::Repository;

fn open_repo(path: &std::path::Path) -> Repository {
    let repo = Repository::open(path).unwrap();
    repo.migrate().unwrap();
    repo
}

#[test]
fn config_self_check_reports_pass_warn_and_fail_items() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    repo.set_metadata_provider_enabled(false).unwrap();
    repo.set_aria2_settings(&Aria2Settings {
        enabled: true,
        host: "127.0.0.1".to_string(),
        port: 6800,
        path: "/jsonrpc".to_string(),
        secret: Some("secret".to_string()),
        timeout_ms: 5000,
        poll_interval_secs: 30,
        tracked_gids: Vec::new(),
    }).unwrap();
    repo.set_remote_scraper_settings(&RemoteScraperSettings {
        enabled: true,
        include_example_fallback: false,
        sources: vec![RemoteScraperSourceSettings {
            id: "javdb".to_string(),
            enabled: true,
            search_url_template: "https://example.test/search".to_string(),
            min_confidence: 0.8,
        }],
        ..RemoteScraperSettings::default()
    }).unwrap();
    let runtime = DaemonControlRuntime::default();
    let daemon = build_daemon_status(&repo, &runtime, false).unwrap();
    let control = ControlServiceHostStatus {
        running: true,
        host: Some("127.0.0.1".to_string()),
        port: Some(13508),
        discovery_path: Some(tmp.path().join("control-service.json").to_string_lossy().to_string()),
        last_error: None,
    };

    let checks = build_config_self_check_items(&repo, Some(control), Some(daemon), true).unwrap();

    assert!(checks.iter().any(|item| item.id == "control_service" && item.severity == SelfCheckSeverity::Pass));
    assert!(checks.iter().any(|item| item.id == "configured_roots" && item.severity == SelfCheckSeverity::Warn));
    assert!(checks.iter().any(|item| item.id == "metadata_source" && item.severity == SelfCheckSeverity::Fail));
    assert!(checks.iter().any(|item| item.id == "aria2_settings" && item.severity == SelfCheckSeverity::Warn));
    assert!(checks.iter().any(|item| item.id == "remote_scraper_settings" && item.severity == SelfCheckSeverity::Fail));
    assert!(checks.iter().any(|item| item.id == "diagnostics" && item.severity == SelfCheckSeverity::Pass));
}
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test self_check config_self_check_reports_pass_warn_and_fail_items -j 1
```

Expected: FAIL because `media_manager::self_check` does not exist.

- [ ] **Step 3: Implement DTOs and config checks**

Add `pub mod self_check;` to `src-tauri/src/lib.rs`.

Create `src-tauri/src/self_check.rs` with class/method comments on every new type/function:

```rust
use crate::aria2::Aria2Settings;
use crate::control_service_host::ControlServiceHostStatus;
use crate::daemon_control::DaemonControlStatus;
use crate::remote_scraper::RemoteScraperSettings;
use crate::storage::Repository;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelfCheckSeverity {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfCheckItem {
    pub id: String,
    pub title: String,
    pub severity: SelfCheckSeverity,
    pub message: String,
    pub action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfCheckSandboxSummary {
    pub root: String,
    pub inbox: String,
    pub archive: String,
    pub video_path: String,
    pub archived_path: Option<String>,
    pub pipeline_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelfCheckOverall {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfCheckReport {
    pub generated_at: String,
    pub overall: SelfCheckOverall,
    pub checks: Vec<SelfCheckItem>,
    pub sandbox: Option<SelfCheckSandboxSummary>,
}

pub fn build_config_self_check_items(
    repo: &Repository,
    control_service: Option<ControlServiceHostStatus>,
    daemon: Option<DaemonControlStatus>,
    diagnostics_available: bool,
) -> Result<Vec<SelfCheckItem>> {
    let mut checks = Vec::new();
    checks.push(check_control_service(control_service));
    checks.push(check_configured_roots(repo)?);
    checks.push(check_metadata_source(repo)?);
    checks.push(check_aria2_settings(&repo.get_aria2_settings()?));
    checks.push(check_remote_scraper_settings(&repo.get_remote_scraper_settings()?));
    checks.push(check_diagnostics(diagnostics_available));
    if let Some(status) = daemon {
        if status.last_error.is_some() {
            checks.push(SelfCheckItem {
                id: "daemon_last_error".to_string(),
                title: "最近运行错误".to_string(),
                severity: SelfCheckSeverity::Warn,
                message: status.last_error.unwrap_or_default(),
                action: Some("如沙盒自检通过，可先把 aria2/远程 scraper 错误按分项处理。".to_string()),
            });
        }
    }
    Ok(checks)
}
```

Implement helpers with deterministic ids and messages:

- `control_service`: pass if running, warn otherwise.
- `configured_roots`: pass if source roots non-empty and archive root set; warn otherwise.
- `metadata_source`: pass if actual metadata availability exists; fail otherwise.
- `aria2_settings`: pass when disabled or enabled with GID and no secret warning; warn when enabled with no GID or secret configured; fail only if normalization errors.
- `remote_scraper_settings`: pass when disabled with fallback or enabled source valid; warn for suspicious proxy scheme; fail for invalid templates or no source/fallback.
- `diagnostics`: pass when writer is available, warn otherwise.

- [ ] **Step 4: Run focused test and verify GREEN**

Run the same cargo command. Expected: PASS.

- [ ] **Step 5: Commit Task 1**

```powershell
git add src-tauri/src/lib.rs src-tauri/src/self_check.rs src-tauri/tests/self_check.rs
git commit -m "实现阶段6F自检配置检查"
```

---

## Task 2: Sandbox Archive Self-Check and Metadata Availability Fix

**Files:**
- Modify: `src-tauri/src/self_check.rs`
- Modify: `src-tauri/src/daemon_control.rs`
- Test: `src-tauri/tests/self_check.rs`
- Test: `src-tauri/tests/daemon_control.rs`

- [ ] **Step 1: Write failing sandbox test**

Append to `src-tauri/tests/self_check.rs`:

```rust
use media_manager::self_check::{run_pipeline_self_check, SelfCheckOverall};

#[test]
fn pipeline_self_check_archives_sandbox_video_without_touching_real_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let app_data = tmp.path().join("app-data");
    let real_repo = open_repo(&tmp.path().join("real.sqlite"));
    real_repo.set_metadata_provider_enabled(false).unwrap();
    real_repo.set_remote_scraper_settings(&RemoteScraperSettings {
        enabled: false,
        include_example_fallback: true,
        ..RemoteScraperSettings::default()
    }).unwrap();

    let report = run_pipeline_self_check(&app_data, &real_repo, None, None, true).unwrap();

    assert_eq!(report.overall, SelfCheckOverall::Pass);
    assert!(report.checks.iter().any(|item| item.id == "sandbox_archive" && item.severity == SelfCheckSeverity::Pass));
    let sandbox = report.sandbox.unwrap();
    let archived = sandbox.archived_path.unwrap();
    assert!(std::path::Path::new(&archived).exists());
    assert_eq!(sandbox.pipeline_status.as_deref(), Some("archived"));
    assert_eq!(real_repo.list_works().unwrap().len(), 0);
    assert_eq!(real_repo.list_pipeline_runs().unwrap().len(), 0);
}
```

- [ ] **Step 2: Write failing metadata availability regression**

Append to `src-tauri/tests/daemon_control.rs`:

```rust
#[test]
fn run_once_allows_example_fallback_when_legacy_metadata_toggle_is_off() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, archive, _assets) = configured_repo(&tmp);
    let video = inbox.join("ABP-701.mp4");
    std::fs::write(&video, b"stable video bytes").unwrap();
    repo.set_remote_scraper_settings(&RemoteScraperSettings {
        enabled: false,
        include_example_fallback: true,
        ..RemoteScraperSettings::default()
    }).unwrap();
    let mut runtime = DaemonControlRuntime::default();

    let report = run_daemon_once_with_transports(
        &repo,
        &mut runtime,
        false,
        CommandAria2Transport { response: "{}".to_string() },
        CommandRemoteClient { html: String::new() },
    ).unwrap();

    assert_eq!(report.process.archived, 1);
    assert!(archive.join("ABP-701").join("ABP-701.mp4").exists());
}
```

- [ ] **Step 3: Run focused tests and verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test self_check pipeline_self_check_archives_sandbox_video_without_touching_real_repo -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test daemon_control run_once_allows_example_fallback_when_legacy_metadata_toggle_is_off -j 1
```

Expected: first fails because sandbox runner is not implemented; second fails with the old metadata gate.

- [ ] **Step 4: Implement sandbox runner**

In `src-tauri/src/self_check.rs`, implement:

```rust
pub fn run_pipeline_self_check(
    app_data_dir: &std::path::Path,
    repo: &Repository,
    control_service: Option<ControlServiceHostStatus>,
    daemon: Option<DaemonControlStatus>,
    diagnostics_available: bool,
) -> Result<SelfCheckReport> {
    let mut checks = build_config_self_check_items(repo, control_service, daemon, diagnostics_available)?;
    let sandbox = run_sandbox_archive_check(app_data_dir, &mut checks);
    let overall = summarize_overall(&checks);
    Ok(SelfCheckReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        overall,
        checks,
        sandbox,
    })
}
```

`run_sandbox_archive_check` must:

- Create `app_data_dir/self-check/<YYYYMMDD-HHMMSSfff>/`.
- Open and migrate `library.sqlite` inside that directory.
- Configure sandbox repo source/archive/assets.
- Persist `metadata_provider_enabled = false`, remote scraper fallback enabled, aria2 disabled.
- Write `MMT-001.mp4`.
- Run `run_daemon_once_with_transports` with fake aria2 transport returning `{}` and fake remote client returning empty HTML.
- Push `sandbox_archive` pass/fail item.

- [ ] **Step 5: Implement actual metadata availability**

In `src-tauri/src/daemon_control.rs`:

- Add an internal `MetadataSourceAvailability` struct or helper.
- Change `ConfiguredPipelineScrapers::with_remote_client` to:
  - normalize remote settings first;
  - build remote sources only when enabled source exists;
  - push `ExamplePipelineScraper` when `metadata_enabled == true` or `include_example_fallback == true`;
  - bail only when no source exists.
- Keep `run_daemon_once` signature unchanged.

- [ ] **Step 6: Run focused tests and verify GREEN**

Run both commands from Step 3. Expected: PASS.

- [ ] **Step 7: Commit Task 2**

```powershell
git add src-tauri/src/self_check.rs src-tauri/src/daemon_control.rs src-tauri/tests/self_check.rs src-tauri/tests/daemon_control.rs
git commit -m "实现阶段6F沙盒归档自检"
```

---

## Task 3: Tauri Command and Diagnostic Logging

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Test: focused commands unit tests inside `commands.rs`

- [ ] **Step 1: Write failing command test**

Inside `#[cfg(test)] mod tests` in `src-tauri/src/commands.rs`, add a unit test around a helper:

```rust
#[test]
fn pipeline_self_check_command_returns_report_with_sandbox_result() {
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState::new();
    let repo = Repository::open(&tmp.path().join("library.sqlite")).unwrap();
    repo.migrate().unwrap();
    repo.set_remote_scraper_settings(&RemoteScraperSettings {
        enabled: false,
        include_example_fallback: true,
        ..RemoteScraperSettings::default()
    }).unwrap();
    *state.repository.lock().unwrap() = Some(repo);

    let response = run_pipeline_self_check_in_state(tmp.path(), &state).unwrap();

    assert_eq!(response.data.overall, SelfCheckOverall::Pass);
    assert!(response.data.checks.iter().any(|item| item.id == "sandbox_archive"));
}
```

- [ ] **Step 2: Run command test and verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::pipeline_self_check_command_returns_report_with_sandbox_result -j 1
```

Expected: FAIL because helper/command does not exist.

- [ ] **Step 3: Implement command helper and Tauri command**

In `commands.rs`:

- Import `SelfCheckReport` and `run_pipeline_self_check`.
- Add private helper:

```rust
fn run_pipeline_self_check_in_state(
    app_data: &Path,
    state: &AppState,
) -> Result<CommandResult<SelfCheckReport>, String> { ... }
```

- Add Tauri command:

```rust
#[tauri::command]
pub fn run_pipeline_self_check_command(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<CommandResult<SelfCheckReport>, String> { ... }
```

- Register it in `generate_handler!`.
- Log started/completed/failed with target `self_check.run`.

- [ ] **Step 4: Run focused test and verify GREEN**

Run the command from Step 2. Expected: PASS.

- [ ] **Step 5: Commit Task 3**

```powershell
git add src-tauri/src/commands.rs
git commit -m "接入阶段6F自检命令"
```

---

## Task 4: Frontend API, Formatting, and UI

**Files:**
- Modify: `src/api.ts`
- Modify: `src/viewModel.ts`
- Modify: `src/viewModel.test.ts`
- Modify: `src/App.tsx`
- Modify: `src/styles.css`

- [ ] **Step 1: Write failing viewModel tests**

Append to `src/viewModel.test.ts`:

```ts
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
```

- [ ] **Step 2: Run frontend focused test and verify RED**

Run:

```powershell
npm test -- src/viewModel.test.ts
```

Expected: FAIL because self-check helpers/types do not exist.

- [ ] **Step 3: Add API DTO and helper functions**

In `src/api.ts`, add:

```ts
export type SelfCheckSeverity = "pass" | "warn" | "fail";
export type SelfCheckOverall = SelfCheckSeverity;

export interface SelfCheckItem {
  id: string;
  title: string;
  severity: SelfCheckSeverity;
  message: string;
  action?: string | null;
}

export interface SelfCheckSandboxSummary {
  root: string;
  inbox: string;
  archive: string;
  video_path: string;
  archived_path?: string | null;
  pipeline_status?: string | null;
}

export interface SelfCheckReport {
  generated_at: string;
  overall: SelfCheckOverall;
  checks: SelfCheckItem[];
  sandbox?: SelfCheckSandboxSummary | null;
}
```

Add API wrapper:

```ts
runPipelineSelfCheck() {
  return command<SelfCheckReport>("run_pipeline_self_check_command");
}
```

In `src/viewModel.ts`, add:

```ts
export function formatSelfCheckSeverity(severity: SelfCheckSeverity): string { ... }
export function formatSelfCheckOverall(overall: SelfCheckOverall): string { ... }
export function formatSelfCheckSummary(report: SelfCheckReport): string { ... }
```

- [ ] **Step 4: Run focused frontend test and verify GREEN**

Run `npm test -- src/viewModel.test.ts`. Expected: PASS.

- [ ] **Step 5: Add App UI**

In `src/App.tsx`:

- Import `SelfCheckReport`, `formatSelfCheckSeverity`, `formatSelfCheckSummary`.
- Add:

```ts
const [selfCheckReport, setSelfCheckReport] = useState<SelfCheckReport | null>(null);
const [selfCheckBusy, setSelfCheckBusy] = useState(false);
```

- Add handler:

```ts
async function runPipelineSelfCheck() {
  if (selfCheckBusy) return;
  setSelfCheckBusy(true);
  setStatus("正在执行自动管线自检...");
  try {
    const report = await api.runPipelineSelfCheck();
    setSelfCheckReport(report);
    setStatus(formatSelfCheckSummary(report));
    await loadDaemonPanelData();
  } catch (error) {
    setStatus(`自动管线自检失败：${String(error)}`);
  } finally {
    setSelfCheckBusy(false);
  }
}
```

- Add button near “刷新状态/运行一轮”.
- Add result panel before diagnostics panel.
- Disable button while `selfCheckBusy` or no backend.

In `src/styles.css`, add `.self-check-panel`, `.self-check-row`, `.self-check-row.pass|warn|fail`.

- [ ] **Step 6: Run TypeScript check**

Run:

```powershell
npx tsc --noEmit
```

Expected: PASS.

- [ ] **Step 7: Commit Task 4**

```powershell
git add src/api.ts src/viewModel.ts src/viewModel.test.ts src/App.tsx src/styles.css
git commit -m "接入阶段6F前端自检面板"
```

---

## Task 5: Full Verification and Handoff

**Files:**
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/progress.md`
- Modify: `.ai_state/lessons.md`
- Modify: `.ai_state/reviews/sprint-13.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Run focused backend suites**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test self_check -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test daemon_control -j 1
```

Expected: PASS.

- [ ] **Step 2: Run full Rust tests**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
```

Expected: PASS. Historical `resource_pool.rs` warning may remain.

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
git diff -- src-tauri/src/self_check.rs src-tauri/src/daemon_control.rs src-tauri/src/commands.rs src/api.ts src/viewModel.ts src/App.tsx
```

Check:

- No WebView2/Tauri GUI invocation.
- No real aria2 or scraper network call in tests.
- Sandbox uses isolated SQLite.
- Every new Rust type/function has a useful comment.
- Chinese source was edited only via `apply_patch` or safe tooling.

- [ ] **Step 5: Update handoff and lessons**

Update `HANDOFF.md` with 6F completion and verification commands.

Append one real lesson to `.ai_state/lessons.md`, for example about optional integrations reporting warnings rather than poisoning the core archive status.

- [ ] **Step 6: Commit verification docs**

```powershell
git add HANDOFF.md
git commit -m "更新阶段6F交接说明"
```

- [ ] **Step 7: Push branch**

```powershell
git push origin codex/stage2-auto-pipeline
```

Expected: push succeeds.
