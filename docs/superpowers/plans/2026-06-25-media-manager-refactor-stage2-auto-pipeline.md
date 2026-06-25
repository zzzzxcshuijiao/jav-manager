# Stage 2 — Automated Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Rust core for the zero-intervention pipeline: completed file detection -> code identification -> multi-source scraping -> self-contained archive layout -> SQLite ingest, with holding and exception routing for the three human-action cases.

**Architecture:** Add a new pure Rust `pipeline` module that does not depend on Tauri/WebView2 and can be tested with `tempfile`. Keep the old ingest/review path intact, but do not extend it as the new product core. Stage 2 owns pipeline orchestration, recoverable move semantics, and storage helpers; Stage 3 will wrap it in the daemon/control interface, and Stage 4 will wire the frontend.

**Tech Stack:** Rust 2021, rusqlite 0.32, serde/serde_json, chrono, tempfile, existing `identifier` / `scanner` / `archive` / `storage` modules, `cargo test`.

---

## Global Constraints

- Never run `tauri dev`, `cargo run`, or `media-manager.exe` in Codex. Use only `cargo test --manifest-path src-tauri/Cargo.toml` and example binaries via `cargo run --example ...` only when the example is CLI-only and does not initialize Tauri. For Stage 2, the new smoke is an example and is safe.
- All tests must create fake files under `tempfile`; no `H:/` videos, no `G:/` image library, and no network.
- New Rust structs and public methods must have doc comments that explain responsibility and boundary. Keep comments accurate when editing existing code.
- Stage 2 must not implement the long-running daemon or HTTP/WebSocket control API; it only provides the tested core that Stage 3 will call.
- `commands.rs:1422` `parse_watch_status` remains Stage 4 work. Do not touch the frontend command parser in this stage unless a test in this plan explicitly requires it.
- Add `remove_work_from_collection` and an `ON DELETE CASCADE` regression test in Stage 2, because Stage 1 review deferred it here.
- Do not create pre-scrape `works` rows before scraping succeeds and the archive layout is ready. Scrape failures write `scrape_jobs`, `exceptions`, and `pipeline_runs`; successful runs create / update `works`.
- Treat move / copy / write failures as pipeline operational failures, not content exceptions. They stay in `pipeline_runs.status = "failed"` for retry/recovery.

---

## File Structure

- **Modify** `src-tauri/src/lib.rs` — expose `pub mod pipeline;`.
- **Modify** `src-tauri/src/domain.rs` — add pipeline-specific DTOs: `CompletedFile`, `ArchiveAsset`, `PipelineStepRecord`, `PipelineOutcome`, `ScrapedWorkMetadata`, `ScrapeAttempt`.
- **Modify** `src-tauri/src/storage.rs` — add direct pipeline storage helpers: nullable scrape job context, `find_file_versions_by_hash`, `upsert_file_version_for_work`, `remove_work_from_collection`; keep Stage 1 CRUD intact.
- **Modify** `src-tauri/src/nfo.rs` — add a small NFO writer for scraped metadata.
- **Create** `src-tauri/src/pipeline.rs` — completion checks, classification, multi-source scraper coordinator, archive layout planner/executor, and `AutoPipeline`.
- **Create** `src-tauri/tests/auto_pipeline.rs` — Stage 2 integration tests using `tempfile`.
- **Create** `src-tauri/examples/stage2_smoke.rs` — self-contained smoke that creates fake video/image files, runs the pipeline, and prints a deterministic summary.

---

## Interfaces To Produce

```rust
// src-tauri/src/domain.rs
pub struct CompletedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size_bytes: u64,
    pub file_hash: Option<String>,
}

pub struct ScrapedWorkMetadata {
    pub source: String,
    pub normalized_code: String,
    pub title: String,
    pub original_title: Option<String>,
    pub summary: Option<String>,
    pub actors: Vec<String>,
    pub genres: Vec<String>,
    pub studio: Option<String>,
    pub director: Option<String>,
    pub release_date: Option<String>,
    pub cover_path: Option<PathBuf>,
}

pub struct ArchiveAsset {
    pub source_path: PathBuf,
    pub relative_target: PathBuf,
}

pub struct PipelineStepRecord {
    pub step: String,
    pub status: String,
    pub message: Option<String>,
}

pub struct PipelineOutcome {
    pub status: String,
    pub work_id: Option<i64>,
    pub archived_video_path: Option<PathBuf>,
    pub holding_id: Option<i64>,
    pub exception_id: Option<i64>,
    pub steps: Vec<PipelineStepRecord>,
}
```

```rust
// src-tauri/src/pipeline.rs
pub trait ScraperSource: Send + Sync {
    fn name(&self) -> &str;
    fn lookup(&self, normalized_code: &str) -> anyhow::Result<Option<ScrapedWorkMetadata>>;
}

pub struct ScrapeCoordinator<'a> {
    pub sources: Vec<&'a dyn ScraperSource>,
}

pub struct ScrapeContext {
    pub work_id: Option<i64>,
    pub normalized_code: String,
    pub object_path: PathBuf,
    pub pipeline_run_id: Option<i64>,
}

pub struct AutoPipeline<'a> {
    pub repo: &'a Repository,
    pub archive_root: PathBuf,
    pub asset_roots: Vec<PathBuf>,
    pub scrapers: ScrapeCoordinator<'a>,
}
```

---

## Task 1: Stage 2 storage cleanup and collection removal

**Files:**
- Modify: `src-tauri/src/storage.rs` (`impl Repository` near collection CRUD)
- Modify: `src-tauri/tests/data_model.rs`

**Interfaces:**
- `Repository::remove_work_from_collection(work_id: i64, collection_id: i64) -> Result<()>`
- Cascade regression: deleting a work removes `work_collections` rows.

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/tests/data_model.rs`:

```rust
#[test]
fn work_can_be_removed_from_collection() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    let mut w = sample_work();
    w.normalized_code = Some("ABP-006".to_string());
    let work_id = repo.upsert_work(&w).unwrap();
    let col_id = repo.create_collection("watch later", None).unwrap();
    repo.add_work_to_collection(work_id, col_id).unwrap();

    repo.remove_work_from_collection(work_id, col_id).unwrap();

    assert!(repo.list_works_in_collection(col_id).unwrap().is_empty());
}

