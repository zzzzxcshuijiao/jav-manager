# Stage 6C Aria2 Config And Polling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist aria2 RPC settings, track user-provided GIDs, and make the existing "run once" daemon path poll aria2 before directory scanning.

**Architecture:** Extend the existing `aria2` module with settings/report DTOs, store the settings as one JSON value in `app_settings`, add a daemon `poll_aria2_once` method that reuses the Stage 6B completion extraction, then expose settings through Tauri commands and the automatic pipeline settings UI. Keep polling manual and run-once scoped; do not add background threads, WebSocket, aria2 process management, or download task management.

**Tech Stack:** Rust 2021, rusqlite 0.32, serde/serde_json, existing Tauri command bridge, React/TypeScript, Vitest, `cargo test -j 1`.

---

## Global Constraints

- Do not run `tauri dev`, default `cargo run`, `media-manager.exe`, or any WebView2/in-app browser path.
- Rust verification on this host must prepend `C:\Users\DELL\.cargo\bin` and use `-j 1`.
- All aria2 tests use fake transport and temp files; no real aria2 process, real H:/ videos, or live network.
- Files containing Chinese UI text must be edited with `apply_patch` or node:fs, not PowerShell string replacement.
- New public Rust structs/methods need doc comments.

---

## File Structure

- **Modify** `src-tauri/src/aria2.rs`
  - Add `Aria2Settings`, `Aria2PollReport`, normalization/validation, and endpoint conversion.
- **Modify** `src-tauri/src/storage.rs`
  - Add `Repository::set_aria2_settings` and `Repository::get_aria2_settings`.
- **Modify** `src-tauri/src/daemon.rs`
  - Add `HeadlessDaemon::poll_aria2_once` and a helper that queues files from an `Aria2Status`.
  - Extend `RunOnceReport` with `aria2: Aria2PollReport`.
- **Modify** `src-tauri/src/daemon_control.rs`
  - Add a generic `run_daemon_once_with_aria2_transport` for tests.
  - Make `run_daemon_once` call it with `HttpAria2Transport`.
- **Modify** `src-tauri/src/commands.rs`
  - Add AppState storage, setup loading, `configure_aria2_settings`, and `get_aria2_settings`.
- **Modify** `src/api.ts`
  - Add `Aria2Settings`, `Aria2PollReport`, settings commands, and `DaemonRunOnceReport.aria2`.
- **Modify** `src/daemonClient.ts` and `src/daemonClient.test.ts`
  - Preserve the expanded run-once DTO through service-first routing.
- **Modify** `src/viewModel.ts` and `src/viewModel.test.ts`
  - Include aria2 poll summary in `summarizeRunOnceReport`.
- **Modify** `src/App.tsx` and `src/styles.css`
  - Add compact aria2 settings controls in the automatic pipeline panel.
- **Modify** `.ai_state/*`, `HANDOFF.md`, and `.ai_state/reviews/sprint-10.md`
  - Track progress, review, lessons, and handoff.

---

## Interfaces To Produce

```rust
pub struct Aria2Settings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub secret: Option<String>,
    pub timeout_ms: u64,
    pub poll_interval_secs: u64,
    pub tracked_gids: Vec<String>,
}

pub struct Aria2PollReport {
    pub enabled: bool,
    pub attempted_gids: usize,
    pub completed_gids: usize,
    pub queued_files: usize,
    pub skipped_files: usize,
    pub failed_gids: usize,
    pub errors: Vec<String>,
}
```

```rust
impl Repository {
    pub fn set_aria2_settings(&self, settings: &Aria2Settings) -> Result<Aria2Settings>;
    pub fn get_aria2_settings(&self) -> Result<Aria2Settings>;
}
```

```rust
impl<'a> HeadlessDaemon<'a> {
    pub fn poll_aria2_once<T: Aria2Transport>(
        &mut self,
        settings: &Aria2Settings,
        transport: T,
    ) -> Result<Aria2PollReport>;
}
```

---

## Task 1: Persist aria2 settings

