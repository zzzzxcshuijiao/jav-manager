use crate::domain::{
    Actor, ArchiveActionLog, CodeConflictEvidence, FileVersion, IngestDecision, IngestItem,
    IngestItemFilters, IngestJobSummary, ProviderMetadata, ReviewReason, WatchStatus, Work,
    CodeKind, DimensionCount, Tag, WorkDetail, WorkFilters, WorkRating, WorkSet,
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
                normalized_code TEXT UNIQUE,
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
        self.ensure_work_metadata_columns()?;
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS actors (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                primary_name TEXT NOT NULL,
                avatar_path TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS actor_names (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                actor_id INTEGER NOT NULL REFERENCES actors(id) ON DELETE CASCADE,
                name TEXT NOT NULL UNIQUE,
                is_primary INTEGER NOT NULL DEFAULT 0,
                source TEXT
            );

            CREATE TABLE IF NOT EXISTS work_actors (
                work_id INTEGER NOT NULL REFERENCES works(id) ON DELETE CASCADE,
                actor_id INTEGER NOT NULL REFERENCES actors(id) ON DELETE CASCADE,
                PRIMARY KEY (work_id, actor_id)
            );

            -- Normalized NFO relation tables (Task 2). tags/sets/labels/studios
            -- are registries with UNIQUE names; work_tags/work_sets link them
            -- to works. work_ratings stores one row per (work, source) so
            -- multiple scrapers can contribute ratings without overwriting.
            CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS work_tags (
                work_id INTEGER NOT NULL REFERENCES works(id) ON DELETE CASCADE,
                tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                PRIMARY KEY (work_id, tag_id)
            );

            CREATE TABLE IF NOT EXISTS sets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS work_sets (
                work_id INTEGER NOT NULL REFERENCES works(id) ON DELETE CASCADE,
                set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
                PRIMARY KEY (work_id, set_id)
            );

            CREATE TABLE IF NOT EXISTS labels (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS studios (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS work_ratings (
                work_id INTEGER NOT NULL REFERENCES works(id) ON DELETE CASCADE,
                source TEXT NOT NULL,
                value REAL NOT NULL,
                max INTEGER NOT NULL,
                votes INTEGER,
                PRIMARY KEY (work_id, source)
            );
            ",
        )?;
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

    fn ensure_work_metadata_columns(&self) -> Result<()> {
        self.ensure_column("works", "genres_json", "TEXT NOT NULL DEFAULT '[]'")?;
        self.ensure_column("works", "studio", "TEXT")?;
        self.ensure_column("works", "director", "TEXT")?;
        self.ensure_column("works", "release_date", "TEXT")?;
        // Rich NFO metadata (Task 2): full-fidelity scalar fields so a Work
        // survives a round trip through the parser without loss. Defaults keep
        // existing rows valid; non-standard works leave normalized_code NULL.
        self.ensure_column("works", "source_code", "TEXT")?;
        self.ensure_column("works", "code_kind", "TEXT NOT NULL DEFAULT 'standard'")?;
        self.ensure_column("works", "outline", "TEXT")?;
        self.ensure_column("works", "poster_path", "TEXT")?;
        self.ensure_column("works", "thumb_path", "TEXT")?;
        self.ensure_column("works", "fanart_path", "TEXT")?;
        self.ensure_column("works", "rating_value", "REAL")?;
        self.ensure_column("works", "rating_max", "INTEGER")?;
        self.ensure_column("works", "rating_votes", "INTEGER")?;
        self.ensure_column("works", "criticrating", "REAL")?;
        self.ensure_column("works", "label", "TEXT")?;
        self.ensure_column("works", "runtime_minutes", "INTEGER")?;
        self.ensure_column("works", "year", "INTEGER")?;
        self.ensure_column("works", "website", "TEXT")?;
        self.ensure_column("works", "mpaa", "TEXT")?;
        self.ensure_column("works", "has_video", "INTEGER NOT NULL DEFAULT 1")?;
        Ok(())
    }

    pub fn upsert_work(&self, work: &Work) -> Result<i64> {
        let code_kind = match work.code_kind {
            CodeKind::Standard => "standard",
            CodeKind::Nonstandard => "nonstandard",
        };
        let aliases_json = serde_json::to_string(&work.aliases)?;
        let tags_json = serde_json::to_string(&work.tags)?;
        let lists_json = serde_json::to_string(&work.lists)?;
        let genres_json = serde_json::to_string(&work.genres)?;
        let cover = work.cover_path.as_ref().map(|p| p.to_string_lossy().to_string());
        let poster = work.poster_path.as_ref().map(|p| p.to_string_lossy().to_string());
        let thumb = work.thumb_path.as_ref().map(|p| p.to_string_lossy().to_string());
        let fanart = work.fanart_path.as_ref().map(|p| p.to_string_lossy().to_string());
        let watch_status = format!("{:?}", work.watch_status);
        let has_video: i64 = if work.has_video { 1 } else { 0 };

        // Merge decision: a canonical studio code wins and uses the existing
        // UNIQUE-index upsert. Non-standard works (normalized_code NULL) merge
        // by source_code: look up first, UPDATE that row, or INSERT if new.
        let id = if let Some(code) = &work.normalized_code {
            self.conn.execute(
                "
                INSERT INTO works (
                    normalized_code,
                    source_code,
                    code_kind,
                    title_zh,
                    original_title,
                    aliases_json,
                    summary,
                    outline,
                    cover_path,
                    poster_path,
                    thumb_path,
                   fanart_path,
                   tags_json,
                   lists_json,
                   rating,
                   rating_value,
                    rating_max,
                    rating_votes,
                    criticrating,
                    watch_status,
                    genres_json,
                    studio,
                    label,
                    director,
                    release_date,
                    runtime_minutes,
                    year,
                    website,
                    mpaa,
                    has_video
                )
                VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                    ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                    ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30
                )
                ON CONFLICT(normalized_code) DO UPDATE SET
                    source_code = COALESCE(works.source_code, excluded.source_code),
                    code_kind = excluded.code_kind,
                    title_zh = COALESCE(works.title_zh, excluded.title_zh),
                    original_title = COALESCE(works.original_title, excluded.original_title),
                    aliases_json = CASE
                        WHEN works.aliases_json = '[]' THEN excluded.aliases_json
                        ELSE works.aliases_json
                    END,
                    summary = COALESCE(works.summary, excluded.summary),
                    outline = COALESCE(works.outline, excluded.outline),
                    cover_path = COALESCE(works.cover_path, excluded.cover_path),
                    poster_path = COALESCE(works.poster_path, excluded.poster_path),
                    thumb_path = COALESCE(works.thumb_path, excluded.thumb_path),
                    fanart_path = COALESCE(works.fanart_path, excluded.fanart_path),
                    tags_json = works.tags_json,
                    lists_json = works.lists_json,
                    rating = works.rating,
                    rating_value = COALESCE(works.rating_value, excluded.rating_value),
                    rating_max = COALESCE(works.rating_max, excluded.rating_max),
                    rating_votes = COALESCE(works.rating_votes, excluded.rating_votes),
                    criticrating = COALESCE(works.criticrating, excluded.criticrating),
                    watch_status = works.watch_status,
                    genres_json = works.genres_json,
                    studio = COALESCE(works.studio, excluded.studio),
                    label = COALESCE(works.label, excluded.label),
                    director = COALESCE(works.director, excluded.director),
                    release_date = COALESCE(works.release_date, excluded.release_date),
                    runtime_minutes = COALESCE(works.runtime_minutes, excluded.runtime_minutes),
                    year = COALESCE(works.year, excluded.year),
                    website = COALESCE(works.website, excluded.website),
                    mpaa = COALESCE(works.mpaa, excluded.mpaa),
                    has_video = excluded.has_video,
                    updated_at = CURRENT_TIMESTAMP
                ",
                params![
                    code,
                    work.source_code,
                    code_kind,
                    work.title_zh,
                    work.original_title,
                    aliases_json,
                    work.summary,
                    work.outline,
                    cover,
                    poster,
                    thumb,
                    fanart,
                   tags_json,
                   lists_json,
                   work.rating,
                    work.rating_value,
                    work.rating_max,
                    work.rating_votes,
                    work.criticrating,
                    watch_status,
                    genres_json,
                    work.studio,
                    work.label,
                    work.director,
                    work.release_date,
                    work.runtime_minutes,
                    work.year,
                    work.website,
                    work.mpaa,
                    has_video,
                ],
            )?;
            self.conn.query_row(
                "SELECT id FROM works WHERE normalized_code = ?1",
                params![code],
                |row| row.get(0),
            )?
        } else {
            // Non-standard work: merge on source_code. Look up first so a
            // re-ingest backfills missing fields onto the existing row instead
            // of creating a duplicate.
            let existing: Option<i64> = self
                .conn
                .query_row(
                    "SELECT id FROM works WHERE source_code = ?1",
                    params![work.source_code],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(id) = existing {
                self.conn.execute(
                    "
                    UPDATE works SET
                        source_code = COALESCE(works.source_code, ?2),
                        title_zh = COALESCE(works.title_zh, ?3),
                        original_title = COALESCE(works.original_title, ?4),
                        aliases_json = CASE
                            WHEN works.aliases_json = '[]' THEN ?5
                            ELSE works.aliases_json
                        END,
                        summary = COALESCE(works.summary, ?6),
                        outline = COALESCE(works.outline, ?7),
                        cover_path = COALESCE(works.cover_path, ?8),
                        poster_path = COALESCE(works.poster_path, ?9),
                        thumb_path = COALESCE(works.thumb_path, ?10),
                        fanart_path = COALESCE(works.fanart_path, ?11),
                        rating_value = COALESCE(works.rating_value, ?12),
                        rating_max = COALESCE(works.rating_max, ?13),
                        rating_votes = COALESCE(works.rating_votes, ?14),
                        criticrating = COALESCE(works.criticrating, ?15),
                        genres_json = CASE
                            WHEN works.genres_json = '[]' THEN ?16
                            ELSE works.genres_json
                        END,
                        studio = COALESCE(works.studio, ?17),
                        label = COALESCE(works.label, ?18),
                        director = COALESCE(works.director, ?19),
                        release_date = COALESCE(works.release_date, ?20),
                        runtime_minutes = COALESCE(works.runtime_minutes, ?21),
                        year = COALESCE(works.year, ?22),
                        website = COALESCE(works.website, ?23),
                        mpaa = COALESCE(works.mpaa, ?24),
                        has_video = ?25,
                        updated_at = CURRENT_TIMESTAMP
                    WHERE id = ?1
                    ",
                    params![
                        id,
                        work.source_code,
                        work.title_zh,
                        work.original_title,
                        aliases_json,
                        work.summary,
                        work.outline,
                        cover,
                        poster,
                        thumb,
                        fanart,
                        work.rating_value,
                        work.rating_max,
                        work.rating_votes,
                        work.criticrating,
                        genres_json,
                        work.studio,
                        work.label,
                        work.director,
                        work.release_date,
                        work.runtime_minutes,
                        work.year,
                        work.website,
                        work.mpaa,
                        has_video,
                    ],
                )?;
                id
            } else {
                self.conn.execute(
                    "
                    INSERT INTO works (
                        normalized_code,
                        source_code,
                        code_kind,
                        title_zh,
                        original_title,
                        aliases_json,
                        summary,
                        outline,
                        cover_path,
                        poster_path,
                        thumb_path,
                       fanart_path,
                       tags_json,
                       lists_json,
                       rating,
                       rating_value,
                        rating_max,
                        rating_votes,
                        criticrating,
                        watch_status,
                        genres_json,
                        studio,
                        label,
                        director,
                        release_date,
                        runtime_minutes,
                        year,
                        website,
                        mpaa,
                        has_video
                   )
                   VALUES (
                       ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                       ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                        ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30
                   )
                   ",
                    params![
                        work.normalized_code,
                        work.source_code,
                        code_kind,
                        work.title_zh,
                        work.original_title,
                        aliases_json,
                        work.summary,
                        work.outline,
                        cover,
                        poster,
                        thumb,
                        fanart,
                       tags_json,
                       lists_json,
                       work.rating,
                        work.rating_value,
                        work.rating_max,
                        work.rating_votes,
                        work.criticrating,
                        watch_status,
                        genres_json,
                        work.studio,
                        work.label,
                        work.director,
                        work.release_date,
                        work.runtime_minutes,
                        work.year,
                        work.website,
                        work.mpaa,
                        has_video,
                    ],
                )?;
                self.conn.last_insert_rowid()
            }
        };

        self.set_work_tags(id, &work.tags)?;
        self.set_work_sets(id, &work.sets)?;
        self.set_work_ratings(id, &work.ratings)?;
        self.register_dimension_name("studios", &work.studio)?;
        self.register_dimension_name("labels", &work.label)?;
        Ok(id)
    }

    /// Mirror the scalar `works.studio`/`works.label` text into the
    /// `studios`/`labels` registries so the dimension listing and id-based
    /// filters resolve to real, stable ids. No link table is needed: each work
    /// carries exactly one studio and one label, so the registry name is the
    /// join key. Blank names are skipped so empty studios never pollute the
    /// dimension panel.
    fn register_dimension_name(&self, table: &str, name: &Option<String>) -> Result<()> {
        let Some(value) = name else { return Ok(()); };
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            &format!("INSERT OR IGNORE INTO {table} (name) VALUES (?1)"),
            params![trimmed],
        )?;
        Ok(())
    }

    pub fn get_work_by_code(&self, normalized_code: &str) -> Result<Option<Work>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {WORK_COLUMNS} FROM works WHERE normalized_code = ?1"
        ))?;
        let mut rows = statement.query(params![normalized_code])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };

        Ok(Some(work_from_row(&row)?))
    }

    pub fn list_works(&self) -> Result<Vec<Work>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {WORK_COLUMNS} FROM works ORDER BY normalized_code ASC"
        ))?;
        let rows = statement.query_map([], work_from_row)?;

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
        // A work targeted by id always has a canonical code (ingest path), so
        // this is the standard-merge branch; a non-standard work without one is
        // not promotable here and surfaces as an explicit error.
        let normalized_code = work
            .normalized_code
            .ok_or_else(|| anyhow::anyhow!("work {work_id} has no canonical code"))?;
        self.promote_ingest_item_to_work(item_id, item, work_id, normalized_code, true)
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
        // Sync actors from local/online metadata into the actor link table.
        if let Some(names) = item.metadata.as_ref().map(|m| m.actors.clone()) {
            if !names.is_empty() {
                let source = item.metadata.as_ref().map(|m| m.provider.as_str()).unwrap_or("local");
                self.set_work_actors(work_id, &names, source)?;
            }
        }
        if let Some(job_id) = item.job_id {
            self.refresh_ingest_job_counts(job_id)?;
        }
        Ok(work_id)
    }

    fn get_work_by_id(&self, work_id: i64) -> Result<Option<Work>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {WORK_COLUMNS} FROM works WHERE id = ?1"
        ))?;
        let mut rows = statement.query(params![work_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(work_from_row(&row)?))
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
                work.normalized_code.as_deref().unwrap_or(""),
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

// ===== actor entity + alias model =====

    fn resolve_actor_by_name(&self, name: &str, source: &str) -> Result<i64> {
        if let Some(id) = self.conn.query_row("SELECT actor_id FROM actor_names WHERE name = ?1", params![name], |row| row.get(0)).optional()? {
            return Ok(id);
        }
        self.conn.execute("INSERT INTO actors (primary_name) VALUES (?1)", params![name])?;
        let actor_id: i64 = self.conn.last_insert_rowid();
        self.conn.execute("INSERT INTO actor_names (actor_id, name, is_primary, source) VALUES (?1, ?2, 1, ?3)", params![actor_id, name, source])?;
        Ok(actor_id)
    }

    pub fn set_work_actors(&self, work_id: i64, names: &[String], source: &str) -> Result<()> {
        self.conn.execute("DELETE FROM work_actors WHERE work_id = ?1", params![work_id])?;
        for name in names {
            let trimmed = name.trim();
            if trimmed.is_empty() { continue; }
            let actor_id = self.resolve_actor_by_name(trimmed, source)?;
            self.conn.execute("INSERT OR IGNORE INTO work_actors (work_id, actor_id) VALUES (?1, ?2)", params![work_id, actor_id])?;
        }
        Ok(())
    }

    pub fn list_work_actors(&self, work_id: i64) -> Result<Vec<Actor>> {
        let mut stmt = self.conn.prepare("SELECT a.id, a.primary_name, a.avatar_path FROM actors a JOIN work_actors w ON w.actor_id = a.id WHERE w.work_id = ?1 ORDER BY a.primary_name")?;
        let rows = stmt.query_map(params![work_id], |row| {
            Ok(Actor {
                id: row.get(0)?,
                primary_name: row.get(1)?,
                avatar_path: row.get::<_, Option<String>>(2)?.map(PathBuf::from),
            })
        })?;
        let result: rusqlite::Result<Vec<_>> = rows.collect();
        Ok(result?)
    }

    pub fn list_actor_names(&self, actor_id: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT name FROM actor_names WHERE actor_id = ?1 ORDER BY is_primary DESC, name")?;
        let rows = stmt.query_map(params![actor_id], |row| row.get::<_, String>(0))?;
        let result: rusqlite::Result<Vec<_>> = rows.collect();
        Ok(result?)
    }

    pub fn add_actor_alias(&self, actor_id: i64, name: &str, source: &str) -> Result<()> {
        self.conn.execute("INSERT OR IGNORE INTO actor_names (actor_id, name, is_primary, source) VALUES (?1, ?2, 0, ?3)", params![actor_id, name, source])?;
        Ok(())
    }

    pub fn merge_actors(&self, primary_id: i64, secondary_id: i64) -> Result<i64> {
        if primary_id == secondary_id { return Ok(primary_id); }
        let secondary_names: Vec<(i64, String, Option<String>)> = {
            let mut stmt = self.conn.prepare("SELECT id, name, source FROM actor_names WHERE actor_id = ?1")?;
            let rows = stmt.query_map(params![secondary_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))
            })?;
            let collected: rusqlite::Result<Vec<_>> = rows.collect();
            collected?
        };
        for (name_id, name, source) in &secondary_names {
            self.conn.execute("DELETE FROM actor_names WHERE id = ?1", params![name_id])?;
            self.conn.execute("INSERT OR IGNORE INTO actor_names (actor_id, name, is_primary, source) VALUES (?1, ?2, 0, ?3)", params![primary_id, name, source])?;
        }
        self.conn.execute("UPDATE OR IGNORE work_actors SET actor_id = ?1 WHERE actor_id = ?2", params![primary_id, secondary_id])?;
        self.conn.execute("DELETE FROM work_actors WHERE actor_id = ?1", params![secondary_id])?;
        self.conn.execute("DELETE FROM actors WHERE id = ?1", params![secondary_id])?;
        Ok(primary_id)
    }

    /// Replace a work's tags with the given names, preserving order. Each name
    /// is upserted into the `tags` registry (UNIQUE) and linked via `work_tags`.
    /// Order is encoded by re-inserting links in input order after the clear.
    pub fn set_work_tags(&self, work_id: i64, names: &[String]) -> Result<()> {
        self.conn.execute("DELETE FROM work_tags WHERE work_id = ?1", params![work_id])?;
        for name in names {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            self.conn.execute(
                "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
                params![trimmed],
            )?;
            let tag_id: i64 = self.conn.query_row(
                "SELECT id FROM tags WHERE name = ?1",
                params![trimmed],
                |row| row.get(0),
            )?;
            self.conn.execute(
                "INSERT OR IGNORE INTO work_tags (work_id, tag_id) VALUES (?1, ?2)",
                params![work_id, tag_id],
            )?;
        }
        Ok(())
    }

    /// List a work's tags in link order (insertion order). Tags deleted from
    /// the registry are skipped, so this never returns dangling names.
    pub fn list_work_tags(&self, work_id: i64) -> Result<Vec<Tag>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name FROM tags t
             JOIN work_tags w ON w.tag_id = t.id
             WHERE w.work_id = ?1",
        )?;
        let rows = stmt.query_map(params![work_id], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })?;
        let result: rusqlite::Result<Vec<_>> = rows.collect();
        Ok(result?)
    }

    /// Replace a work's sets with the given names, preserving order. Mirrors
    /// `set_work_tags` against the `sets`/`work_sets` tables.
    pub fn set_work_sets(&self, work_id: i64, names: &[String]) -> Result<()> {
        self.conn.execute("DELETE FROM work_sets WHERE work_id = ?1", params![work_id])?;
        for name in names {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            self.conn.execute(
                "INSERT OR IGNORE INTO sets (name) VALUES (?1)",
                params![trimmed],
            )?;
            let set_id: i64 = self.conn.query_row(
                "SELECT id FROM sets WHERE name = ?1",
                params![trimmed],
                |row| row.get(0),
            )?;
            self.conn.execute(
                "INSERT OR IGNORE INTO work_sets (work_id, set_id) VALUES (?1, ?2)",
                params![work_id, set_id],
            )?;
        }
        Ok(())
    }

    /// List a work's sets in link order.
    pub fn list_work_sets(&self, work_id: i64) -> Result<Vec<WorkSet>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name FROM sets s
             JOIN work_sets w ON w.set_id = s.id
             WHERE w.work_id = ?1",
        )?;
        let rows = stmt.query_map(params![work_id], |row| {
            Ok(WorkSet {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })?;
        let result: rusqlite::Result<Vec<_>> = rows.collect();
        Ok(result?)
    }

    /// Replace a work's per-source ratings. Each (work, source) pair is one row;
    /// callers merge across sources themselves.
    pub fn set_work_ratings(&self, work_id: i64, ratings: &[WorkRating]) -> Result<()> {
        self.conn
            .execute("DELETE FROM work_ratings WHERE work_id = ?1", params![work_id])?;
        for rating in ratings {
            self.conn.execute(
                "INSERT OR REPLACE INTO work_ratings (work_id, source, value, max, votes)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![work_id, rating.source, rating.value, rating.max, rating.votes],
            )?;
        }
        Ok(())
    }

    /// List a work's per-source ratings.
    pub fn list_work_ratings(&self, work_id: i64) -> Result<Vec<WorkRating>> {
        let mut stmt = self.conn.prepare(
            "SELECT source, value, max, votes FROM work_ratings WHERE work_id = ?1",
        )?;
        let rows = stmt.query_map(params![work_id], |row| {
            Ok(WorkRating {
                source: row.get(0)?,
                value: row.get(1)?,
                max: row.get(2)?,
                votes: row.get(3)?,
            })
        })?;
        let result: rusqlite::Result<Vec<_>> = rows.collect();
        Ok(result?)
    }

    /// Full-fidelity read model for a work: scalar fields plus actors, tags,
    /// sets, file versions, and per-source ratings. Returns None when the work
    /// id does not resolve.
    pub fn get_work_detail(&self, work_id: i64) -> Result<Option<WorkDetail>> {
        let Some(work) = self.get_work_by_id(work_id)? else {
            return Ok(None);
        };
        let actors = self.list_work_actors(work_id)?;
        let tags = self.list_work_tags(work_id)?;
        let sets = self.list_work_sets(work_id)?;
        let file_versions = self.list_file_versions_for_work(work_id)?;
        let ratings = self.list_work_ratings(work_id)?;
       Ok(Some(WorkDetail {
           work,
           actors,
           tags,
           sets,
           file_versions,
           ratings,
       }))
   }

    // --- metadata dimension queries (Task 4) ---

    /// Tag dimension: each tag paired with how many works carry it. Only tags
    /// actually linked to at least one work appear, so the dimension panel
    /// never lists orphaned registry rows.
    pub fn list_tags(&self) -> Result<Vec<DimensionCount>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name, COUNT(DISTINCT wt.work_id) AS work_count
             FROM tags t
             JOIN work_tags wt ON wt.tag_id = t.id
             GROUP BY t.id, t.name
             ORDER BY t.name",
        )?;
        let rows = stmt.query_map([], dimension_count_from_row)?;
        collect_rows(rows)
    }

    /// Set dimension, same shape as tags.
    pub fn list_sets(&self) -> Result<Vec<DimensionCount>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, COUNT(DISTINCT ws.work_id) AS work_count
             FROM sets s
             JOIN work_sets ws ON ws.set_id = s.id
             GROUP BY s.id, s.name
             ORDER BY s.name",
        )?;
        let rows = stmt.query_map([], dimension_count_from_row)?;
        collect_rows(rows)
    }

    /// Studio dimension. Studios have no link table; the registry name is
    /// joined against `works.studio`, so the count reflects how many works name
    /// each studio. Returns only studios that at least one work uses.
    pub fn list_studios(&self) -> Result<Vec<DimensionCount>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, COUNT(*) AS work_count
             FROM studios s
             JOIN works w ON w.studio = s.name
             GROUP BY s.id, s.name
             ORDER BY s.name",
        )?;
        let rows = stmt.query_map([], dimension_count_from_row)?;
        collect_rows(rows)
    }

    /// Label dimension, same shape as studios against `works.label`.
    pub fn list_labels(&self) -> Result<Vec<DimensionCount>> {
        let mut stmt = self.conn.prepare(
            "SELECT l.id, l.name, COUNT(*) AS work_count
             FROM labels l
             JOIN works w ON w.label = l.name
             GROUP BY l.id, l.name
             ORDER BY l.name",
        )?;
        let rows = stmt.query_map([], dimension_count_from_row)?;
        collect_rows(rows)
    }

    /// Distinct actor entities reachable by the given name (primary or alias),
    /// via the `actor_names` table. Used to resolve a typed-in or clicked actor
    /// name to actor ids for filtering.
    pub fn list_work_actors_for_name(&self, name: &str) -> Result<Vec<Actor>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT a.id, a.primary_name, a.avatar_path
             FROM actors a
             JOIN actor_names n ON n.actor_id = a.id
             WHERE n.name = ?1
             ORDER BY a.primary_name",
        )?;
        let rows = stmt.query_map(params![name], |row| {
            Ok(Actor {
                id: row.get(0)?,
                primary_name: row.get(1)?,
                avatar_path: row.get::<_, Option<String>>(2)?.map(PathBuf::from),
            })
        })?;
        collect_rows(rows)
    }

    /// Works matching every populated dimension. For many-per-work dimensions
    /// (tags, sets, actors) a work must link every requested id (intersection
    /// via GROUP BY + HAVING COUNT = requested). For one-per-work dimensions
    /// (studio, label) the work matches any requested id, since a single
    /// `works.studio`/`works.label` column cannot equal two names at once.
    /// `code_kinds` and `has_video` constrain the scalar columns. An
    /// all-default filter matches every work, matching `list_works`.
    pub fn list_works_filtered(&self, filters: WorkFilters) -> Result<Vec<Work>> {
        let mut conditions: Vec<String> = Vec::new();
        let mut params_vec: Vec<rusqlite::types::Value> = Vec::new();

        for (link_table, link_col, ids) in [
            ("work_tags", "tag_id", &filters.tag_ids),
            ("work_sets", "set_id", &filters.set_ids),
            ("work_actors", "actor_id", &filters.actor_ids),
        ] {
            if ids.is_empty() {
                continue;
            }
            let placeholders = vec!["?"; ids.len()].join(", ");
            let want = ids.len();
            params_vec.extend(ids.iter().copied().map(rusqlite::types::Value::Integer));
            conditions.push(format!(
                "works.id IN (
                    SELECT work_id FROM {link_table}
                    WHERE {link_col} IN ({placeholders})
                    GROUP BY work_id
                    HAVING COUNT(DISTINCT {link_col}) = {want}
                )"
            ));
        }

        for (registry, ids) in
            [("studios", &filters.studio_ids), ("labels", &filters.label_ids)]
        {
            if ids.is_empty() {
                continue;
            }
            let placeholders = vec!["?"; ids.len()].join(", ");
            let work_col = if registry == "studios" { "studio" } else { "label" };
            params_vec.extend(ids.iter().copied().map(rusqlite::types::Value::Integer));
            conditions.push(format!(
                "works.{work_col} IN (SELECT name FROM {registry} WHERE id IN ({placeholders}))"
            ));
        }

        if !filters.code_kinds.is_empty() {
            let placeholders = vec!["?"; filters.code_kinds.len()].join(", ");
            params_vec.extend(
                filters
                    .code_kinds
                    .iter()
                    .map(|kind| rusqlite::types::Value::Text(code_kind_str(kind).to_string())),
            );
            conditions.push(format!("works.code_kind IN ({placeholders})"));
        }

        if let Some(has_video) = filters.has_video {
            params_vec.push(rusqlite::types::Value::Integer(if has_video { 1 } else { 0 }));
            conditions.push("works.has_video = ?".to_string());
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        let sql = format!("SELECT {WORK_COLUMNS} FROM works{where_clause} ORDER BY normalized_code ASC");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params_vec.iter()), work_from_row)?;
        let mut works = Vec::new();
        for row in rows {
            works.push(row?);
        }
        Ok(works)
    }

    // --- migration-introspection helpers (test/diagnostic use) ---

    /// Column names of a table, via PRAGMA table_info. Lets tests assert the
    /// schema without poking SQL by hand.
    pub fn debug_table_columns(&self, table: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let result: rusqlite::Result<Vec<_>> = rows.collect();
        Ok(result?)
    }

    /// Whether a table exists in sqlite_master.
    pub fn debug_table_exists(&self, table: &str) -> Result<bool> {
        let exists: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                params![table],
                |row| row.get(0),
           )
           .optional()?;
       Ok(exists.is_some())
   }

    // --- NFO rebuild: full-fidelity re-ingest from NFO files ---

    /// Rebuild the whole library from NFO files under `roots` inside a single
    /// transaction. Every work/file/relation table is cleared first (app
    /// settings and archive logs are preserved), then the scanned NFOs are
    /// grouped by `<num>` and persisted as one work per group with one
    /// file_version per member. Any write failure rolls the entire rebuild
    /// back, leaving the library as it was before the call.
    pub fn rebuild_library(
        &self,
        roots: &[PathBuf],
    ) -> Result<crate::library_rebuild::RebuildReport> {
        // unchecked_transaction borrows the connection by shared reference, so
        // all the &self repository methods below run inside this transaction
        // and are undone together if any one of them errors.
        let tx = self.conn.unchecked_transaction()?;
        self.clear_library_tables()?;
        let scanned = crate::scanner::scan_library_roots(roots)?;
        let grouped = crate::library_rebuild::group_scanned_nfos(&scanned);
        self.persist_grouped_rebuild(&grouped)?;
        tx.commit()?;
        Ok(crate::library_rebuild::summarize_grouped_inputs(
            &grouped.groups,
            &grouped.errors,
        ))
    }

    /// Delete every work, file version, and metadata-relation row so the
    /// rebuild starts from a clean slate. `app_settings` and
    /// `archive_action_logs` are deliberately untouched: config and history
    /// survive a rebuild. Children are cleared before their registries so the
    /// foreign-key graph (PRAGMA foreign_keys = ON) never trips.
    fn clear_library_tables(&self) -> Result<()> {
        for table in [
            "work_tags",
            "work_sets",
            "work_ratings",
            "work_actors",
            "file_versions",
            "ingest_items",
            "ingest_jobs",
            "tags",
            "sets",
            "labels",
            "studios",
            "actor_names",
            "actors",
            "works",
        ] {
            self.conn.execute(&format!("DELETE FROM {table}"), [])?;
        }
        Ok(())
    }

    /// Persist the grouped NFO scan: one work per group (merged on
    /// source_code, with standard/nonstandard classification), one
    /// file_version per member, plus actors/tags/sets/ratings from the main
    /// NFO. Hard-errors (`?`) so the surrounding transaction rolls back on any
    /// write failure.
    fn persist_grouped_rebuild(
        &self,
        grouped: &crate::library_rebuild::GroupedScan,
    ) -> Result<()> {
        for group in &grouped.groups {
            let main = crate::library_rebuild::select_main_nfo(&group.members);
            let doc = &main.document;

            // A well-formed studio code is normalized; anything else keeps its
            // raw source_code and is classified Nonstandard.
            let (normalized_code, code_kind) = match normalize_code(&group.source_code) {
                Some(code) => (Some(code), CodeKind::Standard),
                None => (None, CodeKind::Nonstandard),
            };

            // Scalar rating columns mirror the default (or first) rating
            // source; the full per-source list is preserved in work_ratings.
            let default_rating = doc
                .rating_sources
                .iter()
                .find(|rating| rating.is_default)
                .or_else(|| doc.rating_sources.first());
            let ratings: Vec<WorkRating> = doc
                .rating_sources
                .iter()
                .map(|rating| WorkRating {
                    source: rating.source.clone(),
                    value: rating.value,
                    max: rating.max,
                    votes: rating.votes,
                })
                .collect();

            // Unified tag bag: NFO <tag> union <genre>, de-duplicated in order.
            let mut tags: Vec<String> = Vec::new();
            for tag in doc.tags.iter().chain(doc.genres.iter()) {
                let trimmed = tag.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if !tags.iter().any(|existing| existing == trimmed) {
                    tags.push(trimmed.to_string());
                }
            }

            let has_video = group.members.iter().any(|member| member.paired_video.is_some());

            let work = Work {
                id: None,
                normalized_code,
                source_code: Some(group.source_code.clone()),
                code_kind,
                title_zh: doc.title.clone(),
                original_title: doc.original_title.clone(),
                aliases: Vec::new(),
                summary: doc.summary.clone(),
                outline: doc.outline.clone(),
                cover_path: doc.cover_url.as_ref().map(PathBuf::from),
                poster_path: doc.poster_path.as_ref().map(PathBuf::from),
                thumb_path: doc.thumb_path.as_ref().map(PathBuf::from),
                fanart_path: doc.fanart_path.as_ref().map(PathBuf::from),
                tags,
                sets: doc.sets.clone(),
                lists: Vec::new(),
                rating: None,
                rating_value: default_rating.map(|rating| rating.value),
                rating_max: default_rating.map(|rating| rating.max),
                rating_votes: default_rating.and_then(|rating| rating.votes),
                criticrating: doc.criticrating,
                watch_status: WatchStatus::Unwatched,
                genres: doc.genres.clone(),
                studio: doc.studio.clone(),
                label: doc.label.clone(),
                director: doc.director.clone(),
                release_date: doc.release_date.clone(),
                runtime_minutes: doc.runtime_minutes,
                year: doc.year,
                website: doc.website.clone(),
                mpaa: doc.mpaa.clone(),
                has_video,
                ratings,
            };

            let work_id = self.upsert_work(&work)?;
            // Actors are not written by upsert_work; link them from the NFO.
            self.set_work_actors(work_id, &doc.actors, "nfo")?;

            // One file_version per member: the paired video when present,
            // otherwise the NFO itself as a no-video placeholder (size 0).
            for member in &group.members {
                let (path, file_name, size_bytes) = match &member.paired_video {
                    Some(video) => {
                        let size =
                            std::fs::metadata(video).map(|metadata| metadata.len()).unwrap_or(
                                member.paired_video_size,
                            );
                        let name = video
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("")
                            .to_string();
                        (video.clone(), name, size)
                    }
                    None => (member.nfo_path.clone(), member.nfo_file_name.clone(), 0),
                };
                self.conn.execute(
                    "INSERT INTO file_versions
                        (work_id, source_root, original_path, original_file_name, size_bytes)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        work_id,
                        member.source_root.to_string_lossy(),
                        path.to_string_lossy(),
                        file_name,
                        size_bytes as i64,
                    ],
                )?;
            }
        }
        Ok(())
    }

    /// Drop a table by name. Diagnostic/test helper used to simulate a write
    /// failure so the rebuild's transaction rollback can be exercised.
    pub fn debug_drop_table(&self, table: &str) -> Result<()> {
        self.conn.execute(&format!("DROP TABLE {table}"), [])?;
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

/// Decode a stored `code_kind` text value. Default column value is 'standard';
/// any non-'nonstandard' value falls back to Standard so a malformed/legacy
/// row never panics.
fn parse_code_kind(value: &str) -> CodeKind {
    if value.eq_ignore_ascii_case("nonstandard") {
        CodeKind::Nonstandard
    } else {
        CodeKind::Standard
    }
}

/// Inverse of `parse_code_kind`: the lowercase string persisted in
/// `works.code_kind`. Used by `list_works_filtered` to bind `CodeKind` filter
/// values against the stored column.
fn code_kind_str(kind: &CodeKind) -> &'static str {
    match kind {
        CodeKind::Standard => "standard",
        CodeKind::Nonstandard => "nonstandard",
    }
}