#[test]
fn deleting_work_cascades_collection_links() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    let mut w = sample_work();
    w.normalized_code = Some("ABP-007".to_string());
    let work_id = repo.upsert_work(&w).unwrap();
    let col_id = repo.create_collection("favorites", Some("#00a4dc")).unwrap();
    repo.add_work_to_collection(work_id, col_id).unwrap();

    repo.debug_delete_work(work_id).unwrap();

    assert!(repo.list_works_in_collection(col_id).unwrap().is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test data_model
```

Expected: compile error for `remove_work_from_collection` and `debug_delete_work`.

- [ ] **Step 3: Implement minimal storage helpers**

Add to `impl Repository` near the existing collection methods:

```rust
    /// Remove one work/collection link without deleting either side.
    pub fn remove_work_from_collection(&self, work_id: i64, collection_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM work_collections WHERE work_id = ?1 AND collection_id = ?2",
            params![work_id, collection_id],
        )?;
        Ok(())
    }

    /// Test helper for verifying foreign-key behavior without exposing raw SQL
    /// to tests. Production code should prefer higher-level delete commands.
    pub fn debug_delete_work(&self, work_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM works WHERE id = ?1", params![work_id])?;
        Ok(())
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test data_model
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/storage.rs src-tauri/tests/data_model.rs
git commit -m "补充收藏夹移除与级联测试"
```

---

## Task 2: Pipeline DTOs and completed-file readiness checks

**Files:**
- Modify: `src-tauri/src/domain.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/pipeline.rs`
- Create: `src-tauri/tests/auto_pipeline.rs`

**Interfaces:**
- `CompletedFile::from_path(path: &Path) -> Result<CompletedFile>`
- `Aria2TaskSnapshot { status, completed_length, total_length }`
- `is_aria2_complete(snapshot: &Aria2TaskSnapshot) -> bool`
- `CompletionSnapshot::capture(path: &Path) -> Result<CompletionSnapshot>`
- `is_heuristically_complete(first: &CompletionSnapshot, second: &CompletionSnapshot) -> bool`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/tests/auto_pipeline.rs`:

```rust
use media_manager::domain::CompletedFile;
use media_manager::pipeline::{
    is_aria2_complete, is_heuristically_complete, Aria2TaskSnapshot, CompletionSnapshot,
};

#[test]
fn aria2_snapshot_is_complete_only_when_lengths_match() {
    assert!(is_aria2_complete(&Aria2TaskSnapshot {
        status: "complete".to_string(),
        completed_length: 100,
        total_length: 100,
    }));
    assert!(!is_aria2_complete(&Aria2TaskSnapshot {
        status: "active".to_string(),
        completed_length: 100,
        total_length: 100,
    }));
    assert!(!is_aria2_complete(&Aria2TaskSnapshot {
        status: "complete".to_string(),
        completed_length: 99,
        total_length: 100,
    }));
}

#[test]
fn completed_file_uses_temp_file_metadata_and_fingerprint() {
    let tmp = tempfile::tempdir().unwrap();
    let video = tmp.path().join("ABP-100.mp4");
    std::fs::write(&video, b"fake video bytes").unwrap();

    let completed = CompletedFile::from_path(&video).unwrap();

    assert_eq!(completed.path, video);
    assert_eq!(completed.file_name, "ABP-100.mp4");
    assert_eq!(completed.size_bytes, 16);
    assert!(completed.file_hash.unwrap().starts_with("fp:"));
}

#[test]
fn heuristic_completion_rejects_aria2_control_files_and_accepts_stable_video() {
    let tmp = tempfile::tempdir().unwrap();
    let video = tmp.path().join("ABP-100.mp4");
    std::fs::write(&video, b"video").unwrap();

    let first = CompletionSnapshot::capture(&video).unwrap();
    let second = CompletionSnapshot::capture(&video).unwrap();

    assert!(is_heuristically_complete(&first, &second));

    std::fs::write(tmp.path().join("ABP-100.mp4.aria2"), b"partial").unwrap();
    let blocked = CompletionSnapshot::capture(&video).unwrap();
    assert!(!is_heuristically_complete(&blocked, &blocked));
}

#[test]
fn heuristic_completion_rejects_files_that_change_between_samples() {
    let tmp = tempfile::tempdir().unwrap();
    let video = tmp.path().join("ABP-101.mp4");
    std::fs::write(&video, b"video").unwrap();
    let first = CompletionSnapshot::capture(&video).unwrap();
    std::fs::write(&video, b"video still growing").unwrap();
    let second = CompletionSnapshot::capture(&video).unwrap();

    assert!(!is_heuristically_complete(&first, &second));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline
```

Expected: compile error because `pipeline` module, `CompletedFile`, snapshots, and readiness functions do not exist.

- [ ] **Step 3: Add DTOs and readiness implementation**

In `src-tauri/src/lib.rs`, add:

```rust
pub mod pipeline;
```

In `src-tauri/src/domain.rs`, append:

```rust
/// A single file that the pipeline is allowed to process. It is created only
/// after completion checks pass, so downstream steps can treat the file as
/// immutable for this run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size_bytes: u64,
    pub file_hash: Option<String>,
}

impl CompletedFile {
    /// Build a completed-file snapshot from filesystem metadata and the
    /// bounded sample fingerprint used by the existing scanner.
    pub fn from_path(path: &std::path::Path) -> anyhow::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        Ok(Self {
            path: path.to_path_buf(),
            file_name,
            size_bytes: metadata.len(),
            file_hash: crate::scanner::sample_file_fingerprint(path).ok(),
        })
    }
}
```

Create `src-tauri/src/pipeline.rs`:

```rust
use crate::scanner::is_video_file;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::SystemTime;

/// Minimal aria2 task state needed by the completion detector. Stage 3 will
/// populate this from JSON-RPC; Stage 2 keeps it pure and testable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aria2TaskSnapshot {
    pub status: String,
    pub completed_length: u64,
    pub total_length: u64,
}

/// True when aria2 reports a complete task and byte counts agree.
pub fn is_aria2_complete(snapshot: &Aria2TaskSnapshot) -> bool {
    snapshot.status == "complete"
        && snapshot.total_length > 0
        && snapshot.completed_length == snapshot.total_length
}

/// One filesystem sample used by the non-aria2 completion heuristic. Stage 3
/// controls the delay between samples; Stage 2 owns the deterministic compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionSnapshot {
    pub path: std::path::PathBuf,
    pub exists: bool,
    pub is_video: bool,
    pub has_aria2_control: bool,
    pub size_bytes: u64,
    pub modified_at: Option<SystemTime>,
    pub can_open: bool,
}

impl CompletionSnapshot {
    /// Capture the file state needed to decide whether a non-aria2 file is
    /// stable enough to process.
    pub fn capture(path: &Path) -> anyhow::Result<Self> {
        let metadata = std::fs::metadata(path).ok();
        let control = path.with_extension(format!(
            "{}.aria2",
            path.extension().and_then(|ext| ext.to_str()).unwrap_or_default()
        ));
        Ok(Self {
            path: path.to_path_buf(),
            exists: metadata.is_some(),
            is_video: is_video_file(path),
            has_aria2_control: control.exists(),
            size_bytes: metadata.as_ref().map(|m| m.len()).unwrap_or(0),
            modified_at: metadata.and_then(|m| m.modified().ok()),
            can_open: std::fs::OpenOptions::new().read(true).open(path).is_ok(),
        })
    }
}

/// Fallback completion check for non-aria2 files. A file is complete only when
/// both samples agree on size and mtime, no sibling aria2 control file exists,
/// and the file can be opened for reading.
pub fn is_heuristically_complete(first: &CompletionSnapshot, second: &CompletionSnapshot) -> bool {
    first.path == second.path
        && first.exists
        && second.exists
        && first.is_video
        && second.is_video
        && !first.has_aria2_control
        && !second.has_aria2_control
        && first.size_bytes == second.size_bytes
        && first.modified_at == second.modified_at
        && first.can_open
        && second.can_open
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain.rs src-tauri/src/lib.rs src-tauri/src/pipeline.rs src-tauri/tests/auto_pipeline.rs
git commit -m "新增自动管线完成判定基础"
```

---

## Task 3: Identification routes missing-code files to holding

**Files:**
- Modify: `src-tauri/src/domain.rs`
- Modify: `src-tauri/src/pipeline.rs`
- Modify: `src-tauri/tests/auto_pipeline.rs`

**Interfaces:**
- `identify_completed_file(file: &CompletedFile) -> PipelineIdentification`
- Missing/unsupported files become `HoldingEntry` with `HoldingReason::NoCode` or `HoldingReason::Unrecognizable`.

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/tests/auto_pipeline.rs`:

```rust
use media_manager::domain::HoldingReason;
use media_manager::pipeline::{identify_completed_file, PipelineIdentification};

#[test]
fn identifies_standard_code_from_completed_file_name() {
    let tmp = tempfile::tempdir().unwrap();
    let video = tmp.path().join("[site] abp_100 1080p.mp4");
    std::fs::write(&video, b"video").unwrap();
    let completed = CompletedFile::from_path(&video).unwrap();

    let identified = identify_completed_file(&completed);

    assert_eq!(
        identified,
        PipelineIdentification::Identified {
            normalized_code: "ABP-100".to_string(),
        }
    );
}

#[test]
fn missing_code_is_sent_to_holding_not_exception_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let video = tmp.path().join("random trailer.mp4");
    std::fs::write(&video, b"video").unwrap();
    let completed = CompletedFile::from_path(&video).unwrap();

    let identified = identify_completed_file(&completed);

    assert_eq!(
        identified,
        PipelineIdentification::Holding {
            reason: HoldingReason::NoCode,
        }
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline
```

Expected: compile error for `PipelineIdentification` and `identify_completed_file`.

- [ ] **Step 3: Implement identification**

In `src-tauri/src/pipeline.rs`, add:

```rust
use crate::domain::{CompletedFile, HoldingReason};
use crate::identifier::extract_code_from_text;

/// Classification outcome after the pipeline inspects a completed file name.
/// Only `Identified` continues to scraping; holding outcomes are quiet triage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineIdentification {
    Identified { normalized_code: String },
    Holding { reason: HoldingReason },
}

/// Extract the canonical studio code from a completed video. Files without a
/// parseable standard code are held for later manual triage, not escalated as
/// exceptions.
pub fn identify_completed_file(file: &CompletedFile) -> PipelineIdentification {
    extract_code_from_text(&file.file_name)
        .map(|normalized_code| PipelineIdentification::Identified { normalized_code })
        .unwrap_or(PipelineIdentification::Holding {
            reason: HoldingReason::NoCode,
        })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/pipeline.rs src-tauri/tests/auto_pipeline.rs
git commit -m "实现自动管线番号识别与搁置路由"
```

---

## Task 4: Nullable scrape-job context and multi-source scraping coordinator

**Files:**
- Modify: `src-tauri/src/domain.rs`
- Modify: `src-tauri/src/storage.rs`
- Modify: `src-tauri/src/pipeline.rs`
- Modify: `src-tauri/tests/data_model.rs`
- Modify: `src-tauri/tests/auto_pipeline.rs`

**Interfaces:**
- `ScraperSource` trait
- `ScrapeContext { work_id: Option<i64>, normalized_code, object_path, pipeline_run_id }`
- `ScrapeCoordinator::scrape(repo, context) -> Result<ScrapedWorkMetadata, PipelineScrapeError>`
- Success records a `ScrapeJob::Success`; each failed source records `ScrapeJob::Failed`.
- `scrape_jobs.work_id` is nullable and `normalized_code` / `object_path` are persisted so failed scrapes do not need pre-scrape works.

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/tests/auto_pipeline.rs`:

```rust
use media_manager::domain::{ScrapeStatus, ScrapedWorkMetadata};
use media_manager::pipeline::{PipelineScrapeError, ScrapeContext, ScrapeCoordinator, ScraperSource};
use media_manager::storage::Repository;

struct FakeScraper {
    name: &'static str,
    result: anyhow::Result<Option<ScrapedWorkMetadata>>,
}

impl ScraperSource for FakeScraper {
    fn name(&self) -> &str {
        self.name
    }

    fn lookup(&self, _normalized_code: &str) -> anyhow::Result<Option<ScrapedWorkMetadata>> {
        match &self.result {
            Ok(value) => Ok(value.clone()),
            Err(error) => Err(anyhow::anyhow!(error.to_string())),
        }
    }
}

fn scraped(code: &str, source: &str) -> ScrapedWorkMetadata {
    ScrapedWorkMetadata {
        source: source.to_string(),
        normalized_code: code.to_string(),
        title: format!("{code} title"),
        original_title: Some(format!("{code} original")),
        summary: Some("summary".to_string()),
        actors: vec!["Actor A".to_string()],
        genres: vec!["Genre A".to_string()],
        studio: Some("Studio A".to_string()),
        director: Some("Director A".to_string()),
        release_date: Some("2026-06-25".to_string()),
        cover_path: None,
    }
}

#[test]
fn scraper_coordinator_falls_back_and_records_attempts() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    let first = FakeScraper {
        name: "FANZA",
        result: Ok(None),
    };
    let second = FakeScraper {
        name: "JavBus",
        result: Ok(Some(scraped("ABP-200", "JavBus"))),
    };
    let coordinator = ScrapeCoordinator {
        sources: vec![&first, &second],
    };

    let context = ScrapeContext {
        work_id: None,
        normalized_code: "ABP-200".to_string(),
        object_path: tmp.path().join("ABP-200.mp4"),
        pipeline_run_id: None,
    };
    let metadata = coordinator.scrape(&repo, &context).unwrap();

    assert_eq!(metadata.source, "JavBus");
    let jobs = repo.list_scrape_jobs().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].source, "JavBus");
    assert_eq!(jobs[0].status, ScrapeStatus::Success);
    assert_eq!(jobs[0].work_id, None);
    assert_eq!(jobs[0].normalized_code.as_deref(), Some("ABP-200"));
    assert_eq!(jobs[1].source, "FANZA");
    assert_eq!(jobs[1].status, ScrapeStatus::Failed);
}

#[test]
fn scraper_coordinator_returns_all_failed_error() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    let first = FakeScraper {
        name: "FANZA",
        result: Ok(None),
    };
    let coordinator = ScrapeCoordinator { sources: vec![&first] };

    let context = ScrapeContext {
        work_id: None,
        normalized_code: "ABP-201".to_string(),
        object_path: tmp.path().join("ABP-201.mp4"),
        pipeline_run_id: None,
    };
    let err = coordinator.scrape(&repo, &context).unwrap_err();

    assert_eq!(err, PipelineScrapeError::AllSourcesFailed);
    assert!(repo.get_work_by_code("ABP-201").unwrap().is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline scraper_coordinator
```

Expected: compile error for nullable scrape-job fields, `ScrapedWorkMetadata`, `ScrapeContext`, `ScraperSource`, `ScrapeCoordinator`, and `PipelineScrapeError`.

- [ ] **Step 3: Extend `ScrapeJob` and migrate `scrape_jobs`**

In `src-tauri/src/domain.rs`, change `ScrapeJob` to:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrapeJob {
    pub id: Option<i64>,
    pub work_id: Option<i64>,
    pub normalized_code: Option<String>,
    pub object_path: Option<String>,
    pub pipeline_run_id: Option<i64>,
    pub source: String,
    pub status: ScrapeStatus,
    pub attempts: i64,
    pub last_attempted_at: Option<String>,
    pub error: Option<String>,
}
```

In `src-tauri/src/storage.rs`, update `migrate()` so existing Stage 1 databases are upgraded without losing rows:

```rust
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS scrape_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                work_id INTEGER REFERENCES works(id) ON DELETE CASCADE,
                normalized_code TEXT,
                object_path TEXT,
                pipeline_run_id INTEGER REFERENCES pipeline_runs(id) ON DELETE SET NULL,
                source TEXT NOT NULL,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                last_attempted_at TEXT,
                error TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            ",
        )?;
        self.relax_scrape_jobs_work_id()?;
        self.ensure_column("scrape_jobs", "normalized_code", "TEXT")?;
        self.ensure_column("scrape_jobs", "object_path", "TEXT")?;
        self.ensure_column("scrape_jobs", "pipeline_run_id", "INTEGER")?;
```

Add this helper to `impl Repository`:

```rust
    /// Rebuild the Stage 1 scrape_jobs table when it still has
    /// `work_id INTEGER NOT NULL`. SQLite cannot relax NOT NULL in place.
    fn relax_scrape_jobs_work_id(&self) -> Result<()> {
        let mut statement = self.conn.prepare("PRAGMA table_info(scrape_jobs)")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(3)?))
        })?;
        let mut work_id_not_null = false;
        for row in rows {
            let (name, not_null) = row?;
            if name == "work_id" && not_null == 1 {
                work_id_not_null = true;
            }
        }
        if !work_id_not_null {
            return Ok(());
        }

        self.conn.execute_batch(
            "
            ALTER TABLE scrape_jobs RENAME TO scrape_jobs_old;
            CREATE TABLE scrape_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                work_id INTEGER REFERENCES works(id) ON DELETE CASCADE,
                normalized_code TEXT,
                object_path TEXT,
                pipeline_run_id INTEGER REFERENCES pipeline_runs(id) ON DELETE SET NULL,
                source TEXT NOT NULL,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                last_attempted_at TEXT,
                error TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            INSERT INTO scrape_jobs (
                id, work_id, source, status, attempts, last_attempted_at, error, created_at
            )
            SELECT id, work_id, source, status, attempts, last_attempted_at, error, created_at
            FROM scrape_jobs_old;
            DROP TABLE scrape_jobs_old;
            ",
        )?;
        Ok(())
    }