**Files:**
- Modify: `src-tauri/src/aria2.rs`
- Modify: `src-tauri/src/storage.rs`
- Modify: `src-tauri/tests/core_behaviour.rs`
- Modify: `src-tauri/tests/aria2_rpc.rs`

- [ ] **Step 1: Write failing storage tests**

Append to `src-tauri/tests/core_behaviour.rs`:

```rust
use media_manager::aria2::Aria2Settings;

#[test]
fn repository_defaults_and_persists_aria2_settings() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    {
        let repo = Repository::open(&db_path).unwrap();
        repo.migrate().unwrap();
        let defaults = repo.get_aria2_settings().unwrap();
        assert!(!defaults.enabled);
        assert_eq!(defaults.host, "127.0.0.1");
        assert_eq!(defaults.port, 6800);
        assert_eq!(defaults.path, "/jsonrpc");

        repo.set_aria2_settings(&Aria2Settings {
            enabled: true,
            host: " localhost ".to_string(),
            port: 6801,
            path: "jsonrpc".to_string(),
            secret: Some("  ".to_string()),
            timeout_ms: 7000,
            poll_interval_secs: 45,
            tracked_gids: vec![
                " gid-1 ".to_string(),
                "gid-1".to_string(),
                "".to_string(),
                "gid-2".to_string(),
            ],
        })
        .unwrap();
    }

    let reopened = Repository::open(&db_path).unwrap();
    reopened.migrate().unwrap();
    let settings = reopened.get_aria2_settings().unwrap();

    assert!(settings.enabled);
    assert_eq!(settings.host, "localhost");
    assert_eq!(settings.port, 6801);
    assert_eq!(settings.path, "/jsonrpc");
    assert_eq!(settings.secret, None);
    assert_eq!(settings.timeout_ms, 7000);
    assert_eq!(settings.poll_interval_secs, 45);
    assert_eq!(settings.tracked_gids, vec!["gid-1", "gid-2"]);
}

#[test]
fn repository_rejects_invalid_aria2_settings() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&temp.path().join("library.sqlite")).unwrap();
    repo.migrate().unwrap();

    let mut settings = Aria2Settings::default();
    settings.enabled = true;
    settings.host = " ".to_string();

    let error = repo.set_aria2_settings(&settings).unwrap_err();

    assert!(error.to_string().contains("aria2 host is required"));
}
```

- [ ] **Step 2: Write failing endpoint conversion test**

Append to `src-tauri/tests/aria2_rpc.rs`:

```rust
use media_manager::aria2::Aria2Settings;

#[test]
fn aria2_settings_build_endpoint_with_normalized_path_and_secret() {
    let settings = Aria2Settings {
        enabled: true,
        host: "127.0.0.1".to_string(),
        port: 6800,
        path: "jsonrpc".to_string(),
        secret: Some("secret".to_string()),
        timeout_ms: 9000,
        poll_interval_secs: 30,
        tracked_gids: vec![],
    };

    let endpoint = settings.normalized().unwrap().endpoint().unwrap();

    assert_eq!(endpoint.host, "127.0.0.1");
    assert_eq!(endpoint.port, 6800);
    assert_eq!(endpoint.path, "/jsonrpc");
    assert_eq!(endpoint.secret.as_deref(), Some("secret"));
    assert_eq!(endpoint.timeout_ms, 9000);
}
```

- [ ] **Step 3: Run tests to verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test core_behaviour aria2_settings -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test aria2_rpc aria2_settings -j 1
```

Expected: compile errors for missing `Aria2Settings` and repository methods.

- [ ] **Step 4: Implement settings DTO and storage helpers**

In `src-tauri/src/aria2.rs`, add serde derives to `Aria2RpcEndpoint` if needed, then append:

```rust
/// User-editable aria2 RPC settings persisted in SQLite app_settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aria2Settings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub secret: Option<String>,
    pub timeout_ms: u64,
    pub poll_interval_secs: u64,
    pub tracked_gids: Vec<String>,
}

