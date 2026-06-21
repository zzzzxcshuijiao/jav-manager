# NFO Metadata Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the library around full-fidelity NFO metadata so 526+ NFO files are parsed, normalized, merged into works, and queryable by actors, tags, sets, studios, and labels.

**Architecture:** Split NFO parsing out of scanner.rs into a focused parser module, extend the repository schema for normalized metadata entities, then add a transactional rebuild pipeline that scans NFO + video pairs and merges multi-CD works by source_code. Backend-first: parser, schema, rebuild, query APIs, dry-run example, then a thin settings trigger for preview/rebuild without redesigning the whole library UI.

**Tech Stack:** Rust, rusqlite, Tauri commands, existing repository pattern, React/Tauri thin settings trigger, cargo integration tests.

---

## File Structure

Create:
- src-tauri/src/nfo.rs - Structured NFO parser, CDATA stripping, runtime normalization, ratings extraction, tag cleaning.
- src-tauri/src/library_rebuild.rs - Scan pairing, source_code grouping, main-NFO selection, rebuild orchestration types.
- src-tauri/examples/preview_rebuild.rs - Dry-run CLI entrypoint that scans roots and prints RebuildReport without mutating SQLite.
- src-tauri/tests/nfo_parser.rs - Focused parser tests for CDATA/runtime/ratings/tag cleaning.
- src-tauri/tests/library_rebuild.rs - Rebuild integration tests (multi-CD merge, cross-source merge, transaction rollback, query APIs).

Modify:
- src-tauri/src/domain.rs - Extend Work, add metadata entity structs (Tag, WorkSet, Label, Studio, WorkRating, WorkDetail, RebuildReport, WorkFilters).
- src-tauri/src/scanner.rs - Delegate NFO parsing to nfo.rs, stop carrying ad-hoc XML logic inline; add paired scan_library_roots.
- src-tauri/src/storage.rs - Migrations, normalized metadata tables, repository read/write/query/rebuild methods.
- src-tauri/src/commands.rs - New Tauri commands for preview/rebuild and metadata queries.
- src-tauri/src/lib.rs - Register new modules.
- src/api.ts - Add typed client methods for preview/rebuild and metadata queries.
- src/App.tsx - Add minimal settings buttons for preview/rebuild and report rendering only (no library redesign yet).
- src/styles.css - Minimal button/report styling for the settings trigger.

Keep untouched in this plan:
- src/viewModel.ts - Full library browsing redesign is deferred to the next spec.

---

### Task 1: Build a real NFO parser module

Files:
- Create: src-tauri/src/nfo.rs
- Modify: src-tauri/src/domain.rs
- Modify: src-tauri/src/lib.rs
- Test: src-tauri/tests/nfo_parser.rs

- [ ] Step 1: Write the failing parser tests

See test code block below; covers strip_cdata (CDATA/plain), parse_runtime_minutes (number/Chinese/English/HH:MM:SS/fail), and parse_nfo_document rich-field + nested ratings extraction.

```rust
use media_manager::nfo::{parse_nfo_document, parse_runtime_minutes, strip_cdata, ParsedRatingSource};

#[test]
fn strip_cdata_returns_inner_text_without_markers() {
    assert_eq!(strip_cdata("<![CDATA[TheLifeErotic.19.06.20-Rope 2]]>"), "TheLifeErotic.19.06.20-Rope 2");
    assert_eq!(strip_cdata("plain text"), "plain text");
}

#[test]
fn parse_runtime_minutes_handles_multiple_formats() {
    assert_eq!(parse_runtime_minutes("134"), Some(134));
    assert_eq!(parse_runtime_minutes("9分鍾"), Some(9));
    assert_eq!(parse_runtime_minutes("120 min"), Some(120));
    assert_eq!(parse_runtime_minutes("02:15:00"), Some(135));
    assert_eq!(parse_runtime_minutes("garbage"), None);
}

#[test]
fn parse_nfo_document_extracts_rich_fields_and_nested_ratings() {
    let xml = r#"<movie>
  <title><![CDATA[TheLifeErotic.19.06.20-Rope 2]]></title>
  <originaltitle><![CDATA[TheLifeErotic.19.06.20-Rope 2]]></originaltitle>
  <outline><![CDATA[]]></outline>
  <plot><![CDATA[Plot text]]></plot>
  <num>TheLifeErotic.19.06.20</num>
  <runtime>9分鍾</runtime>
  <year>2019</year>
  <set>The Life Erotic</set>
  <studio>Metartnetwork</studio>
  <label>The Life Erotic</label>
  <premiered>2019-06-20</premiered>
  <poster>poster.jpg</poster>
  <thumb>thumb.jpg</thumb>
  <fanart>fanart.jpg</fanart>
  <cover>https://example.com/cover.jpg</cover>
  <website>https://javdb.com/v/0yqx7</website>
  <tag>中文字幕</tag>
  <genre>H264</genre>
  <ratings><rating name="javdb" max="5" default="true"><value>4.0</value><votes>2</votes></rating></ratings>
</movie>"#;
    let parsed = parse_nfo_document(xml).expect("parse should succeed");
    assert_eq!(parsed.title.as_deref(), Some("TheLifeErotic.19.06.20-Rope 2"));
    assert_eq!(parsed.summary.as_deref(), Some("Plot text"));
    assert_eq!(parsed.source_code.as_deref(), Some("TheLifeErotic.19.06.20"));
    assert_eq!(parsed.runtime_minutes, Some(9));
    assert_eq!(parsed.year, Some(2019));
    assert_eq!(parsed.sets, vec!["The Life Erotic"]);
    assert_eq!(parsed.studio.as_deref(), Some("Metartnetwork"));
    assert_eq!(parsed.label.as_deref(), Some("The Life Erotic"));
    assert_eq!(parsed.tags, vec!["中文字幕"]);
    assert_eq!(parsed.genres, vec!["H264"]);
    assert_eq!(parsed.rating_sources, vec![ParsedRatingSource {
        source: "javdb".to_string(),
        value: 4.0,
        max: 5,
        votes: Some(2),
        is_default: true,
    }]);
}
```

