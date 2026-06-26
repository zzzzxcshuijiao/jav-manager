# Stage 5C Control Service Host Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Start and own the Stage 5A loopback control service from the Tauri application lifecycle so the Stage 5B frontend can use the real REST channel in a desktop run.

**Architecture:** Add a pure Rust `control_service_host` module that converts the app data directory into a startable service host, then wire it into `commands::AppState` and Tauri `setup`. Keep failure non-fatal so the command bridge fallback remains available, and expose a small host status command for diagnostics.

**Tech Stack:** Rust/Tauri command layer, SQLite via existing `Repository`, existing standard-library loopback control service, TypeScript API DTOs, Vitest/tsc/build verification.

---

## File Structure

- Create `src-tauri/src/control_service_host.rs`
  - Own app-data-to-service config, host startup, and host status DTO.
- Modify `src-tauri/src/lib.rs`
  - Export `control_service_host`.
- Modify `src-tauri/src/control_service.rs`
  - Add discovery path cleanup to `ControlServiceHandle`.
  - Add handle accessors.
  - Read metadata provider enabled from SQLite at request time.
- Modify `src-tauri/tests/control_service.rs`
  - Cover metadata provider setting changes after service startup.
- Create `src-tauri/tests/control_service_host.rs`
  - Cover config, startup, health, status, and discovery cleanup.
- Modify `src-tauri/src/commands.rs`
  - Store host handle/error in `AppState`.
  - Start service during `setup`.
  - Add `get_control_service_host_status`.
  - Shutdown service in `Drop`.
- Modify `src/api.ts`
  - Export host status DTO and wrapper.
- Modify `.ai_state/tasks.md`, `.ai_state/progress.md`, `.ai_state/reviews/sprint-7.md`, `.ai_state/lessons.md`, `HANDOFF.md`.

## Task 1: Control Service Host Module

**Files:**
- Create: `src-tauri/src/control_service_host.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/control_service_host.rs`

- [ ] **Step 1: Write failing host tests**

Create `src-tauri/tests/control_service_host.rs`:

```rust
use media_manager::control_service::read_control_service_discovery;
use media_manager::control_service_host::{
    build_control_service_config, control_service_host_status, start_control_service_host,
    CONTROL_SERVICE_HOST, CONTROL_SERVICE_PORT,
};
use std::io::{Read, Write};
use std::net::TcpStream;

fn request(port: u16, raw: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(raw.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn host_config_uses_app_data_discovery_and_loopback_random_port() {
    let tmp = tempfile::tempdir().unwrap();

    let config = build_control_service_config(tmp.path(), true);

    assert_eq!(config.host, CONTROL_SERVICE_HOST);
    assert_eq!(config.port, CONTROL_SERVICE_PORT);
    assert_eq!(config.discovery_path, tmp.path().join("control-service.json"));
    assert!(config.metadata_provider_enabled);
}

#[test]
fn host_status_reports_running_and_stopped_states() {
    let tmp = tempfile::tempdir().unwrap();
    let discovery_path = tmp.path().join("control-service.json");

    let stopped = control_service_host_status(tmp.path(), None, Some("boom".to_string()));

    assert!(!stopped.running);
    assert_eq!(stopped.host, CONTROL_SERVICE_HOST);
    assert_eq!(stopped.port, None);
    assert_eq!(stopped.discovery_path, discovery_path.to_string_lossy());
    assert_eq!(stopped.last_error.as_deref(), Some("boom"));
}

#[test]
fn host_starts_health_endpoint_and_removes_discovery_on_shutdown() {
    let tmp = tempfile::tempdir().unwrap();

    let handle = start_control_service_host(tmp.path()).unwrap();
    let port = handle.port();
    let discovery_path = handle.discovery_path().to_path_buf();
    let discovery = read_control_service_discovery(&discovery_path)
        .unwrap()
        .expect("discovery should be written");

    assert_eq!(discovery.host, CONTROL_SERVICE_HOST);
    assert_eq!(discovery.port, port);

    let status = control_service_host_status(tmp.path(), Some(&handle), None);
    assert!(status.running);
    assert_eq!(status.port, Some(port));

    let response = request(port, "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert!(response.contains("\"status\":\"ok\""));

    handle.shutdown().unwrap();
    assert!(read_control_service_discovery(&discovery_path).unwrap().is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service_host -j 1
```

