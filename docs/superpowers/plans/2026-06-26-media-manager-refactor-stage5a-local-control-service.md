# Stage 5A Local Control Service Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a pure Rust loopback REST control service for the existing daemon control surface, with token auth, port discovery, and temp-directory tests.

**Architecture:** Add `src-tauri/src/control_service.rs` as a small standard-library HTTP service that serializes requests into `Repository` + `DaemonControlRuntime` and reuses `daemon_control` helpers. The service writes a discovery JSON file after binding, accepts only loopback hosts, protects `/v1/*` APIs with Bearer token auth, and leaves Tauri/WebView2, tray, WebSocket, and real network scrapers out of 5A.

**Tech Stack:** Rust 2021, `std::net::TcpListener`, `serde`/`serde_json`, `sha2`, `rusqlite`, existing `daemon_control`, `tempfile` integration tests.

---

## File Structure

- Create `src-tauri/src/control_service.rs`
  - Owns service config, discovery document, token generation, loopback validation, minimal HTTP parsing, response helpers, request routing, and a stoppable test handle.
  - Calls existing functions from `src-tauri/src/daemon_control.rs`; it must not duplicate daemon or pipeline behavior.
- Modify `src-tauri/src/lib.rs`
  - Export `pub mod control_service;`.
- Create `src-tauri/tests/control_service.rs`
  - Starts the service on port `0`, uses temporary SQLite and fake media files, sends raw HTTP requests with `TcpStream`, and asserts response status/body behavior.
- Modify `.ai_state/tasks.md`
  - Replace Sprint 4 task list with Sprint 5A tasks and check them off during implementation.
- Modify `.ai_state/progress.md`
  - Append short progress entries after each task.
- Modify `HANDOFF.md`
  - Add Stage 5A deliverables and validation status after implementation.
- Modify `.ai_state/reviews/sprint-5.md`
  - Add verification and review notes during final gate.

## Task 1: Control Service Config, Discovery, and Auth Primitives

**Files:**
- Create: `src-tauri/src/control_service.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/control_service.rs`

- [ ] **Step 1: Write failing tests for discovery and loopback validation**

Add this initial test file:

```rust
use media_manager::control_service::{ControlServiceConfig, ControlServiceRuntime};
use media_manager::storage::Repository;
use std::path::Path;

fn open_repo(path: &Path) -> Repository {
    let repo = Repository::open(path).unwrap();
    repo.migrate().unwrap();
    repo
}

#[test]
fn config_rejects_non_loopback_hosts() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    let config = ControlServiceConfig {
        host: "0.0.0.0".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };

    let error = ControlServiceRuntime::new(repo, config).unwrap_err();

    assert!(error.to_string().contains("loopback"));
}

#[test]
fn discovery_document_contains_bound_endpoint_and_token() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };
    let runtime = ControlServiceRuntime::new(repo, config).unwrap();

    let discovery = runtime.discovery_for_port(45123);

    assert_eq!(discovery.host, "127.0.0.1");
    assert_eq!(discovery.port, 45123);
    assert_eq!(discovery.token, "stage5-token");
    assert_eq!(discovery.base_url, "http://127.0.0.1:45123");
    assert_eq!(discovery.service, "media-manager-control");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1
```

Expected: FAIL because `media_manager::control_service` does not exist.

- [ ] **Step 3: Implement minimal config and discovery types**

In `src-tauri/src/lib.rs`, add:

```rust
pub mod control_service;
```

Create `src-tauri/src/control_service.rs` with:

```rust
use crate::daemon_control::DaemonControlRuntime;
use crate::storage::Repository;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Configuration for the Stage 5A loopback control service. It contains only
/// local runtime values; product-level persistence remains in SQLite settings.
#[derive(Debug, Clone)]
pub struct ControlServiceConfig {
    pub host: String,
    pub port: u16,
    pub discovery_path: PathBuf,
    pub token: Option<String>,
    pub metadata_provider_enabled: bool,
}

/// Discovery document written after the listener binds to its actual port.
/// Frontend and future tray processes read this file before calling REST APIs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlServiceDiscovery {
    pub service: String,
    pub host: String,
    pub port: u16,
    pub base_url: String,
    pub token: String,
    pub pid: u32,
    pub created_at: String,
}

/// In-process runtime for handling control API requests. Stage 5A keeps this
/// single-threaded and serial so SQLite writes remain ordered through daemon.
#[derive(Debug)]
pub struct ControlServiceRuntime {
    repo: Repository,
    daemon: DaemonControlRuntime,
    config: ControlServiceConfig,
    token: String,
}

impl ControlServiceRuntime {
    /// Create a service runtime after validating that it is loopback-only.
    /// If no token is provided, a per-start token is generated.
    pub fn new(repo: Repository, config: ControlServiceConfig) -> Result<Self> {
        validate_loopback_host(&config.host)?;
        let token = config.token.clone().unwrap_or_else(generate_token);
        Ok(Self {
            repo,
            daemon: DaemonControlRuntime::default(),
            config,
            token,
        })
    }

    /// Build the discovery document for a bound port without writing it.
    pub fn discovery_for_port(&self, port: u16) -> ControlServiceDiscovery {
        ControlServiceDiscovery {
            service: "media-manager-control".to_string(),
            host: self.config.host.clone(),
            port,
            base_url: format!("http://{}:{port}", self.config.host),
            token: self.token.clone(),
            pid: std::process::id(),
            created_at: now_epoch_seconds(),
        }
    }
}

/// Reject non-loopback hosts so the Stage 5A service cannot accidentally bind
/// to a LAN/WAN interface while it has only local token-based security.
pub fn validate_loopback_host(host: &str) -> Result<()> {
    match host {
        "127.0.0.1" | "localhost" => Ok(()),
        _ => Err(anyhow!("control service must bind to a loopback host")),
    }
}

/// Generate a per-start token from process and clock entropy. Stage 5C can
/// replace this with OS randomness without changing the HTTP contract.
pub fn generate_token() -> String {
    let mut hasher = Sha256::new();
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(now_epoch_seconds().as_bytes());
    hasher.update(format!("{:?}", std::thread::current().id()).as_bytes());
    format!("{:x}", hasher.finalize())
}

fn now_epoch_seconds() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}
```

- [ ] **Step 4: Run focused test to verify it passes**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1
```

Expected: PASS for 2 tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/control_service.rs src-tauri/tests/control_service.rs
git commit -m "新增阶段5A控制服务配置"
```

## Task 2: Minimal HTTP Server, Discovery File, and Health Endpoint

**Files:**
- Modify: `src-tauri/src/control_service.rs`
- Modify: `src-tauri/tests/control_service.rs`

- [ ] **Step 1: Write failing health/discovery file test**

Append to `src-tauri/tests/control_service.rs`:

```rust
use std::io::{Read, Write};
use std::net::TcpStream;

fn request(port: u16, raw: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(raw.as_bytes()).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn service_writes_discovery_file_and_serves_health_without_token() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    let discovery_path = tmp.path().join("control.json");
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: discovery_path.clone(),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };
    let handle = ControlServiceRuntime::new(repo, config).unwrap().start().unwrap();

    let discovery: media_manager::control_service::ControlServiceDiscovery =
        serde_json::from_str(&std::fs::read_to_string(&discovery_path).unwrap()).unwrap();
    let response = request(handle.port(), "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");

    assert_eq!(discovery.port, handle.port());
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("\"service\":\"media-manager-control\""));

    handle.shutdown().unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1
```

Expected: FAIL because `ControlServiceRuntime::start` and `ControlServiceHandle` do not exist.

- [ ] **Step 3: Implement minimal server and health route**

Add imports in `control_service.rs`:

```rust
use serde_json::json;
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
```

Add handle and start logic:

```rust
/// Running service handle used by tests and future daemon hosts. Dropping it
/// does not implicitly kill the thread; callers should request shutdown.
pub struct ControlServiceHandle {
    port: u16,
    shutdown_tx: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl ControlServiceHandle {
    /// Return the actual bound port. This matters when tests request port 0.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Ask the listener loop to stop and wait for the service thread.
    pub fn shutdown(mut self) -> Result<()> {
        let _ = self.shutdown_tx.send(());
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow!("control service thread panicked"))?;
        }
        Ok(())
    }
}

impl ControlServiceRuntime {
    /// Start the HTTP service on the configured loopback address and write the
    /// discovery file once the OS has assigned the final port.
    pub fn start(self) -> Result<ControlServiceHandle> {
        let listener = TcpListener::bind((self.config.host.as_str(), self.config.port))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let discovery = self.discovery_for_port(port);
        if let Some(parent) = self.config.discovery_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            &self.config.discovery_path,
            serde_json::to_string_pretty(&discovery)?,
        )?;

        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let thread = thread::spawn(move || run_listener(listener, self, shutdown_rx));

        Ok(ControlServiceHandle {
            port,
            shutdown_tx,
            thread: Some(thread),
        })
    }
}

fn run_listener(
    listener: TcpListener,
    mut runtime: ControlServiceRuntime,
    shutdown_rx: mpsc::Receiver<()>,
) {
    loop {
        if shutdown_rx.try_recv().is_ok() {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = handle_stream(stream, &mut runtime);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn handle_stream(mut stream: TcpStream, runtime: &mut ControlServiceRuntime) -> Result<()> {
    let mut buffer = String::new();
    stream.read_to_string(&mut buffer)?;
    let response = runtime.handle_raw_http(&buffer);
    stream.write_all(response.as_bytes())?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

impl ControlServiceRuntime {
    /// Handle one raw HTTP request and return a complete HTTP response string.
    pub fn handle_raw_http(&mut self, raw: &str) -> String {
        let Some(request) = HttpRequest::parse(raw) else {
            return json_response(400, json!({ "ok": false, "error": "bad request" }));
        };
        if request.method == "GET" && request.path == "/health" {
            return json_response(
                200,
                json!({
                    "ok": true,
                    "data": {
                        "service": "media-manager-control",
                        "status": "ok"
                    }
                }),
            );
        }
        json_response(404, json!({ "ok": false, "error": "not found" }))
    }
}

struct HttpRequest {
    method: String,
    path: String,
}

impl HttpRequest {
    fn parse(raw: &str) -> Option<Self> {
        let first = raw.lines().next()?;
        let mut parts = first.split_whitespace();
        Some(Self {
            method: parts.next()?.to_string(),
            path: parts.next()?.to_string(),
        })
    }
}

fn json_response(status: u16, body: serde_json::Value) -> String {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let body = body.to_string();
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
```

- [ ] **Step 4: Run focused test to verify it passes**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1
```

Expected: PASS for 3 tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/control_service.rs src-tauri/tests/control_service.rs
git commit -m "实现阶段5A控制服务健康检查"
```

## Task 3: Auth, Origin Check, and Status/Pause/Resume Routes

**Files:**
- Modify: `src-tauri/src/control_service.rs`
- Modify: `src-tauri/tests/control_service.rs`

- [ ] **Step 1: Write failing route auth test**

Append:

```rust
fn configured_repo(tmp: &tempfile::TempDir) -> Repository {
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    let inbox = tmp.path().join("inbox");
    let archive = tmp.path().join("archive");
    let assets = tmp.path().join("assets");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    std::fs::create_dir_all(&assets).unwrap();
    repo.set_source_roots(&[inbox]).unwrap();
    repo.set_archive_root(&archive).unwrap();
    repo.set_resource_pool_dirs(&[assets]).unwrap();
    repo
}

fn authorized_get(port: u16, path: &str) -> String {
    request(
        port,
        &format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer stage5-token\r\nOrigin: http://127.0.0.1:1420\r\n\r\n"
        ),
    )
}

fn authorized_post(port: u16, path: &str) -> String {
    request(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer stage5-token\r\nOrigin: tauri://localhost\r\nContent-Length: 0\r\n\r\n"
        ),
    )
}

#[test]
fn v1_routes_require_token_and_allow_pause_resume_status() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = configured_repo(&tmp);
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };
    let handle = ControlServiceRuntime::new(repo, config).unwrap().start().unwrap();

    let missing = request(handle.port(), "GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    let wrong = request(handle.port(), "GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer wrong\r\n\r\n");
    let blocked_origin = request(handle.port(), "GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer stage5-token\r\nOrigin: https://evil.example\r\n\r\n");
    let status = authorized_get(handle.port(), "/v1/status");
    let paused = authorized_post(handle.port(), "/v1/pause");
    let resumed = authorized_post(handle.port(), "/v1/resume");

    assert!(missing.starts_with("HTTP/1.1 401 Unauthorized"));
    assert!(wrong.starts_with("HTTP/1.1 403 Forbidden"));
    assert!(blocked_origin.starts_with("HTTP/1.1 403 Forbidden"));
    assert!(status.contains("\"state\":\"Idle\""));
    assert!(paused.contains("\"state\":\"Paused\""));
    assert!(resumed.contains("\"state\":\"Idle\""));

    handle.shutdown().unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1
```

Expected: FAIL because `/v1/status`, `/v1/pause`, `/v1/resume`, auth, and Origin checks are not implemented.

- [ ] **Step 3: Implement auth, headers, and routes**

Update `HttpRequest`:

```rust
use std::collections::HashMap;

struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
}

impl HttpRequest {
    fn parse(raw: &str) -> Option<Self> {
        let mut lines = raw.lines();
        let first = lines.next()?;
        let mut parts = first.split_whitespace();
        let mut headers = HashMap::new();
        for line in lines {
            if line.trim().is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }
        Some(Self {
            method: parts.next()?.to_string(),
            path: parts.next()?.to_string(),
            headers,
        })
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}
```

Add auth helpers and routes:

```rust
use crate::daemon_control::{build_daemon_status, DaemonControlStatus};

impl ControlServiceRuntime {
    pub fn handle_raw_http(&mut self, raw: &str) -> String {
        let Some(request) = HttpRequest::parse(raw) else {
            return json_response(400, json!({ "ok": false, "error": "bad request" }));
        };
        if request.method == "GET" && request.path == "/health" {
            return ok_json(json!({
                "service": "media-manager-control",
                "status": "ok"
            }));
        }
        if request.path.starts_with("/v1/") {
            if let Err(response) = self.authorize(&request) {
                return response;
            }
            return self.route_v1(&request);
        }
        json_response(404, json!({ "ok": false, "error": "not found" }))
    }

    fn authorize(&self, request: &HttpRequest) -> Result<(), String> {
        if !origin_allowed(request.header("origin")) {
            return Err(json_response(403, json!({ "ok": false, "error": "origin forbidden" })));
        }
        match request.header("authorization") {
            None => Err(json_response(401, json!({ "ok": false, "error": "missing bearer token" }))),
            Some(value) if value == format!("Bearer {}", self.token) => Ok(()),
            Some(_) => Err(json_response(403, json!({ "ok": false, "error": "invalid bearer token" }))),
        }
    }

    fn route_v1(&mut self, request: &HttpRequest) -> String {
        let result: Result<serde_json::Value> = match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/v1/status") => self.status_json(),
            ("POST", "/v1/pause") => {
                self.daemon.paused = true;
                self.status_json()
            }
            ("POST", "/v1/resume") => {
                self.daemon.paused = false;
                self.status_json()
            }
            _ => return json_response(404, json!({ "ok": false, "error": "not found" })),
        };
        match result {
            Ok(data) => ok_json(data),
            Err(error) => json_response(500, json!({ "ok": false, "error": error.to_string() })),
        }
    }

    fn status_json(&self) -> Result<serde_json::Value> {
        let status: DaemonControlStatus = build_daemon_status(
            &self.repo,
            &self.daemon,
            self.config.metadata_provider_enabled,
        )?;
        Ok(serde_json::to_value(status)?)
    }
}

fn origin_allowed(origin: Option<&str>) -> bool {
    match origin {
        None => true,
        Some("tauri://localhost") => true,
        Some(value) => value.starts_with("http://127.0.0.1:")
            || value.starts_with("http://localhost:"),
    }
}

fn ok_json(data: serde_json::Value) -> String {
    json_response(200, json!({ "ok": true, "data": data }))
}
```

- [ ] **Step 4: Run focused test**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1
```

Expected: PASS for 4 tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/control_service.rs src-tauri/tests/control_service.rs
git commit -m "接入阶段5A控制服务鉴权与状态接口"
```

## Task 4: Run Once and Queue Routes

**Files:**
- Modify: `src-tauri/src/control_service.rs`
- Modify: `src-tauri/tests/control_service.rs`

- [ ] **Step 1: Write failing run-once and queue route test**

Append:

```rust
use media_manager::domain::{Exception, ExceptionKind, ExceptionStatus, HoldingEntry, HoldingReason, PipelineRun};

#[test]
fn run_once_and_queue_routes_use_existing_daemon_control_helpers() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = configured_repo(&tmp);
    let inbox = tmp.path().join("inbox");
    std::fs::write(inbox.join("ABP-501.mp4"), b"stable video bytes").unwrap();
    let exception_id = repo.record_exception(&Exception {
        id: None,
        object_path: "H:/Inbox/ABP-500.mp4".to_string(),
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
        file_path: "H:/Inbox/ABP-500.mp4".to_string(),
        started_at: None,
        finished_at: None,
        steps_json: "[]".to_string(),
        status: "exception".to_string(),
        error: Some("not found".to_string()),
    }).unwrap();
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: true,
    };
    let handle = ControlServiceRuntime::new(repo, config).unwrap().start().unwrap();

    let run = authorized_post(handle.port(), "/v1/run-once");
    let holding = authorized_get(handle.port(), "/v1/holding");
    let exceptions = authorized_get(handle.port(), "/v1/exceptions");
    let resolved = authorized_post(handle.port(), &format!("/v1/exceptions/{exception_id}/resolve"));
    let runs = authorized_get(handle.port(), "/v1/runs");

    assert!(run.contains("\"queued_files\":1"));
    assert!(run.contains("\"archived\":1"));
    assert!(holding.contains("manual.mp4"));
    assert!(exceptions.contains("ABP-500.mp4"));
    assert!(resolved.contains("\"status\":\"Resolved\""));
    assert!(runs.contains("\"status\":\"archived\""));

    handle.shutdown().unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1
```