impl Default for Aria2Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "127.0.0.1".to_string(),
            port: 6800,
            path: "/jsonrpc".to_string(),
            secret: None,
            timeout_ms: 5_000,
            poll_interval_secs: 30,
            tracked_gids: Vec::new(),
        }
    }
}

impl Aria2Settings {
    /// Return a validated copy with trimmed strings, normalized path, empty
    /// secret removed, and duplicate GIDs collapsed in first-seen order.
    pub fn normalized(&self) -> Result<Self> {
        let host = self.host.trim().to_string();
        if host.is_empty() {
            return Err(anyhow!("aria2 host is required"));
        }
        if self.port == 0 {
            return Err(anyhow!("aria2 port is required"));
        }
        let raw_path = self.path.trim();
        if raw_path.is_empty() {
            return Err(anyhow!("aria2 RPC path is required"));
        }
        if self.timeout_ms == 0 {
            return Err(anyhow!("aria2 timeout_ms must be greater than zero"));
        }
        if self.poll_interval_secs == 0 {
            return Err(anyhow!("aria2 poll_interval_secs must be greater than zero"));
        }
        let mut tracked_gids = Vec::new();
        for gid in &self.tracked_gids {
            let trimmed = gid.trim();
            if !trimmed.is_empty() && !tracked_gids.iter().any(|existing| existing == trimmed) {
                tracked_gids.push(trimmed.to_string());
            }
        }
        Ok(Self {
            enabled: self.enabled,
            host,
            port: self.port,
            path: if raw_path.starts_with('/') {
                raw_path.to_string()
            } else {
                format!("/{raw_path}")
            },
            secret: self
                .secret
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            timeout_ms: self.timeout_ms,
            poll_interval_secs: self.poll_interval_secs,
            tracked_gids,
        })
    }

    /// Convert the persisted settings into the Stage 6B RPC endpoint DTO.
    pub fn endpoint(&self) -> Result<Aria2RpcEndpoint> {
        let normalized = self.normalized()?;
        Ok(Aria2RpcEndpoint {
            host: normalized.host,
            port: normalized.port,
            path: normalized.path,
            secret: normalized.secret,
            timeout_ms: normalized.timeout_ms,
        })
    }
}
```

In `src-tauri/src/storage.rs`, import `Aria2Settings` and add methods near existing setting helpers:

```rust
    /// Persist normalized aria2 settings as one JSON app_settings value.
    pub fn set_aria2_settings(&self, settings: &Aria2Settings) -> Result<Aria2Settings> {
        let normalized = settings.normalized()?;
        self.set_setting("aria2_settings", &serde_json::to_string(&normalized)?)?;
        Ok(normalized)
    }

    /// Read aria2 settings, returning safe defaults when not configured.
    pub fn get_aria2_settings(&self) -> Result<Aria2Settings> {
        let Some(value) = self.get_setting("aria2_settings")? else {
            return Ok(Aria2Settings::default());
        };
        let settings: Aria2Settings = serde_json::from_str(&value)?;
        settings.normalized()
    }
```

- [ ] **Step 5: Run tests to verify GREEN**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test core_behaviour aria2_settings -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test aria2_rpc aria2_settings -j 1
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/aria2.rs src-tauri/src/storage.rs src-tauri/tests/core_behaviour.rs src-tauri/tests/aria2_rpc.rs
git commit -m "持久化阶段6C aria2配置"
```

---

## Task 2: Add daemon aria2 poll aggregation

**Files:**
- Modify: `src-tauri/src/aria2.rs`
- Modify: `src-tauri/src/daemon.rs`
- Modify: `src-tauri/tests/daemon.rs`

- [ ] **Step 1: Write failing daemon poll tests**

Append to `src-tauri/tests/daemon.rs`:

