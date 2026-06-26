# Stage 6B Aria2 RPC Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a locally testable aria2 JSON-RPC bridge that turns completed aria2 tasks into daemon queue candidates without requiring a real aria2 process.

**Architecture:** Add a focused Rust `aria2` module with DTOs, an injectable transport trait, a minimal standard-library HTTP transport, and completion-file extraction that reuses the existing `pipeline::Aria2TaskSnapshot` detector. Add an explicit daemon entry point for scanning one aria2 GID while keeping the existing directory heuristic unchanged.

**Tech Stack:** Rust 2021, serde/serde_json, std::net::TcpStream, anyhow, existing `CompletedFile` / `HeadlessDaemon` / `ScanReport`, `cargo test --manifest-path src-tauri/Cargo.toml -j 1`.

---

## File Structure

- Create `src-tauri/src/aria2.rs`
  - Owns aria2 RPC endpoint config, transport trait, HTTP transport, JSON-RPC DTO parsing, completion extraction, and helper methods.
- Modify `src-tauri/src/lib.rs`
  - Exports `pub mod aria2;`.
- Modify `src-tauri/src/daemon.rs`
  - Adds `scan_aria2_gid` and a private helper that enqueues completed selected video files from aria2.
- Create `src-tauri/tests/aria2_rpc.rs`
  - Tests request shape, secret placement, response parsing, JSON-RPC error handling, completion selection, and HTTP transport.
- Modify `src-tauri/tests/daemon.rs`
  - Tests daemon-level aria2 GID scanning and duplicate prevention.
- Modify `.ai_state/tasks.md`, `.ai_state/progress.md`, `.ai_state/design.md`, `.ai_state/reviews/sprint-8.md`, `.ai_state/lessons.md`, and `HANDOFF.md`
  - Tracks Sprint 8 state, review evidence, and next handoff. `.ai_state` is gitignored but remains the local truth for this session.

---

### Task 1: aria2 RPC DTO and Fake Transport

**Files:**
- Create: `src-tauri/src/aria2.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/tests/aria2_rpc.rs`

- [ ] **Step 1: Write failing tests for request shape, parsing, and RPC errors**

Add this starting test file:

```rust
use media_manager::aria2::{Aria2Client, Aria2RpcEndpoint, Aria2Transport};
use media_manager::pipeline::Aria2TaskSnapshot;
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct RecordingTransport {
    response: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl RecordingTransport {
    fn new(response: &str) -> Self {
        Self {
            response: response.to_string(),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn last_request_json(&self) -> Value {
        let requests = self.requests.lock().unwrap();
        serde_json::from_str(requests.last().unwrap()).unwrap()
    }
}

impl Aria2Transport for RecordingTransport {
    fn post_json(
        &self,
        _endpoint: &Aria2RpcEndpoint,
        body: &str,
    ) -> anyhow::Result<String> {
        self.requests.lock().unwrap().push(body.to_string());
        Ok(self.response.clone())
    }
}

fn endpoint_with_secret() -> Aria2RpcEndpoint {
    Aria2RpcEndpoint {
        host: "127.0.0.1".to_string(),
        port: 6800,
        path: "/jsonrpc".to_string(),
        secret: Some("secret-value".to_string()),
        timeout_ms: 1_000,
    }
}

#[test]
fn tell_status_places_secret_token_before_gid() {
    let transport = RecordingTransport::new(
        r#"{"jsonrpc":"2.0","id":"media-manager-tell-status","result":{"gid":"abc","status":"active","totalLength":"10","completedLength":"4","files":[]}}"#,
    );
    let client = Aria2Client::new(endpoint_with_secret(), transport.clone());

    let status = client.tell_status("abc").unwrap();

    assert_eq!(status.gid, "abc");
    let request = transport.last_request_json();
    assert_eq!(request["method"], "aria2.tellStatus");
    assert_eq!(request["params"][0], "token:secret-value");
    assert_eq!(request["params"][1], "abc");
    assert_eq!(request["params"][2][0], "gid");
    assert_eq!(request["params"][2][4], "files");
}

#[test]
fn tell_status_parses_string_lengths_into_task_snapshot() {
    let transport = RecordingTransport::new(
        r#"{"jsonrpc":"2.0","id":"media-manager-tell-status","result":{"gid":"done","status":"complete","totalLength":"12","completedLength":"12","files":[]}}"#,
    );
    let client = Aria2Client::new(endpoint_with_secret(), transport);

    let status = client.tell_status("done").unwrap();

    assert_eq!(
        status.to_task_snapshot().unwrap(),
        Aria2TaskSnapshot {
            status: "complete".to_string(),
            completed_length: 12,
            total_length: 12,
        }
    );
    assert!(status.is_complete().unwrap());
}

#[test]
fn tell_status_returns_json_rpc_error_message() {
    let transport = RecordingTransport::new(
        r#"{"jsonrpc":"2.0","id":"media-manager-tell-status","error":{"code":1,"message":"Unauthorized"}}"#,
    );
    let client = Aria2Client::new(endpoint_with_secret(), transport);

    let error = client.tell_status("abc").unwrap_err();

    assert!(error.to_string().contains("aria2 JSON-RPC error 1"));
    assert!(error.to_string().contains("Unauthorized"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test aria2_rpc -j 1
```