Expected: FAIL because run-once and queue routes are not implemented.

- [ ] **Step 3: Implement run-once, queue, and exception resolve routes**

Add imports:

```rust
use crate::daemon_control::{
    list_exception_entries, list_holding_entries, list_recent_pipeline_runs,
    resolve_exception_entry, run_daemon_once,
};
use crate::domain::ExceptionStatus;
```

Extend `route_v1`:

```rust
            ("POST", "/v1/run-once") => {
                let report = run_daemon_once(
                    &self.repo,
                    &mut self.daemon,
                    self.config.metadata_provider_enabled,
                )?;
                Ok(serde_json::to_value(report)?)
            }
            ("GET", "/v1/holding") => Ok(serde_json::to_value(list_holding_entries(&self.repo)?)?),
            ("GET", "/v1/exceptions") => Ok(serde_json::to_value(list_exception_entries(&self.repo)?)?),
            ("GET", "/v1/runs") => Ok(serde_json::to_value(list_recent_pipeline_runs(&self.repo)?)?),
```

Before the final `_` arm, add dynamic exception route handling:

```rust
            ("POST", path)
                if path.starts_with("/v1/exceptions/") && path.ends_with("/resolve") =>
            {
                let id = parse_exception_resolve_path(path)?;
                let exception = resolve_exception_entry(&self.repo, id, ExceptionStatus::Resolved)?;
                Ok(serde_json::to_value(exception)?)
            }
```

Add helper:

```rust
fn parse_exception_resolve_path(path: &str) -> Result<i64> {
    let id = path
        .strip_prefix("/v1/exceptions/")
        .and_then(|tail| tail.strip_suffix("/resolve"))
        .ok_or_else(|| anyhow!("invalid exception resolve path"))?;
    id.parse::<i64>()
        .map_err(|_| anyhow!("invalid exception id"))
}
```

- [ ] **Step 4: Run focused test**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1
```

Expected: PASS for 5 tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/control_service.rs src-tauri/tests/control_service.rs
git commit -m "串联阶段5A控制服务管线接口"
```

## Task 5: Error Semantics, Disabled Metadata Safety, and Documentation

**Files:**
- Modify: `src-tauri/src/control_service.rs`
- Modify: `src-tauri/tests/control_service.rs`
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/progress.md`

- [ ] **Step 1: Write failing disabled metadata and bad route tests**

Append:

```rust
#[test]
fn run_once_without_metadata_source_returns_error_and_keeps_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = configured_repo(&tmp);
    let inbox = tmp.path().join("inbox");
    let video = inbox.join("ABP-599.mp4");
    std::fs::write(&video, b"stable video bytes").unwrap();
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };
    let handle = ControlServiceRuntime::new(repo, config).unwrap().start().unwrap();

    let response = authorized_post(handle.port(), "/v1/run-once");

    assert!(response.starts_with("HTTP/1.1 500 Internal Server Error"));
    assert!(response.contains("示例元数据源未开启"));
    assert!(video.exists());

    handle.shutdown().unwrap();
}

#[test]
fn unknown_v1_route_returns_not_found_json() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = configured_repo(&tmp);
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };
    let handle = ControlServiceRuntime::new(repo, config).unwrap().start().unwrap();

    let response = authorized_get(handle.port(), "/v1/unknown");

    assert!(response.starts_with("HTTP/1.1 404 Not Found"));
    assert!(response.contains("\"ok\":false"));

    handle.shutdown().unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails if error mapping is incomplete**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1
```

Expected: FAIL if route errors do not map to JSON 500 or not-found JSON correctly.

- [ ] **Step 3: Ensure route errors map to JSON and update `.ai_state/tasks.md`**

Set `.ai_state/tasks.md` to:

```markdown
# Sprint 5 Tasks - 阶段 5A 本地控制服务基座

