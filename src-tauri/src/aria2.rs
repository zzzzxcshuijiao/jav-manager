use crate::pipeline::{is_aria2_complete, Aria2TaskSnapshot};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

const TELL_STATUS_ID: &str = "media-manager-tell-status";
const TELL_STATUS_KEYS: [&str; 5] = [
    "gid",
    "status",
    "totalLength",
    "completedLength",
    "files",
];

/// Connection settings for one aria2 JSON-RPC endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aria2RpcEndpoint {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub secret: Option<String>,
    pub timeout_ms: u64,
}

impl Aria2RpcEndpoint {
    /// Build the default loopback endpoint used by aria2 unless users configure otherwise.
    pub fn loopback_default(secret: Option<String>) -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 6800,
            path: "/jsonrpc".to_string(),
            secret,
            timeout_ms: 5_000,
        }
    }
}

/// Transport boundary for posting JSON-RPC bodies to aria2.
pub trait Aria2Transport {
    /// Send one JSON body to the configured endpoint and return the response body.
    fn post_json(&self, endpoint: &Aria2RpcEndpoint, body: &str) -> Result<String>;
}

/// JSON-RPC client for aria2 methods needed by the automatic pipeline.
#[derive(Debug, Clone)]
pub struct Aria2Client<T> {
    endpoint: Aria2RpcEndpoint,
    transport: T,
}

impl<T: Aria2Transport> Aria2Client<T> {
    /// Create a client with an explicit endpoint and transport.
    pub fn new(endpoint: Aria2RpcEndpoint, transport: T) -> Self {
        Self { endpoint, transport }
    }

    /// Fetch a single aria2 task status by GID using `aria2.tellStatus`.
    pub fn tell_status(&self, gid: &str) -> Result<Aria2Status> {
        let body = self.tell_status_request_body(gid)?;
        let response = self.transport.post_json(&self.endpoint, &body)?;
        parse_rpc_response(&response)
    }

    /// Build the JSON-RPC body for `aria2.tellStatus`.
    pub fn tell_status_request_body(&self, gid: &str) -> Result<String> {
        if gid.trim().is_empty() {
            return Err(anyhow!("aria2 gid is required"));
        }
        let mut params = Vec::new();
        if let Some(secret) = &self.endpoint.secret {
            params.push(json!(format!("token:{secret}")));
        }
        params.push(json!(gid));
        params.push(json!(TELL_STATUS_KEYS));
        Ok(json!({
            "jsonrpc": "2.0",
            "id": TELL_STATUS_ID,
            "method": "aria2.tellStatus",
            "params": params,
        })
        .to_string())
    }
}

/// Task status returned by `aria2.tellStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aria2Status {
    #[serde(default)]
    pub gid: String,
    #[serde(default)]
    pub status: String,
    #[serde(rename = "totalLength", default)]
    pub total_length: String,
    #[serde(rename = "completedLength", default)]
    pub completed_length: String,
    #[serde(default)]
    pub files: Vec<Aria2File>,
}

impl Aria2Status {
    /// Convert aria2 string fields to the existing pipeline completion snapshot.
    pub fn to_task_snapshot(&self) -> Result<Aria2TaskSnapshot> {
        Ok(Aria2TaskSnapshot {
            status: self.status.clone(),
            completed_length: parse_u64_field("completedLength", &self.completed_length)?,
            total_length: parse_u64_field("totalLength", &self.total_length)?,
        })
    }

    /// Return true only when the task-level aria2 completion detector accepts the task.
    pub fn is_complete(&self) -> Result<bool> {
        Ok(is_aria2_complete(&self.to_task_snapshot()?))
    }
}

/// One file entry returned by aria2 for single-file or BT downloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aria2File {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub length: String,
    #[serde(rename = "completedLength", default)]
    pub completed_length: String,
    #[serde(default)]
    pub selected: String,
}

impl Aria2File {
    /// Return whether aria2 says this file was selected for download.
    pub fn is_selected(&self) -> bool {
        self.selected != "false"
    }

    /// Return whether this file entry has complete byte counts.
    pub fn is_complete(&self) -> Result<bool> {
        let length = parse_u64_field("files[].length", &self.length)?;
        let completed = parse_u64_field("files[].completedLength", &self.completed_length)?;
        Ok(length > 0 && completed == length)
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcEnvelope {
    result: Option<Aria2Status>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

fn parse_rpc_response(raw: &str) -> Result<Aria2Status> {
    let envelope: JsonRpcEnvelope = serde_json::from_str(raw)?;
    if let Some(error) = envelope.error {
        return Err(anyhow!(
            "aria2 JSON-RPC error {}: {}",
            error.code,
            error.message
        ));
    }
    envelope
        .result
        .ok_or_else(|| anyhow!("aria2 JSON-RPC response missing result"))
}

fn parse_u64_field(name: &str, value: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .map_err(|error| anyhow!("invalid aria2 {name}: {value}: {error}"))
}
