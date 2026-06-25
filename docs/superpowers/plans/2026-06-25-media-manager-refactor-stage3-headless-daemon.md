# 阶段 3 无头守护核心 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把阶段 2 已验证的 `AutoPipeline` 包进一个纯 Rust、可测试、无 WebView2 的后台 daemon core：读取配置、扫描完成文件、维护内存队列、暂停/恢复、逐个处理、输出确定状态。

**Architecture:** 新增 `src-tauri/src/daemon.rs`，只做编排，不重新实现阶段 2 的识别、刮削、归档和入库逻辑。daemon core 同步执行，测试通过方法调用精确验证状态；后续阶段可以把同一组方法包成 Tauri 命令、loopback HTTP API 或托盘控制层。

**Tech Stack:** Rust 2021, anyhow, serde, walkdir, tempfile, existing `Repository` / `AutoPipeline` / `CompletionSnapshot`, `cargo test -j 1`, CLI-only example。

---

## 全局约束

- 不能运行 `tauri dev`、默认 `cargo run`、`media-manager.exe`，也不能使用 Codex in-app browser。阶段 3 smoke 必须是 CLI-only example，命令必须显式指定 `--example stage3_daemon_smoke`。
- 所有测试使用 `tempfile` 创建 inbox/assets/archive/db，不依赖 `H:/`、`G:/`、真实媒体库或网络。
- 不接真实 FANZA/JavBus/JavDB，不访问外网；测试和 smoke 使用确定性 fake scraper。
- 不做 HTTP/WebSocket、托盘、自启、Windows Service、长期后台线程、文件系统 watcher、aria2 RPC。
- 新增 Rust struct、enum、public method 必须有 doc comment，说明职责和边界。
- 完整后端测试在当前 Windows Codex 环境使用 `-j 1`，规避并行 rustc 页面文件 / mmap 错误 `os error 1455`。
- 阶段 3 只允许修改无头 Rust core 和文档；`commands.rs:1422` 的 `parse_watch_status` 仍留到阶段 4 前端连线。

---

## 文件结构

- **Modify** `src-tauri/src/lib.rs`：导出 `pub mod daemon;`。
- **Create** `src-tauri/src/daemon.rs`：daemon 配置、状态、队列、扫描、暂停/恢复、`process_next`、`run_once`。
- **Create** `src-tauri/tests/daemon.rs`：阶段 3 集成测试，全部使用 `tempfile`。
- **Create** `src-tauri/examples/stage3_daemon_smoke.rs`：CLI-only 自包含冒烟验证。
- **Modify** `docs/superpowers/specs/2026-06-25-media-manager-stage3-headless-daemon-design.md`：若实现确认 `RunOnceReport` 是必要接口补充，同步设计文档。
- **Modify** `HANDOFF.md`：阶段 3 完成后更新接手说明和验证命令。

---

## 目标接口

```rust
// src-tauri/src/daemon.rs
pub struct DaemonConfig {
    pub source_roots: Vec<PathBuf>,
    pub archive_root: PathBuf,
    pub asset_roots: Vec<PathBuf>,
}

pub struct CompletionPolicy {
    pub sample_delay: Duration,
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
    pub file_name: String,
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

pub struct RunOnceReport {
    pub scan: ScanReport,
    pub process: ProcessReport,
}

pub struct HeadlessDaemon<'a> {
    pub repo: &'a Repository,
    pub config: DaemonConfig,
    pub scrapers: ScrapeCoordinator<'a>,
    pub completion_policy: CompletionPolicy,
}
```

`RunOnceReport` 是对阶段 3 设计的窄补充：smoke 需要同时打印 `queued=3` 和处理计数，单独的 `ProcessReport` 无法表达扫描阶段的排队数量。

---

