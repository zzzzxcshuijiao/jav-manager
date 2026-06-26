use media_manager::aria2::Aria2Settings;
use media_manager::control_service_host::ControlServiceHostStatus;
use media_manager::daemon_control::{build_daemon_status, DaemonControlRuntime};
use media_manager::remote_scraper::RemoteScraperSettings;
use media_manager::self_check::{
    build_config_self_check_items, run_pipeline_self_check, SelfCheckOverall, SelfCheckSeverity,
};
use media_manager::storage::Repository;

fn open_repo(path: &std::path::Path) -> Repository {
    let repo = Repository::open(path).unwrap();
    repo.migrate().unwrap();
    repo
}

#[test]
fn config_self_check_reports_pass_warn_and_fail_items() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    repo.set_metadata_provider_enabled(false).unwrap();
    repo.set_aria2_settings(&Aria2Settings {
        enabled: true,
        host: "127.0.0.1".to_string(),
        port: 6800,
        path: "/jsonrpc".to_string(),
        secret: Some("secret".to_string()),
        timeout_ms: 5000,
        poll_interval_secs: 30,
        tracked_gids: Vec::new(),
    })
    .unwrap();
    repo.set_remote_scraper_settings(&RemoteScraperSettings {
        enabled: true,
        include_example_fallback: false,
        sources: Vec::new(),
        ..RemoteScraperSettings::default()
    })
    .unwrap();
    let runtime = DaemonControlRuntime::default();
    let daemon = build_daemon_status(&repo, &runtime, false).unwrap();
    let control = ControlServiceHostStatus {
        running: true,
        host: "127.0.0.1".to_string(),
        port: Some(13508),
        discovery_path: tmp
            .path()
            .join("control-service.json")
            .to_string_lossy()
            .to_string(),
        last_error: None,
    };

    let checks = build_config_self_check_items(&repo, Some(control), Some(daemon), true).unwrap();

    assert!(checks
        .iter()
        .any(|item| item.id == "control_service" && item.severity == SelfCheckSeverity::Pass));
    assert!(checks
        .iter()
        .any(|item| item.id == "configured_roots" && item.severity == SelfCheckSeverity::Warn));
    assert!(checks
        .iter()
        .any(|item| item.id == "metadata_source" && item.severity == SelfCheckSeverity::Fail));
    assert!(checks
        .iter()
        .any(|item| item.id == "aria2_settings" && item.severity == SelfCheckSeverity::Warn));
    assert!(checks.iter().any(
        |item| item.id == "remote_scraper_settings" && item.severity == SelfCheckSeverity::Fail
    ));
    assert!(checks
        .iter()
        .any(|item| item.id == "diagnostics" && item.severity == SelfCheckSeverity::Pass));
}

#[test]
fn pipeline_self_check_archives_sandbox_video_without_touching_real_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let app_data = tmp.path().join("app-data");
    let real_repo = open_repo(&tmp.path().join("real.sqlite"));
    let real_inbox = tmp.path().join("real-inbox");
    let real_archive = tmp.path().join("real-archive");
    std::fs::create_dir_all(&real_inbox).unwrap();
    std::fs::create_dir_all(&real_archive).unwrap();
    real_repo.set_source_roots(&[real_inbox]).unwrap();
    real_repo.set_archive_root(&real_archive).unwrap();
    real_repo.set_metadata_provider_enabled(false).unwrap();
    real_repo
        .set_remote_scraper_settings(&RemoteScraperSettings {
            enabled: false,
            include_example_fallback: true,
            ..RemoteScraperSettings::default()
        })
        .unwrap();
    let control = ControlServiceHostStatus {
        running: true,
        host: "127.0.0.1".to_string(),
        port: Some(13508),
        discovery_path: tmp
            .path()
            .join("control-service.json")
            .to_string_lossy()
            .to_string(),
        last_error: None,
    };

    let report = run_pipeline_self_check(&app_data, &real_repo, Some(control), None, true).unwrap();

    assert_eq!(report.overall, SelfCheckOverall::Pass);
    assert!(report
        .checks
        .iter()
        .any(|item| item.id == "sandbox_archive" && item.severity == SelfCheckSeverity::Pass));
    let sandbox = report.sandbox.unwrap();
    let archived = sandbox.archived_path.unwrap();
    assert!(std::path::Path::new(&archived).exists());
    assert_eq!(sandbox.pipeline_status.as_deref(), Some("archived"));
    assert_eq!(real_repo.list_works().unwrap().len(), 0);
    assert_eq!(real_repo.list_pipeline_runs().unwrap().len(), 0);
}