```rust
use media_manager::aria2::{Aria2PollReport, Aria2Settings};

#[derive(Clone)]
struct RoutingAria2Transport {
    video_path: std::path::PathBuf,
    calls: Arc<Mutex<Vec<String>>>,
}

impl RoutingAria2Transport {
    fn new(video_path: std::path::PathBuf) -> Self {
        Self {
            video_path,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Aria2Transport for RoutingAria2Transport {
    fn post_json(&self, _endpoint: &Aria2RpcEndpoint, body: &str) -> anyhow::Result<String> {
        let request: serde_json::Value = serde_json::from_str(body).unwrap();
        let gid = request["params"][0].as_str().unwrap().to_string();
        self.calls.lock().unwrap().push(gid.clone());
        if gid == "gid-error" {
            anyhow::bail!("rpc failed for gid-error");
        }
        let complete = gid == "gid-complete";
        Ok(serde_json::json!({
            "jsonrpc": "2.0",
            "id": "media-manager-tell-status",
            "result": {
                "gid": gid,
                "status": if complete { "complete" } else { "active" },
                "totalLength": "9",
                "completedLength": if complete { "9" } else { "3" },
                "files": if complete {
                    serde_json::json!([
                        {"path": self.video_path.to_string_lossy().to_string(), "length": "9", "completedLength": "9", "selected": "true"}
                    ])
                } else {
                    serde_json::json!([])
                }
            }
        })
        .to_string())
    }
}

fn enabled_aria2_settings(gids: Vec<&str>) -> Aria2Settings {
    Aria2Settings {
        enabled: true,
        host: "127.0.0.1".to_string(),
        port: 6800,
        path: "/jsonrpc".to_string(),
        secret: None,
        timeout_ms: 5000,
        poll_interval_secs: 30,
        tracked_gids: gids.into_iter().map(ToString::to_string).collect(),
    }
}

#[test]
fn poll_aria2_once_aggregates_completed_unfinished_and_failed_gids() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    let video = inbox.join("ABP-300.mp4");
    std::fs::write(&video, b"video-300").unwrap();
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);
    let transport = RoutingAria2Transport::new(video);

    let report = daemon
        .poll_aria2_once(
            &enabled_aria2_settings(vec!["gid-complete", "gid-active", "gid-error"]),
            transport,
        )
        .unwrap();

    assert_eq!(report.enabled, true);
    assert_eq!(report.attempted_gids, 3);
    assert_eq!(report.completed_gids, 1);
    assert_eq!(report.queued_files, 1);
    assert_eq!(report.failed_gids, 1);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(daemon.status().queued, 1);
    assert_eq!(daemon.status().state, DaemonState::Error);
}

#[test]
fn poll_aria2_once_skips_transport_when_disabled_or_paused() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);
    let transport = RoutingAria2Transport::new(inbox.join("ABP-300.mp4"));

    let disabled = daemon
        .poll_aria2_once(&Aria2Settings::default(), transport.clone())
        .unwrap();
    daemon.pause();
    let paused = daemon
        .poll_aria2_once(&enabled_aria2_settings(vec!["gid-complete"]), transport.clone())
        .unwrap();

    assert_eq!(disabled, Aria2PollReport::default());
    assert_eq!(paused.attempted_gids, 0);
    assert!(transport.calls.lock().unwrap().is_empty());
}
```

