# Stage 3 — Headless Daemon Core Design

## Goal

Stage 3 turns the tested Stage 2 `AutoPipeline` into a headless background core that can be driven without Tauri, WebView2, tray UI, HTTP, or real media resources. It provides the runtime boundary that Stage 4 can later expose to the UI/control surface.

## Scope

In scope:

- Load daemon configuration from SQLite settings:
  - `source_roots`: inbound/download directories to scan.
  - `archive_root`: self-contained library target.
  - `resource_pool_dirs`: local artwork/resource roots.
- Scan configured source roots for video candidates.
- Apply Stage 2 completion checks before processing:
  - reject non-video files;
  - reject files with sibling `.aria2` control files;
  - accept stable files only when two snapshots match.
- Queue completed candidates deterministically.
- Process queued candidates through Stage 2 `AutoPipeline`.
- Maintain in-memory daemon state: `idle`, `scanning`, `processing`, `paused`, `error`.
- Provide a pure Rust control facade for:
  - `status`;
  - `pause`;
  - `resume`;
  - `scan_now`;
  - `process_next`;
  - `run_once`.
- Provide self-contained tests and a CLI-only smoke example using `tempfile`.

Out of scope:

- Tauri GUI, tray icon, WebView2, or `media-manager.exe`.
- Long-running process management or Windows startup registration.
- HTTP/WebSocket server, token authentication, Origin checks.
- Real FANZA/JavBus/JavDB scraper adapters or network access.
- aria2 JSON-RPC integration. Stage 3 keeps the local `.aria2` heuristic and leaves RPC polling for a later phase.
- Frontend command rewiring. `commands.rs` remains Stage 4 work except if a test directly requires a small shared helper.

## Architecture

Stage 3 adds a new pure Rust `daemon` module. It owns orchestration state and calls existing modules instead of duplicating their behavior:

```text
Repository settings
      |
      v
DaemonConfig::load(repo)
      |
      v
HeadlessDaemon::scan_now()
      |
      v
CompletionSnapshot x2 -> CompletedFile
      |
      v
HeadlessDaemon queue
      |
      v
AutoPipeline::process_completed_file()
      |
      v
SQLite: works / file_versions / scrape_jobs / pipeline_runs / holding / exceptions
```

The daemon is deliberately synchronous in Stage 3. Tests can call one method at a time and assert exact state. A later long-running wrapper can put this core behind a thread, timer, watcher, HTTP API, or tray process without changing pipeline semantics.

## New Module

Create `src-tauri/src/daemon.rs`.

Primary responsibilities:

- Configuration loading and validation.
- Source-root scanning.
- Completion sampling policy.
- Queue management.
- Pause/resume/status transitions.
- Calling `AutoPipeline`.

The module must not import `tauri`, start a window, bind a socket, or spawn a permanent background thread.

## Public Types

```rust
pub struct DaemonConfig {
    pub source_roots: Vec<PathBuf>,
    pub archive_root: PathBuf,
    pub asset_roots: Vec<PathBuf>,
}

pub enum DaemonState {
    Idle,
    Scanning,
    Processing,
    Paused,
    Error,
}

pub struct DaemonStatus {
    pub state: DaemonState,
    pub queued: usize,
    pub processed: usize,
    pub last_error: Option<String>,
}

pub struct QueuedFile {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub file_hash: Option<String>,
}

pub struct ScanReport {
    pub scanned_files: usize,
    pub queued_files: usize,
    pub skipped_files: usize,
}

pub struct ProcessReport {
    pub processed: usize,
    pub archived: usize,
    pub holding: usize,
    pub exceptions: usize,
    pub failed: usize,
}

pub struct HeadlessDaemon<'a> {
    pub repo: &'a Repository,
    pub config: DaemonConfig,
    pub scrapers: ScrapeCoordinator<'a>,
}
```

Exact field visibility can be adjusted during implementation, but the responsibilities above must remain intact.

## Control Semantics

`DaemonConfig::load(repo)`:

- Reads `source_roots`, `archive_root`, and `resource_pool_dirs`.
- Fails when archive root is not configured.
- Allows empty source roots so the status API can explain the daemon is idle but unconfigured.
- Does not create directories. Missing roots are skipped by scanning.

`HeadlessDaemon::status()`:

- Returns the in-memory state, queue length, processed count, and last error.
- Does not read or write SQLite.

`pause()`:

- Transitions to `Paused`.
- Does not clear the queue.
- `scan_now` and `process_next` return a paused result without doing filesystem work.

`resume()`:

- Transitions from `Paused` to `Idle`.
- Leaves queued files intact.

`scan_now()`:

- Recursively scans `source_roots`.
- Queues only stable completed video files.
- Avoids duplicate queue entries by canonical path when possible, falling back to raw path.
- Does not process files.
- Does not write `works`, `holding`, `exceptions`, or `pipeline_runs`.

`process_next()`:

- Pops one queued file and calls Stage 2 `AutoPipeline`.
- Updates status counts based on `PipelineOutcome`.
- If `AutoPipeline` returns an operational error, increments `failed`, stores `last_error`, and leaves evidence in `pipeline_runs` through Stage 2.

`run_once()`:

- Calls `scan_now`, then processes all queued files until the queue is empty or paused.
- Returns `ProcessReport`.

## Completion Sampling

Stage 3 controls the delay between two `CompletionSnapshot` samples. To keep tests fast and deterministic, the implementation should use a policy object or constructor parameter:

```rust
pub struct CompletionPolicy {
    pub sample_delay: Duration,
}
```

Production defaults can use a non-zero delay. Tests and smoke examples use `Duration::ZERO`.

The implementation must use Stage 2 `CompletionSnapshot::capture` and `is_heuristically_complete`; it must not invent a second completion predicate.

## Scraper Boundary

Stage 3 accepts a `ScrapeCoordinator<'a>` supplied by the caller. The daemon does not know about real providers yet. Tests and smoke examples use deterministic fake scrapers:

- one success source for a known code;
- one no-result source to exercise `ScrapeFailed`.

This keeps Stage 3 network-free and avoids requiring proxy or third-party site availability.

## Storage Boundaries

Stage 3 does not add new tables unless implementation proves it is necessary. It uses:

- `app_settings` for config;
- `pipeline_runs` for operational outcomes;
- Stage 2 tables for terminal routing.

In-memory queue state is intentionally non-persistent in Stage 3. Crash recovery and durable queue replay are a later enhancement once the control surface and process lifecycle are defined.

## Testing Strategy

All tests live under `src-tauri/tests/daemon.rs` and use `tempfile`.

Required coverage:

- config loads `source_roots`, `archive_root`, and `resource_pool_dirs`;
- config fails with a clear error when `archive_root` is missing;
- scan queues stable videos and skips `.aria2` partials;
- scan skips non-video files and missing roots;
- pause prevents scan/process work and resume restores operation;
- `process_next` archives one queued file through `AutoPipeline`;
- scrape failure routes to `exceptions`;
- missing-code file routes to `holding`;
- operational failure is reflected as failed status, not a content exception;
- `run_once` processes a mixed temp inbox and produces deterministic counts.

Add `src-tauri/examples/stage3_daemon_smoke.rs`:

- creates temp inbox/assets/archive/db;
- configures repository settings;
- runs `HeadlessDaemon::run_once`;
- prints deterministic lines:
  - `stage3_daemon_smoke=completed`
  - `queued=3`
  - `archived=1`
  - `holding=1`
  - `exceptions=1`
  - `failed=0`
  - `no_real_resources_required=true`

Verification commands:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
cargo test --manifest-path src-tauri/Cargo.toml -j 1
cargo run --manifest-path src-tauri/Cargo.toml --example stage3_daemon_smoke -j 1
```

The `-j 1` flag is required on the current Windows Codex host because parallel rustc hit page-file/mmap error `os error 1455` during Stage 2 verification.

## Failure Handling

- Missing archive root: config error; no scan.
- Missing source root: skipped and counted; no error.
- Partial download marker: skipped; no SQLite write.
- Pipeline content exception: Stage 2 writes `exceptions`; daemon increments exception count.
- Pipeline holding route: Stage 2 writes `holding`; daemon increments holding count.
- Pipeline operational error: Stage 2 finishes `pipeline_runs.status = "failed"`; daemon increments failed count and records `last_error`.

## Security

No local network server is introduced in Stage 3, so token / Origin / port selection are not implemented here. The design keeps the control facade method-based so Stage 4 can expose the same operations over Tauri commands or a loopback HTTP server with authentication.

## Risks And Mitigations

- Queue duplicates: de-duplicate by canonicalized path when possible.
- Source files changing after scan: Stage 2 `CompletedFile` snapshot plus staged move verification still catches size mismatches.
- Huge source roots: Stage 3 uses recursive scanning for correctness; OS watcher optimization remains later work.
- Long-running daemon lifecycle: deliberately outside this phase, because Codex cannot safely verify tray/UI/process integration.

## Self-Review

- Placeholder scan: no open TODO/TBD items remain.
- Scope check: focused on headless daemon core only; HTTP/WebSocket/tray/autostart are explicitly deferred.
- Consistency check: all data flow uses Stage 2 `AutoPipeline`, `CompletionSnapshot`, and repository settings; no second pipeline model is introduced.
