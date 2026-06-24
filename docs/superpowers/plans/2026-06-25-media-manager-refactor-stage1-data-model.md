# Stage 1 — Data Model Extension Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the data model with user playback fields and six new tables (scrape_jobs / exceptions / holding / pipeline_runs / collections / work_collections) so Stages 2–4 (automated pipeline, daemon, frontend) have everything to read and write — without changing any existing behavior.

**Architecture:** Pure data-layer work. Reuse the existing idempotent migration mechanism in `Repository::migrate()` (`CREATE TABLE IF NOT EXISTS` + `ensure_column()`). Add domain structs + Repository CRUD methods, all driven by TDD against a temp SQLite. No pipeline, no daemon, no UI in this stage.

**Tech Stack:** Rust 2021, rusqlite 0.32 (bundled), serde, tempfile (dev), `cargo test`.

## Global Constraints

- **DB:** rusqlite 0.32 `bundled`. All schema changes are idempotent (`CREATE TABLE IF NOT EXISTS` / `ensure_column`) — re-running `migrate()` on an existing library must be a no-op for already-applied changes.
- **Never run the Tauri GUI in a Codex agent session** (`tauri dev` / `cargo run` / `media-manager.exe` open WebView2 and crash the non-interactive host — see `AGENTS.md`). Verify with `cargo test --manifest-path src-tauri/Cargo.toml` only.
- **Migration entry point:** `src-tauri/src/storage.rs` `Repository::migrate()`. Add new tables as a new `self.conn.execute_batch(...)?` block right before the final `Ok(())`.
- **Works read path is single-site:** `WORK_COLUMNS` (storage.rs:2428) + `work_from_row` (storage.rs:2437) map every `Work`. Adding a persisted `works` field = append to `WORK_COLUMNS`, read by index in `work_from_row`, add to the `Work` struct.
- **Watch status storage:** persisted as `format!("{:?}", variant)` (e.g. `"WantToWatch"`); decoded by `parse_watch_status` (storage.rs:2376). New enum variants need a `parse_watch_status` arm.
- **Editing non-ASCII source:** if any source file gains CJK, edit with `apply_patch`/node:fs, never PowerShell `Set-Content` (GBK console corrupts UTF-8). This stage's code is ASCII, so this is a precaution.

---

## File Structure

- **Modify** `src-tauri/src/domain.rs` — extend `WatchStatus`; add fields to `Work`; add structs `ScrapeJob`/`ScrapeStatus`/`Exception`/`ExceptionKind`/`ExceptionStatus`/`HoldingEntry`/`HoldingReason`/`PipelineRun`/`Collection`.
- **Modify** `src-tauri/src/storage.rs` — `migrate()` adds columns + six tables; `parse_watch_status` new arms; `WORK_COLUMNS`/`work_from_row` for the two new works fields; new Repository CRUD methods.
- **Create** `src-tauri/tests/data_model.rs` — stage-1 integration tests (one `#[test]` per task + a shared `sample_work()` helper).

---

## Task 1: Extend `WatchStatus` with 想看 / 在看 / 搁置

**Files:**
- Modify: `src-tauri/src/domain.rs:73-78` (the `WatchStatus` enum)
- Modify: `src-tauri/src/storage.rs:2376-2382` (`parse_watch_status`)
- Test: `src-tauri/tests/data_model.rs` (create)

**Interfaces:**
- Consumes: existing `Repository::migrate()`, `upsert_work`, `update_work_profile`, `get_work_by_id`.
- Produces: `WatchStatus::{WantToWatch, Watching, OnHold}` round-trippable through the DB.

- [ ] **Step 1: Write the failing test**

Create `src-tauri/tests/data_model.rs`:

```rust
use media_manager::domain::{CodeKind, WatchStatus, Work};
use media_manager::storage::Repository;

/// Minimal valid `Work` for tests. Updated in later tasks as Work gains fields.
fn sample_work() -> Work {
    Work {
        id: None,
        normalized_code: None,
        source_code: None,
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
        has_video: false,
        ratings: Vec::new(),
    }
}

#[test]
fn watch_status_roundtrips_new_variants() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    let mut work = sample_work();
    work.normalized_code = Some("ABP-001".to_string());
    let id = repo.upsert_work(&work).unwrap();

    for status in [
        WatchStatus::WantToWatch,
        WatchStatus::Watching,
        WatchStatus::OnHold,
    ] {
        let updated =
            repo.update_work_profile(id, Vec::new(), Vec::new(), None, status.clone()).unwrap();
        assert_eq!(updated.watch_status, status);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model watch_status_roundtrips_new_variants`