- [ ] **Step 2: Run daemon tests to verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test daemon poll_aria2_once -j 1
```

Expected: compile errors for missing `Aria2PollReport` and `poll_aria2_once`.

- [ ] **Step 3: Implement poll report and daemon polling**

In `src-tauri/src/aria2.rs`, append:

```rust
/// Summary of one configured aria2 polling pass.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aria2PollReport {
    pub enabled: bool,
    pub attempted_gids: usize,
    pub completed_gids: usize,
    pub queued_files: usize,
    pub skipped_files: usize,
    pub failed_gids: usize,
    pub errors: Vec<String>,
}
```

In `src-tauri/src/daemon.rs`:

- Import `Aria2PollReport`, `Aria2Settings`, `Aria2Status`.
- Extend `RunOnceReport`:

```rust
pub struct RunOnceReport {
    pub scan: ScanReport,
    pub process: ProcessReport,
    pub aria2: Aria2PollReport,
}
```

- Add `poll_aria2_once`, refactoring `scan_aria2_gid_inner` through a shared status queue helper:

```rust
    /// Poll configured aria2 GIDs once and enqueue completed selected videos.
    pub fn poll_aria2_once<T: Aria2Transport>(
        &mut self,
        settings: &Aria2Settings,
        transport: T,
    ) -> Result<Aria2PollReport> {
        if self.state == DaemonState::Paused || !settings.enabled {
            return Ok(Aria2PollReport::default());
        }
        let settings = settings.normalized()?;
        let mut report = Aria2PollReport {
            enabled: true,
            ..Aria2PollReport::default()
        };
        if settings.tracked_gids.is_empty() {
            return Ok(report);
        }
        self.state = DaemonState::Scanning;
        let client = Aria2Client::new(settings.endpoint()?, transport);
        for gid in &settings.tracked_gids {
            report.attempted_gids += 1;
            match client.tell_status(gid) {
                Ok(status) => {
                    let complete = status.is_complete()?;
                    let scan = self.queue_aria2_status(status)?;
                    if complete {
                        report.completed_gids += 1;
                    }
                    report.queued_files += scan.queued_files;
                    report.skipped_files += scan.skipped_files;
                }
                Err(error) => {
                    report.failed_gids += 1;
                    report.errors.push(format!("{gid}: {error}"));
                }
            }
        }
        if report.failed_gids > 0 {
            self.state = DaemonState::Error;
            self.last_error = report.errors.first().cloned();
        } else {
            self.state = DaemonState::Idle;
        }
        Ok(report)
    }
```

- Keep `scan_aria2_gid` behavior by making its inner helper call `queue_aria2_status`.

- [ ] **Step 4: Run daemon tests to verify GREEN**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/aria2.rs src-tauri/src/daemon.rs src-tauri/tests/daemon.rs
git commit -m "实现阶段6C aria2单轮轮询"
```

---

## Task 3: Wire aria2 polling into run-once

**Files:**
- Modify: `src-tauri/src/daemon_control.rs`
- Modify: `src-tauri/src/control_service.rs` if response assumptions need updates
- Modify: `src-tauri/tests/daemon_control.rs`
- Modify: `src-tauri/tests/control_service.rs`

- [ ] **Step 1: Write failing daemon_control test**

Append to `src-tauri/tests/daemon_control.rs`:

```rust
use media_manager::aria2::{Aria2RpcEndpoint, Aria2Settings, Aria2Transport};

#[derive(Clone)]
struct CommandAria2Transport {
    response: String,
}

impl Aria2Transport for CommandAria2Transport {
    fn post_json(&self, _endpoint: &Aria2RpcEndpoint, _body: &str) -> anyhow::Result<String> {
        Ok(self.response.clone())
    }
}

#[test]
fn run_once_polls_configured_aria2_gid_before_directory_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, archive, _assets) = configured_repo(&tmp);
    let video = inbox.join("ABP-404.mp4");
    std::fs::write(&video, b"stable video bytes").unwrap();
    repo.set_aria2_settings(&Aria2Settings {
        enabled: true,
        host: "127.0.0.1".to_string(),
        port: 6800,
        path: "/jsonrpc".to_string(),
        secret: None,
        timeout_ms: 5000,
        poll_interval_secs: 30,
        tracked_gids: vec!["gid-404".to_string()],
    })
    .unwrap();
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "media-manager-tell-status",
        "result": {
            "gid": "gid-404",
            "status": "complete",
            "totalLength": "18",
            "completedLength": "18",
            "files": [
                {"path": video.to_string_lossy().to_string(), "length": "18", "completedLength": "18", "selected": "true"}
            ]
        }
    })
    .to_string();
    let mut runtime = DaemonControlRuntime::default();

    let report = run_daemon_once_with_aria2_transport(
        &repo,
        &mut runtime,
        true,
        CommandAria2Transport { response },
    )
    .unwrap();

    assert_eq!(report.aria2.enabled, true);
    assert_eq!(report.aria2.attempted_gids, 1);
    assert_eq!(report.aria2.queued_files, 1);
    assert_eq!(report.process.archived, 1);
    assert!(archive.join("ABP-404").join("ABP-404.mp4").exists());
}
```