```

Update `record_scrape_job` and `list_scrape_jobs` to read/write the new fields. Also update the existing `scrape_jobs_roundtrip` test in `src-tauri/tests/data_model.rs` so the literal includes:

```rust
            work_id: Some(work_id),
            normalized_code: Some("ABP-003".to_string()),
            object_path: Some("H:/dl/ABP-003.mp4".to_string()),
            pipeline_run_id: None,
```

- [ ] **Step 4: Implement scraper DTO and coordinator**

Append `ScrapedWorkMetadata` to `src-tauri/src/domain.rs`:

```rust
/// Normalized metadata returned by one scraper source. It is intentionally
/// local-path based for artwork in Stage 2; network downloads stay behind the
/// scraper adapter boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrapedWorkMetadata {
    pub source: String,
    pub normalized_code: String,
    pub title: String,
    pub original_title: Option<String>,
    pub summary: Option<String>,
    pub actors: Vec<String>,
    pub genres: Vec<String>,
    pub studio: Option<String>,
    pub director: Option<String>,
    pub release_date: Option<String>,
    pub cover_path: Option<PathBuf>,
}
```

Append to `src-tauri/src/pipeline.rs`:

```rust
use crate::domain::{ScrapeJob, ScrapeStatus, ScrapedWorkMetadata};
use crate::storage::Repository;
use std::path::PathBuf;

/// Scraper boundary used by the automatic pipeline. Real FANZA/JavBus/JavDB
/// adapters can be added behind this trait without changing orchestration.
pub trait ScraperSource: Send + Sync {
    fn name(&self) -> &str;
    fn lookup(&self, normalized_code: &str) -> anyhow::Result<Option<ScrapedWorkMetadata>>;
}

/// User-configured ordered scraper list. The first source returning metadata
/// wins; failures are recorded for retry and diagnostics.
pub struct ScrapeCoordinator<'a> {
    pub sources: Vec<&'a dyn ScraperSource>,
}

/// Stable context for recording scrape attempts before a work exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrapeContext {
    pub work_id: Option<i64>,
    pub normalized_code: String,
    pub object_path: PathBuf,
    pub pipeline_run_id: Option<i64>,
}

/// Scrape failure categories that matter to the exception router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineScrapeError {
    AllSourcesFailed,
}