Expected: compile error — `WatchStatus::WantToWatch` / `Watching` / `OnHold` do not exist.

- [ ] **Step 3: Extend the enum and parser**

In `src-tauri/src/domain.rs`, replace the `WatchStatus` enum (lines 73-78) with:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchStatus {
    Unwatched,
    Watched,
    Favorite,
    WantToWatch,
    Watching,
    OnHold,
}
```

In `src-tauri/src/storage.rs`, replace `parse_watch_status` (lines 2376-2382) with:

```rust
fn parse_watch_status(value: &str) -> WatchStatus {
    match value {
        "Watched" => WatchStatus::Watched,
        "Favorite" => WatchStatus::Favorite,
        "WantToWatch" => WatchStatus::WantToWatch,
        "Watching" => WatchStatus::Watching,
        "OnHold" => WatchStatus::OnHold,
        _ => WatchStatus::Unwatched,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model watch_status_roundtrips_new_variants`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain.rs src-tauri/src/storage.rs src-tauri/tests/data_model.rs
git commit -m "feat(domain): extend WatchStatus with WantToWatch/Watching/OnHold"
```

---

## Task 2: Add playback-progress fields to `works`

Persist `watch_progress_seconds` (resume position) and `last_played_at` on `works`. Written by a dedicated `set_watch_progress` (user action), not by `upsert_work` (pipeline metadata).

**Files:**
- Modify: `src-tauri/src/storage.rs` — `ensure_work_metadata_columns` (around :314-340), `WORK_COLUMNS` (:2428), `work_from_row` (:2437); add `set_watch_progress`.
- Modify: `src-tauri/src/domain.rs` — `Work` struct (add 2 fields).
- Modify: `src-tauri/tests/data_model.rs` — `sample_work` + new test.

**Interfaces:**
- Produces: `Work { watch_progress_seconds: Option<i64>, last_played_at: Option<String> }`; `Repository::set_watch_progress(work_id, seconds: Option<i64>, last_played_at: Option<String>) -> Result<Work>`.

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/tests/data_model.rs`:

```rust
#[test]
fn watch_progress_is_persisted_and_read_back() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    let mut work = sample_work();
    work.normalized_code = Some("ABP-002".to_string());
    let id = repo.upsert_work(&work).unwrap();

    let updated =
        repo.set_watch_progress(id, Some(1865), Some("2026-06-25T21:00:00Z".to_string())).unwrap();
    assert_eq!(updated.watch_progress_seconds, Some(1865));
    assert_eq!(updated.last_played_at.as_deref(), Some("2026-06-25T21:00:00Z"));

    let cleared = repo.set_watch_progress(id, None, None).unwrap();
    assert_eq!(cleared.watch_progress_seconds, None);
    assert_eq!(cleared.last_played_at, None);
}
```

Also update `sample_work()` — add these two fields to the struct literal (before the closing brace):

```rust
        watch_progress_seconds: None,
        last_played_at: None,
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model watch_progress_is_persisted_and_read_back`
Expected: compile error — `Work` has no field `watch_progress_seconds`; `set_watch_progress` undefined.

- [ ] **Step 3: Implement the column, struct, read mapping, and writer**

3a. In `src-tauri/src/domain.rs`, add two fields to the `Work` struct, right after `ratings: Vec<WorkRating>,` (line 70):

```rust
    pub watch_progress_seconds: Option<i64>,
    pub last_played_at: Option<String>,
```

3b. In `src-tauri/src/storage.rs` `ensure_work_metadata_columns`, add (before its `Ok(())` at line 340):

```rust
        self.ensure_column("works", "watch_progress_seconds", "INTEGER")?;
        self.ensure_column("works", "last_played_at", "TEXT")?;
```

3c. In `src-tauri/src/storage.rs`, append the two new columns to `WORK_COLUMNS` (lines 2428-2433) — change the `has_video";` ending to:

```rust
const WORK_COLUMNS: &str = "id, normalized_code, source_code, code_kind, \
    title_zh, original_title, aliases_json, summary, outline, \
    cover_path, poster_path, thumb_path, fanart_path, screenshot_path, gif_path, \
    tags_json, lists_json, rating, rating_value, rating_max, rating_votes, criticrating, \
    watch_status, genres_json, studio, label, director, release_date, \
    runtime_minutes, year, website, mpaa, has_video, \
    watch_progress_seconds, last_played_at";
```

3d. In `work_from_row` (around :2437-2488), add two reads and two struct fields. After the `let has_video: i64 = row.get(32)?;` line add:

```rust
    let watch_progress_seconds: Option<i64> = row.get(33)?;
    let last_played_at: Option<String> = row.get(34)?;
```

and inside the `Ok(Work { ... })`, after `ratings: Vec::new(),` add:

```rust
        watch_progress_seconds,
        last_played_at,
```

3e. Add the writer method to `impl Repository` (next to `update_work_profile`, ~line 737):

```rust
    /// Set or clear the user's resume position for a work. Independent of
    /// `upsert_work` (which owns pipeline metadata) so playback never fights
    /// the ingest path. Returns the refreshed work.
    pub fn set_watch_progress(
        &self,
        work_id: i64,
        seconds: Option<i64>,
        last_played_at: Option<String>,
    ) -> Result<Work> {
        self.conn.execute(
            "UPDATE works SET watch_progress_seconds = ?1, last_played_at = ?2, \
             updated_at = CURRENT_TIMESTAMP WHERE id = ?3",
            params![seconds, last_played_at, work_id],
        )?;
        if self.conn.changes() == 0 {
            return Err(anyhow::anyhow!("work {work_id} was not found"));
        }
        self.get_work_by_id(work_id)?
            .ok_or_else(|| anyhow::anyhow!("work {work_id} was not found"))
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain.rs src-tauri/src/storage.rs src-tauri/tests/data_model.rs
git commit -m "feat(storage): persist watch_progress_seconds and last_played_at on works"
```

---

## Task 3: `scrape_jobs` table + CRUD

**Files:**
- Modify: `src-tauri/src/domain.rs` (add `ScrapeStatus`, `ScrapeJob`).
- Modify: `src-tauri/src/storage.rs` (`migrate()` new table; `record_scrape_job`, `list_scrape_jobs`).
- Modify: `src-tauri/tests/data_model.rs`.

**Interfaces:**
- Produces: `ScrapeJob { id, work_id, source, status: ScrapeStatus, attempts: i64, last_attempted_at: Option<String>, error: Option<String> }`; `ScrapeStatus::{Pending, Success, Failed}`; `Repository::record_scrape_job(&ScrapeJob) -> Result<i64>`; `Repository::list_scrape_jobs() -> Result<Vec<ScrapeJob>>`.

- [ ] **Step 1: Write the failing test**

Append:

```rust
use media_manager::domain::{ScrapeJob, ScrapeStatus};

#[test]
fn scrape_jobs_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let mut work = sample_work();
    work.normalized_code = Some("ABP-003".to_string());
    let work_id = repo.upsert_work(&work).unwrap();

    let id = repo
        .record_scrape_job(&ScrapeJob {
            id: None,
            work_id,
            source: "FANZA".to_string(),
            status: ScrapeStatus::Failed,
            attempts: 2,
            last_attempted_at: Some("2026-06-25T20:00:00Z".to_string()),
            error: Some("not found".to_string()),
        })
        .unwrap();

    let jobs = repo.list_scrape_jobs().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, Some(id));
    assert_eq!(jobs[0].source, "FANZA");
    assert_eq!(jobs[0].status, ScrapeStatus::Failed);
    assert_eq!(jobs[0].attempts, 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model scrape_jobs_roundtrip`
Expected: compile error — `ScrapeJob` / `ScrapeStatus` undefined; methods missing.

- [ ] **Step 3: Implement**

3a. In `src-tauri/src/domain.rs` append:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScrapeStatus {
    Pending,
    Success,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrapeJob {
    pub id: Option<i64>,
    pub work_id: i64,
    pub source: String,
    pub status: ScrapeStatus,
    pub attempts: i64,
    pub last_attempted_at: Option<String>,
    pub error: Option<String>,
}
```

3b. In `src-tauri/src/storage.rs` `migrate()`, add a new `execute_batch` block right before the final `Ok(())` (after the `work_ratings` block):

```rust
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS scrape_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                work_id INTEGER NOT NULL REFERENCES works(id) ON DELETE CASCADE,
                source TEXT NOT NULL,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                last_attempted_at TEXT,
                error TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            ",
        )?;
```

3c. Add CRUD to `impl Repository`:

```rust
    pub fn record_scrape_job(&self, job: &ScrapeJob) -> Result<i64> {
        let status = match job.status {
            ScrapeStatus::Pending => "Pending",
            ScrapeStatus::Success => "Success",
            ScrapeStatus::Failed => "Failed",
        };
        self.conn.execute(
            "INSERT INTO scrape_jobs (work_id, source, status, attempts, last_attempted_at, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                job.work_id,
                job.source,
                status,
                job.attempts,
                job.last_attempted_at,
                job.error,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_scrape_jobs(&self) -> Result<Vec<ScrapeJob>> {
        let mut statement = self.conn.prepare(
            "SELECT id, work_id, source, status, attempts, last_attempted_at, error
             FROM scrape_jobs ORDER BY id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            let status: String = row.get(3)?;
            Ok(ScrapeJob {
                id: row.get(0)?,
                work_id: row.get(1)?,
                source: row.get(2)?,
                status: match status.as_str() {
                    "Success" => ScrapeStatus::Success,
                    "Failed" => ScrapeStatus::Failed,
                    _ => ScrapeStatus::Pending,
                },
                attempts: row.get(4)?,
                last_attempted_at: row.get(5)?,
                error: row.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }
```

Also add `ScrapeJob, ScrapeStatus` to the `use crate::domain::{...}` import at the top of `storage.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model scrape_jobs_roundtrip`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain.rs src-tauri/src/storage.rs src-tauri/tests/data_model.rs
git commit -m "feat(storage): scrape_jobs table and CRUD"
```

---

## Task 4: `exceptions` table + CRUD

**Files:** `domain.rs`, `storage.rs`, `tests/data_model.rs`.

**Interfaces:**
- Produces: `ExceptionKind::{CodeConflict, DuplicateCandidate, ScrapeFailed}`; `ExceptionStatus::{Open, Ignored, Resolved}`; `Exception { id, object_path, kind: ExceptionKind, evidence_json: String, status: ExceptionStatus, created_at: Option<String>, resolved_at: Option<String> }`; `Repository::record_exception(&Exception) -> Result<i64>`; `Repository::list_exceptions() -> Result<Vec<Exception>>`; `Repository::resolve_exception(id: i64, status: ExceptionStatus) -> Result<()>`.

- [ ] **Step 1: Write the failing test**

Append:

```rust
use media_manager::domain::{Exception, ExceptionKind, ExceptionStatus};

#[test]
fn exceptions_record_list_and_resolve() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    let id = repo
        .record_exception(&Exception {
            id: None,
            object_path: "H:/dl/x.mp4".to_string(),
            kind: ExceptionKind::ScrapeFailed,
            evidence_json: r#"{"sources":["FANZA","JavBus"]}"#.to_string(),
            status: ExceptionStatus::Open,
            created_at: None,
            resolved_at: None,
        })
        .unwrap();

    let open = repo.list_exceptions().unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].kind, ExceptionKind::ScrapeFailed);

    repo.resolve_exception(id, ExceptionStatus::Resolved).unwrap();
    let after = repo.list_exceptions().unwrap();
    assert_eq!(after[0].status, ExceptionStatus::Resolved);
    assert!(after[0].resolved_at.is_some());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model exceptions_record_list_and_resolve`
Expected: compile error — types/methods missing.

- [ ] **Step 3: Implement**

3a. `domain.rs` append:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExceptionKind {
    CodeConflict,
    DuplicateCandidate,
    ScrapeFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExceptionStatus {
    Open,
    Ignored,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exception {
    pub id: Option<i64>,
    pub object_path: String,
    pub kind: ExceptionKind,
    pub evidence_json: String,
    pub status: ExceptionStatus,
    pub created_at: Option<String>,
    pub resolved_at: Option<String>,
}
```

3b. `storage.rs` `migrate()` — add another `self.conn.execute_batch(...)?` block right before the final `Ok(())` (parallel to the block Task 3 added; do not edit Task 3's block):

```rust
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS exceptions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                object_path TEXT NOT NULL,
                kind TEXT NOT NULL,
                evidence_json TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'Open',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                resolved_at TEXT
            );
            ",
        )?;
```

3c. `impl Repository` add (with the new types imported):

```rust
    pub fn record_exception(&self, ex: &Exception) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO exceptions (object_path, kind, evidence_json, status)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                ex.object_path,
                exception_kind_str(&ex.kind),
                ex.evidence_json,
                exception_status_str(&ex.status),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_exceptions(&self) -> Result<Vec<Exception>> {
        let mut statement = self.conn.prepare(
            "SELECT id, object_path, kind, evidence_json, status, created_at, resolved_at
             FROM exceptions ORDER BY id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            let kind: String = row.get(2)?;
            let status: String = row.get(4)?;
            Ok(Exception {
                id: row.get(0)?,
                object_path: row.get(1)?,
                kind: parse_exception_kind(&kind),
                evidence_json: row.get(3)?,
                status: parse_exception_status(&status),
                created_at: row.get(5)?,
                resolved_at: row.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn resolve_exception(&self, id: i64, status: ExceptionStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE exceptions SET status = ?1, resolved_at = CURRENT_TIMESTAMP WHERE id = ?2",
            params![exception_status_str(&status), id],
        )?;
        Ok(())
    }
```

Add free helper functions near `parse_watch_status`:

```rust
fn exception_kind_str(k: &ExceptionKind) -> &'static str {
    match k {
        ExceptionKind::CodeConflict => "CodeConflict",
        ExceptionKind::DuplicateCandidate => "DuplicateCandidate",
        ExceptionKind::ScrapeFailed => "ScrapeFailed",
    }
}
fn parse_exception_kind(v: &str) -> ExceptionKind {
    match v {
        "DuplicateCandidate" => ExceptionKind::DuplicateCandidate,
        "ScrapeFailed" => ExceptionKind::ScrapeFailed,
        _ => ExceptionKind::CodeConflict,
    }
}
fn exception_status_str(s: &ExceptionStatus) -> &'static str {
    match s {
        ExceptionStatus::Open => "Open",
        ExceptionStatus::Ignored => "Ignored",
        ExceptionStatus::Resolved => "Resolved",
    }
}
fn parse_exception_status(v: &str) -> ExceptionStatus {
    match v {
        "Ignored" => ExceptionStatus::Ignored,
        "Resolved" => ExceptionStatus::Resolved,
        _ => ExceptionStatus::Open,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model exceptions_record_list_and_resolve`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain.rs src-tauri/src/storage.rs src-tauri/tests/data_model.rs
git commit -m "feat(storage): exceptions table and CRUD"
```

---

## Task 5: `holding` table + CRUD

**Files:** `domain.rs`, `storage.rs`, `tests/data_model.rs`.

**Interfaces:**
- Produces: `HoldingReason::{NoCode, ShortVideo, NonJapanese, Unrecognizable}`; `HoldingEntry { id, path, file_name, size_bytes: i64, reason: HoldingReason, created_at: Option<String> }`; `Repository::add_holding(&HoldingEntry) -> Result<i64>`; `Repository::list_holding() -> Result<Vec<HoldingEntry>>`.

- [ ] **Step 1: Write the failing test**

Append:

```rust
use media_manager::domain::{HoldingEntry, HoldingReason};

#[test]
fn holding_entries_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    repo.add_holding(&HoldingEntry {
        id: None,
        path: "H:/dl/trailer.mp4".to_string(),
        file_name: "trailer.mp4".to_string(),
        size_bytes: 120_000_000,
        reason: HoldingReason::ShortVideo,
        created_at: None,
    })
    .unwrap();

    let entries = repo.list_holding().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].file_name, "trailer.mp4");
    assert_eq!(entries[0].reason, HoldingReason::ShortVideo);
    assert_eq!(entries[0].size_bytes, 120_000_000);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model holding_entries_roundtrip`
Expected: compile error — types/methods missing.

- [ ] **Step 3: Implement**

3a. `domain.rs` append:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoldingReason {
    NoCode,
    ShortVideo,
    NonJapanese,
    Unrecognizable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldingEntry {
    pub id: Option<i64>,
    pub path: String,
    pub file_name: String,
    pub size_bytes: i64,
    pub reason: HoldingReason,
    pub created_at: Option<String>,
}
```

3b. `storage.rs` `migrate()` — add another `self.conn.execute_batch(...)?` block right before the final `Ok(())` (parallel to the previous tasks' blocks):

```rust
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS holding (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                file_name TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                reason TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            ",
        )?;
```

3c. `impl Repository` add (import the new types):

```rust
    pub fn add_holding(&self, entry: &HoldingEntry) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO holding (path, file_name, size_bytes, reason)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                entry.path,
                entry.file_name,
                entry.size_bytes,
                holding_reason_str(&entry.reason),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_holding(&self) -> Result<Vec<HoldingEntry>> {
        let mut statement = self.conn.prepare(
            "SELECT id, path, file_name, size_bytes, reason, created_at FROM holding ORDER BY id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            let reason: String = row.get(4)?;
            Ok(HoldingEntry {
                id: row.get(0)?,
                path: row.get(1)?,
                file_name: row.get(2)?,
                size_bytes: row.get(3)?,
                reason: parse_holding_reason(&reason),
                created_at: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }
```

Free helpers near the others:

```rust
fn holding_reason_str(r: &HoldingReason) -> &'static str {
    match r {
        HoldingReason::NoCode => "NoCode",
        HoldingReason::ShortVideo => "ShortVideo",
        HoldingReason::NonJapanese => "NonJapanese",
        HoldingReason::Unrecognizable => "Unrecognizable",
    }
}
fn parse_holding_reason(v: &str) -> HoldingReason {
    match v {
        "ShortVideo" => HoldingReason::ShortVideo,
        "NonJapanese" => HoldingReason::NonJapanese,
        "Unrecognizable" => HoldingReason::Unrecognizable,
        _ => HoldingReason::NoCode,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model holding_entries_roundtrip`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain.rs src-tauri/src/storage.rs src-tauri/tests/data_model.rs
git commit -m "feat(storage): holding table and CRUD"
```

---

## Task 6: `pipeline_runs` table + CRUD

**Files:** `domain.rs`, `storage.rs`, `tests/data_model.rs`.

**Interfaces:**
- Produces: `PipelineRun { id, file_path: String, started_at: Option<String>, finished_at: Option<String>, steps_json: String, status: String, error: Option<String> }`; `Repository::record_pipeline_run(&PipelineRun) -> Result<i64>`; `Repository::list_pipeline_runs() -> Result<Vec<PipelineRun>>`.

- [ ] **Step 1: Write the failing test**

Append:

```rust
use media_manager::domain::PipelineRun;

#[test]
fn pipeline_runs_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    repo.record_pipeline_run(&PipelineRun {
        id: None,
        file_path: "H:/dl/ABP-004.mp4".to_string(),
        started_at: Some("2026-06-25T19:00:00Z".to_string()),
        finished_at: Some("2026-06-25T19:00:05Z".to_string()),
        steps_json: r#"[{"step":"scrape","ok":true}]"#.to_string(),
        status: "done".to_string(),
        error: None,
    })
    .unwrap();

    let runs = repo.list_pipeline_runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].file_path, "H:/dl/ABP-004.mp4");
    assert_eq!(runs[0].status, "done");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model pipeline_runs_roundtrip`
Expected: compile error — type/methods missing.

- [ ] **Step 3: Implement**

3a. `domain.rs` append:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: Option<i64>,
    pub file_path: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub steps_json: String,
    pub status: String,
    pub error: Option<String>,
}
```

3b. `storage.rs` `migrate()` — add another `self.conn.execute_batch(...)?` block right before the final `Ok(())`:

```rust
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS pipeline_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                started_at TEXT,
                finished_at TEXT,
                steps_json TEXT NOT NULL DEFAULT '[]',
                status TEXT NOT NULL,
                error TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            ",
        )?;
```

3c. `impl Repository` add (import `PipelineRun`):

```rust
    pub fn record_pipeline_run(&self, run: &PipelineRun) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO pipeline_runs (file_path, started_at, finished_at, steps_json, status, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                run.file_path,
                run.started_at,
                run.finished_at,
                run.steps_json,
                run.status,
                run.error,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_pipeline_runs(&self) -> Result<Vec<PipelineRun>> {
        let mut statement = self.conn.prepare(
            "SELECT id, file_path, started_at, finished_at, steps_json, status, error
             FROM pipeline_runs ORDER BY id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(PipelineRun {
                id: row.get(0)?,
                file_path: row.get(1)?,
                started_at: row.get(2)?,
                finished_at: row.get(3)?,
                steps_json: row.get(4)?,
                status: row.get(5)?,
                error: row.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model pipeline_runs_roundtrip`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain.rs src-tauri/src/storage.rs src-tauri/tests/data_model.rs
git commit -m "feat(storage): pipeline_runs table and CRUD"
```

---

## Task 7: `collections` + `work_collections` tables + CRUD

**Files:** `domain.rs`, `storage.rs`, `tests/data_model.rs`.

**Interfaces:**
- Produces: `Collection { id, name: String, color: Option<String>, sort_order: i64, created_at: Option<String> }`; `Repository::create_collection(name: &str, color: Option<&str>) -> Result<i64>`; `Repository::list_collections() -> Result<Vec<Collection>>`; `Repository::add_work_to_collection(work_id: i64, collection_id: i64) -> Result<()>`; `Repository::list_works_in_collection(collection_id: i64) -> Result<Vec<i64>>`.

- [ ] **Step 1: Write the failing test**

Append:

```rust
use media_manager::domain::Collection;

#[test]
fn collections_hold_works_many_to_many() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    let mut w = sample_work();
    w.normalized_code = Some("ABP-005".to_string());
    let work_id = repo.upsert_work(&w).unwrap();

    let col_id = repo.create_collection("精选最爱", Some("#00a4dc")).unwrap();
    repo.add_work_to_collection(work_id, col_id).unwrap();
    // idempotent: adding twice does not duplicate
    repo.add_work_to_collection(work_id, col_id).unwrap();

    let cols = repo.list_collections().unwrap();
    assert_eq!(cols.len(), 1);
    assert_eq!(cols[0].name, "精选最爱");
    assert_eq!(cols[0].color.as_deref(), Some("#00a4dc"));

    let work_ids = repo.list_works_in_collection(col_id).unwrap();
    assert_eq!(work_ids, vec![work_id]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model collections_hold_works_many_to_many`
Expected: compile error — type/methods missing.

- [ ] **Step 3: Implement**

3a. `domain.rs` append:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub id: Option<i64>,
    pub name: String,
    pub color: Option<String>,
    pub sort_order: i64,
    pub created_at: Option<String>,
}
```

3b. `storage.rs` `migrate()` — add another `self.conn.execute_batch(...)?` block right before the final `Ok(())`:

```rust
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS collections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                color TEXT,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS work_collections (
                work_id INTEGER NOT NULL REFERENCES works(id) ON DELETE CASCADE,
                collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
                PRIMARY KEY (work_id, collection_id)
            );
            ",
        )?;
```

3c. `impl Repository` add (import `Collection`):

```rust
    pub fn create_collection(&self, name: &str, color: Option<&str>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO collections (name, color) VALUES (?1, ?2)",
            params![name, color],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_collections(&self) -> Result<Vec<Collection>> {
        let mut statement = self
            .conn
            .prepare("SELECT id, name, color, sort_order, created_at FROM collections ORDER BY sort_order ASC, id ASC")?;
        let rows = statement.query_map([], |row| {
            Ok(Collection {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                sort_order: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn add_work_to_collection(&self, work_id: i64, collection_id: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO work_collections (work_id, collection_id) VALUES (?1, ?2)",
            params![work_id, collection_id],
        )?;
        Ok(())
    }

    pub fn list_works_in_collection(&self, collection_id: i64) -> Result<Vec<i64>> {
        let mut statement = self.conn.prepare(
            "SELECT work_id FROM work_collections WHERE collection_id = ?1 ORDER BY work_id ASC",
        )?;
        let rows = statement.query_map(params![collection_id], |row| row.get::<_, i64>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }
```

- [ ] **Step 4: Run the whole stage suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test data_model`
Expected: PASS (all stage-1 tests).

Then run the full backend suite to confirm no regression:
Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (existing tests + new).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain.rs src-tauri/src/storage.rs src-tauri/tests/data_model.rs
git commit -m "feat(storage): collections and work_collections many-to-many"
```

---

## Done criteria for Stage 1

- `cargo test --manifest-path src-tauri/Cargo.toml` is green (existing + 7 new tests).
- Running `migrate()` twice on the same DB is a no-op (idempotency).
- A pre-existing `library.sqlite` (from the old app) can be opened and migrated without data loss — the six new tables are added, `works` gains two NULL columns, existing `watch_status` values (`Unwatched`/`Watched`/`Favorite`) still parse.