## Task 1: 配置加载与基础状态

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/daemon.rs`
- Create: `src-tauri/tests/daemon.rs`

**Interfaces:**
- `DaemonConfig::load(repo: &Repository) -> anyhow::Result<DaemonConfig>`
- `HeadlessDaemon::new(repo, config, scrapers) -> HeadlessDaemon`
- `HeadlessDaemon::with_completion_policy(repo, config, scrapers, completion_policy) -> HeadlessDaemon`
- `HeadlessDaemon::status(&self) -> DaemonStatus`

- [ ] **Step 1: 写失败测试**

Create `src-tauri/tests/daemon.rs`:

```rust
use media_manager::daemon::{
    CompletionPolicy, DaemonConfig, DaemonState, HeadlessDaemon,
};
use media_manager::domain::{ExceptionKind, ScrapedWorkMetadata};
use media_manager::pipeline::{ScrapeCoordinator, ScraperSource};
use media_manager::storage::Repository;
use std::time::Duration;

struct FakeScraper;

impl ScraperSource for FakeScraper {
    fn name(&self) -> &str {
        "fake"
    }

    fn lookup(&self, normalized_code: &str) -> anyhow::Result<Option<ScrapedWorkMetadata>> {
        if normalized_code == "ABP-300" {
            Ok(Some(scraped(normalized_code)))
        } else {
            Ok(None)
        }
    }
}

fn scraped(code: &str) -> ScrapedWorkMetadata {
    ScrapedWorkMetadata {
        source: "fake".to_string(),
        normalized_code: code.to_string(),
        title: format!("{code} title"),
        original_title: Some(format!("{code} original")),
        summary: Some("summary".to_string()),
        actors: vec!["Actor A".to_string()],
        genres: vec!["Genre A".to_string()],
        studio: Some("Studio A".to_string()),
        director: None,
        release_date: Some("2026-06-25".to_string()),
        cover_path: None,
    }
}

fn open_repo(db: &std::path::Path) -> Repository {
    let repo = Repository::open(db).unwrap();
    repo.migrate().unwrap();
    repo
}

fn configured_repo(tmp: &tempfile::TempDir) -> (Repository, std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    let inbox = tmp.path().join("inbox");
    let archive = tmp.path().join("archive");
    let assets = tmp.path().join("assets");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    std::fs::create_dir_all(&assets).unwrap();
    repo.set_source_roots(&[inbox.clone()]).unwrap();
    repo.set_archive_root(&archive).unwrap();
    repo.set_resource_pool_dirs(&[assets.clone()]).unwrap();
    (repo, inbox, archive, assets)
}

fn daemon<'a>(repo: &'a Repository, scraper: &'a FakeScraper) -> HeadlessDaemon<'a> {
    let config = DaemonConfig::load(repo).unwrap();
    HeadlessDaemon::with_completion_policy(
        repo,
        config,
        ScrapeCoordinator {
            sources: vec![scraper],
        },
        CompletionPolicy {
            sample_delay: Duration::ZERO,
        },
    )
}

#[test]
fn daemon_config_loads_roots_from_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, archive, assets) = configured_repo(&tmp);

    let config = DaemonConfig::load(&repo).unwrap();

    assert_eq!(config.source_roots, vec![inbox]);
    assert_eq!(config.archive_root, archive);
    assert_eq!(config.asset_roots, vec![assets]);
}

#[test]
fn daemon_config_requires_archive_root() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    repo.set_source_roots(&[tmp.path().join("inbox")]).unwrap();

    let error = DaemonConfig::load(&repo).unwrap_err();

    assert!(error.to_string().contains("archive_root"));
}

#[test]
fn daemon_status_starts_idle_with_empty_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, _, _, _) = configured_repo(&tmp);
    let scraper = FakeScraper;
    let daemon = daemon(&repo, &scraper);

    let status = daemon.status();

    assert_eq!(status.state, DaemonState::Idle);
    assert_eq!(status.queued, 0);
    assert_eq!(status.processed, 0);
    assert_eq!(status.last_error, None);
}
```

- [ ] **Step 2: 运行失败测试**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: FAIL，提示 `daemon` module 和相关类型不存在。

- [ ] **Step 3: 导出模块并实现最小配置/状态**

In `src-tauri/src/lib.rs`, add:

```rust
pub mod daemon;
```

Create `src-tauri/src/daemon.rs`:

```rust
use crate::pipeline::ScrapeCoordinator;
use crate::storage::Repository;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

/// Runtime configuration for the headless daemon core. It is loaded from
/// SQLite settings and contains only local filesystem roots needed by Stage 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    pub source_roots: Vec<PathBuf>,
    pub archive_root: PathBuf,
    pub asset_roots: Vec<PathBuf>,
}

