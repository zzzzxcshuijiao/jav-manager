# Stage 6E Logging and Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a local structured diagnostic log and redacted diagnostic export so real-environment daemon issues can be investigated without relying on real media resources during tests.

**Architecture:** Create a focused Rust `diagnostics` module that owns JSONL append/tail/rotation/redaction and snapshot export. Tauri command boundaries log key operations through an optional writer in `AppState`; frontend settings adds a compact diagnostics panel that calls explicit commands.

**Tech Stack:** Rust, serde/serde_json, chrono, SQLite repository summaries, Tauri commands, React, TypeScript, Vitest.

---

## File Structure

- Create `src-tauri/src/diagnostics.rs`: structured log DTOs, JSONL writer, tail reader, redaction, snapshot builder, export helper.
- Create `src-tauri/tests/diagnostics.rs`: tempdir-based tests for writer, tail, rotation, redaction, snapshot export.
- Modify `src-tauri/src/lib.rs`: expose `pub mod diagnostics;`.
- Modify `src-tauri/src/commands.rs`: add diagnostics state, setup initialization, commands, and key operation logging.
- Modify `src/api.ts`: add diagnostics DTOs and API wrappers.
- Modify `src/viewModel.ts`: add diagnostics formatting helpers.
- Modify `src/viewModel.test.ts`: cover diagnostics formatting helpers.
- Modify `src/App.tsx`: add diagnostics panel state/actions/UI under settings -> automatic pipeline.
- Modify `src/styles.css`: style the diagnostics panel and log rows.
- Update `.ai_state/tasks.md`, `.ai_state/progress.md`, `.ai_state/design.md`, `.ai_state/project.json`, `HANDOFF.md`, and `.ai_state/reviews/sprint-12.md` during implementation/review.

## Task 1: Structured Diagnostic Log Core

**Files:**
- Create: `src-tauri/src/diagnostics.rs`
- Create: `src-tauri/tests/diagnostics.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing tests for append, tail, rotation, and redaction**

Add `src-tauri/tests/diagnostics.rs`:

```rust
use media_manager::diagnostics::{
    redact_diagnostic_value, redact_proxy_url, DiagnosticLevel, DiagnosticsWriter,
};
use serde_json::json;

#[test]
fn diagnostics_writer_appends_jsonl_and_reads_tail() {
    let tmp = tempfile::tempdir().unwrap();
    let writer = DiagnosticsWriter::new_with_limits(tmp.path().join("logs"), 64 * 1024, 2).unwrap();

    writer
        .append(
            DiagnosticLevel::Info,
            "daemon.run_once",
            "run started",
            json!({ "source_roots": 2 }),
        )
        .unwrap();
    writer
        .append(
            DiagnosticLevel::Warn,
            "settings.aria2",
            "secret configured",
            json!({ "secret": "plain-secret", "tracked_gids": 1 }),
        )
        .unwrap();

    let tail = writer.tail(10).unwrap();

    assert_eq!(tail.len(), 2);
    assert_eq!(tail[0].target, "daemon.run_once");
    assert_eq!(tail[0].message, "run started");
    assert_eq!(tail[1].level, DiagnosticLevel::Warn);
    assert_eq!(tail[1].context["secret"], "***");
    assert!(writer.log_path().ends_with("media-manager.jsonl"));
}

#[test]
fn diagnostics_tail_limit_is_bounded_and_ordered_oldest_to_newest() {
    let tmp = tempfile::tempdir().unwrap();
    let writer = DiagnosticsWriter::new_with_limits(tmp.path().join("logs"), 64 * 1024, 2).unwrap();

    for index in 0..120 {
        writer
            .append(
                DiagnosticLevel::Info,
                "test.sequence",
                format!("entry {index}"),
                json!({ "index": index }),
            )
            .unwrap();
    }

    let tail = writer.tail(3).unwrap();

    assert_eq!(tail.len(), 3);
    assert_eq!(tail[0].message, "entry 117");
    assert_eq!(tail[2].message, "entry 119");

    let bounded = writer.tail(usize::MAX).unwrap();
    assert!(bounded.len() <= 200);
}

#[test]
fn diagnostics_writer_rotates_when_file_exceeds_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let writer = DiagnosticsWriter::new_with_limits(tmp.path().join("logs"), 180, 2).unwrap();

    for index in 0..8 {
        writer
            .append(
                DiagnosticLevel::Info,
                "test.rotation",
                format!("entry {index}"),
                json!({ "padding": "abcdefghijklmnopqrstuvwxyz" }),
            )
            .unwrap();
    }

    assert!(writer.log_path().exists());
    assert!(tmp.path().join("logs").join("media-manager.jsonl.1").exists());
    assert!(!tmp.path().join("logs").join("media-manager.jsonl.3").exists());
}

