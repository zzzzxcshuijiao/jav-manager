use crate::aria2::Aria2Settings;
use crate::aria2::{Aria2RpcEndpoint, Aria2Transport};
use crate::control_service_host::ControlServiceHostStatus;
use crate::daemon_control::{
    run_daemon_once_with_transports, DaemonControlRuntime, DaemonControlStatus,
};
use crate::remote_scraper::{RemoteMetadataHttpClient, RemoteScraperSettings};
use crate::storage::Repository;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Severity of one self-check item. Serialized in lowercase so TypeScript can
/// use a compact string union without additional mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelfCheckSeverity {
    Pass,
    Warn,
    Fail,
}

/// One actionable self-check row shown in the automatic pipeline panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfCheckItem {
    pub id: String,
    pub title: String,
    pub severity: SelfCheckSeverity,
    pub message: String,
    pub action: Option<String>,
}

/// Filesystem summary produced by the isolated sandbox archive run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfCheckSandboxSummary {
    pub root: String,
    pub inbox: String,
    pub archive: String,
    pub video_path: String,
    pub archived_path: Option<String>,
    pub pipeline_status: Option<String>,
}

/// Overall self-check result derived from the strongest item severity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelfCheckOverall {
    Pass,
    Warn,
    Fail,
}

/// Full report returned to the frontend after one self-check command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfCheckReport {
    pub generated_at: String,
    pub overall: SelfCheckOverall,
    pub checks: Vec<SelfCheckItem>,
    pub sandbox: Option<SelfCheckSandboxSummary>,
}

/// Build configuration-only self-check items from the current repository and
/// runtime snapshots. This function does not touch real media files.
pub fn build_config_self_check_items(
    repo: &Repository,
    control_service: Option<ControlServiceHostStatus>,
    daemon: Option<DaemonControlStatus>,
    diagnostics_available: bool,
) -> Result<Vec<SelfCheckItem>> {
    let mut checks = Vec::new();
    checks.push(check_control_service(control_service));
    checks.push(check_configured_roots(repo)?);
    checks.push(check_metadata_source(repo)?);
    checks.push(check_aria2_settings(&repo.get_aria2_settings()?));
    checks.push(check_remote_scraper_settings(
        &repo.get_remote_scraper_settings()?,
    ));
    checks.push(check_diagnostics(diagnostics_available));
    if let Some(status) = daemon {
        if let Some(error) = status.last_error {
            checks.push(SelfCheckItem {
                id: "daemon_last_error".to_string(),
                title: "最近运行错误".to_string(),
                severity: SelfCheckSeverity::Warn,
                message: error,
                action: Some("如果沙盒归档通过，可优先按 aria2 或远程刮削器分项处理。".to_string()),
            });
        }
    }
    Ok(checks)
}

/// Derive the strongest overall status from individual check rows.
pub fn summarize_overall(checks: &[SelfCheckItem]) -> SelfCheckOverall {
    if checks
        .iter()
        .any(|item| item.severity == SelfCheckSeverity::Fail)
    {
        return SelfCheckOverall::Fail;
    }
    if checks
        .iter()
        .any(|item| item.severity == SelfCheckSeverity::Warn)
    {
        return SelfCheckOverall::Warn;
    }
    SelfCheckOverall::Pass
}

/// Run the full self-check: configuration checks plus an isolated sandbox
/// archive pass. The sandbox uses its own SQLite and never writes real library rows.
pub fn run_pipeline_self_check(
    app_data_dir: &Path,
    repo: &Repository,
    control_service: Option<ControlServiceHostStatus>,
    daemon: Option<DaemonControlStatus>,
    diagnostics_available: bool,
) -> Result<SelfCheckReport> {
    let mut checks =
        build_config_self_check_items(repo, control_service, daemon, diagnostics_available)?;
    let sandbox = run_sandbox_archive_check(app_data_dir, &mut checks);
    let overall = summarize_overall(&checks);
    Ok(SelfCheckReport {
        generated_at: Utc::now().to_rfc3339(),
        overall,
        checks,
        sandbox,
    })
}