impl DaemonConfig {
    /// Load daemon roots from Repository settings. Missing source or asset
    /// roots are allowed, but archive_root is required before processing.
    pub fn load(repo: &Repository) -> Result<Self> {
        let archive_root = repo
            .get_archive_root()?
            .ok_or_else(|| anyhow!("archive_root is required before daemon processing"))?;
        Ok(Self {
            source_roots: repo.get_source_roots()?,
            archive_root,
            asset_roots: repo.get_resource_pool_dirs()?,
        })
    }
}

/// Sampling delay between two completion snapshots. Tests set this to zero;
/// production callers can use a non-zero delay without changing daemon logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionPolicy {
    pub sample_delay: Duration,
}

impl Default for CompletionPolicy {
    fn default() -> Self {
        Self {
            sample_delay: Duration::from_secs(1),
        }
    }
}

/// In-memory lifecycle state exposed to future control interfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DaemonState {
    Idle,
    Scanning,
    Processing,
    Paused,
    Error,
}

/// Snapshot of daemon state suitable for Tauri commands or a local HTTP API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub state: DaemonState,
    pub queued: usize,
    pub processed: usize,
    pub last_error: Option<String>,
}

/// One file queued by the daemon after completion checks pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size_bytes: u64,
    pub file_hash: Option<String>,
}

/// Summary of one scan pass over configured source roots.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanReport {
    pub scanned_files: usize,
    pub queued_files: usize,
    pub skipped_files: usize,
}

/// Summary of files processed from the daemon queue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessReport {
    pub processed: usize,
    pub archived: usize,
    pub holding: usize,
    pub exceptions: usize,
    pub failed: usize,
}

/// Combined report returned by run_once: scan counts plus processing counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOnceReport {
    pub scan: ScanReport,
    pub process: ProcessReport,
}

/// Pure Rust daemon core. It owns only in-memory queue/state and delegates all
/// durable writes to Repository and AutoPipeline.
pub struct HeadlessDaemon<'a> {
    pub repo: &'a Repository,
    pub config: DaemonConfig,
    pub scrapers: ScrapeCoordinator<'a>,
    pub completion_policy: CompletionPolicy,
    state: DaemonState,
    queue: VecDeque<QueuedFile>,
    queued_keys: HashSet<PathBuf>,
    processed: usize,
    last_error: Option<String>,
}

impl<'a> HeadlessDaemon<'a> {
    /// Create a daemon with the default completion sampling delay.
    pub fn new(
        repo: &'a Repository,
        config: DaemonConfig,
        scrapers: ScrapeCoordinator<'a>,
    ) -> Self {
        Self::with_completion_policy(repo, config, scrapers, CompletionPolicy::default())
    }

    /// Create a daemon with an explicit sampling policy for deterministic tests.
    pub fn with_completion_policy(
        repo: &'a Repository,
        config: DaemonConfig,
        scrapers: ScrapeCoordinator<'a>,
        completion_policy: CompletionPolicy,
    ) -> Self {
        Self {
            repo,
            config,
            scrapers,
            completion_policy,
            state: DaemonState::Idle,
            queue: VecDeque::new(),
            queued_keys: HashSet::new(),
            processed: 0,
            last_error: None,
        }
    }

