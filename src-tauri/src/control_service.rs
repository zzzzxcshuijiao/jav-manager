use crate::daemon_control::{
    build_daemon_status, list_exception_entries, list_holding_entries, list_recent_pipeline_runs,
    resolve_exception_entry, run_daemon_once, DaemonControlRuntime, DaemonControlStatus,
};
use crate::domain::ExceptionStatus;
use crate::storage::Repository;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_HTTP_REQUEST_BYTES: usize = 64 * 1024;

/// Configuration for the Stage 5A loopback control service. It contains only
/// local runtime values; product-level persistence remains in SQLite settings.
#[derive(Debug, Clone)]
pub struct ControlServiceConfig {
    pub host: String,
    pub port: u16,
    pub discovery_path: PathBuf,
    pub token: Option<String>,
    pub metadata_provider_enabled: bool,
}

/// Discovery document written after the listener binds to its actual port.
/// Frontend and future tray processes read this file before calling REST APIs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlServiceDiscovery {
    pub service: String,
    pub host: String,
    pub port: u16,
    pub base_url: String,
    pub token: String,
    pub pid: u32,
    pub created_at: String,
}

/// In-process runtime for handling control API requests. Stage 5A keeps this
/// single-threaded and serial so SQLite writes remain ordered through daemon.
pub struct ControlServiceRuntime {
    repo: Repository,
    daemon: DaemonControlRuntime,
    config: ControlServiceConfig,
    token: String,
}

/// Running service handle used by tests and future daemon hosts. Dropping it
/// does not implicitly kill the thread; callers should request shutdown.
pub struct ControlServiceHandle {
    host: String,
    port: u16,
    shutdown_tx: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ControlServiceRuntime {
    /// Format the runtime without dumping SQLite internals or the bearer token.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ControlServiceRuntime")
            .field("host", &self.config.host)
            .field("port", &self.config.port)
            .field(
                "metadata_provider_enabled",
                &self.config.metadata_provider_enabled,
            )
            .field("daemon", &self.daemon)
            .finish_non_exhaustive()
    }
}

impl ControlServiceRuntime {
    /// Create a service runtime after validating that it is loopback-only. If
    /// no token is provided, a per-start token is generated.
    pub fn new(repo: Repository, config: ControlServiceConfig) -> Result<Self> {
        validate_loopback_host(&config.host)?;
        let token = config.token.clone().unwrap_or_else(generate_token);
        Ok(Self {
            repo,
            daemon: DaemonControlRuntime::default(),
            config,
            token,
        })
    }

    /// Build the discovery document for a bound port without writing it.
    pub fn discovery_for_port(&self, port: u16) -> ControlServiceDiscovery {
        ControlServiceDiscovery {
            service: "media-manager-control".to_string(),
            host: self.config.host.clone(),
            port,
            base_url: format!("http://{}:{port}", self.config.host),
            token: self.token.clone(),
            pid: std::process::id(),
            created_at: now_epoch_seconds(),
        }
    }

    /// Start the HTTP service on the configured loopback address and write the
    /// discovery file once the OS has assigned the final port.
    pub fn start(self) -> Result<ControlServiceHandle> {
        let listener = TcpListener::bind((self.config.host.as_str(), self.config.port))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let discovery = self.discovery_for_port(port);
        if let Some(parent) = self.config.discovery_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            &self.config.discovery_path,
            serde_json::to_string_pretty(&discovery)?,
        )?;

        let host = self.config.host.clone();
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let thread = thread::spawn(move || run_listener(listener, self, shutdown_rx));

