use crate::domain::{
    ArchiveActionLog, CodeConflictEvidence, FileVersion, IngestDecision, IngestItem,
    IngestItemFilters, IngestJobSummary, ProviderMetadata, ReviewReason, WatchStatus, Work,
};
use crate::archive::normalized_file_name;
use crate::identifier::normalize_code;
use crate::ingest::IngestEngine;
use crate::provider::MetadataProvider;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub struct Repository {
    conn: Connection,
}

impl Repository {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Ok(Self { conn })
    }

    pub fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS works (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                normalized_code TEXT NOT NULL UNIQUE,
                title_zh TEXT,
                original_title TEXT,
                aliases_json TEXT NOT NULL DEFAULT '[]',
                summary TEXT,
                cover_path TEXT,
                tags_json TEXT NOT NULL DEFAULT '[]',
                lists_json TEXT NOT NULL DEFAULT '[]',
                rating INTEGER,
                watch_status TEXT NOT NULL DEFAULT 'Unwatched',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS file_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                work_id INTEGER REFERENCES works(id) ON DELETE SET NULL,
                source_root TEXT NOT NULL,
                original_path TEXT NOT NULL UNIQUE,
                archived_path TEXT,
                original_file_name TEXT NOT NULL,
                normalized_file_name TEXT,
                size_bytes INTEGER NOT NULL,
                duration_seconds INTEGER,
                width INTEGER,
                height INTEGER,
                codec TEXT,
                file_hash TEXT
            );

            CREATE TABLE IF NOT EXISTS ingest_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                status TEXT NOT NULL,
                source_roots_json TEXT NOT NULL,
                total_items INTEGER NOT NULL DEFAULT 0,
                auto_count INTEGER NOT NULL DEFAULT 0,
                review_count INTEGER NOT NULL DEFAULT 0,
                failed_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS ingest_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                job_id INTEGER REFERENCES ingest_jobs(id) ON DELETE CASCADE,
                source_root TEXT NOT NULL,
                path TEXT NOT NULL,
                file_name TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                duration_seconds INTEGER,
                width INTEGER,
                height INTEGER,
                codec TEXT,
                normalized_code TEXT,
                confidence REAL NOT NULL,
                decision TEXT NOT NULL,
                review_reasons_json TEXT NOT NULL DEFAULT '[]',
                code_conflict_json TEXT,
                metadata_json TEXT,
                candidate_work_id INTEGER REFERENCES works(id) ON DELETE SET NULL,
                file_hash TEXT
            );

            CREATE TABLE IF NOT EXISTS archive_action_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                item_id INTEGER REFERENCES ingest_items(id) ON DELETE SET NULL,
                from_path TEXT NOT NULL,
                to_path TEXT NOT NULL,
                status TEXT NOT NULL,
                message TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            ",
        )?;
        self.ensure_column("ingest_items", "duration_seconds", "INTEGER")?;
        self.ensure_column("ingest_items", "width", "INTEGER")?;
        self.ensure_column("ingest_items", "height", "INTEGER")?;
        self.ensure_column("ingest_items", "codec", "TEXT")?;
        self.ensure_column("ingest_items", "file_hash", "TEXT")?;
        self.ensure_column("ingest_items", "code_conflict_json", "TEXT")?;
        self.ensure_column("file_versions", "duration_seconds", "INTEGER")?;
        self.ensure_column("file_versions", "width", "INTEGER")?;
        self.ensure_column("file_versions", "height", "INTEGER")?;
        self.ensure_column("file_versions", "codec", "TEXT")?;
        self.ensure_column("file_versions", "file_hash", "TEXT")?;
        self.ensure_column("archive_action_logs", "job_id", "INTEGER")?;
        Ok(())
    }

    pub fn set_source_roots(&self, paths: &[PathBuf]) -> Result<()> {
        self.set_setting("source_roots", &serde_json::to_string(&source_roots_as_strings(paths))?)
    }

    pub fn get_source_roots(&self) -> Result<Vec<PathBuf>> {
        let Some(value) = self.get_setting("source_roots")? else {
            return Ok(Vec::new());
        };
        let paths: Vec<String> = serde_json::from_str(&value).unwrap_or_default();
        Ok(paths.into_iter().map(PathBuf::from).collect())
    }

    pub fn set_archive_root(&self, path: &Path) -> Result<()> {
        self.set_setting("archive_root", &path.to_string_lossy())
    }

    pub fn get_archive_root(&self) -> Result<Option<PathBuf>> {
        Ok(self.get_setting("archive_root")?.map(PathBuf::from))
    }

    pub fn set_metadata_provider_enabled(&self, enabled: bool) -> Result<()> {
        self.set_setting("metadata_provider_enabled", if enabled { "true" } else { "false" })
    }

    pub fn get_metadata_provider_enabled(&self) -> Result<bool> {
        Ok(matches!(
            self.get_setting("metadata_provider_enabled")?.as_deref(),
            Some("true")
        ))
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "
            INSERT INTO app_settings (key, value)
            VALUES (?1, ?2)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![key, value],
        )?;
        Ok(())
    }

    fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut statement = self
            .conn
            .prepare("SELECT value FROM app_settings WHERE key = ?1")?;
        let mut rows = statement.query(params![key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(row.get(0)?))
    }

    fn ensure_column(&self, table: &str, column: &str, definition: &str) -> Result<()> {
        let mut statement = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
        for row in rows {
            if row? == column {
                return Ok(());
            }
        }
        self.conn
            .execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"), [])?;
        Ok(())
    }

    pub fn upsert_work(&self, work: &Work) -> Result<i64> {
        self.conn.execute(
            "
            INSERT INTO works (
                normalized_code,
                title_zh,
                original_title,
                aliases_json,
                summary,
                cover_path,
                tags_json,
                lists_json,
                rating,
                watch_status
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(normalized_code) DO UPDATE SET
                title_zh = COALESCE(works.title_zh, excluded.title_zh),
                original_title = COALESCE(works.original_title, excluded.original_title),
                aliases_json = CASE
                    WHEN works.aliases_json = '[]' THEN excluded.aliases_json
                    ELSE works.aliases_json
                END,
                summary = COALESCE(works.summary, excluded.summary),
                cover_path = COALESCE(works.cover_path, excluded.cover_path),
                tags_json = works.tags_json,
                lists_json = works.lists_json,
                rating = works.rating,
                watch_status = works.watch_status,
                updated_at = CURRENT_TIMESTAMP
            ",
            params![
                work.normalized_code,
                work.title_zh,
                work.original_title,
                serde_json::to_string(&work.aliases)?,
                work.summary,
                work.cover_path.as_ref().map(|path| path.to_string_lossy().to_string()),
                serde_json::to_string(&work.tags)?,
                serde_json::to_string(&work.lists)?,
                work.rating,
                format!("{:?}", work.watch_status),
            ],
        )?;

        let id = self.conn.query_row(
            "SELECT id FROM works WHERE normalized_code = ?1",
            params![work.normalized_code],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    pub fn get_work_by_code(&self, normalized_code: &str) -> Result<Option<Work>> {
        let mut statement = self.conn.prepare(
            "
            SELECT id, normalized_code, title_zh, original_title, aliases_json, summary,
                   cover_path, tags_json, lists_json, rating, watch_status
            FROM works
            WHERE normalized_code = ?1
            ",
        )?;
        let mut rows = statement.query(params![normalized_code])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };

        let aliases_json: String = row.get(4)?;
        let tags_json: String = row.get(7)?;
        let lists_json: String = row.get(8)?;
        let cover_path: Option<String> = row.get(6)?;
        let status: String = row.get(10)?;

        Ok(Some(Work {
            id: row.get(0)?,
            normalized_code: row.get(1)?,
            title_zh: row.get(2)?,
            original_title: row.get(3)?,
            aliases: serde_json::from_str(&aliases_json)?,
            summary: row.get(5)?,
            cover_path: cover_path.map(Into::into),
            tags: serde_json::from_str(&tags_json)?,
            lists: serde_json::from_str(&lists_json)?,
            rating: row.get::<_, Option<u8>>(9)?,
            watch_status: parse_watch_status(&status),
        }))
    }

    pub fn list_works(&self) -> Result<Vec<Work>> {
        let mut statement = self.conn.prepare(
            "
            SELECT id, normalized_code, title_zh, original_title, aliases_json, summary,
                   cover_path, tags_json, lists_json, rating, watch_status
            FROM works
            ORDER BY normalized_code ASC
            ",
        )?;
        let rows = statement.query_map([], |row| {
            let aliases_json: String = row.get(4)?;
            let tags_json: String = row.get(7)?;
            let lists_json: String = row.get(8)?;
            let cover_path: Option<String> = row.get(6)?;
            let status: String = row.get(10)?;
            Ok(Work {
                id: row.get(0)?,
                normalized_code: row.get(1)?,
                title_zh: row.get(2)?,
                original_title: row.get(3)?,
                aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
                summary: row.get(5)?,
                cover_path: cover_path.map(Into::into),
                tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                lists: serde_json::from_str(&lists_json).unwrap_or_default(),
                rating: row.get::<_, Option<u8>>(9)?,
                watch_status: parse_watch_status(&status),
            })
        })?;

        let mut works = Vec::new();
        for row in rows {
            works.push(row?);
        }
        Ok(works)
    }

    pub fn update_work_profile(
        &self,
        work_id: i64,
        tags: Vec<String>,
        lists: Vec<String>,
        rating: Option<u8>,
        watch_status: WatchStatus,
    ) -> Result<Work> {
        if let Some(value) = rating {
            if value > 10 {
                return Err(anyhow::anyhow!("rating must be between 0 and 10"));
            }
        }
        self.conn.execute(
            "
            UPDATE works
            SET tags_json = ?1,
                lists_json = ?2,
                rating = ?3,
                watch_status = ?4,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?5
            ",
            params![
                serde_json::to_string(&tags)?,
                serde_json::to_string(&lists)?,
                rating,
                format!("{:?}", watch_status),
                work_id,
            ],
        )?;
        if self.conn.changes() == 0 {
            return Err(anyhow::anyhow!("work {work_id} was not found"));
        }
        self.get_work_by_id(work_id)?
            .ok_or_else(|| anyhow::anyhow!("work {work_id} was not found"))
    }

    pub fn create_ingest_job(&self, source_roots: &[PathBuf], items: &[IngestItem]) -> Result<i64> {
        let summary = summarize_items(0, items);
        self.conn.execute(
            "
            INSERT INTO ingest_jobs (
                status,
                source_roots_json,
                total_items,
                auto_count,
                review_count,
                failed_count
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ",
            params![
                "completed",
                serde_json::to_string(&source_roots_as_strings(source_roots))?,
                summary.total_items as i64,
                summary.auto_count as i64,
                summary.review_count as i64,
                summary.failed_count as i64,
            ],
        )?;
        let job_id = self.conn.last_insert_rowid();
        self.replace_ingest_items(job_id, items)?;
        self.auto_promote_auto_archive_items(job_id)?;
        Ok(job_id)
    }

    pub fn get_ingest_job(&self, job_id: i64) -> Result<Option<IngestJobSummary>> {
        let mut statement = self.conn.prepare(
            "
            SELECT id, status, total_items, auto_count, review_count, failed_count
            FROM ingest_jobs
            WHERE id = ?1
            ",
        )?;
        let mut rows = statement.query(params![job_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };

        Ok(Some(IngestJobSummary {
            id: row.get(0)?,
            status: row.get(1)?,
            total_items: row.get::<_, i64>(2)? as usize,
            auto_count: row.get::<_, i64>(3)? as usize,
            review_count: row.get::<_, i64>(4)? as usize,
            failed_count: row.get::<_, i64>(5)? as usize,
        }))
    }

    pub fn get_latest_ingest_job(&self) -> Result<Option<IngestJobSummary>> {
        if let Some(job) = self.query_latest_ingest_job(
            "
            SELECT id, status, total_items, auto_count, review_count, failed_count
            FROM ingest_jobs
            WHERE total_items > 0
            ORDER BY id DESC
            LIMIT 1
            ",
        )? {
            return Ok(Some(job));
        }
        self.query_latest_ingest_job(
            "
            SELECT id, status, total_items, auto_count, review_count, failed_count
            FROM ingest_jobs
            ORDER BY id DESC
            LIMIT 1
            ",
        )
    }

    fn query_latest_ingest_job(&self, sql: &str) -> Result<Option<IngestJobSummary>> {
        let mut statement = self.conn.prepare(sql)?;
        let mut rows = statement.query([])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(IngestJobSummary {
            id: row.get(0)?,
            status: row.get(1)?,
            total_items: row.get::<_, i64>(2)? as usize,
            auto_count: row.get::<_, i64>(3)? as usize,
            review_count: row.get::<_, i64>(4)? as usize,
            failed_count: row.get::<_, i64>(5)? as usize,
        }))
    }

    pub fn list_ingest_items(&self, job_id: i64) -> Result<Vec<IngestItem>> {
        let mut statement = self.conn.prepare(
            "
            SELECT id, job_id, source_root, path, file_name, size_bytes, duration_seconds,
                   width, height, codec, normalized_code, confidence, decision,
                   review_reasons_json, code_conflict_json, metadata_json, candidate_work_id,
                   file_hash
            FROM ingest_items
            WHERE job_id = ?1
            ORDER BY id ASC
            ",
        )?;
        let rows = statement.query_map(params![job_id], ingest_item_from_row)?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    pub fn list_ingest_items_filtered(
        &self,
        job_id: i64,
        filters: &IngestItemFilters,
    ) -> Result<Vec<IngestItem>> {
        Ok(self
            .list_ingest_items(job_id)?
            .into_iter()
            .filter(|item| ingest_item_matches_filters(item, filters))
            .collect())
    }

    pub fn list_ingest_items_by_ids(&self, item_ids: &[i64]) -> Result<Vec<IngestItem>> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut statement = self.conn.prepare(
            "
            SELECT id, job_id, source_root, path, file_name, size_bytes, duration_seconds,
                   width, height, codec, normalized_code, confidence, decision,
                   review_reasons_json, code_conflict_json, metadata_json, candidate_work_id,
                   file_hash
            FROM ingest_items
            WHERE id = ?1
            ",
        )?;
        let mut items = Vec::new();
        for item_id in item_ids {
            let mut rows = statement.query(params![item_id])?;
            if let Some(row) = rows.next()? {
                items.push(ingest_item_from_row(row)?);
            }
        }
        Ok(items)
    }

    pub fn list_archive_candidate_items_for_job(&self, job_id: i64) -> Result<Vec<IngestItem>> {
        Ok(self
            .list_ingest_items(job_id)?
            .into_iter()
            .filter(|item| self.is_archive_candidate_item(item).unwrap_or(false))
            .collect())
    }

    pub fn list_archive_candidate_items_by_ids(&self, item_ids: &[i64]) -> Result<Vec<IngestItem>> {
        Ok(self
            .list_ingest_items_by_ids(item_ids)?
            .into_iter()
            .filter(|item| self.is_archive_candidate_item(item).unwrap_or(false))
            .collect())
    }

    pub fn resolve_ingest_item(
        &self,
        item_id: i64,
        normalized_code: Option<String>,
    ) -> Result<i64> {
        let mut item = self.get_ingest_item(item_id)?;
        if let Some(code) = normalized_code {
            item.normalized_code = normalize_code(&code);
        }
        let normalized_code = item
            .normalized_code
            .clone()
            .ok_or_else(|| anyhow::anyhow!("ingest item {item_id} has no normalized code"))?;
        let work = work_from_ingest_item(&item, &normalized_code);
        let work_id = self.upsert_work(&work)?;
        self.promote_ingest_item_to_work(item_id, item, work_id, normalized_code, true)
    }

    pub fn resolve_ingest_item_to_work(&self, item_id: i64, work_id: i64) -> Result<i64> {
        let item = self.get_ingest_item(item_id)?;
        let work = self
            .get_work_by_id(work_id)?
            .ok_or_else(|| anyhow::anyhow!("work {work_id} was not found"))?;
        self.promote_ingest_item_to_work(item_id, item, work_id, work.normalized_code, true)
    }

    fn auto_promote_auto_archive_items(&self, job_id: i64) -> Result<()> {
        let items = self.list_ingest_items(job_id)?;
        for item in items {
            if item.decision != IngestDecision::AutoArchive || item.candidate_work_id.is_some() {
                continue;
            }
            let Some(normalized_code) = item.normalized_code.clone() else {
                continue;
            };
            let Some(item_id) = item.id else {
                continue;
            };
            let work = work_from_ingest_item(&item, &normalized_code);
            let work_id = self.upsert_work(&work)?;
            self.promote_ingest_item_to_work(item_id, item, work_id, normalized_code, false)?;
        }
        self.refresh_ingest_job_counts(job_id)?;
        Ok(())
    }

    pub fn retry_metadata_for_items<P: MetadataProvider>(
        &self,
        engine: &IngestEngine<P>,
        item_ids: &[i64],
    ) -> Result<Vec<IngestItem>> {
        let mut updated = Vec::new();
        let mut job_ids = BTreeSet::new();
        for item_id in item_ids {
            let mut item = self.get_ingest_item(*item_id)?;
            item.review_reasons
                .retain(|reason| reason != &ReviewReason::ProviderFailed);
            let decided = engine.decide(item);
            self.update_ingest_item_decision(&decided)?;
            let mut returned_item = decided.clone();
            if decided.decision == IngestDecision::AutoArchive {
                if let Some(job_id) = decided.job_id {
                    self.auto_promote_auto_archive_items(job_id)?;
                    returned_item = self.get_ingest_item(*item_id)?;
                }
            }
            if let Some(job_id) = decided.job_id {
                job_ids.insert(job_id);
            }
            updated.push(returned_item);
        }
        for job_id in job_ids {
            self.refresh_ingest_job_counts(job_id)?;
        }
        Ok(updated)
    }

    pub fn revalidate_move_failed_items(&self, item_ids: &[i64]) -> Result<Vec<IngestItem>> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut updated = Vec::new();
        let mut job_ids = BTreeSet::new();
        for item_id in item_ids {
            let mut item = self.get_ingest_item(*item_id)?;
            if !item.review_reasons.contains(&ReviewReason::MoveFailed) || !item.path.exists() {
                continue;
            }
            item.review_reasons
                .retain(|reason| reason != &ReviewReason::MoveFailed);
            item.decision = if item.normalized_code.is_some() && item.candidate_work_id.is_some() {
                IngestDecision::AutoArchive
            } else {
                IngestDecision::NeedsReview
            };
            self.update_ingest_item_decision(&item)?;
            if let Some(job_id) = item.job_id {
                job_ids.insert(job_id);
            }
            updated.push(self.get_ingest_item(*item_id)?);
        }

        for job_id in job_ids {
            self.refresh_ingest_job_counts(job_id)?;
        }
        Ok(updated)
    }

    pub fn ignore_duplicate_items(&self, item_ids: &[i64]) -> Result<Vec<IngestItem>> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut updated = Vec::new();
        let mut job_ids = BTreeSet::new();
        for item_id in item_ids {
            let mut item = self.get_ingest_item(*item_id)?;
            if item.decision != IngestDecision::DuplicateCandidate
                || !item.review_reasons.contains(&ReviewReason::DuplicateFile)
            {
                continue;
            }
            item.decision = IngestDecision::Ignored;
            item.review_reasons.clear();
            self.update_ingest_item_decision(&item)?;
            if let Some(job_id) = item.job_id {
                job_ids.insert(job_id);
            }
            updated.push(self.get_ingest_item(*item_id)?);
        }

        for job_id in job_ids {
            self.refresh_ingest_job_counts(job_id)?;
        }
        Ok(updated)
    }

    /// Physically deletes the source file of each item and marks the item Ignored.
    ///
    /// Safety contract: only DuplicateCandidate and Ignored items may be deleted
    /// (scraper junk such as trailers / theme / ad clips), never AutoArchive,
    /// NeedsReview or Failed items which may hold real media or unresolved work.
    /// A missing file is tolerated (already gone) but the item is still marked Ignored.
    pub fn delete_items(&self, item_ids: &[i64]) -> Result<Vec<IngestItem>> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut updated = Vec::new();
        let mut job_ids = BTreeSet::new();
        for item_id in item_ids {
            let item = self.get_ingest_item(*item_id)?;
            if !matches!(
                item.decision,
                IngestDecision::DuplicateCandidate | IngestDecision::Ignored
            ) {
                anyhow::bail!(
                    "item {} is not deletable (decision {:?}); only duplicates and ignored items can be deleted",
                    item_id,
                    item.decision
                );
            }

            // Best-effort physical delete; a missing file is not an error.
            if item.path.exists() {
                std::fs::remove_file(&item.path).map_err(|e| {
                    anyhow::anyhow!("failed to delete {}: {}", item.path.display(), e)
                })?;
            }

            let mut item = item;
            item.decision = IngestDecision::Ignored;
            item.review_reasons.clear();
            self.update_ingest_item_decision(&item)?;
            if let Some(job_id) = item.job_id {
                job_ids.insert(job_id);
            }
            updated.push(self.get_ingest_item(*item_id)?);
        }

        for job_id in job_ids {
            self.refresh_ingest_job_counts(job_id)?;
        }
        Ok(updated)
    }

    fn promote_ingest_item_to_work(
        &self,
        item_id: i64,
        mut item: IngestItem,
        work_id: i64,
        normalized_code: String,
        confirmed: bool,
    ) -> Result<i64> {
        item.normalized_code = Some(normalized_code.clone());
        item.candidate_work_id = Some(work_id);
        if confirmed {
            item.confidence = 1.0;
        }
        item.decision = IngestDecision::AutoArchive;
        if confirmed {
            item.review_reasons.retain(|reason| {
                !matches!(
                    reason,
                    ReviewReason::MissingCode | ReviewReason::LowConfidence | ReviewReason::CodeConflict
                )
            });
            item.code_conflict = None;
        }

        self.conn.execute(
            "
            UPDATE ingest_items
            SET normalized_code = ?1,
                confidence = ?2,
                decision = ?3,
                review_reasons_json = ?4,
                code_conflict_json = ?5,
                candidate_work_id = ?6
            WHERE id = ?7
            ",
            params![
                item.normalized_code,
                item.confidence as f64,
                format!("{:?}", item.decision),
                serde_json::to_string(&item.review_reasons)?,
                item.code_conflict
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?,
                item.candidate_work_id,
                item_id,
            ],
        )?;
        self.upsert_file_version(work_id, &normalized_code, &item)?;
        if let Some(job_id) = item.job_id {
            self.refresh_ingest_job_counts(job_id)?;
        }
        Ok(work_id)
    }

    fn get_work_by_id(&self, work_id: i64) -> Result<Option<Work>> {
        let mut statement = self.conn.prepare(
            "
            SELECT id, normalized_code, title_zh, original_title, aliases_json, summary,
                   cover_path, tags_json, lists_json, rating, watch_status
            FROM works
            WHERE id = ?1
            ",
        )?;
        let mut rows = statement.query(params![work_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let aliases_json: String = row.get(4)?;
        let tags_json: String = row.get(7)?;
        let lists_json: String = row.get(8)?;
        let cover_path: Option<String> = row.get(6)?;
        let status: String = row.get(10)?;

        Ok(Some(Work {
            id: row.get(0)?,
            normalized_code: row.get(1)?,
            title_zh: row.get(2)?,
            original_title: row.get(3)?,
            aliases: serde_json::from_str(&aliases_json)?,
            summary: row.get(5)?,
            cover_path: cover_path.map(Into::into),
            tags: serde_json::from_str(&tags_json)?,
            lists: serde_json::from_str(&lists_json)?,
            rating: row.get::<_, Option<u8>>(9)?,
            watch_status: parse_watch_status(&status),
        }))
    }

    pub fn list_file_versions_for_work(&self, work_id: i64) -> Result<Vec<FileVersion>> {
        let mut statement = self.conn.prepare(
            "
            SELECT id, work_id, source_root, original_path, archived_path, original_file_name,
                   normalized_file_name, size_bytes, duration_seconds, width, height, codec,
                   file_hash
            FROM file_versions
            WHERE work_id = ?1
            ORDER BY id ASC
            ",
        )?;
        let rows = statement.query_map(params![work_id], |row| {
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
                duration_seconds: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
                width: row.get::<_, Option<i64>>(9)?.map(|value| value as u32),
                height: row.get::<_, Option<i64>>(10)?.map(|value| value as u32),
                codec: row.get(11)?,
                file_hash: row.get(12)?,
            })
        })?;

        let mut versions = Vec::new();
        for row in rows {
            versions.push(row?);
        }
        Ok(versions)
    }

    pub fn merge_file_versions_into_work(
        &self,
        work_id: i64,
        file_version_ids: &[i64],
    ) -> Result<Vec<FileVersion>> {
        if file_version_ids.is_empty() {
            return Err(anyhow::anyhow!("file_version_ids cannot be empty"));
        }
        let work = self
            .get_work_by_id(work_id)?
            .ok_or_else(|| anyhow::anyhow!("work {work_id} was not found"))?;
        let moved_ids: BTreeSet<i64> = file_version_ids.iter().copied().collect();
        let existing_target_count = self
            .list_file_versions_for_work(work_id)?
            .into_iter()
            .filter(|version| {
                version
                    .id
                    .map(|id| !moved_ids.contains(&id))
                    .unwrap_or(true)
            })
            .count();

        for (offset, version_id) in file_version_ids.iter().enumerate() {
            let version = self
                .get_file_version_by_id(*version_id)?
                .ok_or_else(|| anyhow::anyhow!("file version {version_id} was not found"))?;
            let normalized_file_name = normalized_file_name(
                &work.normalized_code,
                &version.original_path,
                existing_target_count + offset + 1,
            );
            self.conn.execute(
                "
                UPDATE file_versions
                SET work_id = ?1,
                    normalized_file_name = ?2
                WHERE id = ?3
                ",
                params![work_id, normalized_file_name, version_id],
            )?;
        }

        self.list_file_versions_for_work(work_id)
    }

    fn get_file_version_by_id(&self, version_id: i64) -> Result<Option<FileVersion>> {
        let mut statement = self.conn.prepare(
            "
            SELECT id, work_id, source_root, original_path, archived_path, original_file_name,
                   normalized_file_name, size_bytes, duration_seconds, width, height, codec,
                   file_hash
            FROM file_versions
            WHERE id = ?1
            ",
        )?;
        let mut rows = statement.query(params![version_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let source_root: String = row.get(2)?;
        let original_path: String = row.get(3)?;
        let archived_path: Option<String> = row.get(4)?;
        Ok(Some(FileVersion {
            id: row.get(0)?,
            work_id: row.get(1)?,
            source_root: PathBuf::from(source_root),
            original_path: PathBuf::from(original_path),
            archived_path: archived_path.map(PathBuf::from),
            original_file_name: row.get(5)?,
            normalized_file_name: row.get(6)?,
            size_bytes: row.get::<_, i64>(7)? as u64,
            duration_seconds: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
            width: row.get::<_, Option<i64>>(9)?.map(|value| value as u32),
            height: row.get::<_, Option<i64>>(10)?.map(|value| value as u32),
            codec: row.get(11)?,
            file_hash: row.get(12)?,
        }))
    }

    pub fn record_archive_action(&self, log: &ArchiveActionLog) -> Result<i64> {
        let job_id = match (log.job_id, log.item_id) {
            (Some(job_id), _) => Some(job_id),
            (None, Some(item_id)) => self.get_ingest_item_job_id(item_id)?,
            (None, None) => None,
        };
        self.conn.execute(
            "
            INSERT INTO archive_action_logs (
                item_id,
                job_id,
                from_path,
                to_path,
                status,
                message
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ",
            params![
                log.item_id,
                job_id,
                log.from_path.to_string_lossy().to_string(),
                log.to_path.to_string_lossy().to_string(),
                log.status,
                log.message,
            ],
        )?;
        let log_id = self.conn.last_insert_rowid();
        if log.status == "moved" {
            self.conn.execute(
                "
                UPDATE file_versions
                SET archived_path = ?1
                WHERE original_path = ?2
                ",
                params![
                    log.to_path.to_string_lossy().to_string(),
                    log.from_path.to_string_lossy().to_string(),
                ],
            )?;
        } else if log.status == "failed" {
            if let Some(item_id) = log.item_id {
                self.mark_ingest_item_move_failed(item_id)?;
            }
        }
        Ok(log_id)
    }

    pub fn get_archive_action_log(&self, log_id: i64) -> Result<Option<ArchiveActionLog>> {
        let mut statement = self.conn.prepare(
            "
            SELECT id, item_id, job_id, from_path, to_path, status, message, created_at
            FROM archive_action_logs
            WHERE id = ?1
            ",
        )?;
        let mut rows = statement.query(params![log_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        archive_action_log_from_row(row).map(Some).map_err(Into::into)
    }

    pub fn list_archive_action_logs(&self) -> Result<Vec<ArchiveActionLog>> {
        let mut statement = self.conn.prepare(
            "
            SELECT id, item_id, job_id, from_path, to_path, status, message, created_at
            FROM archive_action_logs
            ORDER BY id ASC
            ",
        )?;
        let rows = statement.query_map([], archive_action_log_from_row)?;

        let mut logs = Vec::new();
        for row in rows {
            logs.push(row?);
        }
        Ok(logs)
    }

    fn replace_ingest_items(&self, job_id: i64, items: &[IngestItem]) -> Result<()> {
        self.conn.execute(
            "DELETE FROM ingest_items WHERE job_id = ?1",
            params![job_id],
        )?;
        for item in items {
            self.conn.execute(
                "
                INSERT INTO ingest_items (
                    job_id,
                    source_root,
                    path,
                    file_name,
                    size_bytes,
                    duration_seconds,
                    width,
                    height,
                    codec,
                    normalized_code,
                    confidence,
                    decision,
                    review_reasons_json,
                    code_conflict_json,
                    metadata_json,
                    candidate_work_id,
                    file_hash
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
                ",
                params![
                    job_id,
                    item.source_root.to_string_lossy().to_string(),
                    item.path.to_string_lossy().to_string(),
                    item.file_name,
                    item.size_bytes as i64,
                    item.duration_seconds.map(|value| value as i64),
                    item.width.map(|value| value as i64),
                    item.height.map(|value| value as i64),
                    item.codec,
                    item.normalized_code,
                    item.confidence as f64,
                    format!("{:?}", item.decision),
                    serde_json::to_string(&item.review_reasons)?,
                    item.code_conflict
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()?,
                    item.metadata
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()?,
                    item.candidate_work_id,
                    item.file_hash,
                ],
            )?;
        }
        Ok(())
    }

    fn get_ingest_item(&self, item_id: i64) -> Result<IngestItem> {
        let mut statement = self.conn.prepare(
            "
            SELECT id, job_id, source_root, path, file_name, size_bytes, duration_seconds,
                   width, height, codec, normalized_code, confidence, decision,
                   review_reasons_json, code_conflict_json, metadata_json, candidate_work_id,
                   file_hash
            FROM ingest_items
            WHERE id = ?1
            ",
        )?;
        let mut rows = statement.query(params![item_id])?;
        let Some(row) = rows.next()? else {
            return Err(anyhow::anyhow!("ingest item {item_id} was not found"));
        };
        let decision: String = row.get(12)?;
        let review_reasons_json: String = row.get(13)?;
        let code_conflict_json: Option<String> = row.get(14)?;
        let metadata_json: Option<String> = row.get(15)?;
        let source_root: String = row.get(2)?;
        let path: String = row.get(3)?;
        Ok(IngestItem {
            id: row.get(0)?,
            job_id: row.get(1)?,
            source_root: PathBuf::from(source_root),
            path: PathBuf::from(path),
            file_name: row.get(4)?,
            size_bytes: row.get::<_, i64>(5)? as u64,
            duration_seconds: row.get::<_, Option<i64>>(6)?.map(|value| value as u64),
            width: row.get::<_, Option<i64>>(7)?.map(|value| value as u32),
            height: row.get::<_, Option<i64>>(8)?.map(|value| value as u32),
            codec: row.get(9)?,
            normalized_code: row.get(10)?,
            confidence: row.get::<_, f64>(11)? as f32,
            decision: parse_ingest_decision(&decision),
            review_reasons: serde_json::from_str::<Vec<ReviewReason>>(&review_reasons_json)
                .unwrap_or_default(),
            code_conflict: code_conflict_json
                .and_then(|json| serde_json::from_str::<CodeConflictEvidence>(&json).ok()),
            metadata: metadata_json
                .and_then(|json| serde_json::from_str::<ProviderMetadata>(&json).ok()),
            candidate_work_id: row.get(16)?,
            file_hash: row.get(17)?,
        })
    }

    fn get_ingest_item_job_id(&self, item_id: i64) -> Result<Option<i64>> {
        let mut statement = self
            .conn
            .prepare("SELECT job_id FROM ingest_items WHERE id = ?1")?;
        let mut rows = statement.query(params![item_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(row.get(0)?)
    }

    fn is_archive_candidate_item(&self, item: &IngestItem) -> Result<bool> {
        if item.decision != IngestDecision::AutoArchive {
            return Ok(false);
        }
        let archived_path: Option<String> = self
            .conn
            .query_row(
                "
                SELECT archived_path
                FROM file_versions
                WHERE original_path = ?1
                ",
                params![item.path.to_string_lossy().to_string()],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        Ok(archived_path.is_none())
    }

    fn update_ingest_item_decision(&self, item: &IngestItem) -> Result<()> {
        let item_id = item
            .id
            .ok_or_else(|| anyhow::anyhow!("ingest item has no id"))?;
        self.conn.execute(
            "
            UPDATE ingest_items
            SET normalized_code = ?1,
                confidence = ?2,
                decision = ?3,
                review_reasons_json = ?4,
                code_conflict_json = ?5,
                metadata_json = ?6,
                candidate_work_id = ?7,
                file_hash = ?8
            WHERE id = ?9
            ",
            params![
                item.normalized_code,
                item.confidence as f64,
                format!("{:?}", item.decision),
                serde_json::to_string(&item.review_reasons)?,
                item.code_conflict
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?,
                item.metadata
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?,
                item.candidate_work_id,
                item.file_hash,
                item_id,
            ],
        )?;
        Ok(())
    }

    fn mark_ingest_item_move_failed(&self, item_id: i64) -> Result<()> {
        let mut item = self.get_ingest_item(item_id)?;
        item.decision = IngestDecision::Failed;
        if !item.review_reasons.contains(&ReviewReason::MoveFailed) {
            item.review_reasons.push(ReviewReason::MoveFailed);
        }
        self.update_ingest_item_decision(&item)?;
        if let Some(job_id) = item.job_id {
            self.refresh_ingest_job_counts(job_id)?;
        }
        Ok(())
    }

    fn upsert_file_version(
        &self,
        work_id: i64,
        normalized_code: &str,
        item: &IngestItem,
    ) -> Result<i64> {
        let existing_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM file_versions WHERE work_id = ?1 AND original_path != ?2",
            params![work_id, item.path.to_string_lossy().to_string()],
            |row| row.get(0),
        )?;
        let normalized_file_name =
            normalized_file_name(normalized_code, &item.path, existing_count as usize + 1);
        self.conn.execute(
            "
            INSERT INTO file_versions (
                work_id,
                source_root,
                original_path,
                original_file_name,
                normalized_file_name,
                size_bytes,
                duration_seconds,
                width,
                height,
                codec,
                file_hash
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ON CONFLICT(original_path) DO UPDATE SET
                work_id = excluded.work_id,
                normalized_file_name = excluded.normalized_file_name,
                size_bytes = excluded.size_bytes,
                duration_seconds = excluded.duration_seconds,
                width = excluded.width,
                height = excluded.height,
                codec = excluded.codec,
                file_hash = excluded.file_hash
            ",
            params![
                work_id,
                item.source_root.to_string_lossy().to_string(),
                item.path.to_string_lossy().to_string(),
                item.file_name,
                normalized_file_name,
                item.size_bytes as i64,
                item.duration_seconds.map(|value| value as i64),
                item.width.map(|value| value as i64),
                item.height.map(|value| value as i64),
                item.codec,
                item.file_hash,
            ],
        )?;
        let version_id = self.conn.query_row(
            "SELECT id FROM file_versions WHERE original_path = ?1",
            params![item.path.to_string_lossy().to_string()],
            |row| row.get(0),
        )?;
        Ok(version_id)
    }

    fn refresh_ingest_job_counts(&self, job_id: i64) -> Result<()> {
        let items = self.list_ingest_items(job_id)?;
        let summary = summarize_items(job_id, &items);
        self.conn.execute(
            "
            UPDATE ingest_jobs
            SET total_items = ?1,
                auto_count = ?2,
                review_count = ?3,
                failed_count = ?4,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?5
            ",
            params![
                summary.total_items as i64,
                summary.auto_count as i64,
                summary.review_count as i64,
                summary.failed_count as i64,
                job_id,
            ],
        )?;
        Ok(())
    }
}

fn parse_watch_status(value: &str) -> WatchStatus {
    match value {
        "Watched" => WatchStatus::Watched,
        "Favorite" => WatchStatus::Favorite,
        _ => WatchStatus::Unwatched,
    }
}

fn parse_ingest_decision(value: &str) -> IngestDecision {
    match value {
        "AutoArchive" => IngestDecision::AutoArchive,
        "DuplicateCandidate" => IngestDecision::DuplicateCandidate,
        "Failed" => IngestDecision::Failed,
        "Ignored" => IngestDecision::Ignored,
        _ => IngestDecision::NeedsReview,
    }
}

fn ingest_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<IngestItem> {
    let decision: String = row.get(12)?;
    let review_reasons_json: String = row.get(13)?;
    let code_conflict_json: Option<String> = row.get(14)?;
    let metadata_json: Option<String> = row.get(15)?;
    let source_root: String = row.get(2)?;
    let path: String = row.get(3)?;

    Ok(IngestItem {
        id: row.get(0)?,
        job_id: row.get(1)?,
        source_root: PathBuf::from(source_root),
        path: PathBuf::from(path),
        file_name: row.get(4)?,
        size_bytes: row.get::<_, i64>(5)? as u64,
        duration_seconds: row.get::<_, Option<i64>>(6)?.map(|value| value as u64),
        width: row.get::<_, Option<i64>>(7)?.map(|value| value as u32),
        height: row.get::<_, Option<i64>>(8)?.map(|value| value as u32),
        codec: row.get(9)?,
        normalized_code: row.get(10)?,
        confidence: row.get::<_, f64>(11)? as f32,
        decision: parse_ingest_decision(&decision),
        review_reasons: serde_json::from_str::<Vec<ReviewReason>>(&review_reasons_json)
            .unwrap_or_default(),
        code_conflict: code_conflict_json
            .and_then(|json| serde_json::from_str::<CodeConflictEvidence>(&json).ok()),
        metadata: metadata_json
            .and_then(|json| serde_json::from_str::<ProviderMetadata>(&json).ok()),
        candidate_work_id: row.get(16)?,
        file_hash: row.get(17)?,
    })
}

fn archive_action_log_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArchiveActionLog> {
    let from_path: String = row.get(3)?;
    let to_path: String = row.get(4)?;
    Ok(ArchiveActionLog {
        id: row.get(0)?,
        item_id: row.get(1)?,
        job_id: row.get(2)?,
        from_path: PathBuf::from(from_path),
        to_path: PathBuf::from(to_path),
        status: row.get(5)?,
        message: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn work_from_ingest_item(item: &IngestItem, normalized_code: &str) -> Work {
    Work {
        id: None,
        normalized_code: normalized_code.to_string(),
        title_zh: item.metadata.as_ref().and_then(|metadata| metadata.title_zh.clone()),
        original_title: item
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.original_title.clone()),
        aliases: item
            .metadata
            .as_ref()
            .map(|metadata| metadata.aliases.clone())
            .unwrap_or_default(),
        summary: item.metadata.as_ref().and_then(|metadata| metadata.summary.clone()),
        cover_path: item
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.cover_url.clone())
            .map(PathBuf::from),
        tags: vec![],
        lists: vec![],
        rating: None,
        watch_status: WatchStatus::Unwatched,
    }
}

fn summarize_items(job_id: i64, items: &[IngestItem]) -> IngestJobSummary {
    let total_items = items.len();
    let auto_count = items
        .iter()
        .filter(|item| item.decision == IngestDecision::AutoArchive)
        .count();
    let failed_count = items
        .iter()
        .filter(|item| item.decision == IngestDecision::Failed)
        .count();
    let review_count = items
        .iter()
        .filter(|item| {
            matches!(
                item.decision,
                IngestDecision::NeedsReview | IngestDecision::DuplicateCandidate
            )
        })
        .count();
    IngestJobSummary {
        id: job_id,
        status: "completed".to_string(),
        total_items,
        auto_count,
        review_count,
        failed_count,
    }
}

fn ingest_item_matches_filters(item: &IngestItem, filters: &IngestItemFilters) -> bool {
    if let Some(decision) = filters.decision.as_ref() {
        if &item.decision != decision {
            return false;
        }
    }
    if let Some(reason) = filters.review_reason.as_ref() {
        if !item.review_reasons.contains(reason) {
            return false;
        }
    }
    if let Some(has_code) = filters.has_code {
        if item.normalized_code.is_some() != has_code {
            return false;
        }
    }
    true
}

fn source_roots_as_strings(source_roots: &[PathBuf]) -> Vec<String> {
    source_roots
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect()
}