Expected: FAIL because `media_manager::aria2` does not exist.

- [ ] **Step 3: Implement minimal DTOs, client, and fake transport boundary**

Add `pub mod aria2;` to `src-tauri/src/lib.rs`.

Create `src-tauri/src/aria2.rs` with:

```rust
use crate::pipeline::{is_aria2_complete, Aria2TaskSnapshot};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const TELL_STATUS_ID: &str = "media-manager-tell-status";
const TELL_STATUS_KEYS: [&str; 5] = [
    "gid",
    "status",
    "totalLength",
    "completedLength",
    "files",
];

/// Connection settings for one aria2 JSON-RPC endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aria2RpcEndpoint {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub secret: Option<String>,
    pub timeout_ms: u64,
}

impl Aria2RpcEndpoint {
    /// Build the default loopback endpoint used by aria2 unless users configure otherwise.
    pub fn loopback_default(secret: Option<String>) -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 6800,
            path: "/jsonrpc".to_string(),
            secret,
            timeout_ms: 5_000,
        }
    }
}

/// Transport boundary for posting JSON-RPC bodies to aria2.
pub trait Aria2Transport {
    /// Send one JSON body to the configured endpoint and return the response body.
    fn post_json(&self, endpoint: &Aria2RpcEndpoint, body: &str) -> Result<String>;
}

/// JSON-RPC client for aria2 methods needed by the automatic pipeline.
#[derive(Debug, Clone)]
pub struct Aria2Client<T> {
    endpoint: Aria2RpcEndpoint,
    transport: T,
}

impl<T: Aria2Transport> Aria2Client<T> {
    /// Create a client with an explicit endpoint and transport.
    pub fn new(endpoint: Aria2RpcEndpoint, transport: T) -> Self {
        Self { endpoint, transport }
    }

    /// Fetch a single aria2 task status by GID using `aria2.tellStatus`.
    pub fn tell_status(&self, gid: &str) -> Result<Aria2Status> {
        let body = self.tell_status_request_body(gid)?;
        let response = self.transport.post_json(&self.endpoint, &body)?;
        parse_rpc_response(&response)
    }

    /// Build the JSON-RPC body for `aria2.tellStatus`.
    pub fn tell_status_request_body(&self, gid: &str) -> Result<String> {
        if gid.trim().is_empty() {
            return Err(anyhow!("aria2 gid is required"));
        }
        let mut params = Vec::new();
        if let Some(secret) = &self.endpoint.secret {
            params.push(json!(format!("token:{secret}")));
        }
        params.push(json!(gid));
        params.push(json!(TELL_STATUS_KEYS));
        Ok(json!({
            "jsonrpc": "2.0",
            "id": TELL_STATUS_ID,
            "method": "aria2.tellStatus",
            "params": params,
        })
        .to_string())
    }
}

/// Task status returned by `aria2.tellStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aria2Status {
    #[serde(default)]
    pub gid: String,
    #[serde(default)]
    pub status: String,
    #[serde(rename = "totalLength", default)]
    pub total_length: String,
    #[serde(rename = "completedLength", default)]
    pub completed_length: String,
    #[serde(default)]
    pub files: Vec<Aria2File>,
}