#[test]
fn diagnostic_redaction_covers_secrets_tokens_cookies_and_proxy_credentials() {
    let redacted = redact_diagnostic_value(json!({
        "secret": "abc",
        "access_token": "token",
        "headers": {
            "Authorization": "Bearer abc",
            "Cookie": "session=abc",
            "safe": "kept"
        },
        "nested": [{ "password": "pw" }]
    }));

    assert_eq!(redacted["secret"], "***");
    assert_eq!(redacted["access_token"], "***");
    assert_eq!(redacted["headers"]["Authorization"], "***");
    assert_eq!(redacted["headers"]["Cookie"], "***");
    assert_eq!(redacted["headers"]["safe"], "kept");
    assert_eq!(redacted["nested"][0]["password"], "***");
    assert_eq!(
        redact_proxy_url("http://user:pass@127.0.0.1:8080/proxy"),
        "http://***@127.0.0.1:8080/proxy"
    );
}
```

- [ ] **Step 2: Run the focused test and confirm it fails for missing module**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6e'
cargo test --manifest-path src-tauri/Cargo.toml --test diagnostics -j 1
```

Expected: FAIL with unresolved import `media_manager::diagnostics`.

- [ ] **Step 3: Add the module export**

Modify `src-tauri/src/lib.rs`:

```rust
pub mod diagnostics;
```

Place it with the other module declarations near `daemon_control`.

- [ ] **Step 4: Implement the minimal diagnostics module**

Create `src-tauri/src/diagnostics.rs` with these public shapes and class/method comments:

```rust
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_MAX_FILES: usize = 3;
const MAX_TAIL_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticLogEntry {
    pub timestamp: String,
    pub level: DiagnosticLevel,
    pub target: String,
    pub message: String,
    pub context: Value,
}

#[derive(Debug, Clone)]
pub struct DiagnosticsWriter {
    log_dir: PathBuf,
    log_path: PathBuf,
    max_bytes: u64,
    max_files: usize,
}
```

Implement:

```rust
impl DiagnosticsWriter {
    /// Create a diagnostics writer under `<log_dir>/media-manager.jsonl` with production limits.
    pub fn new(log_dir: PathBuf) -> Result<Self> {
        Self::new_with_limits(log_dir, DEFAULT_MAX_BYTES, DEFAULT_MAX_FILES)
    }

    /// Create a diagnostics writer with explicit limits so rotation can be tested in temp dirs.
    pub fn new_with_limits(log_dir: PathBuf, max_bytes: u64, max_files: usize) -> Result<Self> {
        fs::create_dir_all(&log_dir)?;
        Ok(Self {
            log_path: log_dir.join("media-manager.jsonl"),
            log_dir,
            max_bytes: max_bytes.max(1),
            max_files: max_files.max(1),
        })
    }

    /// Return the active JSONL log file path for UI display and tests.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Append one redacted JSONL event. A failing append is returned to callers that want to test it.
    pub fn append(
        &self,
        level: DiagnosticLevel,
        target: impl Into<String>,
        message: impl Into<String>,
        context: Value,
    ) -> Result<()> {
        self.rotate_if_needed()?;
        let entry = DiagnosticLogEntry {
            timestamp: Utc::now().to_rfc3339(),
            level,
            target: target.into(),
            message: message.into(),
            context: redact_diagnostic_value(context),
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;
        writeln!(file, "{}", serde_json::to_string(&entry)?)?;
        Ok(())
    }

    /// Read the last `limit` valid entries from the active log, oldest to newest.
    pub fn tail(&self, limit: usize) -> Result<Vec<DiagnosticLogEntry>> {
        let limit = limit.clamp(1, MAX_TAIL_LIMIT);
        if !self.log_path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&self.log_path)?;
        let reader = BufReader::new(file);
        let mut entries = VecDeque::with_capacity(limit);
        for line in reader.lines() {
            let line = line?;
            if let Ok(entry) = serde_json::from_str::<DiagnosticLogEntry>(&line) {
                if entries.len() == limit {
                    entries.pop_front();
                }
                entries.push_back(entry);
            }
        }
        Ok(entries.into_iter().collect())
    }

    fn rotate_if_needed(&self) -> Result<()> {
        if !self.log_path.exists() || fs::metadata(&self.log_path)?.len() < self.max_bytes {
            return Ok(());
        }
        for index in (1..=self.max_files).rev() {
            let from = self.log_dir.join(format!("media-manager.jsonl.{index}"));
            let to = self.log_dir.join(format!("media-manager.jsonl.{}", index + 1));
            if to.exists() && index == self.max_files {
                fs::remove_file(&to)?;
            }
            if from.exists() && index < self.max_files {
                fs::rename(from, to)?;
            }
        }
        let first = self.log_dir.join("media-manager.jsonl.1");
        if first.exists() {
            fs::remove_file(&first)?;
        }
        fs::rename(&self.log_path, first)?;
        Ok(())
    }
}
```