    /// Return an in-memory status snapshot without reading or writing SQLite.
    pub fn status(&self) -> DaemonStatus {
        DaemonStatus {
            state: self.state.clone(),
            queued: self.queue.len(),
            processed: self.processed,
            last_error: self.last_error.clone(),
        }
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/lib.rs src-tauri/src/daemon.rs src-tauri/tests/daemon.rs
git commit -m "新增阶段3无头守护配置与状态"
```

---

## Task 2: 扫描 source roots 并排队完成视频

**Files:**
- Modify: `src-tauri/src/daemon.rs`
- Modify: `src-tauri/tests/daemon.rs`

**Interfaces:**
- `HeadlessDaemon::scan_now(&mut self) -> anyhow::Result<ScanReport>`
- internal `queue_completed_file`
- internal `queue_key`

- [ ] **Step 1: 写失败测试**

Append to `src-tauri/tests/daemon.rs`:

```rust
#[test]
fn scan_queues_stable_videos_and_skips_incomplete_or_non_video_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    let scraper = FakeScraper;
    std::fs::write(inbox.join("ABP-300.mp4"), b"video-300").unwrap();
    std::fs::write(inbox.join("ABP-301.mp4"), b"video-301").unwrap();
    std::fs::write(inbox.join("ABP-301.mp4.aria2"), b"partial").unwrap();
    std::fs::write(inbox.join("notes.txt"), b"not video").unwrap();
    let mut daemon = daemon(&repo, &scraper);

    let report = daemon.scan_now().unwrap();

    assert_eq!(report.scanned_files, 4);
    assert_eq!(report.queued_files, 1);
    assert_eq!(report.skipped_files, 3);
    assert_eq!(daemon.status().queued, 1);
}

#[test]
fn scan_is_deterministic_and_does_not_queue_duplicates() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    let nested = inbox.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("ABP-300.mp4"), b"video-300").unwrap();
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);

    let first = daemon.scan_now().unwrap();
    let second = daemon.scan_now().unwrap();

    assert_eq!(first.queued_files, 1);
    assert_eq!(second.queued_files, 0);
    assert_eq!(daemon.status().queued, 1);
}

#[test]
fn scan_skips_missing_source_roots_without_error() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, archive, assets) = configured_repo(&tmp);
    let missing = tmp.path().join("missing");
    repo.set_source_roots(&[missing, inbox]).unwrap();
    repo.set_archive_root(&archive).unwrap();
    repo.set_resource_pool_dirs(&[assets]).unwrap();
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);

    let report = daemon.scan_now().unwrap();

    assert_eq!(report.queued_files, 0);
    assert!(report.skipped_files >= 1);
    assert_eq!(daemon.status().state, DaemonState::Idle);
}
```

- [ ] **Step 2: 运行失败测试**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: FAIL，提示 `scan_now` 不存在。

- [ ] **Step 3: 实现扫描与排队**

Append imports in `src-tauri/src/daemon.rs`:

```rust
use crate::domain::CompletedFile;
use crate::pipeline::{is_heuristically_complete, CompletionSnapshot};
use crate::scanner::is_video_file;
use std::path::Path;
use walkdir::WalkDir;
```

Add to `impl HeadlessDaemon<'a>`:

```rust
    /// Scan configured source roots and enqueue only stable completed videos.
    /// This method never runs the pipeline and never writes media rows.
    pub fn scan_now(&mut self) -> Result<ScanReport> {
        if self.state == DaemonState::Paused {
            return Ok(ScanReport::default());
        }

        self.state = DaemonState::Scanning;
        let result = self.scan_roots();
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

    fn scan_roots(&mut self) -> Result<ScanReport> {
        let mut report = ScanReport::default();
        let mut candidates = Vec::new();

        for root in &self.config.source_roots {
            if !root.exists() {
                report.skipped_files += 1;
                continue;
            }
            for entry in WalkDir::new(root).into_iter().filter_map(|entry| entry.ok()) {
                if entry.file_type().is_file() {
                    candidates.push(entry.into_path());
                }
            }
        }

        candidates.sort();

        for path in candidates {
            report.scanned_files += 1;
            if !is_video_file(&path) {
                report.skipped_files += 1;
                continue;
            }

            let first = CompletionSnapshot::capture(&path)?;
            if !self.completion_policy.sample_delay.is_zero() {
                std::thread::sleep(self.completion_policy.sample_delay);
            }
            let second = CompletionSnapshot::capture(&path)?;
            if !is_heuristically_complete(&first, &second) {
                report.skipped_files += 1;
                continue;
            }

            let completed = CompletedFile::from_path(&path)?;
            if self.queue_completed_file(completed)? {
                report.queued_files += 1;
            }
        }

        Ok(report)
    }

    fn queue_completed_file(&mut self, file: CompletedFile) -> Result<bool> {
        let key = queue_key(&file.path);
        if !self.queued_keys.insert(key) {
            return Ok(false);
        }
        self.queue.push_back(QueuedFile {
            path: file.path,
            file_name: file.file_name,
            size_bytes: file.size_bytes,
            file_hash: file.file_hash,
        });
        Ok(true)
    }
}

fn queue_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
```