impl Aria2Status {
    /// Convert aria2 string fields to the existing pipeline completion snapshot.
    pub fn to_task_snapshot(&self) -> Result<Aria2TaskSnapshot> {
        Ok(Aria2TaskSnapshot {
            status: self.status.clone(),
            completed_length: parse_u64_field("completedLength", &self.completed_length)?,
            total_length: parse_u64_field("totalLength", &self.total_length)?,
        })
    }

    /// Return true only when the task-level aria2 completion detector accepts the task.
    pub fn is_complete(&self) -> Result<bool> {
        Ok(is_aria2_complete(&self.to_task_snapshot()?))
    }
}

/// One file entry returned by aria2 for single-file or BT downloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aria2File {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub length: String,
    #[serde(rename = "completedLength", default)]
    pub completed_length: String,
    #[serde(default)]
    pub selected: String,
}

impl Aria2File {
    /// Return whether aria2 says this file was selected for download.
    pub fn is_selected(&self) -> bool {
        self.selected != "false"
    }

    /// Return whether this file entry has complete byte counts.
    pub fn is_complete(&self) -> Result<bool> {
        let length = parse_u64_field("files[].length", &self.length)?;
        let completed = parse_u64_field("files[].completedLength", &self.completed_length)?;
        Ok(length > 0 && completed == length)
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcEnvelope {
    result: Option<Aria2Status>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

fn parse_rpc_response(raw: &str) -> Result<Aria2Status> {
    let envelope: JsonRpcEnvelope = serde_json::from_str(raw)?;
    if let Some(error) = envelope.error {
        return Err(anyhow!(
            "aria2 JSON-RPC error {}: {}",
            error.code,
            error.message
        ));
    }
    envelope
        .result
        .ok_or_else(|| anyhow!("aria2 JSON-RPC response missing result"))
}

fn parse_u64_field(name: &str, value: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .map_err(|error| anyhow!("invalid aria2 {name}: {value}: {error}"))
}
```

- [ ] **Step 4: Run tests to verify Task 1 passes**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test aria2_rpc -j 1
```

Expected: PASS for the three new tests.

- [ ] **Step 5: Commit Task 1**

```powershell
git add src-tauri/src/lib.rs src-tauri/src/aria2.rs src-tauri/tests/aria2_rpc.rs
git commit -m "新增阶段6B aria2 RPC客户端骨架"
```

---

### Task 2: Completed File Selection

**Files:**
- Modify: `src-tauri/src/aria2.rs`
- Modify: `src-tauri/tests/aria2_rpc.rs`

- [ ] **Step 1: Add failing tests for completed selected video extraction**

Append to `src-tauri/tests/aria2_rpc.rs`:

```rust
#[test]
fn completed_selection_keeps_only_selected_completed_existing_videos() {
    let tmp = tempfile::tempdir().unwrap();
    let video = tmp.path().join("ABP-600.mp4");
    let unselected = tmp.path().join("ABP-601.mp4");
    let partial = tmp.path().join("ABP-602.mp4");
    let notes = tmp.path().join("notes.txt");
    std::fs::write(&video, b"good").unwrap();
    std::fs::write(&unselected, b"skip").unwrap();
    std::fs::write(&partial, b"half").unwrap();
    std::fs::write(&notes, b"note").unwrap();
    let response = format!(
        r#"{{
            "jsonrpc":"2.0",
            "id":"media-manager-tell-status",
            "result":{{
                "gid":"done",
                "status":"complete",
                "totalLength":"20",
                "completedLength":"20",
                "files":[
                    {{"path":"{}","length":"4","completedLength":"4","selected":"true"}},
                    {{"path":"{}","length":"4","completedLength":"4","selected":"false"}},
                    {{"path":"{}","length":"4","completedLength":"2","selected":"true"}},
                    {{"path":"{}","length":"4","completedLength":"4","selected":"true"}},
                    {{"path":"{}","length":"4","completedLength":"4","selected":"true"}}
                ]
            }}
        }}"#,
        video.display(),
        unselected.display(),
        partial.display(),
        notes.display(),
        tmp.path().join("missing.mp4").display(),
    );
    let client = Aria2Client::new(endpoint_with_secret(), RecordingTransport::new(&response));

    let status = client.tell_status("done").unwrap();
    let selection = status.completed_selection().unwrap();

    assert_eq!(selection.scanned_files, 5);
    assert_eq!(selection.skipped_files, 4);
    assert_eq!(selection.files.len(), 1);
    assert_eq!(selection.files[0].path, video);
    assert_eq!(selection.files[0].file_name, "ABP-600.mp4");
}

#[test]
fn completed_selection_is_empty_for_unfinished_task() {
    let response = r#"{"jsonrpc":"2.0","id":"media-manager-tell-status","result":{"gid":"active","status":"active","totalLength":"20","completedLength":"20","files":[]}}"#;
    let client = Aria2Client::new(endpoint_with_secret(), RecordingTransport::new(response));

    let status = client.tell_status("active").unwrap();
    let selection = status.completed_selection().unwrap();

    assert_eq!(selection.scanned_files, 0);
    assert_eq!(selection.skipped_files, 0);
    assert!(selection.files.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test aria2_rpc -j 1
```

Expected: FAIL because `completed_selection` does not exist.

- [ ] **Step 3: Implement completion selection**

In `src-tauri/src/aria2.rs`, add imports:

```rust
use crate::domain::CompletedFile;
use crate::scanner::is_video_file;
use std::path::PathBuf;
```

Add this public DTO:

```rust
/// Files selected from a completed aria2 task and ready for daemon enqueue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Aria2CompletedSelection {
    pub scanned_files: usize,
    pub skipped_files: usize,
    pub files: Vec<CompletedFile>,
}
```

Add this method to `impl Aria2Status`:

```rust
    /// Extract selected, completed, local video files from a completed aria2 task.
    pub fn completed_selection(&self) -> Result<Aria2CompletedSelection> {
        if !self.is_complete()? {
            return Ok(Aria2CompletedSelection::default());
        }

        let mut selection = Aria2CompletedSelection {
            scanned_files: self.files.len(),
            ..Aria2CompletedSelection::default()
        };

        for file in &self.files {
            if !file.is_selected() || !file.is_complete()? {
                selection.skipped_files += 1;
                continue;
            }

            let path = PathBuf::from(&file.path);
            if file.path.trim().is_empty() || !path.exists() || !is_video_file(&path) {
                selection.skipped_files += 1;
                continue;
            }

            selection.files.push(CompletedFile::from_path(&path)?);
        }

        Ok(selection)
    }
```

- [ ] **Step 4: Run tests to verify Task 2 passes**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test aria2_rpc -j 1
```

Expected: PASS for all aria2 RPC tests.

- [ ] **Step 5: Commit Task 2**

```powershell
git add src-tauri/src/aria2.rs src-tauri/tests/aria2_rpc.rs
git commit -m "提取阶段6B aria2完成视频文件"
```

---

### Task 3: HTTP POST Transport

**Files:**
- Modify: `src-tauri/src/aria2.rs`
- Modify: `src-tauri/tests/aria2_rpc.rs`

- [ ] **Step 1: Add failing HTTP transport test**

Append to `src-tauri/tests/aria2_rpc.rs`:

```rust
use media_manager::aria2::HttpAria2Transport;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

#[test]
fn http_transport_posts_jsonrpc_body_to_endpoint_path() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let captured = Arc::new(Mutex::new(String::new()));
    let captured_for_thread = captured.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream);
        *captured_for_thread.lock().unwrap() = request.clone();
        let body = r#"{"jsonrpc":"2.0","id":"media-manager-tell-status","result":{"gid":"abc","status":"complete","totalLength":"1","completedLength":"1","files":[]}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    let endpoint = Aria2RpcEndpoint {
        host: "127.0.0.1".to_string(),
        port,
        path: "/jsonrpc".to_string(),
        secret: None,
        timeout_ms: 1_000,
    };
    let client = Aria2Client::new(endpoint, HttpAria2Transport);

