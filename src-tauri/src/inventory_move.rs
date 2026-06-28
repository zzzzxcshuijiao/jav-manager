use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Reserved destination free-space margin for future cross-volume move verification.
pub const CROSS_VOLUME_SPACE_MARGIN_BYTES: u64 = 512 * 1024 * 1024;

/// Filesystem strategy used to classify move operations and query destination capacity.
pub trait InventoryMoveStrategy: Sync {
    /// Return whether `from_path` and `to_path` are on the same filesystem volume.
    fn is_same_volume(&self, from_path: &Path, to_path: &Path) -> Result<bool>;

    /// Return currently available bytes for the filesystem containing `path`.
    fn available_space(&self, path: &Path) -> Result<u64>;
}

/// Production move strategy backed by the host operating system filesystem APIs.
pub struct SystemInventoryMoveStrategy;

impl InventoryMoveStrategy for SystemInventoryMoveStrategy {
    fn is_same_volume(&self, from_path: &Path, to_path: &Path) -> Result<bool> {
        paths_are_on_same_volume(from_path, to_path)
    }

    fn available_space(&self, path: &Path) -> Result<u64> {
        fs2::available_space(path)
            .with_context(|| format!("读取可用空间失败：{}", path.to_string_lossy()))
    }
}

/// Physical move method selected for one inventory action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryMoveMethod {
    SameVolume,
    CrossVolume,
}

/// Result metadata for a file moved by inventory execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryMovedFile {
    pub from_path: PathBuf,
    pub to_path: PathBuf,
    pub method: InventoryMoveMethod,
    pub bytes: u64,
    pub sha256: Option<String>,
}

