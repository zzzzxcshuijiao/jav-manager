use media_manager::control_service::{ControlServiceConfig, ControlServiceRuntime};
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