impl<'a> ScrapeCoordinator<'a> {
    /// Try each source in order, recording one scrape job per attempt.
    pub fn scrape(
        &self,
        repo: &Repository,
        context: &ScrapeContext,
    ) -> Result<ScrapedWorkMetadata, PipelineScrapeError> {
        for source in &self.sources {
            match source.lookup(&context.normalized_code) {
                Ok(Some(metadata)) => {
                    let _ = repo.record_scrape_job(&ScrapeJob {
                        id: None,
                        work_id: context.work_id,
                        normalized_code: Some(context.normalized_code.clone()),
                        object_path: Some(context.object_path.to_string_lossy().to_string()),
                        pipeline_run_id: context.pipeline_run_id,
                        source: source.name().to_string(),
                        status: ScrapeStatus::Success,
                        attempts: 1,
                        last_attempted_at: Some(chrono::Utc::now().to_rfc3339()),
                        error: None,
                    });
                    return Ok(metadata);
                }
                Ok(None) => {
                    let _ = repo.record_scrape_job(&ScrapeJob {
                        id: None,
                        work_id: context.work_id,
                        normalized_code: Some(context.normalized_code.clone()),
                        object_path: Some(context.object_path.to_string_lossy().to_string()),
                        pipeline_run_id: context.pipeline_run_id,
                        source: source.name().to_string(),
                        status: ScrapeStatus::Failed,
                        attempts: 1,
                        last_attempted_at: Some(chrono::Utc::now().to_rfc3339()),
                        error: Some("not found".to_string()),
                    });
                }
                Err(error) => {
                    let _ = repo.record_scrape_job(&ScrapeJob {
                        id: None,
                        work_id: context.work_id,
                        normalized_code: Some(context.normalized_code.clone()),
                        object_path: Some(context.object_path.to_string_lossy().to_string()),
                        pipeline_run_id: context.pipeline_run_id,
                        source: source.name().to_string(),
                        status: ScrapeStatus::Failed,
                        attempts: 1,
                        last_attempted_at: Some(chrono::Utc::now().to_rfc3339()),
                        error: Some(error.to_string()),
                    });
                }
            }
        }
        Err(PipelineScrapeError::AllSourcesFailed)
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline scraper_coordinator
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/domain.rs src-tauri/src/pipeline.rs src-tauri/tests/auto_pipeline.rs
git commit -m "实现多源刮削协调与记录"
```

---

## Task 5: NFO writer for scraped metadata

**Files:**
- Modify: `src-tauri/src/nfo.rs`
- Modify: `src-tauri/tests/auto_pipeline.rs`

**Interfaces:**
- `render_scraped_nfo(metadata: &ScrapedWorkMetadata) -> String`
- Output contains `<movie>`, `<title>`, `<originaltitle>`, `<num>`, `<plot>`, actors, genres, studio, director, and release date when present.

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/tests/auto_pipeline.rs`:

```rust
use media_manager::nfo::render_scraped_nfo;

#[test]
fn nfo_writer_renders_scraped_metadata_as_kodi_movie_xml() {
    let xml = render_scraped_nfo(&scraped("ABP-300", "FANZA"));

    assert!(xml.contains("<movie>"));
    assert!(xml.contains("<num>ABP-300</num>"));
    assert!(xml.contains("<title>ABP-300 title</title>"));
    assert!(xml.contains("<originaltitle>ABP-300 original</originaltitle>"));
    assert!(xml.contains("<actor><name>Actor A</name></actor>"));
    assert!(xml.contains("<genre>Genre A</genre>"));
    assert!(xml.contains("<studio>Studio A</studio>"));
    assert!(xml.contains("<director>Director A</director>"));
    assert!(xml.contains("<premiered>2026-06-25</premiered>"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline nfo_writer
```

Expected: compile error for `render_scraped_nfo`.

- [ ] **Step 3: Implement NFO rendering**

Append to `src-tauri/src/nfo.rs`:

```rust
use crate::domain::ScrapedWorkMetadata;

/// Render scraped metadata to a simple Kodi-compatible movie NFO. This writer
/// owns only Stage 2 generated NFOs; the existing parser remains tolerant of
/// richer third-party NFO files.
pub fn render_scraped_nfo(metadata: &ScrapedWorkMetadata) -> String {
    let mut xml = String::from("<movie>\n");
    push_tag(&mut xml, "num", &metadata.normalized_code);
    push_tag(&mut xml, "title", &metadata.title);
    if let Some(value) = &metadata.original_title {
        push_tag(&mut xml, "originaltitle", value);
    }
    if let Some(value) = &metadata.summary {
        push_tag(&mut xml, "plot", value);
    }
    if let Some(value) = &metadata.studio {
        push_tag(&mut xml, "studio", value);
    }
    if let Some(value) = &metadata.director {
        push_tag(&mut xml, "director", value);
    }
    if let Some(value) = &metadata.release_date {
        push_tag(&mut xml, "premiered", value);
    }
    for actor in &metadata.actors {
        xml.push_str("  <actor>");
        push_inline_tag(&mut xml, "name", actor);
        xml.push_str("</actor>\n");
    }
    for genre in &metadata.genres {
        push_tag(&mut xml, "genre", genre);
    }
    xml.push_str("</movie>\n");
    xml
}

fn push_tag(xml: &mut String, name: &str, value: &str) {
    xml.push_str("  <");
    xml.push_str(name);
    xml.push('>');
    xml.push_str(&escape_xml(value));
    xml.push_str("</");
    xml.push_str(name);
    xml.push_str(">\n");
}

fn push_inline_tag(xml: &mut String, name: &str, value: &str) {
    xml.push('<');
    xml.push_str(name);
    xml.push('>');
    xml.push_str(&escape_xml(value));
    xml.push_str("</");
    xml.push_str(name);
    xml.push('>');
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline nfo_writer
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/nfo.rs src-tauri/tests/auto_pipeline.rs
git commit -m "新增刮削元数据 NFO 写出"
```

---

## Task 6: Archive layout planner copies assets and never overwrites

**Files:**
- Modify: `src-tauri/src/domain.rs`
- Modify: `src-tauri/src/pipeline.rs`
- Modify: `src-tauri/tests/auto_pipeline.rs`

**Interfaces:**
- `plan_archive_layout(archive_root, file, metadata, asset_roots) -> ArchiveLayoutPlan`
- Layout:
  - `<archive_root>/<code>/<code>.nfo`
  - `<archive_root>/<code>/<code>.mp4`
  - `<archive_root>/<code>/<code>-v2.mp4` when base exists
  - `poster.jpg` from metadata cover or `<code>.jpg` in asset roots
  - `screenshot/<original image name>` for `-shot` / `-screenshot`
  - `<code>.gif` when present

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/tests/auto_pipeline.rs`:

```rust
use media_manager::pipeline::plan_archive_layout;

#[test]
fn archive_layout_uses_flat_code_directory_and_version_suffixes() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("archive");
    let source = tmp.path().join("ABP-400.mp4");
    std::fs::create_dir_all(archive.join("ABP-400")).unwrap();
    std::fs::write(archive.join("ABP-400").join("ABP-400.mp4"), b"existing").unwrap();
    std::fs::write(&source, b"video").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();

    let plan = plan_archive_layout(&archive, &file, &scraped("ABP-400", "FANZA"), &[]).unwrap();

    assert_eq!(plan.work_dir, archive.join("ABP-400"));
    assert_eq!(plan.video_target, archive.join("ABP-400").join("ABP-400-v2.mp4"));
    assert_eq!(plan.nfo_target, archive.join("ABP-400").join("ABP-400.nfo"));
}

#[test]
fn archive_layout_discovers_code_named_assets_from_asset_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("archive");
    let assets = tmp.path().join("assets");
    let source = tmp.path().join("ABP-401.mp4");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(&source, b"video").unwrap();
    std::fs::write(assets.join("ABP-401.jpg"), b"poster").unwrap();
    std::fs::write(assets.join("ABP-401-shot.jpg"), b"shot").unwrap();
    std::fs::write(assets.join("ABP-401.gif"), b"gif").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();

    let plan = plan_archive_layout(
        &archive,
        &file,
        &scraped("ABP-401", "FANZA"),
        &[assets.clone()],
    )
    .unwrap();

    assert!(plan.assets.iter().any(|asset| asset.relative_target == std::path::PathBuf::from("poster.jpg")));
    assert!(plan.assets.iter().any(|asset| asset.relative_target == std::path::PathBuf::from("screenshot/ABP-401-shot.jpg")));
    assert!(plan.assets.iter().any(|asset| asset.relative_target == std::path::PathBuf::from("ABP-401.gif")));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline archive_layout
```

Expected: compile error for `plan_archive_layout` and archive layout structs.

- [ ] **Step 3: Implement archive layout plan**

Append to `src-tauri/src/domain.rs`:

```rust
/// One non-video file copied into a self-contained archive directory. The
/// target is relative to the work directory so the executor can validate that
/// every write stays below `<archive_root>/<code>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveAsset {
    pub source_path: PathBuf,
    pub relative_target: PathBuf,
}
```

Append to `src-tauri/src/pipeline.rs`:

```rust
use crate::archive::normalized_file_name;
use crate::domain::ArchiveAsset;
use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Complete filesystem plan for one pipeline archive operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveLayoutPlan {
    pub work_dir: PathBuf,
    pub video_target: PathBuf,
    pub nfo_target: PathBuf,
    pub assets: Vec<ArchiveAsset>,
}