- [ ] **Step 2: Run focused test to verify RED**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test daemon_control run_once_polls_configured_aria2_gid -j 1
```

Expected: compile error for missing `run_daemon_once_with_aria2_transport` and `RunOnceReport.aria2`.

- [ ] **Step 3: Implement command-level integration**

In `src-tauri/src/daemon_control.rs`:

```rust
use crate::aria2::{Aria2Transport, HttpAria2Transport};
```

Change `run_daemon_once` to:

```rust
pub fn run_daemon_once(
    repo: &Repository,
    runtime: &mut DaemonControlRuntime,
    metadata_enabled: bool,
) -> Result<RunOnceReport> {
    run_daemon_once_with_aria2_transport(repo, runtime, metadata_enabled, HttpAria2Transport)
}
```

Add:

```rust
/// Run one daemon pass with an injectable aria2 transport so tests do not need
/// a real aria2 process.
pub fn run_daemon_once_with_aria2_transport<T: Aria2Transport>(
    repo: &Repository,
    runtime: &mut DaemonControlRuntime,
    metadata_enabled: bool,
    aria2_transport: T,
) -> Result<RunOnceReport> {
    if runtime.paused {
        return Ok(RunOnceReport::default());
    }
    if !metadata_enabled {
        bail!("示例元数据源未开启，阶段 4 不会用空 scraper 处理真实文件");
    }

    let scraper = ExamplePipelineScraper;
    let config = DaemonConfig::load(repo)?;
    let mut daemon = HeadlessDaemon::with_completion_policy(
        repo,
        config,
        ScrapeCoordinator { sources: vec![&scraper] },
        CompletionPolicy { sample_delay: Duration::ZERO },
    );
    let aria2_settings = repo.get_aria2_settings()?;
    let aria2 = daemon.poll_aria2_once(&aria2_settings, aria2_transport)?;
    let mut report = daemon.run_once()?;
    report.aria2 = aria2;
    runtime.processed += report.process.processed;
    runtime.last_error = daemon.status().last_error;
    Ok(report)
}
```

- [ ] **Step 4: Run focused Rust tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test daemon_control -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/daemon_control.rs src-tauri/tests/daemon_control.rs src-tauri/tests/control_service.rs
git commit -m "接入阶段6C run-once aria2轮询"
```

---