Implement redaction:

```rust
pub fn redact_diagnostic_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(redact_map(map)),
        Value::Array(items) => Value::Array(items.into_iter().map(redact_diagnostic_value).collect()),
        other => other,
    }
}

fn redact_map(map: Map<String, Value>) -> Map<String, Value> {
    map.into_iter()
        .map(|(key, value)| {
            let lower = key.to_ascii_lowercase();
            let next = if is_secret_key(&lower) {
                Value::String("***".to_string())
            } else if lower.contains("proxy") {
                match value {
                    Value::String(url) => Value::String(redact_proxy_url(&url)),
                    other => redact_diagnostic_value(other),
                }
            } else {
                redact_diagnostic_value(value)
            };
            (key, next)
        })
        .collect()
}

fn is_secret_key(key: &str) -> bool {
    ["secret", "token", "password", "authorization", "cookie"]
        .iter()
        .any(|needle| key.contains(needle))
}

pub fn redact_proxy_url(value: &str) -> String {
    let Some((scheme, rest)) = value.split_once("://") else {
        return value.to_string();
    };
    let Some((_, host)) = rest.split_once('@') else {
        return value.to_string();
    };
    format!("{scheme}://***@{host}")
}
```

- [ ] **Step 5: Run focused tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6e'
cargo test --manifest-path src-tauri/Cargo.toml --test diagnostics -j 1
```

Expected: PASS for the four diagnostics core tests.

- [ ] **Step 6: Commit Task 1**

```powershell
git add src-tauri/src/lib.rs src-tauri/src/diagnostics.rs src-tauri/tests/diagnostics.rs
git commit -m "实现阶段6E诊断日志核心"
```

## Task 2: Diagnostic Snapshot and Export

**Files:**
- Modify: `src-tauri/src/diagnostics.rs`
- Modify: `src-tauri/tests/diagnostics.rs`

- [ ] **Step 1: Write failing snapshot export test**

Append to `src-tauri/tests/diagnostics.rs`:

```rust
use media_manager::aria2::Aria2Settings;
use media_manager::domain::{
    Exception, ExceptionKind, ExceptionStatus, HoldingEntry, HoldingReason, PipelineRun, ScrapeJob,
    ScrapeStatus,
};
use media_manager::diagnostics::{
    build_diagnostic_snapshot, export_diagnostic_snapshot, DiagnosticSnapshotInput,
};
use media_manager::remote_scraper::RemoteScraperSettings;
use media_manager::storage::Repository;
use std::path::Path;

fn open_temp_repo(path: &Path) -> Repository {
    let repo = Repository::open(path).unwrap();
    repo.migrate().unwrap();
    repo
}

