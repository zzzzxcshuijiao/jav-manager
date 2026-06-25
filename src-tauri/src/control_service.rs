use crate::daemon_control::DaemonControlRuntime;
use crate::storage::Repository;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
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

fn now_epoch_seconds() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}