- [ ] Step 2: Run the parser tests to verify they fail

Run: cargo test --manifest-path src-tauri/Cargo.toml --test nfo_parser -q
Expected: FAIL with unresolved import media_manager::nfo and missing functions/types.

- [ ] Step 3: Add the domain types and parser module skeleton

```rust
// src-tauri/src/domain.rs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodeKind { Standard, Nonstandard }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkRating {
    pub source: String,
    pub value: f32,
    pub max: i32,
    pub votes: Option<i64>,
}
```

```rust
// src-tauri/src/nfo.rs
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRatingSource {
    pub source: String,
    pub value: f32,
    pub max: i32,
    pub votes: Option<i64>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedNfoDocument {
    pub source_code: Option<String>,
    pub title: Option<String>,
    pub original_title: Option<String>,
    pub outline: Option<String>,
    pub summary: Option<String>,
    pub runtime_minutes: Option<i64>,
    pub year: Option<i32>,
    pub sets: Vec<String>,
    pub studio: Option<String>,
    pub label: Option<String>,
    pub tags: Vec<String>,
    pub genres: Vec<String>,
    pub release_date: Option<String>,
    pub cover_url: Option<String>,
    pub poster_path: Option<String>,
    pub thumb_path: Option<String>,
    pub fanart_path: Option<String>,
    pub website: Option<String>,
    pub mpaa: Option<String>,
    pub rating_sources: Vec<ParsedRatingSource>,
}

pub fn strip_cdata(value: &str) -> String { todo!() }
pub fn parse_runtime_minutes(value: &str) -> Option<i64> { todo!() }
pub fn parse_nfo_document(xml: &str) -> anyhow::Result<ParsedNfoDocument> { todo!() }
```

```rust
// src-tauri/src/lib.rs
pub mod nfo;
```

- [ ] Step 4: Implement the minimal parser that satisfies the tests

strip_cdata trims, then strips a leading <![CDATA[ and trailing ]]> pair if both present. parse_runtime_minutes tries in order: HH:MM:SS, pure integer, "数字+中文分" (normalize 分鐘/分鍾 to 分), "数字+min/minutes", else None. parse_nfo_document reuses the existing regex helper style but returns ParsedNfoDocument and runs strip_cdata before decode_xml_entities for every text value.

```rust
pub fn strip_cdata(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(inner) = trimmed.strip_prefix("<![CDATA[").and_then(|v| v.strip_suffix("]]>")) {
        inner.to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn parse_runtime_minutes(value: &str) -> Option<i64> {
    let trimmed = strip_cdata(value);
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() == 3 {
        let h = parts[0].parse::<i64>().ok()?;
        let m = parts[1].parse::<i64>().ok()?;
        let s = parts[2].parse::<i64>().ok()?;
        return Some(h * 60 + m + i64::from(s >= 30));
    }
    if let Ok(minutes) = trimmed.parse::<i64>() {
        return Some(minutes);
    }
    let normalized = trimmed.replace("分鐘", "分").replace("分鍾", "分");
    for suffix in ["分", "min", "minutes"] {
        if let Some(num) = normalized.strip_suffix(suffix) {
            if let Ok(minutes) = num.trim().parse::<i64>() {
                return Some(minutes);
            }
        }
    }
    None
}
```

clean_tags replaces scanner.rs clean_genres: merge tags + genres first, drop empty, drop pure-ASCII tokens (H264/1080P), drop code-prefix tokens (^[A-Z]{2,6}[-_]?\d*$), dedupe in order.

- [ ] Step 5: Run the parser tests until they pass

Run: cargo test --manifest-path src-tauri/Cargo.toml --test nfo_parser -q
Expected: PASS with 3 tests.

- [ ] Step 6: Commit

