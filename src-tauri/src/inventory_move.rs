use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Reserved destination free-space margin for future cross-volume move verification.
pub const CROSS_VOLUME_SPACE_MARGIN_BYTES: u64 = 512 * 1024 * 1024;

/// Filesystem strategy used to classify move operations and query destination capacity.
pub trait InventoryMoveStrategy: Sync {
    /// Return whether `from_path` and `to_path` are on the same filesystem volume.
    fn is_same_volume(&self, from_path: &Path, to_path: &Path) -> Result<bool>;

    /// Return currently available bytes for the filesystem containing `path`.
    fn available_space(&self, path: &Path) -> Result<u64>;

    /// Test seam invoked after a target is verified but before source removal.
    fn before_delete_source(&self, _from_path: &Path, _to_path: &Path) -> Result<()> {
        Ok(())
    }
}

/// Production move strategy backed by the host operating system filesystem APIs.
pub struct SystemInventoryMoveStrategy;

impl InventoryMoveStrategy for SystemInventoryMoveStrategy {
    fn is_same_volume(&self, from_path: &Path, to_path: &Path) -> Result<bool> {
        paths_are_on_same_volume(from_path, to_path)
    }

    fn available_space(&self, path: &Path) -> Result<u64> {
        let probe = nearest_existing_ancestor(path)
            .ok_or_else(|| anyhow!("目标空间探测路径不存在：{}", path.to_string_lossy()))?;
        fs2::available_space(&probe)
            .with_context(|| format!("读取可用空间失败：{}", probe.to_string_lossy()))
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

/// Error payload returned when a target was created but source removal could not complete.
#[derive(Debug, Clone)]
pub struct InventoryMoveRetainedTarget {
    pub moved: InventoryMovedFile,
    pub message: String,
}

impl fmt::Display for InventoryMoveRetainedTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for InventoryMoveRetainedTarget {}

/// Internal same-volume move failure used to distinguish cross-device fallback.
enum SameVolumeMoveError {
    CrossVolume,
    Failed(anyhow::Error),
}

impl SameVolumeMoveError {
    /// Convert an internal same-volume failure into a user-facing error.
    fn into_anyhow(self) -> anyhow::Error {
        match self {
            SameVolumeMoveError::CrossVolume => anyhow!("同盘硬链接跨卷，需要使用跨盘迁移"),
            SameVolumeMoveError::Failed(error) => error,
        }
    }
}

type SameVolumeMoveResult = std::result::Result<(), SameVolumeMoveError>;

/// Move one file without replacing an existing target path.
pub fn move_file_no_clobber(
    from_path: &Path,
    to_path: &Path,
    strategy: &dyn InventoryMoveStrategy,
) -> Result<InventoryMovedFile> {
    move_file_no_clobber_with_same_volume_attempt(
        from_path,
        to_path,
        strategy,
        try_move_same_volume_no_copy,
    )
}

/// Move one file while allowing tests to force same-volume cross-device fallback.
fn move_file_no_clobber_with_same_volume_attempt(
    from_path: &Path,
    to_path: &Path,
    strategy: &dyn InventoryMoveStrategy,
    same_volume_move: impl FnOnce(&Path, &Path) -> SameVolumeMoveResult,
) -> Result<InventoryMovedFile> {
    let _parent = target_parent(to_path)?;
    if to_path.exists() {
        bail!("目标路径已存在：{}", to_path.to_string_lossy());
    }
    let source_size = fs::metadata(from_path)
        .with_context(|| format!("读取源文件失败：{}", from_path.to_string_lossy()))?
        .len();

    if strategy.is_same_volume(from_path, to_path)? {
        match same_volume_move(from_path, to_path) {
            Ok(()) => {
                return Ok(InventoryMovedFile {
                    from_path: from_path.to_path_buf(),
                    to_path: to_path.to_path_buf(),
                    method: InventoryMoveMethod::SameVolume,
                    bytes: source_size,
                    sha256: None,
                });
            }
            Err(SameVolumeMoveError::CrossVolume) => {}
            Err(SameVolumeMoveError::Failed(error)) => return Err(error),
        }
    }

    move_cross_volume_verify_delete(from_path, to_path, strategy)
}

/// Move one same-volume file via hard link creation followed by source removal.
pub fn move_same_volume_no_copy(from_path: &Path, to_path: &Path) -> Result<()> {
    try_move_same_volume_no_copy(from_path, to_path).map_err(SameVolumeMoveError::into_anyhow)
}

/// Try the same-volume hard-link move and preserve cross-device classification.
fn try_move_same_volume_no_copy(from_path: &Path, to_path: &Path) -> SameVolumeMoveResult {
    let parent = to_path
        .parent()
        .ok_or_else(|| anyhow!("目标路径没有父目录：{}", to_path.to_string_lossy()))
        .map_err(SameVolumeMoveError::Failed)?;
    fs::create_dir_all(parent).map_err(|error| {
        SameVolumeMoveError::Failed(anyhow!(
            "创建目标目录失败：{}: {}",
            parent.to_string_lossy(),
            error
        ))
    })?;
    if to_path.exists() {
        return Err(SameVolumeMoveError::Failed(anyhow!(
            "目标路径已存在：{}",
            to_path.to_string_lossy()
        )));
    }
    let source_size = fs::metadata(from_path)
        .map_err(|error| {
            SameVolumeMoveError::Failed(anyhow!(
                "读取源文件失败：{}: {}",
                from_path.to_string_lossy(),
                error
            ))
        })?
        .len();
    if let Err(error) = fs::hard_link(from_path, to_path) {
        if hard_link_error_is_cross_volume(&error) {
            return Err(SameVolumeMoveError::CrossVolume);
        }
        return Err(SameVolumeMoveError::Failed(anyhow!(
            "同盘集中迁移硬链接失败：{} -> {}",
            from_path.to_string_lossy(),
            to_path.to_string_lossy()
        )));
    }
    let target_size = fs::metadata(to_path)
        .map_err(|error| {
            SameVolumeMoveError::Failed(anyhow!(
                "读取目标文件失败：{}: {}",
                to_path.to_string_lossy(),
                error
            ))
        })?
        .len();
    if target_size != source_size {
        let _ = fs::remove_file(to_path);
        return Err(SameVolumeMoveError::Failed(anyhow!(
            "同盘集中迁移大小校验失败：{}",
            from_path.to_string_lossy()
        )));
    }
    let target_sha256 = file_sha256(to_path).map_err(SameVolumeMoveError::Failed)?;
    if let Err(error) =
        delete_verified_source_via_quarantine(from_path, to_path, source_size, &target_sha256)
    {
        let moved = InventoryMovedFile {
            from_path: from_path.to_path_buf(),
            to_path: to_path.to_path_buf(),
            method: InventoryMoveMethod::SameVolume,
            bytes: source_size,
            sha256: Some(target_sha256),
        };
        return Err(SameVolumeMoveError::Failed(anyhow!(
            InventoryMoveRetainedTarget {
                moved,
                message: error.to_string(),
            }
        )));
    }
    Ok(())
}

/// Move one cross-volume file by copying, verifying, publishing, then deleting the source.
pub fn move_cross_volume_verify_delete(
    from_path: &Path,
    to_path: &Path,
    strategy: &dyn InventoryMoveStrategy,
) -> Result<InventoryMovedFile> {
    let parent = target_parent(to_path)?;
    if to_path.exists() {
        bail!("目标路径已存在：{}", to_path.to_string_lossy());
    }
    let source_size = fs::metadata(from_path)
        .with_context(|| format!("读取源文件失败：{}", from_path.to_string_lossy()))?
        .len();
    ensure_cross_volume_space(parent, source_size, strategy)?;
    fs::create_dir_all(parent)
        .with_context(|| format!("创建目标目录失败：{}", parent.to_string_lossy()))?;
    if to_path.exists() {
        bail!("目标路径已存在：{}", to_path.to_string_lossy());
    }

    let temp_path = temporary_move_path(to_path)?;
    let (copied_size, source_sha256) = match copy_source_to_new_temp(from_path, &temp_path) {
        Ok(result) => result,
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
    };
    let temp_size = match fs::metadata(&temp_path) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            return Err(error)
                .with_context(|| format!("读取临时迁移文件失败：{}", temp_path.to_string_lossy()));
        }
    };
    if copied_size != source_size || temp_size != source_size {
        let _ = fs::remove_file(&temp_path);
        bail!("跨盘集中迁移大小校验失败：{}", from_path.to_string_lossy());
    }
    let temp_sha256 = match file_sha256(&temp_path) {
        Ok(hash) => hash,
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
    };
    if temp_sha256 != source_sha256 {
        let _ = fs::remove_file(&temp_path);
        bail!("跨盘集中迁移哈希校验失败：{}", from_path.to_string_lossy());
    }