- [x] 固化阶段 5A 中文设计
- [x] 编写阶段 5A implementation plan
- [x] Task 1: 控制服务配置、发现文件和鉴权基础
- [x] Task 2: HTTP 服务启动、发现文件写入和 health endpoint
- [x] Task 3: token / Origin 校验与状态、暂停、恢复 API
- [x] Task 4: 运行一轮、搁置区、异常队列、异常解决、运行记录 API
- [x] Task 5: 错误语义、安全回归、全量验证、评审和交接
```

Append `.ai_state/progress.md` entries after each actual task completion using this format:

```markdown
- 2026-06-26: 阶段 5A Task N 完成；<one short result>。
```

- [ ] **Step 4: Run focused test**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1
```

Expected: PASS for 7 tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/control_service.rs src-tauri/tests/control_service.rs .ai_state/tasks.md .ai_state/progress.md
git commit -m "补齐阶段5A控制服务错误回归"
```

If `.ai_state` is gitignored, commit only tracked source/test files and keep `.ai_state` as local state.

## Task 6: Full Verification, Review Record, HANDOFF, and Final Commit

**Files:**
- Modify: `HANDOFF.md`
- Create: `.ai_state/reviews/sprint-5.md`
- Modify: `.ai_state/progress.md`
- Modify: `.ai_state/lessons.md`
- Modify: `.ai_state/project.json`

- [ ] **Step 1: Run full verification**

Run these commands in order:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

Expected:

- Rust tests pass, allowing existing historical warnings.
- Vitest passes.
- TypeScript check passes.
- Production build passes.

- [ ] **Step 2: Write review record**

Create `.ai_state/reviews/sprint-5.md`:

```markdown
# Sprint 5 Review

## Scope

阶段 5A：纯 Rust loopback REST 控制服务基座。

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`: PASS
- `npm test`: PASS
- `npx tsc --noEmit`: PASS
- `npm run build`: PASS

## Findings

- Main self-review: <fill after diff review>.
- Subagent reviewer: <fill after reviewer result or fallback reason>.

## Deferred

- WebSocket push remains Stage 5B/5C work.
- Tray/autostart remains Stage 5C work.
- Real network scraper remains later Stage 5 work.
- Frontend HTTP client migration remains Stage 5B work.
```

- [ ] **Step 3: Update HANDOFF**

Add Stage 5A to `HANDOFF.md`:

```markdown
**阶段 5A（本地控制服务基座）已实现并验证。**
阶段 5A 新增纯 Rust loopback REST 控制服务：发现文件、Bearer token、Origin 校验、health、status、pause/resume、run-once、holding、exceptions、resolve exception、runs。服务测试使用随机本地端口、临时 SQLite、临时媒体目录和假文件，不依赖真实资源。
```

Add validation command:

```markdown
- `cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1`
```

- [ ] **Step 4: Self-review diff**

Run:

```powershell
git diff --stat HEAD
git diff HEAD -- src-tauri/src/control_service.rs src-tauri/tests/control_service.rs src-tauri/src/lib.rs HANDOFF.md docs/superpowers/plans/2026-06-26-media-manager-refactor-stage5a-local-control-service.md
```

Check:

- No Tauri GUI/WebView2 commands or code paths were introduced.
- Service host validation rejects non-loopback hosts.
- `/health` is unauthenticated; `/v1/*` requires token.
- Origin check rejects unknown origins.
- Tests use only `tempfile` and fake files.

- [ ] **Step 5: Request subagent review if available**

Dispatch reviewer with git range from the Stage 5A design commit to current HEAD. If subagent tooling fails, record the failure and keep the main self-review plus full verification.

- [ ] **Step 6: Commit final docs and review state**

```bash
git add HANDOFF.md docs/superpowers/plans/2026-06-26-media-manager-refactor-stage5a-local-control-service.md
git commit -m "完成阶段5A本地控制服务基座"
```

If implementation commits already include the plan, commit only remaining tracked docs. `.ai_state` may remain untracked if ignored.

## Self-Review

- Spec coverage: every 5A design requirement maps to at least one task.
- Placeholder scan: no `TBD`, no `TODO`, no deferred implementation hole inside 5A.
- Type consistency: plan uses `ControlServiceConfig`, `ControlServiceRuntime`, `ControlServiceDiscovery`, and route names consistently.
- Boundary check: plan does not add WebSocket, tray, autostart, real scraper, Tauri GUI startup, or frontend HTTP migration.