        Ok(ControlServiceHandle {
            host,
            port,
            shutdown_tx,
            thread: Some(thread),
        })
    }

    /// Handle one raw HTTP request and return a complete HTTP response string.
    pub fn handle_raw_http(&mut self, raw: &str) -> String {
        let Some(request) = HttpRequest::parse(raw) else {
            return json_response(400, json!({ "ok": false, "error": "bad request" }));
        };
        if request.method == "GET" && request.path == "/health" {
            return ok_json(json!({
                "service": "media-manager-control",
                "status": "ok"
            }));
        }
        if request.path.starts_with("/v1/") {
            if let Err(response) = self.authorize(&request) {
                return response;
            }
            return self.route_v1(&request);
        }
        json_response(404, json!({ "ok": false, "error": "not found" }))
    }

    /// Validate Stage 5A API authentication and local frontend Origin rules.
    fn authorize(&self, request: &HttpRequest) -> std::result::Result<(), String> {
        if !origin_allowed(request.header("origin")) {
            return Err(json_response(
                403,
                json!({ "ok": false, "error": "origin forbidden" }),
            ));
        }
        match request.header("authorization") {
            None => Err(json_response(
                401,
                json!({ "ok": false, "error": "missing bearer token" }),
            )),
            Some(value) if value == format!("Bearer {}", self.token) => Ok(()),
            Some(_) => Err(json_response(
                403,
                json!({ "ok": false, "error": "invalid bearer token" }),
            )),
        }
    }

    /// Convert a `/v1/*` route result into the shared JSON HTTP envelope.
    fn route_v1(&mut self, request: &HttpRequest) -> String {
        match self.route_v1_result(request) {
            Ok(data) => ok_json(data),
            Err(error) if error.to_string().starts_with("bad request: ") => json_response(
                400,
                json!({
                    "ok": false,
                    "error": error.to_string().trim_start_matches("bad request: ")
                }),
            ),
            Err(error) if error.to_string() == "not found" => {
                json_response(404, json!({ "ok": false, "error": "not found" }))
            }
            Err(error) => json_response(500, json!({ "ok": false, "error": error.to_string() })),
        }
    }

    /// Dispatch authenticated `/v1/*` requests into daemon control helpers.
    fn route_v1_result(&mut self, request: &HttpRequest) -> Result<serde_json::Value> {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/v1/status") => self.status_json(),
            ("POST", "/v1/pause") => {
                self.daemon.paused = true;
                self.status_json()
            }
            ("POST", "/v1/resume") => {
                self.daemon.paused = false;
                self.status_json()
            }
            ("POST", "/v1/run-once") => {
                let report = run_daemon_once(
                    &self.repo,
                    &mut self.daemon,
                    self.config.metadata_provider_enabled,
                )?;
                Ok(serde_json::to_value(report)?)
            }
            ("GET", "/v1/holding") => Ok(serde_json::to_value(list_holding_entries(&self.repo)?)?),
            ("GET", "/v1/exceptions") => {
                Ok(serde_json::to_value(list_exception_entries(&self.repo)?)?)
            }
            ("GET", "/v1/runs") => Ok(serde_json::to_value(list_recent_pipeline_runs(
                &self.repo,
            )?)?),
            ("POST", path) if path.starts_with("/v1/exceptions/") && path.ends_with("/resolve") => {
                let id = parse_exception_resolve_path(path)?;
                let exception = resolve_exception_entry(&self.repo, id, ExceptionStatus::Resolved)?;
                Ok(serde_json::to_value(exception)?)
            }
            _ => Err(anyhow!("not found")),
        }
    }

    /// Build the current daemon status as a JSON value for REST responses.
    fn status_json(&self) -> Result<serde_json::Value> {
        let status: DaemonControlStatus = build_daemon_status(
            &self.repo,
            &self.daemon,
            self.config.metadata_provider_enabled,
        )?;
        Ok(serde_json::to_value(status)?)
    }
}

impl ControlServiceHandle {
    /// Return the actual bound port. This matters when tests request port 0.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Ask the listener loop to stop and wait for the service thread.
    pub fn shutdown(mut self) -> Result<()> {
        let _ = self.shutdown_tx.send(());
        let _ = TcpStream::connect((self.host.as_str(), self.port));
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow!("control service thread panicked"))?;
        }
        Ok(())
    }
}