    persist_temp_without_clobber(&temp_path, to_path)
        .with_context(|| format!("提交跨盘迁移结果失败：{}", to_path.to_string_lossy()))?;
    verify_final_target(to_path, source_size, &source_sha256)?;
    strategy.before_delete_source(from_path, to_path)?;
    let moved = InventoryMovedFile {
        from_path: from_path.to_path_buf(),
        to_path: to_path.to_path_buf(),
        method: InventoryMoveMethod::CrossVolume,
        bytes: source_size,
        sha256: Some(source_sha256),
    };
    if let Err(error) = delete_verified_source_via_quarantine(
        from_path,
        to_path,
        moved.bytes,
        moved.sha256.as_deref().unwrap_or_default(),
    ) {
        return Err(anyhow!(InventoryMoveRetainedTarget {
            moved,
            message: error.to_string(),
        }));
    }

    Ok(moved)
}

/// Return the parent directory for a target path.
fn target_parent(path: &Path) -> Result<&Path> {
    path.parent()
        .ok_or_else(|| anyhow!("目标路径没有父目录：{}", path.to_string_lossy()))
}

/// Ensure the destination volume has enough free space before a cross-volume copy starts.
fn ensure_cross_volume_space(
    parent: &Path,
    source_size: u64,
    strategy: &dyn InventoryMoveStrategy,
) -> Result<()> {
    let probe = nearest_existing_ancestor(parent)
        .ok_or_else(|| anyhow!("目标空间探测路径不存在：{}", parent.to_string_lossy()))?;
    let required = source_size
        .checked_add(CROSS_VOLUME_SPACE_MARGIN_BYTES)
        .ok_or_else(|| anyhow!("跨盘迁移空间需求计算溢出"))?;
    let available = strategy.available_space(&probe)?;
    if available < required {
        bail!("目标磁盘剩余空间不足：需要 {required} 字节，可用 {available} 字节");
    }
    Ok(())
}