#[test]
fn diagnostic_snapshot_exports_redacted_settings_and_recent_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = open_temp_repo(&tmp.path().join("library.sqlite"));
    repo.set_source_roots(&[tmp.path().join("source")]).unwrap();
    repo.set_archive_root(&tmp.path().join("archive")).unwrap();
    repo.set_metadata_provider_enabled(true).unwrap();
    repo.set_aria2_settings(&Aria2Settings {
        enabled: true,
        secret: Some("aria-secret".to_string()),
        tracked_gids: vec!["gid-a".to_string(), "gid-b".to_string()],
        ..Aria2Settings::default()
    })
    .unwrap();
    repo.set_remote_scraper_settings(&RemoteScraperSettings {
        enabled: true,
        proxy_url: Some("http://user:pass@127.0.0.1:8888".to_string()),
        ..RemoteScraperSettings::default()
    })
    .unwrap();
    repo.record_pipeline_run(&PipelineRun {
        id: None,
        file_path: "H:/fake/ABP-001.mp4".to_string(),
        started_at: Some("2026-06-26T00:00:00Z".to_string()),
        finished_at: Some("2026-06-26T00:00:01Z".to_string()),
        steps_json: "[]".to_string(),
        status: "completed".to_string(),
        error: None,
    })
    .unwrap();
    repo.record_scrape_job(&ScrapeJob {
        id: None,
        work_id: None,
        normalized_code: Some("ABP-001".to_string()),
        object_path: Some("H:/fake/ABP-001.mp4".to_string()),
        pipeline_run_id: None,
        source: "javdb".to_string(),
        status: ScrapeStatus::Failed,
        attempts: 1,
        last_attempted_at: Some("2026-06-26T00:00:00Z".to_string()),
        error: Some("fixture failure".to_string()),
    })
    .unwrap();
    repo.record_exception(&Exception {
        id: None,
        object_path: "H:/fake/ABP-001.mp4".to_string(),
        kind: ExceptionKind::ScrapeFailed,
        evidence_json: "{}".to_string(),
        status: ExceptionStatus::Open,
        created_at: None,
        resolved_at: None,
    })
    .unwrap();
    repo.add_holding(&HoldingEntry {
        id: None,
        path: "H:/fake/no-code.mp4".to_string(),
        file_name: "no-code.mp4".to_string(),
        size_bytes: 42,
        reason: HoldingReason::NoCode,
        created_at: None,
    })
    .unwrap();

    let writer = DiagnosticsWriter::new(tmp.path().join("logs")).unwrap();
    writer
        .append(
            DiagnosticLevel::Info,
            "settings.aria2",
            "saved",
            json!({ "secret": "aria-secret" }),
        )
        .unwrap();

    let snapshot = build_diagnostic_snapshot(DiagnosticSnapshotInput {
        app_data_dir: tmp.path(),
        repository: Some(&repo),
        control_service: None,
        daemon: None,
        recent_logs: writer.tail(200).unwrap(),
    })
    .unwrap();
    let result = export_diagnostic_snapshot(tmp.path(), &snapshot).unwrap();
    let exported = std::fs::read_to_string(&result.path).unwrap();

    assert_eq!(result.pipeline_runs, 1);
    assert_eq!(result.scrape_jobs, 1);
    assert_eq!(result.logs, 1);
    assert!(exported.contains("\"aria2_secret_configured\":true"));
    assert!(exported.contains("http://***@127.0.0.1:8888"));
    assert!(!exported.contains("aria-secret"));
    assert!(!exported.contains("user:pass"));
}
```

- [ ] **Step 2: Run focused test and confirm missing snapshot types**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6e'
cargo test --manifest-path src-tauri/Cargo.toml --test diagnostics -j 1
```

Expected: FAIL with unresolved `DiagnosticSnapshotInput`, `build_diagnostic_snapshot`, or `export_diagnostic_snapshot`.

- [ ] **Step 3: Implement snapshot DTOs and settings summary**

Add to `src-tauri/src/diagnostics.rs`:

```rust
use crate::control_service_host::ControlServiceHostStatus;
use crate::daemon_control::DaemonControlStatus;
use crate::domain::{Exception, HoldingEntry, PipelineRun, ScrapeJob};
use crate::storage::Repository;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticSettingsSummary {
    pub source_root_count: usize,
    pub archive_root_configured: bool,
    pub metadata_provider_enabled: bool,
    pub aria2_enabled: bool,
    pub aria2_host: String,
    pub aria2_port: u16,
    pub aria2_secret_configured: bool,
    pub aria2_tracked_gids: usize,
    pub remote_scraper_enabled: bool,
    pub remote_scraper_sources: usize,
    pub remote_scraper_proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticSnapshot {
    pub generated_at: String,
    pub app_data_dir: String,
    pub control_service: Option<ControlServiceHostStatus>,
    pub daemon: Option<DaemonControlStatus>,
    pub settings: Option<DiagnosticSettingsSummary>,
    pub recent_pipeline_runs: Vec<PipelineRun>,
    pub recent_scrape_jobs: Vec<ScrapeJob>,
    pub open_exceptions: Vec<Exception>,
    pub holding_items: Vec<HoldingEntry>,
    pub recent_logs: Vec<DiagnosticLogEntry>,
}

pub struct DiagnosticSnapshotInput<'a> {
    pub app_data_dir: &'a Path,
    pub repository: Option<&'a Repository>,
    pub control_service: Option<ControlServiceHostStatus>,
    pub daemon: Option<DaemonControlStatus>,
    pub recent_logs: Vec<DiagnosticLogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticExportResult {
    pub path: String,
    pub logs: usize,
    pub pipeline_runs: usize,
    pub scrape_jobs: usize,
    pub open_exceptions: usize,
    pub holding_items: usize,
}
```

Implement `build_diagnostic_snapshot` so it:

- Reads repository settings when `repository` is `Some`.
- Limits pipeline runs to 10, scrape jobs to 20, open exceptions to 20, holding to 20.
- Keeps only `ExceptionStatus::Open` in `open_exceptions`.
- Applies `redact_proxy_url` to `remote_scraper_proxy_url`.
- Returns empty vectors and `settings: None` when repository is missing.

Implement `export_diagnostic_snapshot`:

```rust
pub fn export_diagnostic_snapshot(
    app_data_dir: &Path,
    snapshot: &DiagnosticSnapshot,
) -> Result<DiagnosticExportResult> {
    let output_dir = app_data_dir.join("diagnostics");
    fs::create_dir_all(&output_dir)?;
    let file_name = format!(
        "diagnostics-{}.json",
        Utc::now().format("%Y%m%d-%H%M%S")
    );
    let path = output_dir.join(file_name);
    fs::write(&path, serde_json::to_string_pretty(snapshot)?)?;
    Ok(DiagnosticExportResult {
        path: path.to_string_lossy().to_string(),
        logs: snapshot.recent_logs.len(),
        pipeline_runs: snapshot.recent_pipeline_runs.len(),
        scrape_jobs: snapshot.recent_scrape_jobs.len(),
        open_exceptions: snapshot.open_exceptions.len(),
        holding_items: snapshot.holding_items.len(),
    })
}
```

- [ ] **Step 4: Run focused diagnostics tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6e'
cargo test --manifest-path src-tauri/Cargo.toml --test diagnostics -j 1
```

Expected: PASS.

- [ ] **Step 5: Commit Task 2**

```powershell
git add src-tauri/src/diagnostics.rs src-tauri/tests/diagnostics.rs
git commit -m "实现阶段6E诊断快照导出"
```

## Task 3: Tauri Commands and Operation Logging

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/tests/diagnostics.rs` or `src-tauri/src/commands.rs` test module

- [ ] **Step 1: Add command-layer helper tests**

In `src-tauri/src/commands.rs` test module, add tests that do not require a Tauri runtime:

```rust
use crate::diagnostics::{DiagnosticLevel, DiagnosticsWriter};
use serde_json::json;

#[test]
fn app_state_diagnostic_logging_is_optional() {
    let state = AppState::default();

    log_diagnostic_event(
        &state,
        DiagnosticLevel::Info,
        "test.optional",
        "no writer",
        json!({ "ok": true }),
    );

    assert!(state.diagnostics.lock().unwrap().is_none());
}

#[test]
fn app_state_diagnostic_logging_writes_when_initialized() {
    let state = AppState::default();
    let tmp = tempfile::tempdir().unwrap();
    let writer = DiagnosticsWriter::new(tmp.path().join("logs")).unwrap();
    *state.diagnostics.lock().unwrap() = Some(writer.clone());

    log_diagnostic_event(
        &state,
        DiagnosticLevel::Error,
        "test.initialized",
        "failed",
        json!({ "token": "secret-token" }),
    );

    let tail = writer.tail(10).unwrap();
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].target, "test.initialized");
    assert_eq!(tail[0].context["token"], "***");
}
```