Expected: FAIL because `media_manager::control_service_host` does not exist.

- [ ] **Step 3: Implement host module and export**

Add `pub mod control_service_host;` to `src-tauri/src/lib.rs`.

Create `src-tauri/src/control_service_host.rs`:

```rust
use crate::control_service::{
    control_service_discovery_path, ControlServiceConfig, ControlServiceHandle,
    ControlServiceRuntime,
};
use crate::storage::Repository;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const CONTROL_SERVICE_HOST: &str = "127.0.0.1";
pub const CONTROL_SERVICE_PORT: u16 = 0;
pub const CONTROL_SERVICE_DB_FILE: &str = "library.sqlite";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlServiceHostStatus {
    pub running: bool,
    pub host: String,
    pub port: Option<u16>,
    pub discovery_path: String,
    pub last_error: Option<String>,
}

/// Build the runtime config for the app-owned loopback control service.
pub fn build_control_service_config(
    app_data_dir: &Path,
    metadata_provider_enabled: bool,
) -> ControlServiceConfig {
    ControlServiceConfig {
        host: CONTROL_SERVICE_HOST.to_string(),
        port: CONTROL_SERVICE_PORT,
        discovery_path: control_service_discovery_path(app_data_dir),
        token: None,
        metadata_provider_enabled,
    }
}

/// Start the app-owned control service using a dedicated SQLite connection.
pub fn start_control_service_host(app_data_dir: &Path) -> Result<ControlServiceHandle> {
    fs::create_dir_all(app_data_dir)?;
    let repo = Repository::open(&app_data_dir.join(CONTROL_SERVICE_DB_FILE))?;
    repo.migrate()?;
    let metadata_provider_enabled = repo.get_metadata_provider_enabled()?;
    let config = build_control_service_config(app_data_dir, metadata_provider_enabled);
    ControlServiceRuntime::new(repo, config)?.start()
}

/// Return a serializable snapshot of the app-owned control service host.
pub fn control_service_host_status(
    app_data_dir: &Path,
    handle: Option<&ControlServiceHandle>,
    last_error: Option<String>,
) -> ControlServiceHostStatus {
    ControlServiceHostStatus {
        running: handle.is_some(),
        host: handle
            .map(|value| value.host().to_string())
            .unwrap_or_else(|| CONTROL_SERVICE_HOST.to_string()),
        port: handle.map(ControlServiceHandle::port),
        discovery_path: control_service_discovery_path(app_data_dir)
            .to_string_lossy()
            .to_string(),
        last_error,
    }
}
```

- [ ] **Step 4: Run focused test**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service_host -j 1
```

Expected: still FAIL because `ControlServiceHandle::host` and `discovery_path` do not exist and shutdown does not remove discovery yet. Task 2 implements those.

## Task 2: Control Service Handle Cleanup and Dynamic Metadata Setting

**Files:**
- Modify: `src-tauri/src/control_service.rs`
- Modify: `src-tauri/tests/control_service.rs`
- Test: `src-tauri/tests/control_service_host.rs`

- [ ] **Step 1: Write failing dynamic metadata test**

Append to `src-tauri/tests/control_service.rs`:

```rust
#[test]
fn service_reads_metadata_provider_setting_after_startup() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = configured_repo(&tmp);
    repo.set_metadata_provider_enabled(false).unwrap();
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };
    let handle = ControlServiceRuntime::new(repo, config).unwrap().start().unwrap();
    let port = handle.port();

    let disabled = authorized_get(port, "/v1/status");
    assert!(disabled.contains("\"metadata_source\":\"disabled\""));

    let repo = open_repo(&tmp.path().join("library.sqlite"));
    repo.set_metadata_provider_enabled(true).unwrap();

    let enabled = authorized_get(port, "/v1/status");
    assert!(enabled.contains("\"metadata_source\":\"example\""));

    handle.shutdown().unwrap();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service service_reads_metadata_provider_setting_after_startup -j 1
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service_host -j 1
```

Expected: dynamic metadata test FAILS because service still uses startup snapshot; host test FAILS because handle accessors/cleanup do not exist.

- [ ] **Step 3: Implement handle accessors, discovery cleanup, and dynamic metadata**

In `src-tauri/src/control_service.rs`, add `discovery_path` to `ControlServiceHandle`:

```rust
pub struct ControlServiceHandle {
    host: String,
    port: u16,
    discovery_path: PathBuf,
    shutdown_tx: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}
