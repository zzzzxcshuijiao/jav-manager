use media_manager::control_service::{ControlServiceConfig, ControlServiceRuntime};
use media_manager::domain::{
    Exception, ExceptionKind, ExceptionStatus, HoldingEntry, HoldingReason, PipelineRun,
};
use media_manager::storage::Repository;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;

fn open_repo(path: &Path) -> Repository {
    let repo = Repository::open(path).unwrap();
    repo.migrate().unwrap();
    repo
}

#[test]
fn config_rejects_non_loopback_hosts() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    let config = ControlServiceConfig {
        host: "0.0.0.0".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };

    let error = ControlServiceRuntime::new(repo, config).unwrap_err();

    assert!(error.to_string().contains("loopback"));
}

#[test]
fn discovery_document_contains_bound_endpoint_and_token() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };
    let runtime = ControlServiceRuntime::new(repo, config).unwrap();

    let discovery = runtime.discovery_for_port(45123);

    assert_eq!(discovery.host, "127.0.0.1");
    assert_eq!(discovery.port, 45123);
    assert_eq!(discovery.token, "stage5-token");
    assert_eq!(discovery.base_url, "http://127.0.0.1:45123");
    assert_eq!(discovery.service, "media-manager-control");
}

fn request(port: u16, raw: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(raw.as_bytes()).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn configured_repo(tmp: &tempfile::TempDir) -> Repository {
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    let inbox = tmp.path().join("inbox");
    let archive = tmp.path().join("archive");
    let assets = tmp.path().join("assets");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    std::fs::create_dir_all(&assets).unwrap();
    repo.set_source_roots(&[inbox]).unwrap();
    repo.set_archive_root(&archive).unwrap();
    repo.set_resource_pool_dirs(&[assets]).unwrap();
    repo
}

fn authorized_get(port: u16, path: &str) -> String {
    request(
        port,
        &format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer stage5-token\r\nOrigin: http://127.0.0.1:1420\r\n\r\n"
        ),
    )
}

fn authorized_post(port: u16, path: &str) -> String {
    request(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer stage5-token\r\nOrigin: tauri://localhost\r\nContent-Length: 0\r\n\r\n"
        ),
    )
}

#[test]
fn service_writes_discovery_file_and_serves_health_without_token() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    let discovery_path = tmp.path().join("control.json");
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: discovery_path.clone(),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };
    let handle = ControlServiceRuntime::new(repo, config)
        .unwrap()
        .start()
        .unwrap();

    let discovery: media_manager::control_service::ControlServiceDiscovery =
        serde_json::from_str(&std::fs::read_to_string(&discovery_path).unwrap()).unwrap();
    let response = request(
        handle.port(),
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
    );

    assert_eq!(discovery.port, handle.port());
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("\"service\":\"media-manager-control\""));

    handle.shutdown().unwrap();
}

#[test]
fn v1_routes_require_token_and_allow_pause_resume_status() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = configured_repo(&tmp);
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };
    let handle = ControlServiceRuntime::new(repo, config)
        .unwrap()
        .start()
        .unwrap();

    let missing = request(
        handle.port(),
        "GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
    );
    let wrong = request(
        handle.port(),
        "GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer wrong\r\n\r\n",
    );
    let blocked_origin = request(
        handle.port(),
        "GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer stage5-token\r\nOrigin: https://evil.example\r\n\r\n",
    );
    let status = authorized_get(handle.port(), "/v1/status");
    let paused = authorized_post(handle.port(), "/v1/pause");
    let resumed = authorized_post(handle.port(), "/v1/resume");

    assert!(missing.starts_with("HTTP/1.1 401 Unauthorized"));
    assert!(wrong.starts_with("HTTP/1.1 403 Forbidden"));
    assert!(blocked_origin.starts_with("HTTP/1.1 403 Forbidden"));
    assert!(status.contains("\"state\":\"Idle\""));
    assert!(paused.contains("\"state\":\"Paused\""));
    assert!(resumed.contains("\"state\":\"Idle\""));

    handle.shutdown().unwrap();
}

#[test]
fn run_once_and_queue_routes_use_existing_daemon_control_helpers() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = configured_repo(&tmp);
    let inbox = tmp.path().join("inbox");
    std::fs::write(inbox.join("ABP-501.mp4"), b"stable video bytes").unwrap();
    let exception_id = repo
        .record_exception(&Exception {
            id: None,
            object_path: "H:/Inbox/ABP-500.mp4".to_string(),
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
        file_path: "H:/Inbox/ABP-500.mp4".to_string(),
        started_at: None,
        finished_at: None,
        steps_json: "[]".to_string(),
        status: "exception".to_string(),
        error: Some("not found".to_string()),
    })
    .unwrap();
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: true,
    };
    let handle = ControlServiceRuntime::new(repo, config)
        .unwrap()
        .start()
        .unwrap();

    let run = authorized_post(handle.port(), "/v1/run-once");
    let holding = authorized_get(handle.port(), "/v1/holding");
    let exceptions = authorized_get(handle.port(), "/v1/exceptions");
    let resolved = authorized_post(
        handle.port(),
        &format!("/v1/exceptions/{exception_id}/resolve"),
    );
    let runs = authorized_get(handle.port(), "/v1/runs");

    assert!(run.contains("\"queued_files\":1"));
    assert!(run.contains("\"archived\":1"));
    assert!(holding.contains("manual.mp4"));
    assert!(exceptions.contains("ABP-500.mp4"));
    assert!(resolved.contains("\"status\":\"Resolved\""));
    assert!(runs.contains("\"status\":\"archived\""));

    handle.shutdown().unwrap();
}

#[test]
fn run_once_without_metadata_source_returns_error_and_keeps_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = configured_repo(&tmp);
    let inbox = tmp.path().join("inbox");
    let video = inbox.join("ABP-599.mp4");
    std::fs::write(&video, b"stable video bytes").unwrap();
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };
    let handle = ControlServiceRuntime::new(repo, config)
        .unwrap()
        .start()
        .unwrap();

    let response = authorized_post(handle.port(), "/v1/run-once");

    assert!(response.starts_with("HTTP/1.1 500 Internal Server Error"));
    assert!(response.contains("示例元数据源未开启"));
    assert!(video.exists());

    handle.shutdown().unwrap();
}

#[test]
fn unknown_v1_route_returns_not_found_json() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = configured_repo(&tmp);
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };
    let handle = ControlServiceRuntime::new(repo, config)
        .unwrap()
        .start()
        .unwrap();

    let response = authorized_get(handle.port(), "/v1/unknown");

    assert!(response.starts_with("HTTP/1.1 404 Not Found"));
    assert!(response.contains("\"ok\":false"));

    handle.shutdown().unwrap();
}

#[test]
fn invalid_exception_resolve_id_returns_bad_request_json() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = configured_repo(&tmp);
    let config = ControlServiceConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        discovery_path: tmp.path().join("control.json"),
        token: Some("stage5-token".to_string()),
        metadata_provider_enabled: false,
    };
    let handle = ControlServiceRuntime::new(repo, config)
        .unwrap()
        .start()
        .unwrap();

    let response = authorized_post(handle.port(), "/v1/exceptions/not-a-number/resolve");

    assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(response.contains("invalid exception id"));

    handle.shutdown().unwrap();
}
