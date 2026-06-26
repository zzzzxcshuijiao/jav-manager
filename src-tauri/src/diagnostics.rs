use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_MAX_FILES: usize = 3;
const MAX_TAIL_LIMIT: usize = 200;

/// Severity for one structured diagnostic event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticLevel {
    Info,
    Warn,
    Error,
}

/// One JSONL diagnostic event written by the local diagnostics subsystem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticLogEntry {
    pub timestamp: String,
    pub level: DiagnosticLevel,
    pub target: String,
    pub message: String,
    pub context: Value,
}

/// App-data backed JSONL writer for local diagnostic events.
#[derive(Debug, Clone)]
pub struct DiagnosticsWriter {
    log_dir: PathBuf,
    log_path: PathBuf,
    max_bytes: u64,
    max_files: usize,
}

impl DiagnosticsWriter {
    /// Create a diagnostics writer under `<log_dir>/media-manager.jsonl`.
    pub fn new(log_dir: PathBuf) -> Result<Self> {
        Self::new_with_limits(log_dir, DEFAULT_MAX_BYTES, DEFAULT_MAX_FILES)
    }

    /// Create a diagnostics writer with explicit limits for deterministic tests.
    pub fn new_with_limits(log_dir: PathBuf, max_bytes: u64, max_files: usize) -> Result<Self> {
        fs::create_dir_all(&log_dir)?;
        Ok(Self {
            log_path: log_dir.join("media-manager.jsonl"),
            log_dir,
            max_bytes: max_bytes.max(1),
            max_files: max_files.max(1),
        })
    }

    /// Return the active JSONL file path.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Append one redacted JSONL event to the active diagnostics file.
    pub fn append(
        &self,
        level: DiagnosticLevel,
        target: impl Into<String>,
        message: impl Into<String>,
        context: Value,
    ) -> Result<()> {
        self.rotate_if_needed()?;
        let entry = DiagnosticLogEntry {
            timestamp: Utc::now().to_rfc3339(),
            level,
            target: target.into(),
            message: message.into(),
            context: redact_diagnostic_value(context),
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;
        writeln!(file, "{}", serde_json::to_string(&entry)?)?;
        Ok(())
    }

    /// Read the last `limit` valid events from the active log, oldest to newest.
    pub fn tail(&self, limit: usize) -> Result<Vec<DiagnosticLogEntry>> {
        let limit = limit.clamp(1, MAX_TAIL_LIMIT);
        if !self.log_path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&self.log_path)?;
        let reader = BufReader::new(file);
        let mut entries = VecDeque::with_capacity(limit);
        for line in reader.lines() {
            let line = line?;
            if let Ok(entry) = serde_json::from_str::<DiagnosticLogEntry>(&line) {
                if entries.len() == limit {
                    entries.pop_front();
                }
                entries.push_back(entry);
            }
        }
        Ok(entries.into_iter().collect())
    }

    fn rotate_if_needed(&self) -> Result<()> {
        if !self.log_path.exists() || fs::metadata(&self.log_path)?.len() < self.max_bytes {
            return Ok(());
        }
        for index in (1..=self.max_files).rev() {
            let from = self.rotated_path(index);
            if !from.exists() {
                continue;
            }
            if index == self.max_files {
                fs::remove_file(from)?;
            } else {
                fs::rename(from, self.rotated_path(index + 1))?;
            }
        }
        fs::rename(&self.log_path, self.rotated_path(1))?;
        Ok(())
    }

    fn rotated_path(&self, index: usize) -> PathBuf {
        self.log_dir.join(format!("media-manager.jsonl.{index}"))
    }
}

/// Recursively redact sensitive diagnostic context values before writing them.
pub fn redact_diagnostic_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(redact_map(map)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(redact_diagnostic_value)
                .collect(),
        ),
        other => other,
    }
}

fn redact_map(map: Map<String, Value>) -> Map<String, Value> {
    map.into_iter()
        .map(|(key, value)| {
            let lower = key.to_ascii_lowercase();
            let redacted = if is_secret_key(&lower) {
                Value::String("***".to_string())
            } else if lower.contains("proxy") {
                match value {
                    Value::String(url) => Value::String(redact_proxy_url(&url)),
                    other => redact_diagnostic_value(other),
                }
            } else {
                redact_diagnostic_value(value)
            };
            (key, redacted)
        })
        .collect()
}

fn is_secret_key(key: &str) -> bool {
    ["secret", "token", "password", "authorization", "cookie"]
        .iter()
        .any(|needle| key.contains(needle))
}

/// Remove credentials from proxy URLs while preserving the endpoint location.
pub fn redact_proxy_url(value: &str) -> String {
    let Some((scheme, rest)) = value.split_once("://") else {
        return value.to_string();
    };
    let Some((_, host)) = rest.split_once('@') else {
        return value.to_string();
    };
    format!("{scheme}://***@{host}")
}