/// Map a `(id, name, work_count)` row to a `DimensionCount`. Shared by every
/// dimension listing query so the column-order convention lives in one place.
fn dimension_count_from_row(row: &rusqlite::Row) -> rusqlite::Result<DimensionCount> {
    Ok(DimensionCount {
        id: row.get(0)?,
        name: row.get(1)?,
        work_count: row.get(2)?,
    })
}

/// Collect a `query_map` iterator into a `Vec`, surfacing the first row error.
/// Keeps the listing methods free of repeated collect boilerplate.
fn collect_rows<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> Result<Vec<T>> {
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Canonical column list read for a `Work`. Every row-to-Work mapping site
/// selects exactly this list and maps it through `work_from_row`, so adding a
/// new persisted field is a one-place change instead of three parallel edits.
const WORK_COLUMNS: &str = "id, normalized_code, source_code, code_kind, \
    title_zh, original_title, aliases_json, summary, outline, \
    cover_path, poster_path, thumb_path, fanart_path, \
    tags_json, lists_json, rating, rating_value, rating_max, rating_votes, criticrating, \
    watch_status, genres_json, studio, label, director, release_date, \
    runtime_minutes, year, website, mpaa, has_video";

/// Build a `Work` from a row whose selected columns match `WORK_COLUMNS`.
/// Image-path columns come back as TEXT and are wrapped into `PathBuf`.
fn work_from_row(row: &rusqlite::Row) -> rusqlite::Result<Work> {
    let aliases_json: String = row.get(6)?;
    let tags_json: String = row.get(13)?;
    let lists_json: String = row.get(14)?;
    let cover: Option<String> = row.get(9)?;
    let poster: Option<String> = row.get(10)?;
    let thumb: Option<String> = row.get(11)?;
    let fanart: Option<String> = row.get(12)?;
    let status: String = row.get(20)?;
    let genres_json: String = row.get(21)?;
    let code_kind: String = row.get(3)?;
    let has_video: i64 = row.get(30)?;
    Ok(Work {
        id: row.get(0)?,
        normalized_code: row.get(1)?,
        source_code: row.get(2)?,
        code_kind: parse_code_kind(&code_kind),
        title_zh: row.get(4)?,
        original_title: row.get(5)?,
        aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
        summary: row.get(7)?,
        outline: row.get(8)?,
        cover_path: cover.map(PathBuf::from),
        poster_path: poster.map(PathBuf::from),
        thumb_path: thumb.map(PathBuf::from),
        fanart_path: fanart.map(PathBuf::from),
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        sets: Vec::new(),
        lists: serde_json::from_str(&lists_json).unwrap_or_default(),
        rating: row.get::<_, Option<u8>>(15)?,
        rating_value: row.get(16)?,
        rating_max: row.get(17)?,
        rating_votes: row.get(18)?,
        criticrating: row.get(19)?,
        watch_status: parse_watch_status(&status),
        genres: serde_json::from_str(&genres_json).unwrap_or_default(),
        studio: row.get(22)?,
        label: row.get(23)?,
        director: row.get(24)?,
        release_date: row.get(25)?,
        runtime_minutes: row.get(26)?,
        year: row.get(27)?,
        website: row.get(28)?,
        mpaa: row.get(29)?,
        has_video: has_video != 0,
        ratings: Vec::new(),
    })
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
        normalized_code: Some(normalized_code.to_string()),
        // Ingest-time works are always keyed on a canonical code; non-standard
        // source_code flows in via the NFO/scraper path, not the ingest path.
        source_code: None,
        code_kind: CodeKind::Standard,
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
        outline: None,
        cover_path: item
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.cover_url.clone())
            .map(PathBuf::from),
        poster_path: None,
        thumb_path: None,
        fanart_path: None,
        tags: vec![],
        sets: vec![],
        lists: vec![],
        rating: None,
        rating_value: None,
        rating_max: None,
        rating_votes: None,
        criticrating: None,
        watch_status: WatchStatus::Unwatched,
        genres: item
            .metadata
            .as_ref()
            .map(|metadata| metadata.genres.clone())
            .unwrap_or_default(),
        studio: item.metadata.as_ref().and_then(|metadata| metadata.studio.clone()),
        label: None,
        director: item
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.director.clone()),
        release_date: item
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.release_date.clone()),
        runtime_minutes: None,
        year: None,
        website: None,
        mpaa: None,
        has_video: true,
        ratings: vec![],
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