- [ ] **Step 4: 运行测试确认通过**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/daemon.rs src-tauri/tests/daemon.rs
git commit -m "实现无头守护扫描与排队"
```

---

## Task 3: 暂停/恢复与单文件处理

**Files:**
- Modify: `src-tauri/src/daemon.rs`
- Modify: `src-tauri/tests/daemon.rs`

**Interfaces:**
- `HeadlessDaemon::pause(&mut self)`
- `HeadlessDaemon::resume(&mut self)`
- `HeadlessDaemon::process_next(&mut self) -> anyhow::Result<ProcessReport>`

- [ ] **Step 1: 写失败测试**

Append to `src-tauri/tests/daemon.rs`:

```rust
#[test]
fn pause_blocks_scan_and_process_until_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    std::fs::write(inbox.join("ABP-300.mp4"), b"video-300").unwrap();
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);

    daemon.pause();
    let paused_scan = daemon.scan_now().unwrap();
    let paused_process = daemon.process_next().unwrap();

    assert_eq!(paused_scan.queued_files, 0);
    assert_eq!(paused_process.processed, 0);
    assert_eq!(daemon.status().state, DaemonState::Paused);

    daemon.resume();
    let resumed_scan = daemon.scan_now().unwrap();

    assert_eq!(resumed_scan.queued_files, 1);
    assert_eq!(daemon.status().state, DaemonState::Idle);
}

#[test]
fn process_next_archives_one_queued_file_through_auto_pipeline() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, archive, _) = configured_repo(&tmp);
    std::fs::write(inbox.join("ABP-300.mp4"), b"video-300").unwrap();
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);
    daemon.scan_now().unwrap();

    let report = daemon.process_next().unwrap();

    assert_eq!(report.processed, 1);
    assert_eq!(report.archived, 1);
    assert_eq!(report.holding, 0);
    assert_eq!(report.exceptions, 0);
    assert_eq!(report.failed, 0);
    assert_eq!(daemon.status().queued, 0);
    assert_eq!(daemon.status().processed, 1);
    assert!(archive.join("ABP-300/ABP-300.mp4").exists());
    assert_eq!(repo.list_works().unwrap().len(), 1);
}

#[test]
fn process_next_routes_missing_code_to_holding() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    std::fs::write(inbox.join("random.mp4"), b"random-video").unwrap();
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);
    daemon.scan_now().unwrap();

    let report = daemon.process_next().unwrap();

    assert_eq!(report.processed, 1);
    assert_eq!(report.holding, 1);
    assert_eq!(repo.list_holding().unwrap().len(), 1);
    assert!(repo.list_exceptions().unwrap().is_empty());
}

#[test]
fn process_next_routes_scrape_failure_to_exception_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    std::fs::write(inbox.join("ABP-301.mp4"), b"video-301").unwrap();
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);
    daemon.scan_now().unwrap();

    let report = daemon.process_next().unwrap();

    assert_eq!(report.processed, 1);
    assert_eq!(report.exceptions, 1);
    assert_eq!(repo.list_exceptions().unwrap()[0].kind, ExceptionKind::ScrapeFailed);
}
```

- [ ] **Step 2: 运行失败测试**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: FAIL，提示 `pause`、`resume`、`process_next` 不存在。

- [ ] **Step 3: 实现暂停、恢复和单文件处理**

Append imports in `src-tauri/src/daemon.rs`:

```rust
use crate::domain::PipelineOutcome;
use crate::pipeline::AutoPipeline;
```

Add to `impl HeadlessDaemon<'a>`:

```rust
    /// Pause future scans and processing without clearing queued files.
    pub fn pause(&mut self) {
        self.state = DaemonState::Paused;
    }

    /// Resume a paused daemon and keep any files that were already queued.
    pub fn resume(&mut self) {
        if self.state == DaemonState::Paused {
            self.state = DaemonState::Idle;
        }
    }

    /// Process one queued file through the Stage 2 AutoPipeline.
    pub fn process_next(&mut self) -> Result<ProcessReport> {
        if self.state == DaemonState::Paused {
            return Ok(ProcessReport::default());
        }

        let Some(queued) = self.queue.pop_front() else {
            return Ok(ProcessReport::default());
        };
        self.queued_keys.remove(&queue_key(&queued.path));

        self.state = DaemonState::Processing;
        let completed = queued.into_completed_file();
        let pipeline = AutoPipeline {
            repo: self.repo,
            archive_root: self.config.archive_root.clone(),
            asset_roots: self.config.asset_roots.clone(),
            scrapers: ScrapeCoordinator {
                sources: self.scrapers.sources.clone(),
            },
        };

        let mut report = ProcessReport {
            processed: 1,
            ..ProcessReport::default()
        };

        match pipeline.process_completed_file(completed) {
            Ok(outcome) => self.apply_outcome(&outcome, &mut report),
            Err(error) => {
                report.failed = 1;
                self.last_error = Some(error.to_string());
                self.state = DaemonState::Error;
                self.processed += 1;
                return Ok(report);
            }
        }

        self.processed += 1;
        self.state = DaemonState::Idle;
        Ok(report)
    }

    fn apply_outcome(&mut self, outcome: &PipelineOutcome, report: &mut ProcessReport) {
        match outcome.status.as_str() {
            "archived" => report.archived = 1,
            "holding" => report.holding = 1,
            "exception" => report.exceptions = 1,
            "failed" => report.failed = 1,
            other => {
                report.failed = 1;
                self.last_error = Some(format!("unknown pipeline outcome: {other}"));
            }
        }
    }
```

Add after the `impl HeadlessDaemon<'a>` block:

```rust
impl QueuedFile {
    /// Convert a queued file back into the Stage 2 CompletedFile DTO.
    pub fn into_completed_file(self) -> CompletedFile {
        CompletedFile {
            path: self.path,
            file_name: self.file_name,
            size_bytes: self.size_bytes,
            file_hash: self.file_hash,
        }
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: PASS。若 `Vec<&dyn ScraperSource>` clone 触发类型问题，把 `ScrapeCoordinator` 构造改为：

```rust
let pipeline = AutoPipeline {
    repo: self.repo,
    archive_root: self.config.archive_root.clone(),
    asset_roots: self.config.asset_roots.clone(),
    scrapers: ScrapeCoordinator {
        sources: self.scrapers.sources.iter().copied().collect(),
    },
};
```

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/daemon.rs src-tauri/tests/daemon.rs
git commit -m "串联无头守护单文件处理"
```

---

## Task 4: run_once 聚合与操作失败计数

**Files:**
- Modify: `src-tauri/src/daemon.rs`
- Modify: `src-tauri/tests/daemon.rs`
- Modify: `docs/superpowers/specs/2026-06-25-media-manager-stage3-headless-daemon-design.md`

**Interfaces:**
- `HeadlessDaemon::run_once(&mut self) -> anyhow::Result<RunOnceReport>`

- [ ] **Step 1: 写失败测试**

Append to `src-tauri/tests/daemon.rs`:

```rust
#[test]
fn run_once_scans_and_processes_mixed_inbox_with_deterministic_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    std::fs::write(inbox.join("ABP-300.mp4"), b"video-300").unwrap();
    std::fs::write(inbox.join("random.mp4"), b"random-video").unwrap();
    std::fs::write(inbox.join("ABP-301.mp4"), b"video-301").unwrap();
    std::fs::write(inbox.join("notes.txt"), b"not-video").unwrap();
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);

    let report = daemon.run_once().unwrap();

    assert_eq!(report.scan.queued_files, 3);
    assert_eq!(report.process.processed, 3);
    assert_eq!(report.process.archived, 1);
    assert_eq!(report.process.holding, 1);
    assert_eq!(report.process.exceptions, 1);
    assert_eq!(report.process.failed, 0);
    assert_eq!(daemon.status().queued, 0);
}

#[test]
fn operational_archive_failure_counts_failed_without_content_exception() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    let inbox = tmp.path().join("inbox");
    let archive_file = tmp.path().join("archive-is-a-file");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::write(&archive_file, b"not a directory").unwrap();
    std::fs::write(inbox.join("ABP-300.mp4"), b"video-300").unwrap();
    repo.set_source_roots(&[inbox]).unwrap();
    repo.set_archive_root(&archive_file).unwrap();
    repo.set_resource_pool_dirs(&[]).unwrap();
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);

    let report = daemon.run_once().unwrap();

    assert_eq!(report.scan.queued_files, 1);
    assert_eq!(report.process.processed, 1);
    assert_eq!(report.process.failed, 1);
    assert!(repo.list_exceptions().unwrap().is_empty());
    assert_eq!(repo.list_pipeline_runs().unwrap()[0].status, "failed");
    assert_eq!(daemon.status().state, DaemonState::Error);
    assert!(daemon.status().last_error.is_some());
}
```