## Task 4: Expose aria2 settings to frontend and UI

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/api.ts`
- Modify: `src/daemonClient.ts`
- Modify: `src/daemonClient.test.ts`
- Modify: `src/viewModel.ts`
- Modify: `src/viewModel.test.ts`
- Modify: `src/App.tsx`
- Modify: `src/styles.css`

- [ ] **Step 1: Write failing frontend tests**

In `src/viewModel.test.ts`, update the daemon summary test:

```ts
expect(summarizeRunOnceReport({
  scan: { scanned_files: 3, queued_files: 2, skipped_files: 1 },
  aria2: {
    enabled: true,
    attempted_gids: 2,
    completed_gids: 1,
    queued_files: 1,
    skipped_files: 0,
    failed_gids: 1,
    errors: ["gid-bad: forbidden"]
  },
  process: { processed: 2, archived: 1, holding: 1, exceptions: 0, failed: 0 }
})).toBe("aria2 尝试 2 个 GID，完成 1 个，入队 1 个，失败 1 个；扫描 3 个文件，入队 2 个，跳过 1 个；处理 2 个：归档 1，搁置 1，异常 0，失败 0。");
```

In `src/daemonClient.test.ts`, add:

```ts
it("returns aria2 run report from REST run-once", async () => {
  const command = vi.fn();
  const fetchImpl = vi
    .fn()
    .mockResolvedValueOnce(jsonResponse({ ok: true, data: { service: "media-manager-control", status: "ok" } }))
    .mockResolvedValueOnce(jsonResponse({
      ok: true,
      data: {
        scan: { scanned_files: 0, queued_files: 0, skipped_files: 0 },
        aria2: { enabled: true, attempted_gids: 1, completed_gids: 1, queued_files: 1, skipped_files: 0, failed_gids: 0, errors: [] },
        process: { processed: 1, archived: 1, holding: 0, exceptions: 0, failed: 0 }
      }
    }));
  const client = createDaemonControlClient({ command, fetchImpl, getDiscovery: async () => discovery });

  const report = await client.runOnce();

  expect(report.aria2?.queued_files).toBe(1);
  expect(command).not.toHaveBeenCalled();
});
```

- [ ] **Step 2: Run frontend tests to verify RED**

Run:

```powershell
npm test -- src/viewModel.test.ts src/daemonClient.test.ts
```

Expected: TypeScript/Vitest failure because `aria2` is not in `DaemonRunOnceReport` and summary ignores it.

- [ ] **Step 3: Add Rust commands**

In `src-tauri/src/commands.rs`:

- Import `Aria2Settings`.
- Add `pub aria2_settings: Mutex<Aria2Settings>` to `AppState`.
- Initialize it with `Aria2Settings::default()`.
- During setup repository load, set it from `repo.get_aria2_settings()`.
- Add commands:

```rust
/// Persist aria2 RPC settings used by the automatic pipeline run-once path.
#[tauri::command]
pub fn configure_aria2_settings(
    settings: Aria2Settings,
    state: State<'_, AppState>,
) -> Result<CommandResult<Aria2Settings>, String> {
    let normalized = if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        repo.set_aria2_settings(&settings)
            .map_err(|error| error.to_string())?
    } else {
        settings.normalized().map_err(|error| error.to_string())?
    };
    *state.aria2_settings.lock().map_err(|error| error.to_string())? = normalized.clone();
    Ok(CommandResult { data: normalized })
}

/// Return the current aria2 RPC settings for the settings page.
#[tauri::command]
pub fn get_aria2_settings(
    state: State<'_, AppState>,
) -> Result<CommandResult<Aria2Settings>, String> {
    Ok(CommandResult {
        data: state.aria2_settings.lock().map_err(|error| error.to_string())?.clone(),
    })
}
```

- Register both commands in `generate_handler!`.

- [ ] **Step 4: Add TypeScript API and view helpers**

In `src/api.ts`, add:

```ts
export interface Aria2Settings {
  enabled: boolean;
  host: string;
  port: number;
  path: string;
  secret?: string | null;
  timeout_ms: number;
  poll_interval_secs: number;
  tracked_gids: string[];
}

export interface Aria2PollReport {
  enabled: boolean;
  attempted_gids: number;
  completed_gids: number;
  queued_files: number;
  skipped_files: number;
  failed_gids: number;
  errors: string[];
}
```

Update `DaemonRunOnceReport`:

```ts
export interface DaemonRunOnceReport {
  scan: DaemonScanReport;
  aria2?: Aria2PollReport;
  process: DaemonProcessReport;
}
```

Add API methods:

```ts
configureAria2Settings(settings: Aria2Settings) {
  return command<Aria2Settings>("configure_aria2_settings", { settings });
},
getAria2Settings() {
  return command<Aria2Settings>("get_aria2_settings");
},
```

In `src/viewModel.ts`, prepend aria2 summary when `report.aria2?.enabled` and any aria2 count is non-zero.

- [ ] **Step 5: Add UI controls**

In `src/App.tsx`:

- Import `Aria2Settings`.
- Add state:

```ts
const [aria2Settings, setAria2Settings] = useState<Aria2Settings>({
  enabled: false,
  host: "127.0.0.1",
  port: 6800,
  path: "/jsonrpc",
  secret: "",
  timeout_ms: 5000,
  poll_interval_secs: 30,
  tracked_gids: []
});
const [aria2GidsText, setAria2GidsText] = useState("");
const [aria2Busy, setAria2Busy] = useState(false);
```

- Load settings when automatic pipeline tab opens.
- Add `saveAria2Settings` with loading/status feedback.
- Render a `.aria2-settings` block inside the daemon panel with checkbox, inputs, textarea, and save button.

In `src/styles.css`, add compact grid styles for `.aria2-settings`.

- [ ] **Step 6: Run focused frontend and command tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test core_behaviour aria2_settings -j 1
npm test -- src/viewModel.test.ts src/daemonClient.test.ts
npx tsc --noEmit
```

