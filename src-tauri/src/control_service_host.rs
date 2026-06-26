use crate::control_service::{
    control_service_discovery_path, ControlServiceConfig, ControlServiceHandle,
    ControlServiceRuntime,
};
use crate::storage::Repository;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const CONTROL_SERVICE_HOST: &str = "127.0.0.1";
pub const CONTROL_SERVICE_PORT: u16 = 0;
pub const CONTROL_SERVICE_DB_FILE: &str = "library.sqlite";

/// Serializable diagnostic snapshot for the app-owned control service host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlServiceHostStatus {
    pub running: bool,
    pub host: String,
    pub port: Option<u16>,
    pub discovery_path: String,
    pub last_error: Option<String>,
}

/// Build the runtime config for the app-owned loopback control service.
pub fn build_control_service_config(
    app_data_dir: &Path,
    metadata_provider_enabled: bool,
) -> ControlServiceConfig {
    ControlServiceConfig {
        host: CONTROL_SERVICE_HOST.to_string(),
        port: CONTROL_SERVICE_PORT,
        discovery_path: control_service_discovery_path(app_data_dir),
        token: None,
        metadata_provider_enabled,
    }
}

/// Start the app-owned control service using a dedicated SQLite connection.
pub fn start_control_service_host(app_data_dir: &Path) -> Result<ControlServiceHandle> {
    fs::create_dir_all(app_data_dir)?;
    let repo = Repository::open(&app_data_dir.join(CONTROL_SERVICE_DB_FILE))?;
    repo.migrate()?;
    let metadata_provider_enabled = repo.get_metadata_provider_enabled()?;
    let config = build_control_service_config(app_data_dir, metadata_provider_enabled);
    ControlServiceRuntime::new(repo, config)?.start()
}

/// Return a serializable snapshot of the app-owned control service host.
pub fn control_service_host_status(
    app_data_dir: &Path,
    handle: Option<&ControlServiceHandle>,
    last_error: Option<String>,
) -> ControlServiceHostStatus {
    ControlServiceHostStatus {
        running: handle.is_some(),
        host: handle
            .map(|value| value.host().to_string())
            .unwrap_or_else(|| CONTROL_SERVICE_HOST.to_string()),
        port: handle.map(ControlServiceHandle::port),
        discovery_path: control_service_discovery_path(app_data_dir)
            .to_string_lossy()
            .to_string(),
        last_error,
    }
}