```bash
git add src-tauri/src/domain.rs src-tauri/src/lib.rs src-tauri/src/nfo.rs src-tauri/tests/nfo_parser.rs
git commit -m "feat(nfo): add structured parser for cdata runtime and ratings"
```
git commit -m "feat(nfo): add structured parser for cdata runtime and ratings"
```

### Task 2: Extend schema and repository persistence for normalized metadata

Files:
- Modify: src-tauri/src/domain.rs
- Modify: src-tauri/src/storage.rs
- Test: src-tauri/tests/library_rebuild.rs

- [ ] Step 1: Write failing repository tests for schema migration and rich work persistence

```rust
use media_manager::domain::{CodeKind, Work, WorkRating, WatchStatus};
use media_manager::storage::Repository;
use tempfile::tempdir;

#[test]
fn migrate_adds_rich_work_columns_and_relation_tables() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();

    let columns = repo.debug_table_columns("works").unwrap();
    assert!(columns.contains(&"source_code".to_string()));
    assert!(columns.contains(&"runtime_minutes".to_string()));
    assert!(columns.contains(&"website".to_string()));
    assert!(columns.contains(&"rating_value".to_string()));
    assert!(repo.debug_table_exists("tags").unwrap());
    assert!(repo.debug_table_exists("work_tags").unwrap());
    assert!(repo.debug_table_exists("sets").unwrap());
    assert!(repo.debug_table_exists("labels").unwrap());
    assert!(repo.debug_table_exists("studios").unwrap());
    assert!(repo.debug_table_exists("work_ratings").unwrap());
}

#[test]
fn upsert_work_persists_rich_metadata_and_relations() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();

    let work = Work {
        id: None,
        normalized_code: None,
        source_code: Some("TheLifeErotic.19.06.20".to_string()),
        code_kind: CodeKind::Nonstandard,
        title_zh: Some("TheLifeErotic.19.06.20-Rope 2".to_string()),
        original_title: Some("TheLifeErotic.19.06.20-Rope 2".to_string()),
        aliases: vec![],
        summary: Some("Plot text".to_string()),
        outline: None,
        cover_path: Some("https://example.com/cover.jpg".to_string().into()),
        poster_path: Some("poster.jpg".to_string().into()),
        thumb_path: Some("thumb.jpg".to_string().into()),
        fanart_path: Some("fanart.jpg".to_string().into()),
        tags: vec!["中文字幕".to_string(), "微變態".to_string()],
        sets: vec!["The Life Erotic".to_string()],
        lists: vec![],
        rating: None,
        rating_value: Some(4.0),
        rating_max: Some(5),
        rating_votes: Some(2),
        criticrating: Some(80.0),
        watch_status: WatchStatus::Unwatched,
        genres: vec![],
        studio: Some("Metartnetwork".to_string()),
        label: Some("The Life Erotic".to_string()),
        director: None,
        release_date: Some("2019-06-20".to_string()),
        runtime_minutes: Some(9),
        year: Some(2019),
        website: Some("https://javdb.com/v/0yqx7".to_string()),
        mpaa: Some("JP-18+".to_string()),
        has_video: false,
        ratings: vec![WorkRating { source: "javdb".to_string(), value: 4.0, max: 5, votes: Some(2) }],
    };

    let work_id = repo.upsert_work(&work).unwrap();
    let stored = repo.get_work_detail(work_id).unwrap().unwrap();
    assert_eq!(stored.work.source_code.as_deref(), Some("TheLifeErotic.19.06.20"));
    assert_eq!(stored.work.code_kind, CodeKind::Nonstandard);
    assert_eq!(stored.tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["中文字幕", "微變態"]);
    assert_eq!(stored.sets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["The Life Erotic"]);
    assert_eq!(stored.ratings[0].source, "javdb");
    assert!(!stored.work.has_video);
}
```

- [ ] Step 2: Run the repository tests to verify they fail

Run: cargo test --manifest-path src-tauri/Cargo.toml --test library_rebuild migrate_adds_rich_work_columns_and_relation_tables -q
Expected: FAIL with missing fields on Work, missing repository helpers, and missing tables/columns.

- [ ] Step 3: Extend domain.rs with the new structs and fields

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag { pub id: Option<i64>, pub name: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSet { pub id: Option<i64>, pub name: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label { pub id: Option<i64>, pub name: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Studio { pub id: Option<i64>, pub name: String }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkDetail {
    pub work: Work,
    pub actors: Vec<Actor>,
    pub tags: Vec<Tag>,
    pub sets: Vec<WorkSet>,
    pub file_versions: Vec<FileVersion>,
    pub ratings: Vec<WorkRating>,
}
```

Update Work to add: source_code, code_kind, outline, runtime_minutes, year, website, mpaa, poster_path, thumb_path, fanart_path, label, rating_value, rating_max, rating_votes, criticrating, has_video, ratings.

- [ ] Step 4: Migrate the schema and repository read/write paths