/// Return whether any metadata source can feed the automatic pipeline.
pub fn metadata_source_available(
    metadata_provider_enabled: bool,
    settings: &RemoteScraperSettings,
) -> Result<bool> {
    if metadata_provider_enabled {
        return Ok(true);
    }
    let normalized = settings.normalized()?;
    if normalized.include_example_fallback {
        return Ok(true);
    }
    Ok(!normalized.enabled_sources()?.is_empty())
}

fn check_control_service(control_service: Option<ControlServiceHostStatus>) -> SelfCheckItem {
    match control_service {
        Some(status) if status.running => SelfCheckItem {
            id: "control_service".to_string(),
            title: "控制服务".to_string(),
            severity: SelfCheckSeverity::Pass,
            message: format!(
                "本地控制服务正在运行：{}:{}。",
                status.host,
                status
                    .port
                    .map(|port| port.to_string())
                    .unwrap_or_else(|| "未分配端口".to_string())
            ),
            action: None,
        },
        Some(status) => SelfCheckItem {
            id: "control_service".to_string(),
            title: "控制服务".to_string(),
            severity: SelfCheckSeverity::Warn,
            message: status
                .last_error
                .unwrap_or_else(|| "本地控制服务未运行，前端会回退到命令桥。".to_string()),
            action: Some("重启应用后再次刷新自动管线状态。".to_string()),
        },
        None => SelfCheckItem {
            id: "control_service".to_string(),
            title: "控制服务".to_string(),
            severity: SelfCheckSeverity::Warn,
            message: "未提供控制服务状态，自检只能验证命令桥路径。".to_string(),
            action: Some("刷新自动管线状态，确认控制通道是否为本地服务。".to_string()),
        },
    }
}

fn check_configured_roots(repo: &Repository) -> Result<SelfCheckItem> {
    let source_roots = repo.get_source_roots()?;
    let archive_root = repo.get_archive_root()?;
    if !source_roots.is_empty() && archive_root.is_some() {
        Ok(SelfCheckItem {
            id: "configured_roots".to_string(),
            title: "真实目录配置".to_string(),
            severity: SelfCheckSeverity::Pass,
            message: format!("已配置 {} 个来源目录和归档根目录。", source_roots.len()),
            action: None,
        })
    } else {
        Ok(SelfCheckItem {
            id: "configured_roots".to_string(),
            title: "真实目录配置".to_string(),
            severity: SelfCheckSeverity::Warn,
            message: "真实来源目录或归档根目录尚未完整配置；沙盒自检仍会使用隔离目录。".to_string(),
            action: Some("在“目录与资源池”页保存来源目录和归档根目录。".to_string()),
        })
    }
}

fn check_metadata_source(repo: &Repository) -> Result<SelfCheckItem> {
    let metadata_enabled = repo.get_metadata_provider_enabled()?;
    let remote_settings = repo.get_remote_scraper_settings()?;
    if metadata_source_available(metadata_enabled, &remote_settings)? {
        Ok(SelfCheckItem {
            id: "metadata_source".to_string(),
            title: "元数据源".to_string(),
            severity: SelfCheckSeverity::Pass,
            message: "至少一个元数据源可用于自动管线。".to_string(),
            action: None,
        })
    } else {
        Ok(SelfCheckItem {
            id: "metadata_source".to_string(),
            title: "元数据源".to_string(),
            severity: SelfCheckSeverity::Fail,
            message: "没有可用元数据源，自动管线不会处理真实文件。".to_string(),
            action: Some(
                "启用示例元数据源、保留示例 fallback，或启用至少一个远程 scraper source。"
                    .to_string(),
            ),
        })
    }
}