    let status = client.tell_status("abc").unwrap();
    server.join().unwrap();

    assert!(status.is_complete().unwrap());
    let request = captured.lock().unwrap().clone();
    assert!(request.starts_with("POST /jsonrpc HTTP/1.1"));
    assert!(request.contains("Content-Type: application/json"));
    assert!(request.contains(r#""method":"aria2.tellStatus""#));
    assert!(request.contains(r#""params":["abc","#));
}

fn read_http_request(stream: &mut std::net::TcpStream) -> String {
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..read]);
        if let Some(total) = expected_http_request_len(&raw) {
            if raw.len() >= total {
                break;
            }
        }
    }
    String::from_utf8(raw).unwrap()
}

fn expected_http_request_len(raw: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(raw);
    let header_end = text.find("\r\n\r\n")? + 4;
    let content_length = text
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    Some(header_end + content_length)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test aria2_rpc -j 1
```

Expected: FAIL because `HttpAria2Transport` does not exist.

- [ ] **Step 3: Implement HTTP transport**

In `src-tauri/src/aria2.rs`, add imports:

```rust
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
```

Add:

```rust
/// Standard-library HTTP transport for aria2 JSON-RPC POST requests.
#[derive(Debug, Clone, Copy, Default)]
pub struct HttpAria2Transport;

impl Aria2Transport for HttpAria2Transport {
    /// POST one JSON-RPC body and return the HTTP response body.
    fn post_json(&self, endpoint: &Aria2RpcEndpoint, body: &str) -> Result<String> {
        let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))?;
        let timeout = Duration::from_millis(endpoint.timeout_ms);
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;

        let path = if endpoint.path.starts_with('/') {
            endpoint.path.clone()
        } else {
            format!("/{}", endpoint.path)
        };
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            endpoint.host,
            endpoint.port,
            body.len(),
            body
        );