Expected: PASS.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/commands.rs src/api.ts src/daemonClient.ts src/daemonClient.test.ts src/viewModel.ts src/viewModel.test.ts src/App.tsx src/styles.css
git commit -m "接入阶段6C aria2前端配置"
```

---

## Task 5: Full verification, review, handoff

**Files:**
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/progress.md`
- Create: `.ai_state/reviews/sprint-10.md`
- Modify: `.ai_state/lessons.md`
- Modify: `.ai_state/project.json`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Run full verification**

Run:

```powershell
$ErrorActionPreference='Stop'
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
npm test
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
npx tsc --noEmit
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
npm run build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

Expected: all commands exit 0.

- [ ] **Step 2: Self-review diff**

Run:

```powershell
git diff --stat HEAD~4..HEAD
git diff --check HEAD~4..HEAD
rg -n "TODO|TBD|placeholder|unwrap\\(\\)" src-tauri/src/aria2.rs src-tauri/src/daemon.rs src-tauri/src/daemon_control.rs src-tauri/src/commands.rs src src-tauri/tests
```

Expected: no whitespace errors. Any intentional `unwrap()` in tests is acceptable; production TODO/TBD must be absent.

- [ ] **Step 3: Record review**

Create `.ai_state/reviews/sprint-10.md` with:

```markdown
# Sprint 10 Review - 阶段 6C aria2 配置与轮询入口

## Scope

- Persisted aria2 settings in app_settings.
- Added run-once scoped aria2 GID polling.
- Exposed aria2 settings in Tauri commands and settings UI.

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`
- `npm test`
- `npx tsc --noEmit`
- `npm run build`

## Findings

- Critical: none.
- Important: none.

## Residual Risks

- Real aria2 process and real download task validation remain user-side/manual.
- Background polling, callback/WebSocket, and download task management remain deferred.
```

- [ ] **Step 4: Update handoff and lessons**

Add Stage 6C current progress, verification command, deliverables, and deferred items to `HANDOFF.md`.

Append to `.ai_state/lessons.md`:

```markdown
## [2026-06-26 Sprint 10] aria2 轮询先接 run-once 不接常驻线程

- **Pattern**: aria2 endpoint 和 tracked GIDs 先作为 `app_settings` JSON 保存，run-once 前置轮询复用现有 daemon queue，避免为了真实环境验证提前引入后台线程生命周期。
- **Constraint**: aria2 RPC 错误应进入 poll report / daemon last_error，不创建内容异常，也不 fallback 成目录扫描。
```

- [ ] **Step 5: Commit final docs**

```powershell
git add HANDOFF.md
git commit -m "更新阶段6C交接说明"
```

`.ai_state` is gitignored; keep it updated locally.

---

## Done Criteria For Stage 6C

- `Aria2Settings` defaults, validation, normalization, endpoint conversion, and SQLite persistence are tested.
- `HeadlessDaemon::poll_aria2_once` polls configured GIDs with fake transport and aggregates queued/skipped/errors.
- `run_daemon_once` polls aria2 before directory scanning when enabled, and default disabled behavior stays unchanged.
- Tauri commands can get/save aria2 settings.
- Frontend settings panel can edit aria2 settings and run summary displays aria2 poll counts.
- Full Rust and frontend verification passes.
- No real aria2 process, WebView2, or real media resources are required for Codex verification.

## Self-Review Checklist

- Spec coverage: all Stage 6C design goals map to Tasks 1-5.
- Placeholder scan: no TBD/TODO/undefined "implement later" steps.
- Type consistency: `Aria2Settings`, `Aria2PollReport`, `RunOnceReport.aria2`, and command names match across Rust and TypeScript.