fn check_aria2_settings(settings: &Aria2Settings) -> SelfCheckItem {
    let normalized = match settings.normalized() {
        Ok(value) => value,
        Err(error) => {
            return SelfCheckItem {
                id: "aria2_settings".to_string(),
                title: "aria2 配置".to_string(),
                severity: SelfCheckSeverity::Fail,
                message: format!("aria2 配置无法归一化：{error}。"),
                action: Some(
                    "修正 aria2 主机、端口、RPC 路径、超时或轮询间隔后重新保存。".to_string(),
                ),
            }
        }
    };
    if !normalized.enabled {
        return SelfCheckItem {
            id: "aria2_settings".to_string(),
            title: "aria2 配置".to_string(),
            severity: SelfCheckSeverity::Pass,
            message: "aria2 轮询未启用，不影响本地目录归档。".to_string(),
            action: None,
        };
    }
    if normalized.tracked_gids.is_empty() {
        return SelfCheckItem {
            id: "aria2_settings".to_string(),
            title: "aria2 配置".to_string(),
            severity: SelfCheckSeverity::Warn,
            message: "aria2 已启用，但没有跟踪 GID。".to_string(),
            action: Some("填入真实已完成任务的 GID，或关闭 aria2 轮询。".to_string()),
        };
    }
    if normalized.secret.is_some() {
        return SelfCheckItem {
            id: "aria2_settings".to_string(),
            title: "aria2 配置".to_string(),
            severity: SelfCheckSeverity::Warn,
            message: format!(
                "aria2 已启用并跟踪 {} 个 GID，同时配置了 RPC secret。",
                normalized.tracked_gids.len()
            ),
            action: Some("确认 aria2 启动参数确实包含相同的 --rpc-secret。".to_string()),
        };
    }
    SelfCheckItem {
        id: "aria2_settings".to_string(),
        title: "aria2 配置".to_string(),
        severity: SelfCheckSeverity::Pass,
        message: format!(
            "aria2 已启用并跟踪 {} 个 GID。",
            normalized.tracked_gids.len()
        ),
        action: None,
    }
}

fn check_remote_scraper_settings(settings: &RemoteScraperSettings) -> SelfCheckItem {
    let normalized = match settings.normalized() {
        Ok(value) => value,
        Err(error) => {
            return SelfCheckItem {
                id: "remote_scraper_settings".to_string(),
                title: "远程刮削器配置".to_string(),
                severity: SelfCheckSeverity::Fail,
                message: format!("远程刮削器配置无效：{error}。"),
                action: Some(
                    "检查 source URL 模板是否包含 {code}，以及置信度是否在 0 到 1 之间。"
                        .to_string(),
                ),
            }
        }
    };
    let enabled_sources = match normalized.enabled_sources() {
        Ok(value) => value,
        Err(error) => {
            return SelfCheckItem {
                id: "remote_scraper_settings".to_string(),
                title: "远程刮削器配置".to_string(),
                severity: SelfCheckSeverity::Fail,
                message: format!("远程刮削器 source 无法使用：{error}。"),
                action: Some("修正启用 source 的搜索 URL 模板。".to_string()),
            }
        }
    };
    if normalized.enabled && enabled_sources.is_empty() && !normalized.include_example_fallback {
        return SelfCheckItem {
            id: "remote_scraper_settings".to_string(),
            title: "远程刮削器配置".to_string(),
            severity: SelfCheckSeverity::Fail,
            message: "远程刮削器已启用，但没有启用任何 source，且示例 fallback 已关闭。"
                .to_string(),
            action: Some("启用至少一个远程 source，或重新打开示例 fallback。".to_string()),
        };
    }
    if let Some(proxy) = normalized.proxy_url.as_deref() {
        if !(proxy.starts_with("http://") || proxy.starts_with("https://")) {
            return SelfCheckItem {
                id: "remote_scraper_settings".to_string(),
                title: "远程刮削器配置".to_string(),
                severity: SelfCheckSeverity::Warn,
                message: format!("代理 URL 看起来不是 http/https：{proxy}。"),
                action: Some("使用类似 http://127.0.0.1:7890 的代理地址，或清空代理。".to_string()),
            };
        }
    }
    SelfCheckItem {
        id: "remote_scraper_settings".to_string(),
        title: "远程刮削器配置".to_string(),
        severity: SelfCheckSeverity::Pass,
        message: if normalized.enabled {
            format!(
                "远程刮削器配置可用：{} 个 source 已启用。",
                enabled_sources.len()
            )
        } else if normalized.include_example_fallback {
            "远程刮削器未启用，示例 fallback 可用于基础归档验证。".to_string()
        } else {
            "远程刮削器未启用。".to_string()
        },
        action: None,
    }
}