        stream.write_all(request.as_bytes())?;
        stream.flush()?;

        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        parse_http_response_body(&response)
    }
}

fn parse_http_response_body(response: &str) -> Result<String> {
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("aria2 HTTP response missing header terminator"))?;
    let status_line = headers
        .lines()
        .next()
        .ok_or_else(|| anyhow!("aria2 HTTP response missing status line"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("aria2 HTTP response missing status code"))?
        .parse::<u16>()?;
    if !(200..=299).contains(&status) {
        return Err(anyhow!("aria2 HTTP request failed with status {status}"));
    }
    Ok(body.to_string())
}
```

- [ ] **Step 4: Run tests to verify Task 3 passes**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test aria2_rpc -j 1
```

Expected: PASS for all aria2 RPC tests.

- [ ] **Step 5: Commit Task 3**

```powershell
git add src-tauri/src/aria2.rs src-tauri/tests/aria2_rpc.rs
git commit -m "实现阶段6B aria2 HTTP传输"
```

---

### Task 4: Daemon GID Scan Entry

**Files:**
- Modify: `src-tauri/src/daemon.rs`
- Modify: `src-tauri/tests/daemon.rs`

- [ ] **Step 1: Add failing daemon tests**

Modify `src-tauri/tests/daemon.rs` imports:

```rust
use media_manager::aria2::{Aria2Client, Aria2RpcEndpoint, Aria2Transport};
use std::sync::{Arc, Mutex};
```

Add fake transport after `FakeScraper`:

```rust
#[derive(Clone)]
struct StaticAria2Transport {
    response: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl StaticAria2Transport {
    fn new(response: String) -> Self {
        Self {
            response,
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Aria2Transport for StaticAria2Transport {
    fn post_json(
        &self,
        _endpoint: &Aria2RpcEndpoint,
        body: &str,
    ) -> anyhow::Result<String> {
        self.requests.lock().unwrap().push(body.to_string());
        Ok(self.response.clone())
    }
}

fn aria2_client(response: String) -> Aria2Client<StaticAria2Transport> {
    Aria2Client::new(
        Aria2RpcEndpoint::loopback_default(None),
        StaticAria2Transport::new(response),
    )
}
```

Append tests:

```rust
#[test]
fn scan_aria2_gid_queues_completed_selected_video() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    let video = inbox.join("ABP-300.mp4");
    std::fs::write(&video, b"video-300").unwrap();
    let response = format!(
        r#"{{
            "jsonrpc":"2.0",
            "id":"media-manager-tell-status",
            "result":{{
                "gid":"gid-1",
                "status":"complete",
                "totalLength":"9",
                "completedLength":"9",
                "files":[{{"path":"{}","length":"9","completedLength":"9","selected":"true"}}]
            }}
        }}"#,
        video.display()
    );
    let client = aria2_client(response);
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);

    let report = daemon.scan_aria2_gid(&client, "gid-1").unwrap();

    assert_eq!(report.scanned_files, 1);
    assert_eq!(report.queued_files, 1);
    assert_eq!(report.skipped_files, 0);
    assert_eq!(daemon.status().queued, 1);
}

#[test]
fn scan_aria2_gid_does_not_duplicate_known_completed_video() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    let video = inbox.join("ABP-300.mp4");
    std::fs::write(&video, b"video-300").unwrap();
    let response = format!(
        r#"{{
            "jsonrpc":"2.0",
            "id":"media-manager-tell-status",
            "result":{{
                "gid":"gid-1",
                "status":"complete",
                "totalLength":"9",
                "completedLength":"9",
                "files":[{{"path":"{}","length":"9","completedLength":"9","selected":"true"}}]
            }}
        }}"#,
        video.display()
    );
    let client = aria2_client(response);
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);

    let first = daemon.scan_aria2_gid(&client, "gid-1").unwrap();
    let second = daemon.scan_aria2_gid(&client, "gid-1").unwrap();

    assert_eq!(first.queued_files, 1);
    assert_eq!(second.scanned_files, 1);
    assert_eq!(second.queued_files, 0);
    assert_eq!(daemon.status().queued, 1);
}

#[test]
fn scan_aria2_gid_ignores_unfinished_task_without_error() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, _, _, _) = configured_repo(&tmp);
    let response = r#"{"jsonrpc":"2.0","id":"media-manager-tell-status","result":{"gid":"gid-1","status":"active","totalLength":"9","completedLength":"9","files":[]}}"#.to_string();
    let client = aria2_client(response);
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);

    let report = daemon.scan_aria2_gid(&client, "gid-1").unwrap();

    assert_eq!(report.scanned_files, 0);
    assert_eq!(report.queued_files, 0);
    assert_eq!(report.skipped_files, 0);
    assert_eq!(daemon.status().state, DaemonState::Idle);
}
```

- [ ] **Step 2: Run daemon tests to verify they fail**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: FAIL because `scan_aria2_gid` does not exist.

- [ ] **Step 3: Implement daemon entry**

In `src-tauri/src/daemon.rs`, add import:

```rust
use crate::aria2::{Aria2Client, Aria2Transport};
```

Add public method to `impl<'a> HeadlessDaemon<'a>`:

```rust
    /// Poll one aria2 GID and enqueue completed selected video files.
    pub fn scan_aria2_gid<T: Aria2Transport>(
        &mut self,
        client: &Aria2Client<T>,
        gid: &str,
    ) -> Result<ScanReport> {
        if self.state == DaemonState::Paused {
            return Ok(ScanReport::default());
        }

        self.state = DaemonState::Scanning;
        let result = self.scan_aria2_gid_inner(client, gid);
        self.state = if result.is_ok() {
            DaemonState::Idle
        } else {
            DaemonState::Error
        };
        if let Err(error) = &result {
            self.last_error = Some(error.to_string());
        }
        result
    }

    fn scan_aria2_gid_inner<T: Aria2Transport>(
        &mut self,
        client: &Aria2Client<T>,
        gid: &str,
    ) -> Result<ScanReport> {
        let status = client.tell_status(gid)?;
        let selection = status.completed_selection()?;
        let mut report = ScanReport {
            scanned_files: selection.scanned_files,
            skipped_files: selection.skipped_files,
            ..ScanReport::default()
        };

        for file in selection.files {
            if self.queue_completed_file(file) {
                report.queued_files += 1;
            }
        }

        Ok(report)
    }
```

