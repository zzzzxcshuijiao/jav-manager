use crate::daemon_control::DaemonControlRuntime;
use crate::storage::Repository;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

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
            .field("metadata_provider_enabled", &self.config.metadata_provider_enabled)
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
            return json_response(
                200,
                json!({
                    "ok": true,
                    "data": {
                        "service": "media-manager-control",
                        "status": "ok"
                    }
                }),
            );
        }
        json_response(404, json!({ "ok": false, "error": "not found" }))
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

fn handle_stream(mut stream: TcpStream, runtime: &mut ControlServiceRuntime) -> Result<()> {
    let mut buffer = String::new();
    stream.read_to_string(&mut buffer)?;
    let response = runtime.handle_raw_http(&buffer);
    stream.write_all(response.as_bytes())?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

struct HttpRequest {
    method: String,
    path: String,
}

impl HttpRequest {
    fn parse(raw: &str) -> Option<Self> {
        let first = raw.lines().next()?;
        let mut parts = first.split_whitespace();
        Some(Self {
            method: parts.next()?.to_string(),
            path: parts.next()?.to_string(),
        })
    }
}

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

fn now_epoch_seconds() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}