fn check_diagnostics(diagnostics_available: bool) -> SelfCheckItem {
    if diagnostics_available {
        SelfCheckItem {
            id: "diagnostics".to_string(),
            title: "诊断系统".to_string(),
            severity: SelfCheckSeverity::Pass,
            message: "诊断日志可用。".to_string(),
            action: None,
        }
    } else {
        SelfCheckItem {
            id: "diagnostics".to_string(),
            title: "诊断系统".to_string(),
            severity: SelfCheckSeverity::Warn,
            message: "诊断日志 writer 不可用，自检仍可运行但排障信息会减少。".to_string(),
            action: Some("重启应用后再导出诊断快照。".to_string()),
        }
    }
}

fn run_sandbox_archive_check(
    app_data_dir: &Path,
    checks: &mut Vec<SelfCheckItem>,
) -> Option<SelfCheckSandboxSummary> {
    match try_run_sandbox_archive_check(app_data_dir) {
        Ok(summary) => {
            checks.push(SelfCheckItem {
                id: "sandbox_archive".to_string(),
                title: "沙盒归档".to_string(),
                severity: SelfCheckSeverity::Pass,
                message: "沙盒视频已成功归档，自动管线核心链路可用。".to_string(),
                action: None,
            });
            Some(summary)
        }
        Err(error) => {
            checks.push(SelfCheckItem {
                id: "sandbox_archive".to_string(),
                title: "沙盒归档".to_string(),
                severity: SelfCheckSeverity::Fail,
                message: format!("沙盒归档未完成：{error}。"),
                action: Some("导出诊断快照并检查自动管线最近运行记录。".to_string()),
            });
            None
        }
    }
}

fn try_run_sandbox_archive_check(app_data_dir: &Path) -> Result<SelfCheckSandboxSummary> {
    let root = unique_sandbox_root(app_data_dir);
    let inbox = root.join("inbox");
    let archive = root.join("archive");
    let assets = root.join("assets");
    fs::create_dir_all(&inbox)?;
    fs::create_dir_all(&archive)?;
    fs::create_dir_all(&assets)?;

    let video_path = inbox.join("MMT-001.mp4");
    fs::write(&video_path, b"stable self-check video bytes")?;

    let repo = Repository::open(&root.join("library.sqlite"))?;
    repo.migrate()?;
    repo.set_source_roots(&[inbox.clone()])?;
    repo.set_archive_root(&archive)?;
    repo.set_resource_pool_dirs(&[assets])?;
    repo.set_metadata_provider_enabled(false)?;
    repo.set_remote_scraper_settings(&RemoteScraperSettings {
        enabled: false,
        include_example_fallback: true,
        ..RemoteScraperSettings::default()
    })?;
    repo.set_aria2_settings(&Aria2Settings::default())?;

    let mut runtime = DaemonControlRuntime::default();
    let report = run_daemon_once_with_transports(
        &repo,
        &mut runtime,
        false,
        SelfCheckAria2Transport,
        SelfCheckRemoteClient,
    )?;
    let run = repo.list_pipeline_runs()?.into_iter().next();
    let archived_path = archive.join("MMT-001").join("MMT-001.mp4");
    if report.process.archived != 1 || !archived_path.exists() {
        anyhow::bail!(
            "expected one archived sandbox video, got archived={}",
            report.process.archived
        );
    }

    Ok(SelfCheckSandboxSummary {
        root: path_to_string(root),
        inbox: path_to_string(inbox),
        archive: path_to_string(archive),
        video_path: path_to_string(video_path),
        archived_path: Some(path_to_string(archived_path)),
        pipeline_status: run.map(|value| value.status),
    })
}

fn unique_sandbox_root(app_data_dir: &Path) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S%.3f");
    app_data_dir.join("self-check").join(stamp.to_string())
}

fn path_to_string(path: PathBuf) -> String {
    path.to_string_lossy().to_string()
}

#[derive(Debug, Clone, Copy)]
struct SelfCheckAria2Transport;

impl Aria2Transport for SelfCheckAria2Transport {
    fn post_json(&self, _endpoint: &Aria2RpcEndpoint, _body: &str) -> Result<String> {
        Ok("{}".to_string())
    }
}

#[derive(Debug, Clone, Copy)]
struct SelfCheckRemoteClient;

impl RemoteMetadataHttpClient for SelfCheckRemoteClient {
    fn get_text(&self, _url: &str) -> Result<String> {
        Ok(String::new())
    }
}