```rust
self.ensure_column("works", "source_code", "TEXT")?;
self.ensure_column("works", "code_kind", "TEXT NOT NULL DEFAULT 'standard'")?;
self.ensure_column("works", "runtime_minutes", "INTEGER")?;
self.ensure_column("works", "year", "INTEGER")?;
self.ensure_column("works", "website", "TEXT")?;
self.ensure_column("works", "mpaa", "TEXT")?;
self.ensure_column("works", "outline", "TEXT")?;
self.ensure_column("works", "poster_path", "TEXT")?;
self.ensure_column("works", "thumb_path", "TEXT")?;
self.ensure_column("works", "fanart_path", "TEXT")?;
self.ensure_column("works", "criticrating", "REAL")?;
self.ensure_column("works", "rating_value", "REAL")?;
self.ensure_column("works", "rating_max", "INTEGER")?;
self.ensure_column("works", "rating_votes", "INTEGER")?;
self.ensure_column("works", "has_video", "INTEGER NOT NULL DEFAULT 1")?;
```

Add the normalized metadata tables (tags/work_tags, sets/work_sets, labels, studios, work_ratings) following the actors/work_actors/actor_names pattern. Migrate also backfills rating_value from the legacy rating INTEGER column where rating_value IS NULL.

Implement upsert_work (now keyed on source_code with COALESCE-backfill semantics so existing non-null fields are preserved), set_work_tags, set_work_sets, set_work_ratings, get_work_detail, debug_table_columns, and debug_table_exists.

- [ ] Step 5: Run the repository tests until they pass

Run: cargo test --manifest-path src-tauri/Cargo.toml --test library_rebuild -q
Expected: PASS for the migration and persistence tests created in this task.

- [ ] Step 6: Commit

```bash
git add src-tauri/src/domain.rs src-tauri/src/storage.rs src-tauri/tests/library_rebuild.rs
git commit -m "feat(storage): persist normalized nfo metadata relations"
```

### Task 3: Implement rebuild orchestration and multi-CD grouping

Files:
- Create: src-tauri/src/library_rebuild.rs
- Modify: src-tauri/src/scanner.rs
- Modify: src-tauri/src/storage.rs
- Modify: src-tauri/src/lib.rs
- Test: src-tauri/tests/library_rebuild.rs

- [ ] Step 1: Write failing rebuild tests for multi-CD merge, nonstandard codes, and no-video NFOs

```rust
#[test]
fn rebuild_merges_multi_cd_nfos_into_one_work_and_multiple_versions() {
    let sandbox = TestLibrary::new();
    sandbox.write_nfo("Aoi\\IPX-607\\IPX-607.nfo", SAMPLE_IPX_607_MAIN);
    sandbox.write_nfo("Aoi\\IPX-607\\IPX-607-cd2.nfo", SAMPLE_IPX_607_CD2);
    sandbox.write_video("Aoi\\IPX-607\\IPX-607.mp4", 1024 * 1024);
    sandbox.write_video("Aoi\\IPX-607\\IPX-607-cd2.mp4", 1024 * 1024);

    let repo = sandbox.open_repo();
    repo.migrate().unwrap();
    let report = repo.rebuild_library(&[sandbox.root().to_path_buf()]).unwrap();

    assert_eq!(report.works_created, 1);
    assert_eq!(report.works_merged, 1);
    let works = repo.list_works().unwrap();
    assert_eq!(works.len(), 1);
    let detail = repo.get_work_detail(works[0].id.unwrap()).unwrap().unwrap();
    assert_eq!(detail.file_versions.len(), 2);
}

#[test]
fn rebuild_marks_nonstandard_num_as_nonstandard_and_keeps_source_code() {
    let sandbox = TestLibrary::new();
    sandbox.write_nfo("Violet\\TheLifeErotic\\TheLifeErotic.19.06.20-C.nfo", SAMPLE_THE_LIFE_EROTIC);

    let repo = sandbox.open_repo();
    repo.migrate().unwrap();
    repo.rebuild_library(&[sandbox.root().to_path_buf()]).unwrap();

    let works = repo.list_works().unwrap();
    assert_eq!(works[0].source_code.as_deref(), Some("TheLifeErotic.19.06.20"));
    assert!(works[0].normalized_code.is_none());
    assert_eq!(works[0].code_kind, CodeKind::Nonstandard);
    assert!(!works[0].has_video);
}

#[test]
fn rebuild_rolls_back_when_relation_write_fails() {
    let sandbox = TestLibrary::new();
    sandbox.write_nfo("Broken\\ABP-001\\ABP-001.nfo", SAMPLE_MINIMAL_AV);

    let repo = sandbox.open_repo();
    repo.migrate().unwrap();
    repo.debug_drop_table("tags").unwrap();

    let before = repo.list_works().unwrap();
    let result = repo.rebuild_library(&[sandbox.root().to_path_buf()]);
    assert!(result.is_err());
    let after = repo.list_works().unwrap();
    assert_eq!(before, after);
}
```