- [ ] **Step 2: Run command tests and confirm missing field/helper**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6e'
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::app_state_diagnostic -j 1
```

Expected: FAIL because `AppState.diagnostics` and `log_diagnostic_event` do not exist.

- [ ] **Step 3: Add AppState diagnostics field**

Modify `src-tauri/src/commands.rs` imports:

```rust
use crate::diagnostics::{
    build_diagnostic_snapshot, export_diagnostic_snapshot, DiagnosticExportResult,
    DiagnosticLevel, DiagnosticLogEntry, DiagnosticSnapshotInput, DiagnosticsWriter,
};
use serde_json::json;
```

Add field:

```rust
pub diagnostics: Mutex<Option<DiagnosticsWriter>>,
```

Initialize in `Default`:

```rust
diagnostics: Mutex::new(None),
```

- [ ] **Step 4: Add no-fail logging helper**

Add near other private helpers:

```rust
fn log_diagnostic_event(
    state: &AppState,
    level: DiagnosticLevel,
    target: &str,
    message: impl Into<String>,
    context: serde_json::Value,
) {
    let Ok(guard) = state.diagnostics.lock() else {
        return;
    };
    let Some(writer) = guard.as_ref() else {
        return;
    };
    let _ = writer.append(level, target, message, context);
}
```

- [ ] **Step 5: Add Tauri commands**

Add public commands:

```rust
#[tauri::command]
pub fn get_diagnostic_log_tail(
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<DiagnosticLogEntry>>, String> {
    let guard = state.diagnostics.lock().map_err(|error| error.to_string())?;
    let logs = match guard.as_ref() {
        Some(writer) => writer.tail(limit.unwrap_or(80)).map_err(|error| error.to_string())?,
        None => Vec::new(),
    };
    Ok(CommandResult { data: logs })
}

#[tauri::command]
pub fn export_diagnostics_snapshot_command(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<CommandResult<DiagnosticExportResult>, String> {
    let app_data = app.path().app_data_dir().map_err(|error| error.to_string())?;
    let recent_logs = {
        let guard = state.diagnostics.lock().map_err(|error| error.to_string())?;
        match guard.as_ref() {
            Some(writer) => writer.tail(200).map_err(|error| error.to_string())?,
            None => Vec::new(),
        }
    };
    let repo_guard = state.repository.lock().map_err(|error| error.to_string())?;
    let daemon = read_daemon_status(&state).ok();
    let control_service = Some(state.control_service_host_status(
        &app_data,
        state.control_service_error.lock().ok().and_then(|error| error.clone()),
    ));
    let snapshot = build_diagnostic_snapshot(DiagnosticSnapshotInput {
        app_data_dir: &app_data,
        repository: repo_guard.as_ref(),
        control_service,
        daemon,
        recent_logs,
    })
    .map_err(|error| error.to_string())?;
    let result = export_diagnostic_snapshot(&app_data, &snapshot).map_err(|error| error.to_string())?;
    log_diagnostic_event(
        &state,
        DiagnosticLevel::Info,
        "diagnostics.export",
        "diagnostic snapshot exported",
        json!({ "path": result.path, "logs": result.logs }),
    );
    Ok(CommandResult { data: result })
}
```

Register both commands in `generate_handler!`.

- [ ] **Step 6: Initialize writer during setup**

Inside `.setup`, after `fs::create_dir_all(&app_data)`:

```rust
let diagnostics = DiagnosticsWriter::new(app_data.join("logs"))
    .map_err(|error| error.to_string())?;
let state = app.state::<AppState>();
*state.diagnostics.lock().map_err(|error| error.to_string())? = Some(diagnostics);
log_diagnostic_event(
    &state,
    DiagnosticLevel::Info,
    "app.setup",
    "app data initialized",
    json!({ "app_data_dir": app_data.to_string_lossy() }),
);
```

Keep the existing `let state = app.state::<AppState>();` single-owned; move it up rather than declaring twice.

- [ ] **Step 7: Log key operations**

Add successful/error logging in:

- `configure_aria2_settings`: log enabled, host, port, tracked gid count, secret configured.
- `configure_remote_scraper_settings_in_state`: log enabled, source count, proxy configured.
- `pause_daemon` and `resume_daemon`: log resulting state.
- `run_daemon_once_command`: log start, success summary, and error.

Use this pattern:

```rust
log_diagnostic_event(
    &state,
    DiagnosticLevel::Info,
        "daemon.run_once",
        "run completed",
        json!({
            "scan_scanned_files": report.scan.scanned_files,
            "scan_queued_files": report.scan.queued_files,
            "scan_skipped_files": report.scan.skipped_files,
            "aria2_enabled": report.aria2.enabled,
            "aria2_completed_gids": report.aria2.completed_gids,
            "aria2_failed_gids": report.aria2.failed_gids,
            "processed": report.process.processed,
            "archived": report.process.archived,
            "holding": report.process.holding,
            "exceptions": report.process.exceptions,
            "failed": report.process.failed,
        }),
);
```

- [ ] **Step 8: Run focused Rust tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6e'
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::app_state_diagnostic -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test diagnostics -j 1
```

Expected: PASS.

- [ ] **Step 9: Commit Task 3**

```powershell
git add src-tauri/src/commands.rs src-tauri/src/diagnostics.rs src-tauri/tests/diagnostics.rs
git commit -m "接入阶段6E诊断命令日志"
```

## Task 4: Frontend Diagnostics Panel

**Files:**
- Modify: `src/api.ts`
- Modify: `src/viewModel.ts`
- Modify: `src/viewModel.test.ts`
- Modify: `src/App.tsx`
- Modify: `src/styles.css`

- [ ] **Step 1: Write failing frontend helper tests**

Append to `src/viewModel.test.ts`:

```ts
describe("diagnostics formatting", () => {
  it("formats diagnostic log lines", () => {
    expect(
      formatDiagnosticLogLine({
        timestamp: "2026-06-26T10:00:00Z",
        level: "Error",
        target: "daemon.run_once",
        message: "run failed",
        context: { error: "boom" }
      })
    ).toBe("2026-06-26T10:00:00Z · 错误 · daemon.run_once · run failed");
  });

  it("formats diagnostic export summaries", () => {
    expect(
      formatDiagnosticExportSummary({
        path: "C:/Users/DELL/AppData/Roaming/local.media-manager/diagnostics/diagnostics-20260626-100000.json",
        logs: 12,
        pipeline_runs: 2,
        scrape_jobs: 3,
        open_exceptions: 1,
        holding_items: 4
      })
    ).toContain("已导出诊断快照");
  });
});
```

Add imports:

```ts
formatDiagnosticExportSummary,
formatDiagnosticLogLine,
```

- [ ] **Step 2: Run focused Vitest and confirm missing helpers**

Run:

```powershell
npm test -- src/viewModel.test.ts
```

Expected: FAIL because diagnostics formatting helpers are not exported.

- [ ] **Step 3: Add API DTOs and wrappers**

Modify `src/api.ts`:

```ts
export type DiagnosticLevel = "Info" | "Warn" | "Error";

export interface DiagnosticLogEntry {
  timestamp: string;
  level: DiagnosticLevel;
  target: string;
  message: string;
  context: unknown;
}

export interface DiagnosticExportResult {
  path: string;
  logs: number;
  pipeline_runs: number;
  scrape_jobs: number;
  open_exceptions: number;
  holding_items: number;
}
```

Add methods:

```ts
getDiagnosticLogTail(limit = 80) {
  return command<DiagnosticLogEntry[]>("get_diagnostic_log_tail", { limit });
},
exportDiagnosticsSnapshot() {
  return command<DiagnosticExportResult>("export_diagnostics_snapshot_command");
},
```

- [ ] **Step 4: Add view model helpers**

Modify `src/viewModel.ts` imports to include diagnostics types:

```ts
DiagnosticExportResult,
DiagnosticLogEntry,
```

Add helpers near daemon formatting helpers:

```ts
export function formatDiagnosticLevel(level: DiagnosticLogEntry["level"]): string {
  const labels: Record<DiagnosticLogEntry["level"], string> = {
    Info: "信息",
    Warn: "警告",
    Error: "错误"
  };
  return labels[level] ?? level;
}

export function formatDiagnosticLogLine(entry: DiagnosticLogEntry): string {
  return `${entry.timestamp} · ${formatDiagnosticLevel(entry.level)} · ${entry.target} · ${entry.message}`;
}

export function formatDiagnosticExportSummary(result: DiagnosticExportResult): string {
  return `已导出诊断快照：${result.path}（日志 ${result.logs} 条，管线 ${result.pipeline_runs} 条，刮削 ${result.scrape_jobs} 条，异常 ${result.open_exceptions} 条，搁置 ${result.holding_items} 条）`;
}
```

- [ ] **Step 5: Wire App state and actions**

Modify `src/App.tsx` imports from `./api`:

```ts
DiagnosticLogEntry,
```

Modify imports from `./viewModel`:

```ts
formatDiagnosticExportSummary,
formatDiagnosticLogLine,
```

Add state near daemon state:

```ts
const [diagnosticLogs, setDiagnosticLogs] = useState<DiagnosticLogEntry[]>([]);
const [diagnosticsBusy, setDiagnosticsBusy] = useState<"refresh" | "export" | null>(null);
```

Add functions near daemon handlers:

```ts
async function refreshDiagnosticLogs() {
  if (diagnosticsBusy) return;
  setDiagnosticsBusy("refresh");
  setStatus("正在刷新诊断日志...");
  try {
    const logs = await api.getDiagnosticLogTail(80);
    setDiagnosticLogs(logs);
    setStatus(`已刷新诊断日志：${logs.length} 条。`);
  } catch (error) {
    setStatus(`刷新诊断日志失败：${String(error)}`);
  } finally {
    setDiagnosticsBusy(null);
  }
}

async function exportDiagnosticsSnapshot() {
  if (diagnosticsBusy) return;
  setDiagnosticsBusy("export");
  setStatus("正在导出诊断快照...");
  try {
    const result = await api.exportDiagnosticsSnapshot();
    setStatus(formatDiagnosticExportSummary(result));
    const logs = await api.getDiagnosticLogTail(80);
    setDiagnosticLogs(logs);
  } catch (error) {
    setStatus(`导出诊断快照失败：${String(error)}`);
  } finally {
    setDiagnosticsBusy(null);
  }
}
```

In the settings daemon `useEffect`, after `refreshDaemonPanel();`, also call `refreshDiagnosticLogs();` only when backend exists. If double loading status is noisy, call a helper that loads logs without changing status:

```ts
api.getDiagnosticLogTail(80).then(setDiagnosticLogs).catch(() => setDiagnosticLogs([]));
```

- [ ] **Step 6: Add diagnostics UI block**

Inside the daemon panel after recent pipeline runs, add:

```tsx
<div className="diagnostics-panel">
  <div className="daemon-list-head">
    <h3>诊断日志</h3>
    <span>{diagnosticLogs.length} 条</span>
  </div>
  <div className="daemon-actions">
    <button type="button" onClick={refreshDiagnosticLogs} disabled={diagnosticsBusy !== null || !hasBackend}>
      <RefreshCw size={16} /> {diagnosticsBusy === "refresh" ? "刷新中" : "刷新日志"}
    </button>
    <button type="button" onClick={exportDiagnosticsSnapshot} disabled={diagnosticsBusy !== null || !hasBackend}>
      <Settings size={16} /> {diagnosticsBusy === "export" ? "导出中" : "导出诊断"}
    </button>
  </div>
  <div className="diagnostic-log-list">
    {diagnosticLogs.length === 0 ? (
      <p className="empty-state">暂无诊断日志。</p>
    ) : (
      diagnosticLogs.slice(-20).map((entry, index) => (
        <div className={`diagnostic-log-row ${entry.level.toLowerCase()}`} key={`${entry.timestamp}-${entry.target}-${index}`}>
          {formatDiagnosticLogLine(entry)}
        </div>
      ))
    )}
  </div>
</div>
```

- [ ] **Step 7: Add CSS**

Modify `src/styles.css`:

```css
.diagnostics-panel {
  display: grid;
  gap: 12px;
}

.diagnostic-log-list {
  display: grid;
  gap: 6px;
  max-height: 280px;
  overflow: auto;
}

.diagnostic-log-row {
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 8px 10px;
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text);
  overflow-wrap: anywhere;
}

.diagnostic-log-row.warn {
  border-color: rgba(217, 119, 6, 0.45);
}

.diagnostic-log-row.error {
  border-color: rgba(220, 38, 38, 0.55);
  color: #991b1b;
}
```

Use existing CSS variables if names differ; keep the block visually consistent with the daemon lists.

- [ ] **Step 8: Run frontend checks**

Run:

```powershell
npm test -- src/viewModel.test.ts
npx tsc --noEmit
```

Expected: PASS.

- [ ] **Step 9: Commit Task 4**

```powershell
git add src/api.ts src/viewModel.ts src/viewModel.test.ts src/App.tsx src/styles.css
git commit -m "接入阶段6E前端诊断面板"
```

## Task 5: Full Verification, Review, and Handoff

**Files:**
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/progress.md`
- Modify: `.ai_state/reviews/sprint-12.md`
- Modify: `.ai_state/lessons.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Mark tasks complete as implementation progresses**

Update `.ai_state/tasks.md` after each task:

```markdown
- [x] Task 1: 结构化诊断日志核心
- [x] Task 2: 诊断快照与脱敏导出
- [x] Task 3: Tauri 命令与关键操作日志接入
- [x] Task 4: 前端诊断面板
```

- [ ] **Step 2: Run full safe gate**

Before cargo, check for stuck Rust processes:

```powershell
Get-Process cargo,rustc,link -ErrorAction SilentlyContinue
```

Then run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6e'
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

Expected: all commands PASS. Do not run `tauri dev`, `cargo run`, `media-manager.exe`, or any WebView2/in-app-browser path.

- [ ] **Step 3: Self-review the diff**

Run:

```powershell
git diff --stat
git diff -- src-tauri/src/diagnostics.rs src-tauri/src/commands.rs src/api.ts src/App.tsx src/viewModel.ts src/viewModel.test.ts src/styles.css
```

Check:

- Log writes are best-effort and cannot fail business commands.
- Snapshot export redacts secrets and does not include media/NFO/image contents.
- JSONL tail is bounded.
- Frontend long operations have disabled/loading/status feedback.
- No PowerShell write operation touched CJK source.

- [ ] **Step 4: Write review record**

Create `.ai_state/reviews/sprint-12.md`:

```markdown
# Sprint 12 Review - 阶段 6E 日志与诊断系统

## Scope

- 结构化 JSONL 诊断日志
- 脱敏诊断快照导出
- Tauri 诊断 commands 与关键操作日志
- 设置页诊断面板

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`
- `npm test`
- `npx tsc --noEmit`
- `npm run build`

## Findings

- PASS: 日志和快照测试使用临时目录和假数据，不依赖真实资源。
- PASS: 导出快照不包含 secret/token/password/cookie/proxy credentials 明文。
- PASS: 诊断操作有 loading/disabled/status 反馈。

## Residual Risk

- 本阶段只在命令边界记录事件，深层 daemon/aria2/scraper 内部细粒度日志留给常驻 daemon 阶段。
```

- [ ] **Step 5: Update lessons and handoff**

Append one concrete lesson to `.ai_state/lessons.md`:

```markdown
- 2026-06-26: 诊断日志应独立于业务 SQLite 表；命令边界日志能先提供时间线，同时日志写入必须 best-effort，避免诊断层阻断主流程。
```

Update `HANDOFF.md`:

- Current status: Stage 6E complete.
- Safe verification commands and results.
- Diagnostics files location: app data `logs/media-manager.jsonl` and `diagnostics/diagnostics-*.json`.
- Known boundary: no real-time streaming and no deep module logger injection yet.

- [ ] **Step 6: Commit verification and handoff**

```powershell
git add HANDOFF.md
git commit -m "更新阶段6E交接说明"
```

`.ai_state` is gitignored, so it stays local.

- [ ] **Step 7: Final status**

Report:

- Commits created.
- Tests run and pass/fail status.
- Diagnostic artifact behavior.
- Remaining non-goals.
