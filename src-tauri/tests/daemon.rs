use media_manager::aria2::{
    Aria2Client, Aria2PollReport, Aria2RpcEndpoint, Aria2Settings, Aria2Transport,
};
use media_manager::daemon::{CompletionPolicy, DaemonConfig, DaemonState, HeadlessDaemon};
use media_manager::domain::{ExceptionKind, ScrapedWorkMetadata};
use media_manager::pipeline::{ScrapeCoordinator, ScraperSource};
use media_manager::storage::Repository;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct FakeScraper;

#[derive(Clone)]
struct StaticAria2Transport {
    response: String,
    requests: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone)]
struct RoutingAria2Transport {
    video_path: std::path::PathBuf,
    calls: Arc<Mutex<Vec<String>>>,
}

impl StaticAria2Transport {
    fn new(response: String) -> Self {
        Self {
            response,
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl RoutingAria2Transport {
    fn new(video_path: std::path::PathBuf) -> Self {
        Self {
            video_path,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Aria2Transport for StaticAria2Transport {
    fn post_json(&self, _endpoint: &Aria2RpcEndpoint, body: &str) -> anyhow::Result<String> {
        self.requests.lock().unwrap().push(body.to_string());
        Ok(self.response.clone())
    }
}

impl Aria2Transport for RoutingAria2Transport {
    fn post_json(&self, _endpoint: &Aria2RpcEndpoint, body: &str) -> anyhow::Result<String> {
        let request: serde_json::Value = serde_json::from_str(body).unwrap();
        let gid = request["params"][0].as_str().unwrap().to_string();
        self.calls.lock().unwrap().push(gid.clone());
        if gid == "gid-error" {
            anyhow::bail!("rpc failed for gid-error");
        }
        let complete = gid == "gid-complete";
        let files = if complete {
            serde_json::json!([
                {"path": self.video_path.to_string_lossy().to_string(), "length": "9", "completedLength": "9", "selected": "true"}
            ])
        } else {
            serde_json::json!([])
        };
        Ok(serde_json::json!({
            "jsonrpc": "2.0",
            "id": "media-manager-tell-status",
            "result": {
                "gid": gid,
                "status": if complete { "complete" } else { "active" },
                "totalLength": "9",
                "completedLength": if complete { "9" } else { "3" },
                "files": files
            }
        })
        .to_string())
    }
}

fn aria2_client(response: String) -> Aria2Client<StaticAria2Transport> {
    Aria2Client::new(
        Aria2RpcEndpoint::loopback_default(None),
        StaticAria2Transport::new(response),
    )
}

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

fn enabled_aria2_settings(gids: Vec<&str>) -> Aria2Settings {
    Aria2Settings {
        enabled: true,
        host: "127.0.0.1".to_string(),
        port: 6800,
        path: "/jsonrpc".to_string(),
        secret: None,
        timeout_ms: 5000,
        poll_interval_secs: 30,
        tracked_gids: gids.into_iter().map(ToString::to_string).collect(),
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

fn configured_repo(
    tmp: &tempfile::TempDir,
) -> (
    Repository,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
) {
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
    assert_eq!(
        repo.list_exceptions().unwrap()[0].kind,
        ExceptionKind::ScrapeFailed
    );
}

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
fn repeated_scan_does_not_duplicate_already_held_file_in_same_daemon() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    std::fs::write(inbox.join("random.mp4"), b"random-video").unwrap();
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);

    let first = daemon.run_once().unwrap();
    let second = daemon.run_once().unwrap();

    assert_eq!(first.process.holding, 1);
    assert_eq!(second.scan.queued_files, 0);
    assert_eq!(second.process.processed, 0);
    assert_eq!(repo.list_holding().unwrap().len(), 1);
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

#[test]
fn scan_aria2_gid_queues_completed_selected_video() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    let video = inbox.join("ABP-300.mp4");
    std::fs::write(&video, b"video-300").unwrap();
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "media-manager-tell-status",
        "result": {
            "gid": "gid-1",
            "status": "complete",
            "totalLength": "9",
            "completedLength": "9",
            "files": [
                {"path": video.to_string_lossy().to_string(), "length": "9", "completedLength": "9", "selected": "true"}
            ]
        }
    })
    .to_string();
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
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "media-manager-tell-status",
        "result": {
            "gid": "gid-1",
            "status": "complete",
            "totalLength": "9",
            "completedLength": "9",
            "files": [
                {"path": video.to_string_lossy().to_string(), "length": "9", "completedLength": "9", "selected": "true"}
            ]
        }
    })
    .to_string();
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

#[test]
fn poll_aria2_once_aggregates_completed_unfinished_and_failed_gids() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    let video = inbox.join("ABP-300.mp4");
    std::fs::write(&video, b"video-300").unwrap();
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);
    let transport = RoutingAria2Transport::new(video);

    let report = daemon
        .poll_aria2_once(
            &enabled_aria2_settings(vec!["gid-complete", "gid-active", "gid-error"]),
            transport,
        )
        .unwrap();

    assert_eq!(report.enabled, true);
    assert_eq!(report.attempted_gids, 3);
    assert_eq!(report.completed_gids, 1);
    assert_eq!(report.queued_files, 1);
    assert_eq!(report.failed_gids, 1);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(daemon.status().queued, 1);
    assert_eq!(daemon.status().state, DaemonState::Error);
}

#[test]
fn poll_aria2_once_skips_transport_when_disabled_or_paused() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _, _) = configured_repo(&tmp);
    let scraper = FakeScraper;
    let mut daemon = daemon(&repo, &scraper);
    let transport = RoutingAria2Transport::new(inbox.join("ABP-300.mp4"));

    let disabled = daemon
        .poll_aria2_once(&Aria2Settings::default(), transport.clone())
        .unwrap();
    daemon.pause();
    let paused = daemon
        .poll_aria2_once(&enabled_aria2_settings(vec!["gid-complete"]), transport.clone())
        .unwrap();

    assert_eq!(disabled, Aria2PollReport::default());
    assert_eq!(paused.attempted_gids, 0);
    assert!(transport.calls.lock().unwrap().is_empty());
}