/// Build a same-directory temp file path that avoids existing temp files.
fn temporary_move_path(to_path: &Path) -> Result<PathBuf> {
    let parent = target_parent(to_path)?;
    let file_name = to_path
        .file_name()
        .ok_or_else(|| anyhow!("目标路径没有文件名：{}", to_path.to_string_lossy()))?
        .to_string_lossy();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default();
    for attempt in 0..100 {
        let candidate = parent.join(format!(
            ".{file_name}.mm-moving-{}-{timestamp}-{attempt}",
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!("无法生成临时迁移文件名：{}", to_path.to_string_lossy());
}

/// Copy the source into a newly created temp file while hashing the source bytes.
fn copy_source_to_new_temp(from_path: &Path, temp_path: &Path) -> Result<(u64, String)> {
    let mut input = File::open(from_path)
        .with_context(|| format!("打开源文件失败：{}", from_path.to_string_lossy()))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .with_context(|| format!("创建临时迁移文件失败：{}", temp_path.to_string_lossy()))?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .with_context(|| format!("读取源文件失败：{}", from_path.to_string_lossy()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .with_context(|| format!("写入临时迁移文件失败：{}", temp_path.to_string_lossy()))?;
        total += read as u64;
    }
    output
        .sync_all()
        .with_context(|| format!("同步临时迁移文件失败：{}", temp_path.to_string_lossy()))?;
    let digest = hasher.finalize();
    Ok((total, hex_digest(&digest)))
}

/// Make the verified temp file visible at the final path without overwriting a target.
fn persist_temp_without_clobber(temp_path: &Path, to_path: &Path) -> Result<()> {
    match fs::hard_link(temp_path, to_path) {
        Ok(()) => {
            let _ = fs::remove_file(temp_path);
            Ok(())
        }
        Err(hard_link_error) => {
            if to_path.exists() {
                let _ = fs::remove_file(temp_path);
                bail!("目标路径已存在：{}", to_path.to_string_lossy());
            }
            match copy_temp_to_new_target(temp_path, to_path) {
                Ok(()) => {
                    let _ = fs::remove_file(temp_path);
                    Ok(())
                }
                Err(error) => {
                    let _ = fs::remove_file(temp_path);
                    Err(error).with_context(|| {
                        format!("hard link fallback after failure: {hard_link_error}")
                    })
                }
            }
        }
    }
}

/// Fallback for filesystems that cannot hard-link the temp file into place.
fn copy_temp_to_new_target(temp_path: &Path, to_path: &Path) -> Result<()> {
    let mut input = File::open(temp_path)
        .with_context(|| format!("打开临时迁移文件失败：{}", temp_path.to_string_lossy()))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to_path)
        .with_context(|| format!("创建目标文件失败：{}", to_path.to_string_lossy()))?;
    if let Err(error) = io::copy(&mut input, &mut output) {
        let _ = fs::remove_file(to_path);
        return Err(error)
            .with_context(|| format!("写入目标文件失败：{}", to_path.to_string_lossy()));
    }
    if let Err(error) = output.sync_all() {
        let _ = fs::remove_file(to_path);
        return Err(error)
            .with_context(|| format!("同步目标文件失败：{}", to_path.to_string_lossy()));
    }
    Ok(())
}

/// Verify the final published target still matches the source bytes before source deletion.
fn verify_final_target(to_path: &Path, source_size: u64, source_sha256: &str) -> Result<()> {
    let target_size = fs::metadata(to_path)
        .with_context(|| format!("读取目标文件失败：{}", to_path.to_string_lossy()))?
        .len();
    if target_size != source_size {
        let _ = fs::remove_file(to_path);
        bail!(
            "跨盘集中迁移目标大小校验失败：{}",
            to_path.to_string_lossy()
        );
    }
    let target_sha256 = file_sha256(to_path)?;
    if target_sha256 != source_sha256 {
        let _ = fs::remove_file(to_path);
        bail!(
            "跨盘集中迁移目标哈希校验失败：{}",
            to_path.to_string_lossy()
        );
    }
    Ok(())
}

/// Compute a file SHA-256 hash for cross-volume verification.
fn file_sha256(path: &Path) -> Result<String> {
    let mut input =
        File::open(path).with_context(|| format!("打开文件失败：{}", path.to_string_lossy()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .with_context(|| format!("读取文件失败：{}", path.to_string_lossy()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    Ok(hex_digest(&digest))
}

/// Format a digest as lowercase hexadecimal without adding another dependency.
fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Delete a source only after moving the exact path to a quarantine file.
fn delete_verified_source_via_quarantine(
    from_path: &Path,
    to_path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<()> {
    let quarantine_path = temporary_source_delete_path(from_path)?;
    fs::rename(from_path, &quarantine_path).with_context(|| {
        format!(
            "隔离源文件失败：{}；迁移目标已保留：{}",
            from_path.to_string_lossy(),
            to_path.to_string_lossy()
        )
    })?;
    let current_size = match fs::metadata(&quarantine_path) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            let restore = restore_quarantined_source(&quarantine_path, from_path);
            bail!(
                "源文件迁移期间发生变化：无法重新读取隔离源文件 {}；迁移目标已保留：{} ({})；{}",
                quarantine_path.to_string_lossy(),
                to_path.to_string_lossy(),
                error,
                restore
            );
        }
    };
    if current_size != expected_size {
        let restore = restore_quarantined_source(&quarantine_path, from_path);
        bail!(
            "源文件迁移期间发生变化：大小 {} -> {}；迁移目标已保留：{}；{}",
            expected_size,
            current_size,
            to_path.to_string_lossy(),
            restore
        );
    }
    let current_sha256 = match file_sha256(&quarantine_path) {
        Ok(hash) => hash,
        Err(error) => {
            let restore = restore_quarantined_source(&quarantine_path, from_path);
            bail!(
                "源文件迁移期间发生变化：无法校验隔离源文件 {}；迁移目标已保留：{} ({})；{}",
                quarantine_path.to_string_lossy(),
                to_path.to_string_lossy(),
                error,
                restore
            );
        }
    };
    if current_sha256 != expected_sha256 {
        let restore = restore_quarantined_source(&quarantine_path, from_path);
        bail!(
            "源文件迁移期间发生变化：哈希不一致；迁移目标已保留：{}；{}",
            to_path.to_string_lossy(),
            restore
        );
    }
    if let Err(error) = fs::remove_file(&quarantine_path) {
        let restore = restore_quarantined_source(&quarantine_path, from_path);
        bail!(
            "删除源文件失败：{}；迁移目标已保留：{} ({})；{}",
            from_path.to_string_lossy(),
            to_path.to_string_lossy(),
            error,
            restore
        );
    }
    if from_path.exists() {
        bail!(
            "集中迁移后源路径出现新文件：{}；迁移目标已保留：{}",
            from_path.to_string_lossy(),
            to_path.to_string_lossy()
        );
    }
    Ok(())
}

/// Build a same-directory quarantine path for source deletion.
fn temporary_source_delete_path(from_path: &Path) -> Result<PathBuf> {
    let parent = from_path
        .parent()
        .ok_or_else(|| anyhow!("源路径没有父目录：{}", from_path.to_string_lossy()))?;
    let file_name = from_path
        .file_name()
        .ok_or_else(|| anyhow!("源路径没有文件名：{}", from_path.to_string_lossy()))?
        .to_string_lossy();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default();
    for attempt in 0..100 {
        let candidate = parent.join(format!(
            ".{file_name}.mm-source-delete-{}-{timestamp}-{attempt}",
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!("无法生成源文件隔离路径：{}", from_path.to_string_lossy());
}

/// Restore a quarantined source to its original path when deletion safety checks fail.
fn restore_quarantined_source(quarantine_path: &Path, from_path: &Path) -> String {
    if from_path.exists() {
        return format!(
            "源文件保留在隔离路径：{}",
            quarantine_path.to_string_lossy()
        );
    }
    match fs::rename(quarantine_path, from_path) {
        Ok(()) => format!("源文件已恢复：{}", from_path.to_string_lossy()),
        Err(error) => format!(
            "源文件保留在隔离路径：{}；恢复失败：{}",
            quarantine_path.to_string_lossy(),
            error
        ),
    }
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
fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut candidate = path.to_path_buf();
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
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Test move strategy that forces cross-volume behavior with fixed capacity.
    struct FakeCrossVolumeMoveStrategy {
        available_space: u64,
    }

    impl InventoryMoveStrategy for FakeCrossVolumeMoveStrategy {
        fn is_same_volume(&self, _from_path: &Path, _to_path: &Path) -> Result<bool> {
            Ok(false)
        }

        fn available_space(&self, _path: &Path) -> Result<u64> {
            Ok(self.available_space)
        }
    }

    /// Test move strategy that reports same-volume while still exposing capacity for fallback.
    struct FakeSameVolumeMoveStrategy {
        available_space: u64,
    }

    impl InventoryMoveStrategy for FakeSameVolumeMoveStrategy {
        fn is_same_volume(&self, _from_path: &Path, _to_path: &Path) -> Result<bool> {
            Ok(true)
        }

        fn available_space(&self, _path: &Path) -> Result<u64> {
            Ok(self.available_space)
        }
    }

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
    fn quarantine_delete_removes_only_verified_source() {
        let tmp = tempfile::tempdir().unwrap();
        let from_path = tmp.path().join("source.mp4");
        let to_path = tmp.path().join("target.mp4");
        fs::write(&from_path, b"original").unwrap();
        fs::write(&to_path, b"original").unwrap();
        let expected_sha256 = file_sha256(&from_path).unwrap();

        delete_verified_source_via_quarantine(&from_path, &to_path, 8, &expected_sha256).unwrap();

        assert!(!from_path.exists());
        assert_eq!(fs::read(&to_path).unwrap(), b"original");
    }

    #[test]
    fn nearest_existing_ancestor_returns_existing_path_itself() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("archive");
        fs::create_dir_all(&archive).unwrap();

        let ancestor = nearest_existing_ancestor(&archive).unwrap();

        assert_eq!(ancestor, archive);
    }

    #[test]
    fn source_change_before_delete_preserves_source_and_target() {
        let tmp = tempfile::tempdir().unwrap();
        let from_path = tmp.path().join("source.mp4");
        let to_path = tmp.path().join("archive").join("target.mp4");
        fs::create_dir_all(to_path.parent().unwrap()).unwrap();
        fs::write(&from_path, b"original").unwrap();
        fs::write(&to_path, b"original").unwrap();
        let expected_sha256 = file_sha256(&from_path).unwrap();
        fs::write(&from_path, b"changed-after-copy").unwrap();

        let error =
            delete_verified_source_via_quarantine(&from_path, &to_path, 8, &expected_sha256)
                .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("源文件迁移期间发生变化"));
        assert!(message.contains("迁移目标已保留"));
        assert!(message.contains("源文件已恢复"));
        assert_eq!(fs::read(&from_path).unwrap(), b"changed-after-copy");
        assert_eq!(fs::read(&to_path).unwrap(), b"original");
    }

    #[test]
    fn cross_volume_move_returns_verified_hash_and_deletes_source() {
        let tmp = tempfile::tempdir().unwrap();
        let from_path = tmp.path().join("source.mp4");
        let to_path = tmp.path().join("archive").join("target.mp4");
        fs::write(&from_path, b"cross-video").unwrap();
        let strategy = FakeCrossVolumeMoveStrategy {
            available_space: u64::MAX,
        };

        let moved = move_cross_volume_verify_delete(&from_path, &to_path, &strategy).unwrap();

        assert_eq!(moved.method, InventoryMoveMethod::CrossVolume);
        assert_eq!(moved.bytes, 11);
        assert_eq!(
            moved.sha256.as_deref(),
            Some("c20cb4cfd91b5fffedc40d1d8ec88a99c0aac76bba564e8f9d3359904bbd15e8")
        );
        assert!(!from_path.exists());
        assert_eq!(fs::read(&to_path).unwrap(), b"cross-video");
    }

    #[test]
    fn same_volume_cross_device_attempt_falls_back_to_cross_volume_move() {
        let tmp = tempfile::tempdir().unwrap();
        let from_path = tmp.path().join("source.mp4");
        let to_path = tmp.path().join("archive").join("target.mp4");
        fs::write(&from_path, b"fallback-video").unwrap();
        let strategy = FakeSameVolumeMoveStrategy {
            available_space: u64::MAX,
        };
        let attempted_same_volume = AtomicBool::new(false);

        let moved = move_file_no_clobber_with_same_volume_attempt(
            &from_path,
            &to_path,
            &strategy,
            |_from_path, _to_path| {
                attempted_same_volume.store(true, Ordering::SeqCst);
                Err(SameVolumeMoveError::CrossVolume)
            },
        )
        .unwrap();

        assert!(attempted_same_volume.load(Ordering::SeqCst));
        assert_eq!(moved.method, InventoryMoveMethod::CrossVolume);
        assert!(!from_path.exists());
        assert_eq!(fs::read(&to_path).unwrap(), b"fallback-video");
        assert!(moved.sha256.is_some());
    }
}
