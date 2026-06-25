use media_manager::daemon_control::{
    build_daemon_status, list_exception_entries, list_holding_entries, list_recent_pipeline_runs,
    resolve_exception_entry, run_daemon_once, DaemonControlRuntime, ExamplePipelineScraper,
    MetadataSource,
};
use media_manager::domain::{
    Exception, ExceptionKind, ExceptionStatus, HoldingEntry, HoldingReason, PipelineRun,
};
use media_manager::pipeline::ScraperSource;
use media_manager::storage::Repository;
use std::path::{Path, PathBuf};

fn open_repo(path: &Path) -> Repository {
    let repo = Repository::open(path).unwrap();
    repo.migrate().unwrap();
    repo
}

fn configured_repo(tmp: &tempfile::TempDir) -> (Repository, PathBuf, PathBuf, PathBuf) {
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

#[test]
fn example_pipeline_scraper_generates_local_metadata_without_network() {
    let scraper = ExamplePipelineScraper;

    let metadata = scraper.lookup("ABP-404").unwrap().unwrap();

    assert_eq!(scraper.name(), "example");
    assert_eq!(metadata.normalized_code, "ABP-404");
    assert_eq!(metadata.source, "example");
    assert!(metadata.title.contains("ABP-404"));
    assert!(metadata.cover_path.is_none());
}

#[test]
fn daemon_status_reports_configuration_and_queue_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, _inbox, archive, assets) = configured_repo(&tmp);
    repo.record_exception(&Exception {
        id: None,
        object_path: "H:/Inbox/ABP-001.mp4".to_string(),
        kind: ExceptionKind::ScrapeFailed,
        evidence_json: "{}".to_string(),
        status: ExceptionStatus::Open,
        created_at: None,
        resolved_at: None,
    })
    .unwrap();
    repo.add_holding(&HoldingEntry {
        id: None,
        path: "H:/Inbox/no-code.mp4".to_string(),
        file_name: "no-code.mp4".to_string(),
        size_bytes: 10,
        reason: HoldingReason::NoCode,
        created_at: None,
    })
    .unwrap();
    repo.record_pipeline_run(&PipelineRun {
        id: None,
        file_path: "H:/Inbox/ABP-001.mp4".to_string(),
        started_at: None,
        finished_at: None,
        steps_json: "[]".to_string(),
        status: "exception".to_string(),
        error: None,
    })
    .unwrap();
    let runtime = DaemonControlRuntime {
        paused: false,
        processed: 7,
        last_error: Some("previous error".to_string()),
    };

    let status = build_daemon_status(&repo, &runtime, true).unwrap();

    assert!(status.configured);
    assert_eq!(status.state, "Idle");
    assert_eq!(status.archive_root, Some(archive.to_string_lossy().to_string()));
    assert_eq!(status.asset_roots, vec![assets.to_string_lossy().to_string()]);
    assert_eq!(status.processed, 7);
    assert_eq!(status.open_exceptions, 1);
    assert_eq!(status.holding_items, 1);
    assert_eq!(status.recent_runs, 1);
    assert_eq!(status.metadata_source, MetadataSource::Example);
    assert_eq!(status.last_error.as_deref(), Some("previous error"));
}

#[test]
fn run_once_requires_example_metadata_source_before_touching_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, _archive, _assets) = configured_repo(&tmp);
    let video = inbox.join("ABP-404.mp4");
    std::fs::write(&video, b"stable video bytes").unwrap();
    let mut runtime = DaemonControlRuntime::default();

    let error = run_daemon_once(&repo, &mut runtime, false).unwrap_err();

    assert!(error.to_string().contains("示例元数据源未开启"));
    assert!(video.exists());
    assert_eq!(repo.list_pipeline_runs().unwrap().len(), 0);
}

#[test]
fn run_once_archives_with_example_scraper_when_enabled() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, archive, _assets) = configured_repo(&tmp);
    let video = inbox.join("ABP-404.mp4");
    std::fs::write(&video, b"stable video bytes").unwrap();
    let mut runtime = DaemonControlRuntime::default();

    let report = run_daemon_once(&repo, &mut runtime, true).unwrap();

    assert_eq!(report.scan.queued_files, 1);
    assert_eq!(report.process.archived, 1);
    assert_eq!(runtime.processed, 1);
    assert!(archive.join("ABP-404").join("ABP-404.mp4").exists());
    assert_eq!(repo.list_pipeline_runs().unwrap()[0].status, "archived");
}

#[test]
fn queue_list_helpers_return_repository_rows_and_resolve_exceptions() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, _inbox, _archive, _assets) = configured_repo(&tmp);
    let exception_id = repo
        .record_exception(&Exception {
            id: None,
            object_path: "H:/Inbox/ABP-404.mp4".to_string(),
            kind: ExceptionKind::ScrapeFailed,
            evidence_json: "{\"source\":\"example\"}".to_string(),
            status: ExceptionStatus::Open,
            created_at: None,
            resolved_at: None,
        })
        .unwrap();
    repo.add_holding(&HoldingEntry {
        id: None,
        path: "H:/Inbox/manual.mp4".to_string(),
        file_name: "manual.mp4".to_string(),
        size_bytes: 42,
        reason: HoldingReason::Unrecognizable,
        created_at: None,
    })
    .unwrap();
    repo.record_pipeline_run(&PipelineRun {
        id: None,
        file_path: "H:/Inbox/ABP-404.mp4".to_string(),
        started_at: None,
        finished_at: None,
        steps_json: "[]".to_string(),
        status: "exception".to_string(),
        error: Some("not found".to_string()),
    })
    .unwrap();

    assert_eq!(list_exception_entries(&repo).unwrap().len(), 1);
    assert_eq!(list_holding_entries(&repo).unwrap().len(), 1);
    assert_eq!(list_recent_pipeline_runs(&repo).unwrap().len(), 1);

    resolve_exception_entry(&repo, exception_id, ExceptionStatus::Resolved).unwrap();

    assert_eq!(
        list_exception_entries(&repo).unwrap()[0].status,
        ExceptionStatus::Resolved
    );
}