TestLibrary is a small fixture helper in the same test file: tempdir + write_nfo/write_video/open_repo helpers, plus shared SAMPLE_* NFO constants.

- [ ] Step 2: Run the rebuild tests to verify they fail

Run: cargo test --manifest-path src-tauri/Cargo.toml --test library_rebuild rebuild_merges_multi_cd_nfos_into_one_work_and_multiple_versions -q
Expected: FAIL because rebuild_library, TestLibrary, and grouping helpers do not exist yet.

- [ ] Step 3: Add grouping and report types

```rust
// library_rebuild.rs
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuildError { pub nfo_path: String, pub message: String }

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuildReport {
    pub nfos_scanned: usize,
    pub works_created: usize,
    pub works_merged: usize,
    pub tags_extracted: usize,
    pub sets_extracted: usize,
    pub actors_extracted: usize,
    pub file_versions_created: usize,
    pub errors: Vec<RebuildError>,
}

#[derive(Debug, Clone)]
pub struct GroupedWorkMember {
    pub nfo_path: PathBuf,
    pub nfo_stem: String,
    pub nfo_file_name: String,
    pub document: ParsedNfoDocument,
    pub paired_video: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct GroupedWorkInput {
    pub source_code: String,
    pub members: Vec<GroupedWorkMember>,
}

pub fn group_scanned_nfos(scanned: &ScannedLibrary) -> Result<Vec<GroupedWorkInput>> { /* group by source_code */ }
pub fn select_main_nfo<'a>(members: &'a [GroupedWorkMember]) -> &'a GroupedWorkMember { /* min by (stem_len, file_name) */ }
pub fn summarize_grouped_inputs(groups: &[GroupedWorkInput]) -> Result<RebuildReport> { /* counts only, no writes */ }
```

select_main_nfo picks the shortest NFO stem (IPX-607.nfo beats IPX-607-cd3.nfo); ties broken by ASCII-smallest file name for determinism.

- [ ] Step 4: Implement scanner pairing + transactional rebuild

```rust
// scanner.rs
pub struct ScannedLibrary {
    pub items: Vec<IngestItem>,
    pub nfo_documents: Vec<ScannedNfoDocument>,
}

pub struct ScannedNfoDocument {
    pub nfo_path: PathBuf,
    pub document: ParsedNfoDocument,
    pub paired_video: Option<PathBuf>,
}

pub fn scan_library_roots(roots: &[PathBuf]) -> Result<ScannedLibrary> { /* pair videos + nfos */ }
```

Pairing rule: for each NFO, look in the same directory for a video whose stem equals the NFO stem after stripping a trailing -cdN/-CDN segment.

```rust
// storage.rs
pub fn rebuild_library(&self, roots: &[PathBuf]) -> Result<RebuildReport> {
    let tx = self.conn.unchecked_transaction()?;
    self.clear_library_tables(&tx)?;
    let scanned = crate::scanner::scan_library_roots(roots)?;
    let grouped = crate::library_rebuild::group_scanned_nfos(&scanned)?;
    let report = self.persist_grouped_rebuild(&tx, &grouped)?;
    tx.commit()?;
    Ok(report)
}
```

persist_grouped_rebuild must:
- use source_code as the work merge key;
- choose the main NFO per group via select_main_nfo;
- write the main NFO fields into works plus relation tables (actors, tags, sets, labels, studios, work_ratings);
- create one FileVersion per group member, even when paired_video is None (in that case size_bytes=0 and has_video=false);
- set work.has_video = true if any member has a paired video;
- collect RebuildReport counts as it writes, and push into errors when a single NFO fails to parse instead of aborting the whole rebuild.

clear_library_tables truncates works, file_versions, work_tags, work_sets, work_ratings, work_actors, ingest_items, ingest_jobs, tags, sets, labels, studios, actors, actor_names; it must NOT touch app_settings or archive_action_logs.

- [ ] Step 5: Run the rebuild tests until they pass

Run: cargo test --manifest-path src-tauri/Cargo.toml --test library_rebuild -q
Expected: PASS for multi-CD merge, nonstandard code handling, and transaction rollback.

- [ ] Step 6: Commit

```bash
git add src-tauri/src/lib.rs src-tauri/src/scanner.rs src-tauri/src/library_rebuild.rs src-tauri/src/storage.rs src-tauri/tests/library_rebuild.rs
git commit -m "feat(rebuild): add transactional nfo rebuild and multi-cd merge"
```

### Task 4: Add metadata query APIs and Tauri command surface

Files:
- Modify: src-tauri/src/domain.rs
- Modify: src-tauri/src/storage.rs
- Modify: src-tauri/src/commands.rs
- Test: src-tauri/tests/library_rebuild.rs

- [ ] Step 1: Write failing query tests for tags, sets, and AND filters