/// Move one file without replacing an existing target path.
pub fn move_file_no_clobber(
    from_path: &Path,
    to_path: &Path,
    strategy: &dyn InventoryMoveStrategy,
) -> Result<InventoryMovedFile> {
    let parent = to_path
        .parent()
        .ok_or_else(|| anyhow!("目标路径没有父目录：{}", to_path.to_string_lossy()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("创建目标目录失败：{}", parent.to_string_lossy()))?;
    if to_path.exists() {
        bail!("目标路径已存在：{}", to_path.to_string_lossy());
    }
    let source_size = fs::metadata(from_path)
        .with_context(|| format!("读取源文件失败：{}", from_path.to_string_lossy()))?
        .len();

    if strategy.is_same_volume(from_path, to_path)? {
        move_same_volume_no_copy(from_path, to_path)?;
        return Ok(InventoryMovedFile {
            from_path: from_path.to_path_buf(),
            to_path: to_path.to_path_buf(),
            method: InventoryMoveMethod::SameVolume,
            bytes: source_size,
            sha256: None,
        });
    }

    move_cross_volume_verify_delete(from_path, to_path, strategy)
}

/// Move one same-volume file via hard link creation followed by source removal.
pub fn move_same_volume_no_copy(from_path: &Path, to_path: &Path) -> Result<()> {
    let parent = to_path
        .parent()
        .ok_or_else(|| anyhow!("目标路径没有父目录：{}", to_path.to_string_lossy()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("创建目标目录失败：{}", parent.to_string_lossy()))?;
    if to_path.exists() {
        bail!("目标路径已存在：{}", to_path.to_string_lossy());
    }
    let source_size = fs::metadata(from_path)
        .with_context(|| format!("读取源文件失败：{}", from_path.to_string_lossy()))?
        .len();
    if let Err(error) = fs::hard_link(from_path, to_path) {
        if hard_link_error_is_cross_volume(&error) {
            bail!("跨盘集中迁移尚未启用");
        }
        return Err(error).with_context(|| {
            format!(
                "同盘集中迁移硬链接失败：{} -> {}",
                from_path.to_string_lossy(),
                to_path.to_string_lossy()
            )
        });
    }
    let target_size = fs::metadata(to_path)
        .with_context(|| format!("读取目标文件失败：{}", to_path.to_string_lossy()))?
        .len();
    if target_size != source_size {
        let _ = fs::remove_file(to_path);
        bail!("同盘集中迁移大小校验失败：{}", from_path.to_string_lossy());
    }
    if let Err(error) = fs::remove_file(from_path) {
        let _ = fs::remove_file(to_path);
        return Err(error)
            .with_context(|| format!("删除源文件失败：{}", from_path.to_string_lossy()));
    }
    ensure_source_path_removed_after_move(from_path, to_path)?;
    Ok(())
}

/// Confirm the source path is gone after move while retaining the migrated target on doubt.
fn ensure_source_path_removed_after_move(from_path: &Path, to_path: &Path) -> Result<()> {
    if from_path.exists() {
        bail!(
            "同盘集中迁移后源路径仍存在：{}；迁移目标已保留以避免数据丢失：{}",
            from_path.to_string_lossy(),
            to_path.to_string_lossy()
        );
    }
    Ok(())
}

/// Reject cross-volume moves until copy-verify-delete migration is explicitly enabled.
pub fn move_cross_volume_verify_delete(
    _from_path: &Path,
    _to_path: &Path,
    _strategy: &dyn InventoryMoveStrategy,
) -> Result<InventoryMovedFile> {
    bail!("跨盘集中迁移尚未启用")
}

/// Return true when a hard-link failure indicates the paths are on different volumes.
fn hard_link_error_is_cross_volume(error: &io::Error) -> bool {
    match error.raw_os_error() {
        #[cfg(windows)]
        Some(17) => true,
        #[cfg(unix)]
        Some(18) => true,
        _ => false,
    }
}

/// Compare two paths by their operating-system volume identity.
#[cfg(windows)]
fn paths_are_on_same_volume(from_path: &Path, to_path: &Path) -> Result<bool> {
    Ok(windows_volume_root(from_path)? == windows_volume_root(to_path)?)
}

/// Extract the drive or UNC share root used as a Windows volume key.
#[cfg(windows)]
fn windows_volume_root(path: &Path) -> Result<String> {
    use std::path::{Component, Prefix};

    let absolute = absolute_lexical_path(path)?;
    for component in absolute.components() {
        if let Component::Prefix(prefix) = component {
            let key = match prefix.kind() {
                Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                    format!("disk:{}", (drive as char).to_ascii_lowercase())
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
                    "unc:{}/{}",
                    server.to_string_lossy().to_ascii_lowercase(),
                    share.to_string_lossy().to_ascii_lowercase()
                ),
                other => bail!("不支持的 Windows 路径卷前缀：{:?}", other),
            };
            return Ok(key);
        }
    }
    bail!("无法识别路径所在卷：{}", path.to_string_lossy())
}

/// Compare two paths by their Unix device id.
#[cfg(unix)]
fn paths_are_on_same_volume(from_path: &Path, to_path: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let from_dev = fs::metadata(from_path)
        .with_context(|| format!("读取源文件失败：{}", from_path.to_string_lossy()))?
        .dev();
    let target_probe = nearest_existing_ancestor(to_path)
        .ok_or_else(|| anyhow!("目标父目录无法校验：{}", to_path.to_string_lossy()))?;
    let to_dev = fs::metadata(&target_probe)
        .with_context(|| format!("读取目标卷失败：{}", target_probe.to_string_lossy()))?
        .dev();
    Ok(from_dev == to_dev)
}

/// Build an absolute path without requiring the final target to exist.
#[cfg(windows)]
fn absolute_lexical_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

/// Find the closest existing ancestor for a target whose file may not exist yet.
#[cfg(unix)]
fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut candidate = path.parent().unwrap_or(path).to_path_buf();
    loop {
        if candidate.exists() {
            return Some(candidate);
        }
        if !candidate.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn hard_link_cross_device_error_is_classified_as_cross_volume() {
        #[cfg(windows)]
        let error = io::Error::from_raw_os_error(17);
        #[cfg(unix)]
        let error = io::Error::from_raw_os_error(18);

        assert!(hard_link_error_is_cross_volume(&error));
        assert!(!hard_link_error_is_cross_volume(
            &io::Error::from_raw_os_error(5)
        ));
    }

    #[test]
    fn source_still_visible_after_remove_keeps_moved_target() {
        let tmp = tempfile::tempdir().unwrap();
        let from_path = tmp.path().join("source.mp4");
        let to_path = tmp.path().join("target.mp4");
        fs::write(&from_path, b"recreated").unwrap();
        fs::write(&to_path, b"moved").unwrap();

        let error = ensure_source_path_removed_after_move(&from_path, &to_path).unwrap_err();

        assert!(error.to_string().contains("迁移目标已保留以避免数据丢失"));
        assert_eq!(fs::read(&to_path).unwrap(), b"moved");
    }
}
