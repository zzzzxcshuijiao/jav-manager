use media_manager::aria2::Aria2Settings;
use media_manager::diagnostics::{
    build_diagnostic_snapshot, export_diagnostic_snapshot, redact_diagnostic_value,
    redact_proxy_url, DiagnosticLevel, DiagnosticSnapshotInput, DiagnosticsWriter,
};
use media_manager::domain::{
    Exception, ExceptionKind, ExceptionStatus, HoldingEntry, HoldingReason, PipelineRun, ScrapeJob,
    ScrapeStatus,
};
use media_manager::remote_scraper::RemoteScraperSettings;
use media_manager::storage::Repository;
use serde_json::json;
use std::path::Path;

fn open_temp_repo(path: &Path) -> Repository {
    let repo = Repository::open(path).unwrap();
    repo.migrate().unwrap();
    repo
}

#[test]
fn diagnostics_writer_appends_jsonl_and_reads_tail() {
    let tmp = tempfile::tempdir().unwrap();
    let writer =
        DiagnosticsWriter::new_with_limits(tmp.path().join("logs"), 64 * 1024, 2).unwrap();

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
    let writer =
        DiagnosticsWriter::new_with_limits(tmp.path().join("logs"), 64 * 1024, 2).unwrap();

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
    let exported = std::fs::read_to_string(Path::new(&result.path)).unwrap();

    assert_eq!(result.pipeline_runs, 1);
    assert_eq!(result.scrape_jobs, 1);
    assert_eq!(result.logs, 1);
    assert!(exported.contains("\"aria2_secret_configured\": true"));
    assert!(exported.contains("http://***@127.0.0.1:8888"));
    assert!(!exported.contains("aria-secret"));
    assert!(!exported.contains("user:pass"));
}