```rust
#[test]
fn query_apis_return_dimension_counts_and_and_filtered_works() {
    let sandbox = TestLibrary::new();
    sandbox.write_nfo("A\\ABP-001\\ABP-001.nfo", SAMPLE_WITH_TAGS_A);
    sandbox.write_nfo("B\\ABP-002\\ABP-002.nfo", SAMPLE_WITH_TAGS_B);

    let repo = sandbox.open_repo();
    repo.migrate().unwrap();
    repo.rebuild_library(&[sandbox.root().to_path_buf()]).unwrap();

    let tags = repo.list_tags().unwrap();
    let giant = tags.iter().find(|tag| tag.name == "巨乳").unwrap();
    assert!(giant.work_count >= 1);

    let actors = repo.list_work_actors_for_name("某演员").unwrap();
    let filtered = repo.list_works_filtered(WorkFilters {
        tag_ids: vec![giant.id.unwrap()],
        actor_ids: vec![actors[0].id.unwrap()],
        ..Default::default()
    }).unwrap();
    assert_eq!(filtered.len(), 1);
}
```

- [ ] Step 2: Run the query tests to verify they fail

Run: cargo test --manifest-path src-tauri/Cargo.toml --test library_rebuild query_apis_return_dimension_counts_and_and_filtered_works -q
Expected: FAIL with missing repository methods and WorkFilters type.