/// Build the target layout for one completed file and its scraped metadata.
/// The plan performs no writes and picks the next free version suffix.
pub fn plan_archive_layout(
    archive_root: &Path,
    file: &CompletedFile,
    metadata: &ScrapedWorkMetadata,
    asset_roots: &[PathBuf],
) -> Result<ArchiveLayoutPlan> {
    let code = &metadata.normalized_code;
    let work_dir = archive_root.join(code);
    let mut version = 1usize;
    let file_name = loop {
        let candidate = normalized_file_name(code, &file.path, version);
        if !work_dir.join(&candidate).exists() {
            break candidate;
        }
        version += 1;
    };
    let assets = discover_assets(code, metadata.cover_path.as_ref(), asset_roots);
    Ok(ArchiveLayoutPlan {
        video_target: work_dir.join(file_name),
        nfo_target: work_dir.join(format!("{code}.nfo")),
        work_dir,
        assets,
    })
}

fn discover_assets(
    code: &str,
    cover_path: Option<&PathBuf>,
    asset_roots: &[PathBuf],
) -> Vec<ArchiveAsset> {
    let mut assets = Vec::new();
    if let Some(path) = cover_path {
        assets.push(ArchiveAsset {
            source_path: path.clone(),
            relative_target: PathBuf::from("poster.jpg"),
        });
    }
    for root in asset_roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            let lower = name.to_ascii_lowercase();
            let code_lower = code.to_ascii_lowercase();
            if !lower.starts_with(&code_lower) {
                continue;
            }
            let relative_target = if lower.ends_with(".gif") {
                PathBuf::from(format!("{code}.gif"))
            } else if lower.contains("-shot") || lower.contains("-screenshot") {
                PathBuf::from("screenshot").join(name)
            } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".png") || lower.ends_with(".webp") {
                PathBuf::from("poster.jpg")
            } else {
                continue;
            };
            if !assets.iter().any(|asset| asset.relative_target == relative_target) {
                assets.push(ArchiveAsset {
                    source_path: path.to_path_buf(),
                    relative_target,
                });
            }
        }
    }
    assets
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline archive_layout
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain.rs src-tauri/src/pipeline.rs src-tauri/tests/auto_pipeline.rs
git commit -m "实现自动归档布局规划"
```

---

## Task 7: Archive executor stages video, writes NFO, copies assets, and rolls back on failure

**Files:**
- Modify: `src-tauri/src/pipeline.rs`
- Modify: `src-tauri/tests/auto_pipeline.rs`

**Interfaces:**
- `execute_archive_layout(plan, file, metadata) -> Result<ExecutedArchive>`
- On successful execution, source video no longer exists, target video exists, NFO exists, assets copied.
- Video transfer uses `<target>.moving` + size verification + final rename, so cross-volume moves do not depend on `fs::rename`.
- If a later copy/write fails, the final target is moved back to the original source path when possible; otherwise the source copy is left untouched until cleanup.

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/tests/auto_pipeline.rs`:

```rust
use media_manager::pipeline::execute_archive_layout;

#[test]
fn archive_executor_moves_video_writes_nfo_and_copies_assets() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("archive");
    let assets = tmp.path().join("assets");
    let source = tmp.path().join("ABP-500.mp4");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(&source, b"video").unwrap();
    std::fs::write(assets.join("ABP-500.jpg"), b"poster").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();
    let metadata = scraped("ABP-500", "FANZA");
    let plan = plan_archive_layout(&archive, &file, &metadata, &[assets]).unwrap();

    let executed = execute_archive_layout(&plan, &file, &metadata).unwrap();

    assert!(!source.exists());
    assert!(executed.video_path.exists());
    assert!(executed.nfo_path.exists());
    assert!(archive.join("ABP-500").join("poster.jpg").exists());
    let nfo = std::fs::read_to_string(executed.nfo_path).unwrap();
    assert!(nfo.contains("<num>ABP-500</num>"));
}

#[test]
fn archive_executor_rolls_video_back_when_asset_copy_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("archive");
    let source = tmp.path().join("ABP-501.mp4");
    let missing_asset = tmp.path().join("missing").join("ABP-501.jpg");
    std::fs::write(&source, b"video").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();
    let metadata = scraped("ABP-501", "FANZA");
    let mut plan = plan_archive_layout(&archive, &file, &metadata, &[]).unwrap();
    plan.assets.push(media_manager::domain::ArchiveAsset {
        source_path: missing_asset,
        relative_target: std::path::PathBuf::from("poster.jpg"),
    });

    let result = execute_archive_layout(&plan, &file, &metadata);

    assert!(result.is_err());
    assert!(source.exists(), "video should be restored to original path");
    assert!(!plan.video_target.exists(), "failed archive target should not retain moved video");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline archive_executor
```

Expected: compile error for `execute_archive_layout` and `ExecutedArchive`.

- [ ] **Step 3: Implement executor**

Append to `src-tauri/src/pipeline.rs`:

```rust
use crate::nfo::render_scraped_nfo;
use std::fs;

/// Result of a successful archive execution. Paths are final archive paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutedArchive {
    pub video_path: PathBuf,
    pub nfo_path: PathBuf,
}

/// Execute one archive layout. Video transfer is staged through a temporary
/// target and verified before the source is removed; NFO/assets are writes or
/// copies. If a later step fails, the video is restored when possible.
pub fn execute_archive_layout(
    plan: &ArchiveLayoutPlan,
    file: &CompletedFile,
    metadata: &ScrapedWorkMetadata,
) -> Result<ExecutedArchive> {
    fs::create_dir_all(&plan.work_dir)?;
    stage_video_move(file, &plan.video_target)?;
    let execution = (|| -> Result<()> {
        fs::write(&plan.nfo_target, render_scraped_nfo(metadata))?;
        for asset in &plan.assets {
            let target = plan.work_dir.join(&asset.relative_target);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            if !target.exists() {
                fs::copy(&asset.source_path, &target)?;
            }
        }
        Ok(())
    })();

    if let Err(error) = execution {
        let _ = restore_video(&plan.video_target, &file.path);
        return Err(error);
    }

    Ok(ExecutedArchive {
        video_path: plan.video_target.clone(),
        nfo_path: plan.nfo_target.clone(),
    })
}

fn stage_video_move(file: &CompletedFile, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let moving = target.with_extension(format!(
        "{}.moving",
        target.extension().and_then(|ext| ext.to_str()).unwrap_or_default()
    ));
    if moving.exists() {
        fs::remove_file(&moving)?;
    }
    fs::copy(&file.path, &moving)?;
    let copied_size = fs::metadata(&moving)?.len();
    if copied_size != file.size_bytes {
        let _ = fs::remove_file(&moving);
        return Err(anyhow::anyhow!("staged video size mismatch"));
    }
    fs::rename(&moving, target)?;
    fs::remove_file(&file.path)?;
    Ok(())
}

fn restore_video(target: &Path, original: &Path) -> Result<()> {
    if original.exists() {
        if target.exists() {
            let _ = fs::remove_file(target);
        }
        return Ok(());
    }
    if target.exists() {
        fs::rename(target, original)?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline archive_executor
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/pipeline.rs src-tauri/tests/auto_pipeline.rs
git commit -m "实现自动归档执行与回滚"
```

---

## Task 8: Direct pipeline persistence into works and file_versions

**Files:**
- Modify: `src-tauri/src/storage.rs`
- Modify: `src-tauri/src/pipeline.rs`
- Modify: `src-tauri/tests/auto_pipeline.rs`

**Interfaces:**
- `Repository::find_file_versions_by_hash(file_hash: &str) -> Result<Vec<FileVersion>>`
- `Repository::upsert_file_version_for_work(work_id, source_root, original_path, archived_path, original_file_name, normalized_file_name, size_bytes, file_hash) -> Result<i64>`
- `persist_pipeline_success(repo, completed, metadata, executed) -> Result<i64>` returns `work_id`.

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/tests/auto_pipeline.rs`:

```rust
use media_manager::pipeline::{persist_pipeline_success, ExecutedArchive};
use media_manager::domain::{CodeKind, WatchStatus, Work};

fn persisted_work(code: &str) -> Work {
    Work {
        id: None,
        normalized_code: Some(code.to_string()),
        source_code: Some(code.to_string()),
        code_kind: CodeKind::Standard,
        title_zh: None,
        original_title: None,
        aliases: Vec::new(),
        summary: None,
        outline: None,
        cover_path: None,
        poster_path: None,
        thumb_path: None,
        fanart_path: None,
        screenshot_path: None,
        gif_path: None,
        tags: Vec::new(),
        sets: Vec::new(),
        lists: Vec::new(),
        rating: None,
        rating_value: None,
        rating_max: None,
        rating_votes: None,
        criticrating: None,
        watch_status: WatchStatus::Unwatched,
        genres: Vec::new(),
        studio: None,
        label: None,
        director: None,
        release_date: None,
        runtime_minutes: None,
        year: None,
        website: None,
        mpaa: None,
        has_video: true,
        ratings: Vec::new(),
        watch_progress_seconds: None,
        last_played_at: None,
    }
}

