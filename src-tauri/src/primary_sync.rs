//! Primary-library synchronization helpers.
//!
//! This module keeps the "copy missing resources into the primary library"
//! path separate from the retired migration entry points. It is used by the
//! library rebuild/sync flow and never moves source files.

use anyhow::Result;
use std::fs;
use std::path::Path;

/// Copy one pooled work's missing resources into its primary-library work dir.
///
/// Resources already under `primary_dir` are skipped so repeated syncs do not
/// re-copy the same files. Video and artwork files are copied, not moved; the
/// primary library becomes the read authority while the source pool remains
/// intact. Returns the number of files copied.
pub fn sync_work_into_primary(
    work: &crate::domain::PooledWork,
    work_dir: &Path,
    primary_dir: &Path,
) -> Result<usize> {
    let mut copied = 0usize;

    if let Some(nfo) = &work.nfo_path {
        if !is_under(nfo, primary_dir) {
            let target = work_dir.join(format!("{}.nfo", work.code));
            if !target.exists() {
                if let Some(parent) = work_dir.parent() {
                    fs::create_dir_all(parent).ok();
                }
                fs::create_dir_all(work_dir).ok();
                fs::copy(nfo, &target)?;
                copied += 1;
            }
        }
    }

    for video in &work.videos {
        if is_under(video, primary_dir) {
            continue;
        }
        let name = video
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("video")
            .to_string();
        let target = work_dir.join(&name);
        if !target.exists() {
            fs::copy(video, &target)?;
            copied += 1;
        }
    }

    let mut copy_image = |src: &Path, sub: &str| -> Result<()> {
        if is_under(src, primary_dir) {
            return Ok(());
        }
        let dir = if sub.is_empty() {
            work_dir.to_path_buf()
        } else {
            work_dir.join(sub)
        };
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("image")
            .to_string();
        let target = dir.join(&name);
        if !target.exists() {
            fs::create_dir_all(&dir)?;
            fs::copy(src, &target)?;
            copied += 1;
        }
        Ok(())
    };

    if let Some(p) = &work.poster {
        copy_image(p, "")?;
    }
    if let Some(p) = &work.fanart {
        copy_image(p, "")?;
    }
    if let Some(p) = &work.thumb {
        copy_image(p, "")?;
    }
    for s in &work.screenshots {
        copy_image(s, "screenshots")?;
    }
    for g in &work.gifs {
        copy_image(g, "")?;
    }

    Ok(copied)
}

/// Return true when `path` is located inside `base`.
///
/// The comparison is case-insensitive to match Windows path behavior, which is
/// the primary deployment environment for this app.
fn is_under(path: &Path, base: &Path) -> bool {
    let path_str = path.to_string_lossy().to_ascii_lowercase();
    let base_str = base.to_string_lossy().to_ascii_lowercase();
    path_str.starts_with(&base_str)
}