- [ ] Step 3: Add repository query types and methods

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkFilters {
    pub tag_ids: Vec<i64>,
    pub set_ids: Vec<i64>,
    pub actor_ids: Vec<i64>,
    pub studio_ids: Vec<i64>,
    pub label_ids: Vec<i64>,
    pub code_kinds: Vec<CodeKind>,
    pub has_video: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DimensionCount { pub id: i64, pub name: String, pub work_count: i64 }
```

Implement list_tags, list_sets, list_studios, list_labels, list_work_actors_for_name, and list_works_filtered. Dimension counts use GROUP BY name with COUNT(DISTINCT work_id). The AND filter joins work_tags/work_actors/etc. once per requested id set and uses HAVING COUNT(DISTINCT id) = requested_count so multiple tag_ids AND actor_ids all intersect.

- [ ] Step 4: Expose Tauri commands for the new query APIs

```rust
#[tauri::command]
pub fn list_tags(state: State<'_, AppState>) -> Result<CommandResult<Vec<DimensionCount>>, String> { /* ... */ }

#[tauri::command]
pub fn list_sets(state: State<'_, AppState>) -> Result<CommandResult<Vec<DimensionCount>>, String> { /* ... */ }

#[tauri::command]
pub fn list_studios(state: State<'_, AppState>) -> Result<CommandResult<Vec<DimensionCount>>, String> { /* ... */ }

#[tauri::command]
pub fn list_labels(state: State<'_, AppState>) -> Result<CommandResult<Vec<DimensionCount>>, String> { /* ... */ }

#[tauri::command]
pub fn list_works_filtered(
    filters: WorkFilters,
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<Work>>, String> { /* ... */ }

#[tauri::command]
pub fn get_work_detail(
    work_id: i64,
    state: State<'_, AppState>,
) -> Result<CommandResult<WorkDetail>, String> { /* ... */ }
```

Add them to generate_handler! and keep the existing no-arg list_works() command intact for backward compatibility (equivalent to list_works_filtered({})).

- [ ] Step 5: Run the query tests until they pass

Run: cargo test --manifest-path src-tauri/Cargo.toml --test library_rebuild -q
Expected: PASS for dimension count and AND-filter tests.

- [ ] Step 6: Commit

```bash
git add src-tauri/src/domain.rs src-tauri/src/storage.rs src-tauri/src/commands.rs src-tauri/tests/library_rebuild.rs
git commit -m "feat(query): add metadata dimension queries and filters"
```
git commit -m "feat(query): add metadata dimension queries and filters"
```

### Task 5: Add preview/rebuild command entrypoints and CLI example

Files:
- Modify: src-tauri/src/commands.rs
- Create: src-tauri/examples/preview_rebuild.rs
- Modify: src-tauri/src/storage.rs
- Test: src-tauri/tests/library_rebuild.rs

- [ ] Step 1: Write failing tests for preview mode and command report shape

```rust
#[test]
fn preview_rebuild_reports_counts_without_mutating_db() {
    let sandbox = TestLibrary::new();
    sandbox.write_nfo("A\\ABP-001\\ABP-001.nfo", SAMPLE_MINIMAL_AV);

    let repo = sandbox.open_repo();
    repo.migrate().unwrap();
    let before = repo.list_works().unwrap();
    let report = repo.preview_rebuild(&[sandbox.root().to_path_buf()]).unwrap();
    let after = repo.list_works().unwrap();

    assert_eq!(before, after);
    assert_eq!(report.nfos_scanned, 1);
    assert_eq!(report.works_created, 1);
}
```

- [ ] Step 2: Run the preview tests to verify they fail

Run: cargo test --manifest-path src-tauri/Cargo.toml --test library_rebuild preview_rebuild_reports_counts_without_mutating_db -q
Expected: FAIL with missing preview_rebuild.

- [ ] Step 3: Implement preview mode in the repository and Tauri commands

```rust
// storage.rs
pub fn preview_rebuild(&self, roots: &[PathBuf]) -> Result<RebuildReport> {
    let scanned = crate::scanner::scan_library_roots(roots)?;
    let grouped = crate::library_rebuild::group_scanned_nfos(&scanned)?;
    crate::library_rebuild::summarize_grouped_inputs(&grouped)
}
```

```rust
// commands.rs
#[tauri::command]
pub fn preview_rebuild(
    source_roots: Vec<String>,
    state: State<'_, AppState>,
) -> Result<CommandResult<RebuildReport>, String> {
    let repo_guard = state.repository.lock().map_err(|e| e.to_string())?;
    let Some(repo) = repo_guard.as_ref() else { return Err("repository is not available".into()); };
    let roots: Vec<PathBuf> = source_roots.into_iter().map(PathBuf::from).collect();
    Ok(CommandResult { data: repo.preview_rebuild(&roots).map_err(|e| e.to_string())? })
}

#[tauri::command]
pub fn rebuild_library_from_nfo(
    source_roots: Vec<String>,
    state: State<'_, AppState>,
) -> Result<CommandResult<RebuildReport>, String> {
    let repo_guard = state.repository.lock().map_err(|e| e.to_string())?;
    let Some(repo) = repo_guard.as_ref() else { return Err("repository is not available".into()); };
    let roots: Vec<PathBuf> = source_roots.into_iter().map(PathBuf::from).collect();
    Ok(CommandResult { data: repo.rebuild_library(&roots).map_err(|e| e.to_string())? })
}
```

Register both commands in generate_handler!.

- [ ] Step 4: Add the CLI example

```rust
// src-tauri/examples/preview_rebuild.rs
use media_manager::commands::open_repository;
use std::env;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = env::args().skip(1);
    let db_path = PathBuf::from(args.next().expect("usage: preview_rebuild <db_path> <source_roots...>"));
    let roots: Vec<PathBuf> = args.map(PathBuf::from).collect();
    let repo = open_repository(&db_path)?;
    let report = repo.preview_rebuild(&roots)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
```

open_repository must be pub (it already is in commands.rs).

- [ ] Step 5: Run tests and the example

Run: cargo test --manifest-path src-tauri/Cargo.toml --test library_rebuild -q
Expected: PASS.

Run: cargo run --manifest-path src-tauri/Cargo.toml --example preview_rebuild -- G:\source_code\media-manager\.tmp-preview\library.sqlite H:\CineMingle-1.3.0\JAV_output
Expected: JSON report with nfos_scanned, works_created, works_merged, and no mutation side effects on the temp DB.

- [ ] Step 6: Commit

```bash
git add src-tauri/src/commands.rs src-tauri/src/storage.rs src-tauri/examples/preview_rebuild.rs src-tauri/tests/library_rebuild.rs
git commit -m "feat(rebuild): add preview command and cli example"
```

### Task 6: Add a thin settings trigger for preview/rebuild

Files:
- Modify: src/api.ts
- Modify: src/App.tsx
- Modify: src/styles.css
- Test: src/viewModel.test.ts

- [ ] Step 1: Write the failing client-side smoke test for rebuild report presentation

```typescript
import { describe, expect, it } from "vitest";

describe("rebuild report presentation", () => {
  it("formats preview and rebuild counts for the settings status bar", () => {
    const report = {
      nfos_scanned: 526,
      works_created: 480,
      works_merged: 18,
      tags_extracted: 900,
      sets_extracted: 200,
      actors_extracted: 320,
      file_versions_created: 550,
      errors: [],
    };
    const message = `预览完成：${report.nfos_scanned} 个 NFO，${report.works_created} 个作品，${report.works_merged} 个多文件合并组。`;
    expect(message).toContain("预览完成");
    expect(message).toContain("526");
  });
});
```

Add a helper formatRebuildReport(mode, report) in viewModel.ts and assert against it; the inline version above is the seed.

- [ ] Step 2: Run the frontend tests to verify they fail

Run: npm test
Expected: FAIL on the new describe block until the helper exists.

- [ ] Step 3: Add typed API methods for preview/rebuild

```typescript
// src/api.ts
export interface RebuildReport {
  nfos_scanned: number;
  works_created: number;
  works_merged: number;
  tags_extracted: number;
  sets_extracted: number;
  actors_extracted: number;
  file_versions_created: number;
  errors: { nfo_path: string; message: string }[];
}

// inside the api object
previewRebuild(sourceRoots: string[]) {
  return command<RebuildReport>("preview_rebuild", { sourceRoots });
},
rebuildLibraryFromNfo(sourceRoots: string[]) {
  return command<RebuildReport>("rebuild_library_from_nfo", { sourceRoots });
},
```

- [ ] Step 4: Add minimal settings buttons and report rendering

```tsx
const [rebuildReport, setRebuildReport] = useState<RebuildReport | null>(null);
const [rebuildMode, setRebuildMode] = useState<"preview" | "rebuild">("preview");

async function previewRebuild() {
  try {
    const report = await api.previewRebuild(parsedSourceRoots());
    setRebuildMode("preview");
    setRebuildReport(report);
    setStatus(`预览完成：${report.nfos_scanned} 个 NFO，${report.works_created} 个作品，${report.works_merged} 个合并组。`);
  } catch (error) {
    setStatus(`预览重建失败：${String(error)}`);
  }
}

async function runRebuildLibrary() {
  if (!window.confirm("重建将清空现有作品数据，从所有来源目录的 NFO 重新采集。配置和历史日志保留。确认继续？")) {
    return;
  }
  try {
    const report = await api.rebuildLibraryFromNfo(parsedSourceRoots());
    setRebuildMode("rebuild");
    setRebuildReport(report);
    await refreshWorks();
    setStatus(`重建完成：${report.works_created} 个作品，${report.errors.length} 个错误。`);
  } catch (error) {
    setStatus(`重建失败：${String(error)}`);
  }
}
```

Render a rebuild-tools block inside the settings panel with previewRebuild / runRebuildLibrary buttons and a rebuild-report summary (nfos_scanned / works_created / works_merged / errors.length). Add a small .rebuild-tools / .rebuild-report rule in styles.css.

- [ ] Step 5: Run frontend tests and build

Run: npm test
Expected: PASS.

Run: npm run build
Expected: PASS.

- [ ] Step 6: Commit

```bash
git add src/api.ts src/App.tsx src/styles.css src/viewModel.ts src/viewModel.test.ts
git commit -m "feat(ui): add settings preview and rebuild controls"
```

### Task 7: Final verification sweep

Files:
- Modify: none unless a test exposes a bug
- Test: src-tauri/tests/nfo_parser.rs
- Test: src-tauri/tests/library_rebuild.rs

- [ ] Step 1: Run the focused backend test suite

Run: cargo test --manifest-path src-tauri/Cargo.toml --test nfo_parser --test library_rebuild -q
Expected: PASS.

- [ ] Step 2: Run the full backend test suite

Run: cargo test --manifest-path src-tauri/Cargo.toml -q
Expected: PASS for the full Rust suite, including prior archive/ingest coverage.

- [ ] Step 3: Run frontend verification for the thin settings trigger

Run: npm test
Run: npm run build
Expected: PASS for both.

- [ ] Step 4: Do a dry-run on the real library roots

Run: cargo run --manifest-path src-tauri/Cargo.toml --example preview_rebuild -- G:\source_code\media-manager\.tmp-preview\library.sqlite H:\AV H:\CineMingle-1.3.0\JAV_output
Expected: Non-zero nfos_scanned, merged works for multi-CD sets, nonstandard source_code values present in the report, and no mutation of the temp DB unless explicitly rebuilt.

- [ ] Step 5: Commit any final fixes

```bash
git add -A
git commit -m "test: verify nfo rebuild pipeline end to end"
```

---

## Self-Review Notes

### Spec coverage

- Schema normalization (tags/sets/labels/studios/work_ratings): Task 2
- CDATA/runtime/rating fixes: Task 1
- Dual-source scan + no-video NFO ingestion + multi-CD merge + transaction rollback: Task 3
- Query APIs and AND filters: Task 4
- preview/rebuild example + command: Task 5
- Thin settings trigger for preview/rebuild (UI button decided as part of the dry-run both decision): Task 6
- Backend-first delivery: Tasks 1-5 are backend; Task 6 is the minimal UI shell, not the library redesign

### Placeholder scan

- No TODO, TBD, or "appropriate error handling" placeholders remain in actionable steps.
- Every test step has an actual test function and exact command.
- Every implementation step points to exact files and concrete types/functions; helper bodies that are large (parse_nfo_document full body, repository SQL bodies) are described with concrete signatures plus the rules they must implement, because the existing repo functions they extend are already present and the focus is on the new contract, not rewriting existing SQL line by line.

### Type consistency

- source_code is the merge key everywhere (Task 3 grouping, Task 2 upsert_work conflict target, Task 4/5 commands).
- CodeKind::Standard | Nonstandard is used consistently across domain, storage, commands, and tests.
- RebuildReport field names match across library_rebuild.rs, storage.rs, commands.rs, examples/preview_rebuild.rs, and src/api.ts.
- Tag, WorkSet, Label, Studio, WorkRating, WorkDetail, DimensionCount, WorkFilters names are consistent across all tasks.
- has_video is a bool on Work (Task 2) and an INTEGER column (default 1) in SQLite (Task 2 migrate); rebuild sets it per group (Task 3).

### Deferred (next spec)

- Library view redesign: card grid, collapsible dimension panel, multi-dimension AND filter UI, no-video visual differentiation, runtime/year/rating range filters. The Task 4 query APIs are designed to support all of this without change.
- Incremental scan, archive move fixes, pagination.