```

When constructing the handle in `ControlServiceRuntime::start`, set:

```rust
discovery_path: self.config.discovery_path.clone(),
```

Add methods:

```rust
/// Return the loopback host that owns the listener.
pub fn host(&self) -> &str {
    &self.host
}

/// Return the discovery file path written for this service instance.
pub fn discovery_path(&self) -> &Path {
    &self.discovery_path
}
```

Update `shutdown`:

```rust
/// Ask the listener loop to stop, wait for the service thread, and remove discovery.
pub fn shutdown(mut self) -> Result<()> {
    let _ = self.shutdown_tx.send(());
    let _ = TcpStream::connect((self.host.as_str(), self.port));
    if let Some(thread) = self.thread.take() {
        thread
            .join()
            .map_err(|_| anyhow!("control service thread panicked"))?;
    }
    let _ = fs::remove_file(&self.discovery_path);
    Ok(())
}
```

Add a helper on `ControlServiceRuntime`:

```rust
/// Read the current metadata provider flag from SQLite, falling back to startup config.
fn metadata_provider_enabled(&self) -> bool {
    self.repo
        .get_metadata_provider_enabled()
        .unwrap_or(self.config.metadata_provider_enabled)
}
```

Use it in `route_v1_result` and `status_json`:

```rust
let report = run_daemon_once(&self.repo, &mut self.daemon, self.metadata_provider_enabled())?;
```

```rust
let status: DaemonControlStatus =
    build_daemon_status(&self.repo, &self.daemon, self.metadata_provider_enabled())?;
```

- [ ] **Step 4: Run focused tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service service_reads_metadata_provider_setting_after_startup -j 1
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service_host -j 1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/control_service.rs src-tauri/src/control_service_host.rs src-tauri/tests/control_service.rs src-tauri/tests/control_service_host.rs
git commit -m "新增阶段5C控制服务宿主模块"
```

## Task 3: Tauri AppState and Setup Integration

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Test: existing Rust tests compile and commands unit tests.

- [ ] **Step 1: Write command-layer unit test for host status fallback**

In `src-tauri/src/commands.rs` test module, add:

```rust
    #[test]
    fn default_app_state_reports_no_control_service_handle() {
        let state = AppState::default();
        let error = Some("startup failed".to_string());
        let tmp = tempfile::tempdir().unwrap();

        let status = state.control_service_host_status(tmp.path(), error.clone());

        assert!(!status.running);
        assert_eq!(status.port, None);
        assert_eq!(status.last_error, error);
        assert!(status.discovery_path.ends_with("control-service.json"));
    }
```

- [ ] **Step 2: Run unit test to verify it fails**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml commands::tests::default_app_state_reports_no_control_service_handle -j 1
```

Expected: FAIL because `AppState::control_service_host_status` does not exist.

- [ ] **Step 3: Implement AppState host fields and status helper**

Update imports:

```rust
use crate::control_service::ControlServiceHandle;
use crate::control_service_host::{
    control_service_host_status as build_control_service_host_status,
    start_control_service_host, ControlServiceHostStatus,
};
```

Add fields:

```rust
pub control_service: Mutex<Option<ControlServiceHandle>>,
pub control_service_error: Mutex<Option<String>>,
```

Set defaults:

```rust
control_service: Mutex::new(None),
control_service_error: Mutex::new(None),
```

Add methods:

```rust
impl AppState {
    /// Return a serializable snapshot of the app-owned control service host.
    pub fn control_service_host_status(
        &self,
        app_data_dir: &Path,
        last_error: Option<String>,
    ) -> ControlServiceHostStatus {
        let handle_guard = self.control_service.lock().ok();
        build_control_service_host_status(
            app_data_dir,
            handle_guard.as_deref().and_then(|slot| slot.as_ref()),
            last_error,
        )
    }
}