- [ ] **Step 4: Run daemon and aria2 tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test aria2_rpc -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: both test binaries PASS.

- [ ] **Step 5: Commit Task 4**

```powershell
git add src-tauri/src/daemon.rs src-tauri/tests/daemon.rs
git commit -m "接入阶段6B daemon aria2完成扫描"
```

---

### Task 5: State, Review, and Full Verification

**Files:**
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/progress.md`
- Modify: `.ai_state/design.md`
- Create: `.ai_state/reviews/sprint-8.md`
- Modify: `.ai_state/lessons.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Update Sprint 8 task state as each task completes**

Use this task list in `.ai_state/tasks.md`:

```markdown
# Sprint 8 Tasks - 阶段 6B aria2 RPC 集成骨架

- [x] 固化阶段 6B 中文设计
- [x] 编写阶段 6B implementation plan
- [x] Task 1: aria2 RPC DTO 与 fake transport 客户端
- [x] Task 2: aria2 completed selected 视频文件提取
- [x] Task 3: 标准库 HTTP POST transport
- [x] Task 4: daemon scan_aria2_gid 可测试入口
- [x] Task 5: 全量验证、评审和交接
```

- [ ] **Step 2: Run focused verification**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test aria2_rpc -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: both PASS.

- [ ] **Step 3: Run full verification with stop-on-error**

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

Expected: all PASS. Existing `resource_pool.rs` warnings may remain historical.

- [ ] **Step 4: Write review and lessons**

Create `.ai_state/reviews/sprint-8.md` with:

```markdown
# Sprint 8 Review - 阶段 6B aria2 RPC 集成骨架

## Scope

- Added aria2 JSON-RPC DTO/client/transport boundary.
- Added completed selected video extraction.
- Added minimal HTTP POST transport with temp TCP test.
- Added daemon `scan_aria2_gid` entry that queues completed aria2 videos.

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml --test aria2_rpc -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`
- `npm test`
- `npx tsc --noEmit`
- `npm run build`

## Findings

- No blocking findings after self-review.
- Real aria2 process, long-running polling, WebSocket notifications, and download management remain deferred.
```

Append to `.ai_state/lessons.md`:

```markdown
## [2026-06-26 Sprint 8] aria2 RPC 边界要先隔离 transport

- **Pattern**: aria2 JSON-RPC client should build/parse request bodies behind an injectable transport before daemon integration, so fake transport tests can cover secret placement and error semantics without a real aria2 process.
- **Constraint**: Stage 6B only polls explicit GIDs. It does not own aria2 process lifecycle, task creation, WebSocket notifications, or background polling.
```

- [ ] **Step 5: Update HANDOFF**

Add Stage 6B deliverables and next-step options:

```markdown
**阶段 6B（aria2 RPC 集成骨架）已实现并验证。**
阶段 6B 新增纯 Rust aria2 JSON-RPC client、fake transport 测试、标准库 HTTP POST transport、完成任务 selected 视频文件提取，以及 daemon `scan_aria2_gid` 可测试入口。验证不依赖真实 aria2 进程或真实媒体盘。
```

Next step should mention either Stage 6A real scraper adapter skeleton or Stage 6C aria2 config/loop polling.

- [ ] **Step 6: Commit final docs/state handoff**

```powershell
git add HANDOFF.md
git commit -m "更新阶段6B交接说明"
```

---

## Self-Review Checklist

- Spec coverage: Covers RPC endpoint, secret token, `tellStatus`, response parsing, JSON-RPC errors, selected completed file extraction, HTTP transport, daemon queue entry, and verification.
- Scope guard: Does not implement aria2 process management, download creation, UI download panel, WebSocket, callbacks, tray, or background polling.
- Type consistency: Uses `Aria2RpcEndpoint`, `Aria2Transport`, `HttpAria2Transport`, `Aria2Client`, `Aria2Status`, `Aria2File`, `Aria2CompletedSelection`, `HeadlessDaemon::scan_aria2_gid`, and existing `ScanReport` consistently.
- Testability: Every new behavior can run with fake transport, temp files, or a temp TCP server.