#[test]
fn pipeline_success_persists_work_relations_and_file_version() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let source = tmp.path().join("ABP-600.mp4");
    let archived = tmp.path().join("archive").join("ABP-600").join("ABP-600.mp4");
    let nfo = tmp.path().join("archive").join("ABP-600").join("ABP-600.nfo");
    std::fs::create_dir_all(archived.parent().unwrap()).unwrap();
    std::fs::write(&source, b"video").unwrap();
    std::fs::write(&archived, b"video").unwrap();
    std::fs::write(&nfo, b"nfo").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();

    let work_id = persist_pipeline_success(
        &repo,
        &file,
        &scraped("ABP-600", "FANZA"),
        &ExecutedArchive {
            video_path: archived.clone(),
            nfo_path: nfo,
        },
    )
    .unwrap();

    let work = repo.get_work_by_id(work_id).unwrap().unwrap();
    assert_eq!(work.normalized_code.as_deref(), Some("ABP-600"));
    assert_eq!(work.title_zh.as_deref(), Some("ABP-600 title"));
    assert_eq!(work.studio.as_deref(), Some("Studio A"));
    assert_eq!(repo.list_work_actors(work_id).unwrap()[0].primary_name, "Actor A");
    assert_eq!(repo.list_work_tags(work_id).unwrap()[0].name, "Genre A");
    let versions = repo.list_file_versions_for_work(work_id).unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].archived_path.as_ref(), Some(&archived));
    assert_eq!(versions[0].normalized_file_name.as_deref(), Some("ABP-600.mp4"));
}