impl Drop for AppState {
    /// Shut down the hosted loopback control service when Tauri releases app state.
    fn drop(&mut self) {
        if let Ok(slot) = self.control_service.get_mut() {
            if let Some(handle) = slot.take() {
                let _ = handle.shutdown();
            }
        }
    }
}
```

- [ ] **Step 4: Add host status command**

Add near discovery command:

```rust
/// Return the app-owned control service host status for diagnostics.
#[tauri::command]
pub fn get_control_service_host_status(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<CommandResult<ControlServiceHostStatus>, String> {
    let app_data = app.path().app_data_dir().map_err(|error| error.to_string())?;
    let last_error = state
        .control_service_error
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    Ok(CommandResult {
        data: state.control_service_host_status(&app_data, last_error),
    })
}
```

Register it after `get_control_service_discovery`:

```rust
get_control_service_host_status,
```

- [ ] **Step 5: Start host in setup**

After storing `state.repository`, add:

```rust
            match start_control_service_host(&app_data) {
                Ok(handle) => {
                    *state
                        .control_service
                        .lock()
                        .map_err(|error| error.to_string())? = Some(handle);
                    *state
                        .control_service_error
                        .lock()
                        .map_err(|error| error.to_string())? = None;
                }
                Err(error) => {
                    *state
                        .control_service_error
                        .lock()
                        .map_err(|lock_error| lock_error.to_string())? = Some(error.to_string());
                }
            }
```

- [ ] **Step 6: Run focused Rust tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml commands::tests::default_app_state_reports_no_control_service_handle -j 1
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service_host -j 1
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "接入阶段5C控制服务宿主生命周期"
```

## Task 4: Frontend API Wrapper

**Files:**
- Modify: `src/api.ts`

- [ ] **Step 1: Add TypeScript DTO and wrapper**

In `src/api.ts`, add near `ControlServiceDiscovery` re-export area:

```ts
export interface ControlServiceHostStatus {
  running: boolean;
  host: string;
  port?: number | null;
  discovery_path: string;
  last_error?: string | null;
}
```

Add API method near discovery methods:

```ts
  getControlServiceHostStatus() {
    return command<ControlServiceHostStatus>("get_control_service_host_status");
  },
```

- [ ] **Step 2: Run frontend typecheck**

Run:

```powershell
npx tsc --noEmit
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/api.ts
git commit -m "暴露阶段5C控制服务宿主状态"
```

## Task 5: Verification, Review, HANDOFF

**Files:**
- Modify: `HANDOFF.md`
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/progress.md`
- Create: `.ai_state/reviews/sprint-7.md`
- Modify: `.ai_state/lessons.md`
- Modify: `.ai_state/project.json`

- [ ] **Step 1: Update local state**

Set `.ai_state/tasks.md`:

```markdown
# Sprint 7 Tasks - 阶段 5C 控制服务宿主与生命周期

- [x] 固化阶段 5C 中文设计
- [x] 编写阶段 5C implementation plan
- [x] Task 1: 控制服务宿主模块
- [x] Task 2: 控制服务 handle 清理和动态 metadata 设置
- [x] Task 3: Tauri AppState/setup 接入宿主生命周期
- [x] Task 4: 前端 API 暴露宿主状态
- [x] Task 5: 全量验证、评审和交接
```

- [ ] **Step 2: Run full verification**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

Expected all pass.

- [ ] **Step 3: Review**

Run:

```powershell
git diff --check 37f1aec..HEAD
git diff --stat 37f1aec..HEAD
$env:npm_config_cache='.npm-cache'; npx ecc-agentshield scan
```

Record ECC as not applicable if it still scans global `.claude` instead of this repository.

- [ ] **Step 4: Update HANDOFF**

Add Stage 5C deliverables:

```markdown
**阶段 5C（控制服务宿主与生命周期）已实现并验证。**
阶段 5C 新增 app-owned control service host。Tauri setup 会在 app data 初始化后尝试启动 loopback REST 服务，写入 `control-service.json`，并把 handle 存进 AppState；启动失败不阻断应用，前端仍可 fallback 到 command bridge。AppState 释放时会 shutdown 服务并删除 discovery。
```

- [ ] **Step 5: Commit docs**

```bash
git add HANDOFF.md
git commit -m "更新阶段5C交接说明"
```

## Self-Review

- Spec coverage: host module, setup lifecycle, discovery cleanup, dynamic metadata setting, host status command, TS wrapper, verification all have tasks.
- Placeholder scan: checked for unfinished markers and none remain.
- Boundary check: no Tauri GUI launch, no WebView2, no tray/autostart, no WebSocket, no real scraper.
- Type consistency: Rust DTO `ControlServiceHostStatus` maps directly to TypeScript `ControlServiceHostStatus`.