- [ ] **Step 2: 运行失败测试**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: FAIL，提示 `run_once` 不存在。

- [ ] **Step 3: 实现 run_once 与 report 合并**

Add to `impl HeadlessDaemon<'a>`:

```rust
    /// Run one synchronous daemon pass: scan once, then drain the queue until
    /// it is empty, paused, or a processing error is recorded.
    pub fn run_once(&mut self) -> Result<RunOnceReport> {
        let scan = self.scan_now()?;
        let mut process = ProcessReport::default();

        while self.state != DaemonState::Paused && !self.queue.is_empty() {
            let next = self.process_next()?;
            process.processed += next.processed;
            process.archived += next.archived;
            process.holding += next.holding;
            process.exceptions += next.exceptions;
            process.failed += next.failed;
            if next.failed > 0 {
                break;
            }
        }

        Ok(RunOnceReport { scan, process })
    }
```

Update Stage 3 design doc section "公开类型" to include:

```rust
pub struct RunOnceReport {
    pub scan: ScanReport,
    pub process: ProcessReport,
}
```

Update "run_once" control semantics to say it returns `RunOnceReport`, not only `ProcessReport`.

- [ ] **Step 4: 运行测试确认通过**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/daemon.rs src-tauri/tests/daemon.rs docs/superpowers/specs/2026-06-25-media-manager-stage3-headless-daemon-design.md
git commit -m "补充无头守护单轮运行报告"
```

---

## Task 5: CLI-only smoke 与完整验证

**Files:**
- Create: `src-tauri/examples/stage3_daemon_smoke.rs`
- Modify: `HANDOFF.md`

**Interfaces:**
- `cargo run --manifest-path src-tauri/Cargo.toml --example stage3_daemon_smoke -j 1`

- [ ] **Step 1: 写 smoke example**

Create `src-tauri/examples/stage3_daemon_smoke.rs`:

```rust
use media_manager::daemon::{CompletionPolicy, DaemonConfig, HeadlessDaemon};
use media_manager::domain::ScrapedWorkMetadata;
use media_manager::pipeline::{ScrapeCoordinator, ScraperSource};
use media_manager::storage::Repository;
use std::time::Duration;

/// Deterministic scraper for daemon smoke; it succeeds for ABP-300 and lets
/// every other recognized code exercise the exception path.
struct SmokeScraper;

impl ScraperSource for SmokeScraper {
    fn name(&self) -> &str {
        "smoke"
    }