#[test]
fn repository_can_find_existing_versions_by_file_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let work_id = repo.upsert_work(&persisted_work("ABP-601")).unwrap();

    repo.upsert_file_version_for_work(
        work_id,
        tmp.path(),
        &tmp.path().join("ABP-601.mp4"),
        Some(&tmp.path().join("archive/ABP-601/ABP-601.mp4")),
        "ABP-601.mp4",
        Some("ABP-601.mp4"),
        10,
        Some("fp:same"),
    )
    .unwrap();

    let matches = repo.find_file_versions_by_hash("fp:same").unwrap();

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].work_id, Some(work_id));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline
```

Expected: compile errors for storage helpers and `persist_pipeline_success`.

- [ ] **Step 3: Implement storage helpers**

Add to `impl Repository` near file-version methods:

```rust
    /// Find archived or pending file versions by exact sample fingerprint.
    /// Stage 2 uses this to route duplicate candidates into the exception queue.
    pub fn find_file_versions_by_hash(&self, file_hash: &str) -> Result<Vec<FileVersion>> {
        let mut statement = self.conn.prepare(
            "
            SELECT id, work_id, source_root, original_path, archived_path, original_file_name,
                   normalized_file_name, size_bytes, duration_seconds, width, height, codec, file_hash
            FROM file_versions
            WHERE file_hash = ?1
            ORDER BY id ASC
            ",
        )?;
        let rows = statement.query_map(params![file_hash], file_version_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Insert or update a file version written by the automatic pipeline.
    /// Unlike the old ingest path, this accepts the final archive path directly.
    pub fn upsert_file_version_for_work(
        &self,
        work_id: i64,
        source_root: &Path,
        original_path: &Path,
        archived_path: Option<&Path>,
        original_file_name: &str,
        normalized_file_name: Option<&str>,
        size_bytes: u64,
        file_hash: Option<&str>,
    ) -> Result<i64> {
        let source_root = source_root.to_string_lossy().to_string();
        let original_path = original_path.to_string_lossy().to_string();
        let archived_path = archived_path.map(|p| p.to_string_lossy().to_string());
        self.conn.execute(
            "
            INSERT INTO file_versions (
                work_id, source_root, original_path, archived_path, original_file_name,
                normalized_file_name, size_bytes, file_hash
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(original_path) DO UPDATE SET
                work_id = excluded.work_id,
                archived_path = excluded.archived_path,
                normalized_file_name = excluded.normalized_file_name,
                size_bytes = excluded.size_bytes,
                file_hash = excluded.file_hash
            ",
            params![
                work_id,
                source_root,
                original_path,
                archived_path,
                original_file_name,
                normalized_file_name,
                size_bytes as i64,
                file_hash,
            ],
        )?;
        self.conn.query_row(
            "SELECT id FROM file_versions WHERE original_path = ?1",
            params![original_path],
            |row| row.get(0),
        ).map_err(Into::into)
    }
```

If `file_version_from_row` is currently private and already exists, reuse it. If not, extract the existing row mapping from `list_file_versions_for_work` into a private helper:

```rust
fn file_version_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileVersion> {
    let source_root: String = row.get(2)?;
    let original_path: String = row.get(3)?;
    let archived_path: Option<String> = row.get(4)?;
    Ok(FileVersion {
        id: row.get(0)?,
        work_id: row.get(1)?,
        source_root: PathBuf::from(source_root),
        original_path: PathBuf::from(original_path),
        archived_path: archived_path.map(PathBuf::from),
        original_file_name: row.get(5)?,
        normalized_file_name: row.get(6)?,
        size_bytes: row.get::<_, i64>(7)? as u64,
        duration_seconds: row.get::<_, Option<i64>>(8)?.map(|v| v as u64),
        width: row.get::<_, Option<i64>>(9)?.map(|v| v as u32),
        height: row.get::<_, Option<i64>>(10)?.map(|v| v as u32),
        codec: row.get(11)?,
        file_hash: row.get(12)?,
    })
}
```

- [ ] **Step 4: Implement pipeline persistence**

Append to `src-tauri/src/pipeline.rs`:

```rust
use crate::domain::{CodeKind, WatchStatus, Work};

/// Persist a successfully archived file as a work plus one file version.
/// User-owned fields are left at defaults so future manual edits stay separate
/// from scraper-owned metadata.
pub fn persist_pipeline_success(
    repo: &Repository,
    file: &CompletedFile,
    metadata: &ScrapedWorkMetadata,
    executed: &ExecutedArchive,
) -> anyhow::Result<i64> {
    let work = Work {
        id: None,
        normalized_code: Some(metadata.normalized_code.clone()),
        source_code: Some(metadata.normalized_code.clone()),
        code_kind: CodeKind::Standard,
        title_zh: Some(metadata.title.clone()),
        original_title: metadata.original_title.clone(),
        aliases: Vec::new(),
        summary: metadata.summary.clone(),
        outline: None,
        cover_path: metadata.cover_path.clone(),
        poster_path: Some(executed.video_path.parent().unwrap().join("poster.jpg")),
        thumb_path: None,
        fanart_path: None,
        screenshot_path: None,
        gif_path: Some(executed.video_path.parent().unwrap().join(format!("{}.gif", metadata.normalized_code))),
        tags: metadata.genres.clone(),
        sets: Vec::new(),
        lists: Vec::new(),
        rating: None,
        rating_value: None,
        rating_max: None,
        rating_votes: None,
        criticrating: None,
        watch_status: WatchStatus::Unwatched,
        genres: metadata.genres.clone(),
        studio: metadata.studio.clone(),
        label: None,
        director: metadata.director.clone(),
        release_date: metadata.release_date.clone(),
        runtime_minutes: None,
        year: metadata
            .release_date
            .as_ref()
            .and_then(|date| date.get(0..4))
            .and_then(|year| year.parse::<i32>().ok()),
        website: None,
        mpaa: None,
        has_video: true,
        ratings: Vec::new(),
        watch_progress_seconds: None,
        last_played_at: None,
    };
    let work_id = repo.upsert_work(&work)?;
    repo.set_work_actors(work_id, &metadata.actors, &metadata.source)?;
    repo.set_work_tags(work_id, &metadata.genres)?;
    let normalized_file_name = executed
        .video_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&file.file_name);
    let source_root = file.path.parent().unwrap_or_else(|| std::path::Path::new(""));
    repo.upsert_file_version_for_work(
        work_id,
        source_root,
        &file.path,
        Some(&executed.video_path),
        &file.file_name,
        Some(normalized_file_name),
        file.size_bytes,
        file.file_hash.as_deref(),
    )?;
    Ok(work_id)
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/storage.rs src-tauri/src/pipeline.rs src-tauri/tests/auto_pipeline.rs
git commit -m "实现自动管线入库写入"
```

---

## Task 9: End-to-end `AutoPipeline::process_completed_file`

**Files:**
- Modify: `src-tauri/src/domain.rs`
- Modify: `src-tauri/src/pipeline.rs`
- Modify: `src-tauri/tests/auto_pipeline.rs`

**Interfaces:**
- `PipelineStepRecord`
- `PipelineOutcome`
- `Repository::start_pipeline_run(file_path: &Path) -> Result<i64>`
- `Repository::finish_pipeline_run(run_id, status, steps, error) -> Result<()>`
- `AutoPipeline::process_completed_file(file: CompletedFile) -> Result<PipelineOutcome>`
- Routes:
  - Missing code -> `holding`
  - Existing fingerprint -> `exceptions` with `DuplicateCandidate`
  - All scrapers fail -> `exceptions` with `ScrapeFailed`
  - Success -> archive + work + file_version + `pipeline_runs`
  - Operational move/write failures -> `pipeline_runs.status = "failed"`, not `exceptions`

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/tests/auto_pipeline.rs`:

```rust
use media_manager::domain::{ExceptionKind, PipelineOutcome};
use media_manager::pipeline::AutoPipeline;

#[test]
fn auto_pipeline_success_archives_and_records_run() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let source = tmp.path().join("inbox").join("ABP-700.mp4");
    let assets = tmp.path().join("assets");
    let archive = tmp.path().join("archive");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(&source, b"video").unwrap();
    std::fs::write(assets.join("ABP-700.jpg"), b"poster").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();
    let scraper = FakeScraper {
        name: "FANZA",
        result: Ok(Some(scraped("ABP-700", "FANZA"))),
    };
    let pipeline = AutoPipeline {
        repo: &repo,
        archive_root: archive.clone(),
        asset_roots: vec![assets],
        scrapers: ScrapeCoordinator {
            sources: vec![&scraper],
        },
    };

    let outcome = pipeline.process_completed_file(file).unwrap();

    assert_eq!(outcome.status, "archived");
    assert!(outcome.work_id.is_some());
    assert!(archive.join("ABP-700").join("ABP-700.mp4").exists());
    assert!(archive.join("ABP-700").join("ABP-700.nfo").exists());
    assert_eq!(repo.list_pipeline_runs().unwrap()[0].status, "archived");
}

#[test]
fn auto_pipeline_routes_missing_code_to_holding() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let source = tmp.path().join("random.mp4");
    std::fs::write(&source, b"video").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();
    let scraper = FakeScraper {
        name: "FANZA",
        result: Ok(None),
    };
    let pipeline = AutoPipeline {
        repo: &repo,
        archive_root: tmp.path().join("archive"),
        asset_roots: vec![],
        scrapers: ScrapeCoordinator {
            sources: vec![&scraper],
        },
    };

    let outcome = pipeline.process_completed_file(file).unwrap();

    assert_eq!(outcome.status, "holding");
    assert!(outcome.holding_id.is_some());
    assert_eq!(repo.list_holding().unwrap()[0].reason, HoldingReason::NoCode);
    assert!(repo.list_exceptions().unwrap().is_empty());
}

#[test]
fn auto_pipeline_routes_scrape_failure_to_exception_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let source = tmp.path().join("ABP-701.mp4");
    std::fs::write(&source, b"video").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();
    let scraper = FakeScraper {
        name: "FANZA",
        result: Ok(None),
    };
    let pipeline = AutoPipeline {
        repo: &repo,
        archive_root: tmp.path().join("archive"),
        asset_roots: vec![],
        scrapers: ScrapeCoordinator {
            sources: vec![&scraper],
        },
    };

    let outcome = pipeline.process_completed_file(file).unwrap();

    assert_eq!(outcome.status, "exception");
    let exceptions = repo.list_exceptions().unwrap();
    assert_eq!(exceptions[0].kind, ExceptionKind::ScrapeFailed);
    assert!(exceptions[0].evidence_json.contains("ABP-701"));
}

#[test]
fn auto_pipeline_routes_duplicate_fingerprint_to_exception_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let first = tmp.path().join("ABP-702.mp4");
    let second = tmp.path().join("ABP-703.mp4");
    std::fs::write(&first, b"same bytes").unwrap();
    std::fs::write(&second, b"same bytes").unwrap();
    let existing = CompletedFile::from_path(&first).unwrap();
    let work_id = repo.upsert_work(&persisted_work("ABP-702")).unwrap();
    repo.upsert_file_version_for_work(
        work_id,
        first.parent().unwrap(),
        &first,
        None,
        "ABP-702.mp4",
        Some("ABP-702.mp4"),
        existing.size_bytes,
        existing.file_hash.as_deref(),
    )
    .unwrap();
    let file = CompletedFile::from_path(&second).unwrap();
    let scraper = FakeScraper {
        name: "FANZA",
        result: Ok(Some(scraped("ABP-703", "FANZA"))),
    };
    let pipeline = AutoPipeline {
        repo: &repo,
        archive_root: tmp.path().join("archive"),
        asset_roots: vec![],
        scrapers: ScrapeCoordinator {
            sources: vec![&scraper],
        },
    };

    let outcome = pipeline.process_completed_file(file).unwrap();

    assert_eq!(outcome.status, "exception");
    assert_eq!(repo.list_exceptions().unwrap()[0].kind, ExceptionKind::DuplicateCandidate);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline auto_pipeline_
```

Expected: compile errors for `PipelineOutcome`, `AutoPipeline`, and `process_completed_file`.

- [ ] **Step 3: Add outcome DTOs**

Append to `src-tauri/src/domain.rs`:

```rust
/// One visible step in a pipeline run. Stored as JSON in `pipeline_runs` so the
/// daemon and UI can show progress without schema changes for every new step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineStepRecord {
    pub step: String,
    pub status: String,
    pub message: Option<String>,
}

/// High-level result of processing one completed file. Exactly one of
/// `work_id`, `holding_id`, or `exception_id` is populated for terminal states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineOutcome {
    pub status: String,
    pub work_id: Option<i64>,
    pub archived_video_path: Option<PathBuf>,
    pub holding_id: Option<i64>,
    pub exception_id: Option<i64>,
    pub steps: Vec<PipelineStepRecord>,
}
```

- [ ] **Step 4: Add pipeline-run storage helpers and implement orchestration**

Add to `impl Repository` in `src-tauri/src/storage.rs`:

```rust
    /// Create a running pipeline row before any destructive filesystem step.
    /// The returned id is used to correlate scrape jobs and recovery state.
    pub fn start_pipeline_run(&self, file_path: &Path) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO pipeline_runs (file_path, started_at, steps_json, status)
             VALUES (?1, CURRENT_TIMESTAMP, '[]', 'running')",
            params![file_path.to_string_lossy().to_string()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Finish a pipeline row with the terminal status and serialized step log.
    pub fn finish_pipeline_run(
        &self,
        run_id: i64,
        status: &str,
        steps: &[PipelineStepRecord],
        error: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE pipeline_runs
             SET finished_at = CURRENT_TIMESTAMP, steps_json = ?1, status = ?2, error = ?3
             WHERE id = ?4",
            params![serde_json::to_string(steps)?, status, error, run_id],
        )?;
        Ok(())
    }
```

Append to `src-tauri/src/pipeline.rs`:

```rust
use crate::domain::{
    Exception, ExceptionKind, ExceptionStatus, HoldingEntry, PipelineOutcome, PipelineStepRecord,
};

/// Stage 2 automatic pipeline core. It processes one already-complete file and
/// performs all durable writes synchronously; Stage 3 will call this from the
/// daemon worker loop.
pub struct AutoPipeline<'a> {
    pub repo: &'a Repository,
    pub archive_root: PathBuf,
    pub asset_roots: Vec<PathBuf>,
    pub scrapers: ScrapeCoordinator<'a>,
}

impl<'a> AutoPipeline<'a> {
    /// Process one completed file through identification, duplicate check,
    /// scraping, archive execution, and SQLite persistence.
    pub fn process_completed_file(&self, file: CompletedFile) -> anyhow::Result<PipelineOutcome> {
        let mut steps = Vec::new();
        let run_id = self.repo.start_pipeline_run(&file.path)?;

        let result = (|| -> anyhow::Result<PipelineOutcome> {
            match identify_completed_file(&file) {
            PipelineIdentification::Holding { reason } => {
                steps.push(step("identify", "holding", Some("missing standard code")));
                let id = self.repo.add_holding(&HoldingEntry {
                    id: None,
                    path: file.path.to_string_lossy().to_string(),
                    file_name: file.file_name.clone(),
                    size_bytes: file.size_bytes as i64,
                    reason,
                    created_at: None,
                })?;
                Ok(PipelineOutcome {
                    status: "holding".to_string(),
                    work_id: None,
                    archived_video_path: None,
                    holding_id: Some(id),
                    exception_id: None,
                    steps: steps.clone(),
                })
            }
            PipelineIdentification::Identified { normalized_code } => {
                steps.push(step("identify", "ok", Some(&normalized_code)));
                if let Some(hash) = file.file_hash.as_deref() {
                    let matches = self.repo.find_file_versions_by_hash(hash)?;
                    if !matches.is_empty() {
                        let evidence = serde_json::json!({
                            "normalized_code": normalized_code,
                            "file_hash": hash,
                            "matches": matches.iter().map(|v| v.id).collect::<Vec<_>>(),
                        });
                        let id = self.repo.record_exception(&Exception {
                            id: None,
                            object_path: file.path.to_string_lossy().to_string(),
                            kind: ExceptionKind::DuplicateCandidate,
                            evidence_json: evidence.to_string(),
                            status: ExceptionStatus::Open,
                            created_at: None,
                            resolved_at: None,
                        })?;
                        Ok(PipelineOutcome {
                            status: "exception".to_string(),
                            work_id: None,
                            archived_video_path: None,
                            holding_id: None,
                            exception_id: Some(id),
                            steps: steps.clone(),
                        })
                    } else {
                        let scrape_context = ScrapeContext {
                            work_id: None,
                            normalized_code: normalized_code.clone(),
                            object_path: file.path.clone(),
                            pipeline_run_id: Some(run_id),
                        };
                        match self.scrapers.scrape(self.repo, &scrape_context) {
                            Ok(metadata) => {
                                steps.push(step("scrape", "ok", Some(&metadata.source)));
                                let plan = plan_archive_layout(
                                    &self.archive_root,
                                    &file,
                                    &metadata,
                                    &self.asset_roots,
                                )?;
                                let executed = execute_archive_layout(&plan, &file, &metadata)?;
                                let work_id = persist_pipeline_success(self.repo, &file, &metadata, &executed)?;
                                Ok(PipelineOutcome {
                                    status: "archived".to_string(),
                                    work_id: Some(work_id),
                                    archived_video_path: Some(executed.video_path),
                                    holding_id: None,
                                    exception_id: None,
                                    steps: steps.clone(),
                                })
                            }
                            Err(PipelineScrapeError::AllSourcesFailed) => {
                                steps.push(step("scrape", "failed", Some("all sources failed")));
                                let evidence = serde_json::json!({ "normalized_code": normalized_code });
                                let id = self.repo.record_exception(&Exception {
                                    id: None,
                                    object_path: file.path.to_string_lossy().to_string(),
                                    kind: ExceptionKind::ScrapeFailed,
                                    evidence_json: evidence.to_string(),
                                    status: ExceptionStatus::Open,
                                    created_at: None,
                                    resolved_at: None,
                                })?;
                                Ok(PipelineOutcome {
                                    status: "exception".to_string(),
                                    work_id: None,
                                    archived_video_path: None,
                                    holding_id: None,
                                    exception_id: Some(id),
                                    steps: steps.clone(),
                                })
                            }
                        }
                    }
                } else {
                    let id = self.repo.record_exception(&Exception {
                        id: None,
                        object_path: file.path.to_string_lossy().to_string(),
                        kind: ExceptionKind::DuplicateCandidate,
                        evidence_json: serde_json::json!({
                            "normalized_code": normalized_code,
                            "message": "fingerprint unavailable"
                        }).to_string(),
                        status: ExceptionStatus::Open,
                        created_at: None,
                        resolved_at: None,
                    })?;
                    Ok(PipelineOutcome {
                        status: "exception".to_string(),
                        work_id: None,
                        archived_video_path: None,
                        holding_id: None,
                        exception_id: Some(id),
                        steps: steps.clone(),
                    })
                }
            }
            }
        })();

        match result {
            Ok(outcome) => {
                self.repo.finish_pipeline_run(run_id, &outcome.status, &outcome.steps, None)?;
                Ok(outcome)
            }
            Err(error) => {
                steps.push(step("pipeline", "failed", Some(&error.to_string())));
                self.repo.finish_pipeline_run(run_id, "failed", &steps, Some(&error.to_string()))?;
                Err(error)
            }
        }
    }
}

fn step(step: &str, status: &str, message: Option<&str>) -> PipelineStepRecord {
    PipelineStepRecord {
        step: step.to_string(),
        status: status.to_string(),
        message: message.map(ToString::to_string),
    }
}

```

- [ ] **Step 5: Run tests to verify they pass**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline auto_pipeline_
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/domain.rs src-tauri/src/pipeline.rs src-tauri/tests/auto_pipeline.rs
git commit -m "串联自动管线端到端处理"
```

---

## Task 10: Stage 2 smoke example and full verification

**Files:**
- Create: `src-tauri/examples/stage2_smoke.rs`
- Modify: `docs/superpowers/specs/2026-06-25-media-manager-refactor-design.md` only if implementation discovers a design correction that affects Stage 2 boundaries.

**Interfaces:**
- `cargo run --manifest-path src-tauri/Cargo.toml --example stage2_smoke`
- Prints deterministic summary lines:
  - `stage2_smoke=completed`
  - `archived=1`
  - `holding=1`
  - `exceptions=1`
  - `no_real_resources_required=true`

- [ ] **Step 1: Write the smoke example**

Create `src-tauri/examples/stage2_smoke.rs`:

```rust
use media_manager::domain::{CompletedFile, ScrapedWorkMetadata};
use media_manager::pipeline::{AutoPipeline, ScrapeCoordinator, ScraperSource};
use media_manager::storage::Repository;

struct SmokeScraper;

impl ScraperSource for SmokeScraper {
    fn name(&self) -> &str {
        "smoke"
    }

    fn lookup(&self, normalized_code: &str) -> anyhow::Result<Option<ScrapedWorkMetadata>> {
        if normalized_code == "ABP-900" {
            Ok(Some(ScrapedWorkMetadata {
                source: self.name().to_string(),
                normalized_code: normalized_code.to_string(),
                title: "ABP-900 smoke title".to_string(),
                original_title: None,
                summary: Some("smoke summary".to_string()),
                actors: vec!["Smoke Actor".to_string()],
                genres: vec!["Smoke Genre".to_string()],
                studio: Some("Smoke Studio".to_string()),
                director: None,
                release_date: Some("2026-06-25".to_string()),
                cover_path: None,
            }))
        } else {
            Ok(None)
        }
    }
}

fn main() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let db = tmp.path().join("library.sqlite");
    let inbox = tmp.path().join("inbox");
    let assets = tmp.path().join("assets");
    let archive = tmp.path().join("archive");
    std::fs::create_dir_all(&inbox)?;
    std::fs::create_dir_all(&assets)?;
    std::fs::write(inbox.join("ABP-900.mp4"), b"video")?;
    std::fs::write(inbox.join("random.mp4"), b"video")?;
    std::fs::write(inbox.join("ABP-901.mp4"), b"video")?;
    std::fs::write(assets.join("ABP-900.jpg"), b"poster")?;

    let repo = Repository::open(&db)?;
    repo.migrate()?;
    let scraper = SmokeScraper;
    let pipeline = AutoPipeline {
        repo: &repo,
        archive_root: archive.clone(),
        asset_roots: vec![assets],
        scrapers: ScrapeCoordinator {
            sources: vec![&scraper],
        },
    };

    for name in ["ABP-900.mp4", "random.mp4", "ABP-901.mp4"] {
        let file = CompletedFile::from_path(&inbox.join(name))?;
        let _ = pipeline.process_completed_file(file)?;
    }

    println!("stage2_smoke=completed");
    let archived = repo
        .list_pipeline_runs()?
        .iter()
        .filter(|run| run.status == "archived")
        .count();
    println!("archived={archived}");
    println!("holding={}", repo.list_holding()?.len());
    println!("exceptions={}", repo.list_exceptions()?.len());
    println!("archive_has_video={}", archive.join("ABP-900/ABP-900.mp4").exists());
    println!("archive_has_nfo={}", archive.join("ABP-900/ABP-900.nfo").exists());
    println!("no_real_resources_required=true");
    Ok(())
}
```

- [ ] **Step 2: Run Stage 2 focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test data_model
cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline
```

Expected: both PASS.

- [ ] **Step 3: Run full backend suite**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: PASS. The exact test count may increase from Stage 1's 87 because Stage 2 adds integration tests.

- [ ] **Step 4: Run self-contained smoke**

Run:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --example stage2_smoke
```

Expected output includes:

```text
stage2_smoke=completed
archived=1
holding=1
exceptions=1
archive_has_video=true
archive_has_nfo=true
no_real_resources_required=true
```

This is a CLI-only example; it must not initialize Tauri.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/examples/stage2_smoke.rs docs/superpowers/specs/2026-06-25-media-manager-refactor-design.md
git commit -m "新增阶段2自动管线冒烟验证"
```

---

## Done Criteria For Stage 2

- Automatic pipeline core processes one completed fake video into `<archive>/<code>/` with video, generated NFO, copied poster, SQLite work, actors/tags, file version, scrape job, and pipeline run.
- Scrape failures do not create pre-scrape `works` rows.
- Missing-code files go to `holding`, not `exceptions`.
- Duplicate fingerprints and scrape failures go to `exceptions`.
- File movement uses staged copy/verify/finalize semantics and rollback if NFO/asset steps fail after moving the video.
- Operational move/write failures finish the existing `pipeline_runs` row as `failed`; they do not create content exceptions.
- `remove_work_from_collection` exists and `work_collections` cascade behavior is covered.
- `cargo test --manifest-path src-tauri/Cargo.toml` passes.
- `cargo run --manifest-path src-tauri/Cargo.toml --example stage2_smoke` prints the expected self-contained summary and uses only temp files.

## Self-Review Checklist

- Spec coverage: covers design §5 monitor/completion predicate, identification, scrape fallback, organize assets, move/archive layout, SQLite ingest, holding, and exception routing. Daemon runtime and UI remain Stage 3/4 by design.
- Incomplete-marker scan: no incomplete markers, no open-ended error-handling tasks, no unbounded test-writing tasks; every task has concrete tests and commands.
- Type consistency: `ScrapedWorkMetadata`, `ArchiveAsset`, `CompletedFile`, `PipelineOutcome`, `ScrapeCoordinator`, and `AutoPipeline` names are consistent across tasks.
