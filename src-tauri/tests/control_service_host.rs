use media_manager::control_service::read_control_service_discovery;
use media_manager::control_service_host::{
    build_control_service_config, control_service_host_status, start_control_service_host,
    CONTROL_SERVICE_HOST, CONTROL_SERVICE_PORT,
};
use std::io::{Read, Write};
use std::net::TcpStream;

fn request(port: u16, raw: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(raw.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn host_config_uses_app_data_discovery_and_loopback_random_port() {
    let tmp = tempfile::tempdir().unwrap();

    let config = build_control_service_config(tmp.path(), true);

    assert_eq!(config.host, CONTROL_SERVICE_HOST);
    assert_eq!(config.port, CONTROL_SERVICE_PORT);
    assert_eq!(config.discovery_path, tmp.path().join("control-service.json"));
    assert!(config.metadata_provider_enabled);
}

#[test]
fn host_status_reports_running_and_stopped_states() {
    let tmp = tempfile::tempdir().unwrap();
    let discovery_path = tmp.path().join("control-service.json");

    let stopped = control_service_host_status(tmp.path(), None, Some("boom".to_string()));

    assert!(!stopped.running);
    assert_eq!(stopped.host, CONTROL_SERVICE_HOST);
    assert_eq!(stopped.port, None);
    assert_eq!(stopped.discovery_path, discovery_path.to_string_lossy());
    assert_eq!(stopped.last_error.as_deref(), Some("boom"));
}

#[test]
fn host_starts_health_endpoint_and_removes_discovery_on_shutdown() {
    let tmp = tempfile::tempdir().unwrap();

    let handle = start_control_service_host(tmp.path()).unwrap();
    let port = handle.port();
    let discovery_path = handle.discovery_path().to_path_buf();
    let discovery = read_control_service_discovery(&discovery_path)
        .unwrap()
        .expect("discovery should be written");

    assert_eq!(discovery.host, CONTROL_SERVICE_HOST);
    assert_eq!(discovery.port, port);

    let status = control_service_host_status(tmp.path(), Some(&handle), None);
    assert!(status.running);
    assert_eq!(status.port, Some(port));

    let response = request(port, "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert!(response.contains("\"status\":\"ok\""));

    handle.shutdown().unwrap();
    assert!(read_control_service_discovery(&discovery_path)
        .unwrap()
        .is_none());
}
