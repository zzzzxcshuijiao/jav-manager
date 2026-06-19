use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

pub const DEFAULT_THUMBNAIL_CACHE_LIMIT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbnailCacheSummary {
    pub file_count: usize,
    pub total_bytes: u64,
}

pub fn thumbnail_cache_path(cache_root: &Path, video_path: &Path) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(video_path.to_string_lossy().as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    cache_root.join(format!("{digest}.jpg"))
}

pub fn thumbnail_cache_summary(cache_root: &Path) -> Result<ThumbnailCacheSummary> {
    if !cache_root.exists() {
        return Ok(ThumbnailCacheSummary::default());
    }

    let mut summary = ThumbnailCacheSummary::default();
    for entry in fs::read_dir(cache_root)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let metadata = fs::metadata(&path)?;
        summary.file_count += 1;
        summary.total_bytes = summary.total_bytes.saturating_add(metadata.len());
    }
    Ok(summary)
}

pub fn clear_thumbnail_cache(cache_root: &Path) -> Result<ThumbnailCacheSummary> {
    let summary = thumbnail_cache_summary(cache_root)?;
    if !cache_root.exists() {
        return Ok(summary);
    }

    for entry in fs::read_dir(cache_root)? {
        let path = entry?.path();
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    Ok(summary)
}

pub fn get_or_create_thumbnail(
    video_path: &Path,
    cache_root: &Path,
    max_cache_bytes: u64,
) -> Result<Option<PathBuf>> {
    if !video_path.exists() {
        return Err(anyhow!(
            "video path does not exist: {}",
            video_path.to_string_lossy()
        ));
    }
    fs::create_dir_all(cache_root)?;
    let target = thumbnail_cache_path(cache_root, video_path);
    if target.exists() {
        return Ok(Some(target));
    }

    if !generate_thumbnail_frame(video_path, &target, "00:00:03")
        && !generate_thumbnail_frame(video_path, &target, "00:00:00")
    {
        let _ = fs::remove_file(&target);
        return Ok(None);
    }

    enforce_cache_size(cache_root, max_cache_bytes)?;
    Ok(Some(target))
}

pub fn enforce_cache_size(cache_root: &Path, max_bytes: u64) -> Result<()> {
    if !cache_root.exists() {
        return Ok(());
    }

    let mut entries = Vec::new();
    let mut total_bytes = 0_u64;
    for entry in fs::read_dir(cache_root)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let metadata = fs::metadata(&path)?;
        let size = metadata.len();
        total_bytes = total_bytes.saturating_add(size);
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        entries.push((path, size, modified));
    }

    if total_bytes <= max_bytes {
        return Ok(());
    }

    entries.sort_by_key(|(_, _, modified)| *modified);
    for (path, size, _) in entries {
        if total_bytes <= max_bytes {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total_bytes = total_bytes.saturating_sub(size);
        }
    }

    Ok(())
}

fn generate_thumbnail_frame(video_path: &Path, target: &Path, timestamp: &str) -> bool {
    let _ = fs::remove_file(target);
    let status = Command::new(ffmpeg_executable())
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss", timestamp, "-i"])
        .arg(video_path)
        .args(["-frames:v", "1", "-vf", "scale=640:-1"])
        .arg(target)
        .status();

    let Ok(status) = status else {
        return false;
    };
    if !status.success() {
        return false;
    }
    fs::metadata(target)
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false)
}

fn ffmpeg_executable() -> PathBuf {
    if let Ok(path) = std::env::var("FFMPEG_PATH") {
        let path = PathBuf::from(path);
        if path.exists() {
            return path;
        }
    }

    let bundled = PathBuf::from(r"D:\Program Files\ffmpeg-20191206-b66a800-win64-static\bin\ffmpeg.exe");
    if bundled.exists() {
        return bundled;
    }

    PathBuf::from("ffmpeg")
}