    fn lookup(&self, normalized_code: &str) -> anyhow::Result<Option<ScrapedWorkMetadata>> {
        if normalized_code == "ABP-300" {
            Ok(Some(ScrapedWorkMetadata {
                source: self.name().to_string(),
                normalized_code: normalized_code.to_string(),
                title: "ABP-300 smoke title".to_string(),
                original_title: Some("ABP-300 smoke original".to_string()),
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
    std::fs::create_dir_all(&archive)?;
    std::fs::write(inbox.join("ABP-300.mp4"), b"video-300")?;
    std::fs::write(inbox.join("random.mp4"), b"random-video")?;
    std::fs::write(inbox.join("ABP-301.mp4"), b"video-301")?;
    std::fs::write(inbox.join("notes.txt"), b"not-video")?;

    let repo = Repository::open(&db)?;
    repo.migrate()?;
    repo.set_source_roots(&[inbox])?;
    repo.set_archive_root(&archive)?;
    repo.set_resource_pool_dirs(&[assets])?;

    let scraper = SmokeScraper;
    let config = DaemonConfig::load(&repo)?;
    let mut daemon = HeadlessDaemon::with_completion_policy(
        &repo,
        config,
        ScrapeCoordinator {
            sources: vec![&scraper],
        },
        CompletionPolicy {
            sample_delay: Duration::ZERO,
        },
    );

    let report = daemon.run_once()?;

    println!("stage3_daemon_smoke=completed");
    println!("queued={}", report.scan.queued_files);
    println!("archived={}", report.process.archived);
    println!("holding={}", report.process.holding);
    println!("exceptions={}", report.process.exceptions);
    println!("failed={}", report.process.failed);
    println!("no_real_resources_required=true");
    Ok(())
}
```

- [ ] **Step 2: 运行阶段 3 focused tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
```

Expected: PASS。

- [ ] **Step 3: 运行完整后端测试**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -j 1
```

Expected: PASS。现有 `resource_pool.rs` warning 若仍出现，不作为阶段 3 失败。

- [ ] **Step 4: 运行 CLI-only smoke**

Run:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --example stage3_daemon_smoke -j 1
```

Expected output includes:

```text
stage3_daemon_smoke=completed
queued=3
archived=1
holding=1
exceptions=1
failed=0
no_real_resources_required=true
```

- [ ] **Step 5: 更新 HANDOFF**

Update `HANDOFF.md`:

```markdown
**阶段 3（无头守护核心）已实现并验证。**

阶段 3 提供纯 Rust、无 WebView2 的 daemon core：从 SQLite settings 读取 source roots / archive root / resource pool dirs，扫描完成视频，维护内存队列，支持 pause/resume/status/process_next/run_once，并调用阶段 2 AutoPipeline 完成归档、搁置、异常路由和失败计数。

新增验证：

- `cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`
- `cargo run --manifest-path src-tauri/Cargo.toml --example stage3_daemon_smoke -j 1`
```

- [ ] **Step 6: 提交**

```bash
git add src-tauri/examples/stage3_daemon_smoke.rs HANDOFF.md
git commit -m "新增阶段3无头守护冒烟验证"
```

---

## Done Criteria For Stage 3

- `DaemonConfig::load` 从 SQLite settings 读取 `source_roots`、`archive_root`、`resource_pool_dirs`，缺 `archive_root` 返回明确错误。
- `HeadlessDaemon::status` 不读写 SQLite，能反映 state、queue、processed、last_error。
- `scan_now` 递归扫描 source roots，跳过不存在 root、非视频、`.aria2` 未完成文件，只排队稳定视频。
- 队列顺序确定，重复扫描不重复排队。
- `pause` 不清队列但阻止 scan/process；`resume` 回到 idle。
- `process_next` 调用阶段 2 `AutoPipeline`，正确计数 archived / holding / exceptions / failed。
- 操作性归档失败计入 failed，`pipeline_runs.status = "failed"`，不进入内容异常队列。
- `run_once` 能扫描并处理混合 inbox，返回确定 `RunOnceReport`。
- `stage3_daemon_smoke` 使用临时目录和 fake scraper，输出固定摘要，不启动 Tauri/WebView2。
- 以下命令通过：
  - `cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1`
  - `cargo test --manifest-path src-tauri/Cargo.toml -j 1`
  - `cargo run --manifest-path src-tauri/Cargo.toml --example stage3_daemon_smoke -j 1`

## Self-Review Checklist

- Spec coverage: 覆盖阶段 3 设计中的配置加载、完成采样、扫描、队列、状态、暂停/恢复、process_next、run_once、失败边界、smoke；明确排除 HTTP/WebSocket、托盘、自启、真实 scraper、aria2 RPC、前端连线。
- Placeholder scan: 不包含未完成标记或空泛占位；每个任务都有具体文件、测试、实现片段、命令和提交信息。
- Type consistency: `DaemonConfig`、`CompletionPolicy`、`DaemonState`、`DaemonStatus`、`QueuedFile`、`ScanReport`、`ProcessReport`、`RunOnceReport`、`HeadlessDaemon` 在测试、实现和 smoke 中命名一致。