/// Reject non-loopback hosts so the Stage 5A service cannot accidentally bind
/// to a LAN/WAN interface while it has only local token-based security.
pub fn validate_loopback_host(host: &str) -> Result<()> {
    match host {
        "127.0.0.1" | "localhost" => Ok(()),
        _ => Err(anyhow!("control service must bind to a loopback host")),
    }
}

/// Generate a per-start token from process and clock entropy. Stage 5C can
/// replace this with OS randomness without changing the HTTP contract.
pub fn generate_token() -> String {
    let mut hasher = Sha256::new();
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(now_epoch_seconds().as_bytes());
    hasher.update(format!("{:?}", std::thread::current().id()).as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Run the single-threaded accept loop until shutdown is requested.
fn run_listener(
    listener: TcpListener,
    mut runtime: ControlServiceRuntime,
    shutdown_rx: mpsc::Receiver<()>,
) {
    loop {
        if shutdown_rx.try_recv().is_ok() {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = handle_stream(stream, &mut runtime);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

/// Read one complete short-lived HTTP request and write one JSON response.
fn handle_stream(mut stream: TcpStream, runtime: &mut ControlServiceRuntime) -> Result<()> {
    let buffer = read_http_request(&mut stream)?;
    let response = runtime.handle_raw_http(&buffer);
    stream.write_all(response.as_bytes())?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

/// Read through HTTP headers and the declared body without waiting for EOF.
fn read_http_request(stream: &mut TcpStream) -> Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 512];
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..count]);
        if buffer.len() > MAX_HTTP_REQUEST_BYTES {
            return Err(anyhow!("request too large"));
        }
        if let Some(required_len) = required_http_request_len(&buffer) {
            if buffer.len() >= required_len {
                break;
            }
        }
    }
    Ok(String::from_utf8(buffer)?)
}

/// Return the byte length needed for the full request once headers are present.
fn required_http_request_len(buffer: &[u8]) -> Option<usize> {
    let header_end = buffer.windows(4).position(|window| window == b"\r\n\r\n")?;
    let header_len = header_end + 4;
    let headers = std::str::from_utf8(&buffer[..header_end]).ok()?;
    let content_len = headers
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .find_map(|(name, value)| {
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(0);
    Some(header_len + content_len)
}

/// Minimal HTTP request representation for the narrow Stage 5A REST surface.
struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
}

impl HttpRequest {
    /// Parse the request line and headers needed by the local control service.
    fn parse(raw: &str) -> Option<Self> {
        let mut lines = raw.lines();
        let first = lines.next()?;
        let mut parts = first.split_whitespace();
        let mut headers = HashMap::new();
        for line in lines {
            if line.trim().is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }
        Some(Self {
            method: parts.next()?.to_string(),
            path: parts.next()?.to_string(),
            headers,
        })
    }

    /// Return one case-insensitive header value.
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

/// Allow absent Origin plus known local/Tauri origins; reject everything else.
fn origin_allowed(origin: Option<&str>) -> bool {
    match origin {
        None => true,
        Some("tauri://localhost") => true,
        Some(value) => {
            value.starts_with("http://127.0.0.1:") || value.starts_with("http://localhost:")
        }
    }
}

/// Wrap successful route data in the common `{ ok, data }` envelope.
fn ok_json(data: serde_json::Value) -> String {
    json_response(200, json!({ "ok": true, "data": data }))
}

/// Extract the numeric exception id from `/v1/exceptions/{id}/resolve`.
fn parse_exception_resolve_path(path: &str) -> Result<i64> {
    let id = path
        .strip_prefix("/v1/exceptions/")
        .and_then(|tail| tail.strip_suffix("/resolve"))
        .ok_or_else(|| anyhow!("bad request: invalid exception resolve path"))?;
    id.parse::<i64>()
        .map_err(|_| anyhow!("bad request: invalid exception id"))
}

/// Render a compact HTTP/1.1 JSON response for short loopback requests.
fn json_response(status: u16, body: serde_json::Value) -> String {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let body = body.to_string();
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Return a simple epoch-second timestamp for discovery documents and tokens.
fn now_epoch_seconds() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}
