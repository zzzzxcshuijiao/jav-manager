//! Centralized archive migration: gather scattered NFO + video files into
//! work-specific directories for portability and self-contained library structure.
//!
//! Typical use case: user has JAV_output NFOs in演员/番号/ structure, videos
//! in a flat H:\avlibrary\ directory, and wants to consolidate into
//! H:\consolidated\<code>\<code>.nfo + <code>.mp4 + <code>-v2.mp4 etc.

use crate::domain::MigrationPlan;
use crate::domain::MigrationWorkPlan;
use crate::domain::ResourcePool;
use crate::domain::UnifiedMigrationPlan;
use crate::domain::UnifiedMigrationWorkPlan;
use crate::identifier::normalize_code;
use crate::nfo::parse_nfo_document;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Scan nfo_dir for all .nfo files, extract codes, match videos in video_dir
/// by code + optional -v<n> suffix, and produce a migration plan targeting
/// target_dir/<code>/ subdirectories. Does not modify the filesystem.
pub fn plan_migration(
    nfo_dir: &Path,
    video_dir: &Path,
    target_dir: &Path,
) -> Result<MigrationPlan> {
    // Step 1: Scan all NFOs and extract normalized codes
    let mut nfo_by_code: HashMap<String, PathBuf> = HashMap::new();
    for entry in WalkDir::new(nfo_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("nfo") {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(parsed) = parse_nfo_document(&content) {
                    if let Some(source_code) = parsed.source_code {
                        if let Some(normalized) = normalize_code(&source_code) {
                            nfo_by_code.insert(normalized, path.to_path_buf());
                        }
                    }
                }
            }
        }
    }

    // Step 2: Scan video_dir for files matching <code>.ext or <code>-v<n>.ext
    let mut video_by_code: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for entry in WalkDir::new(video_dir)
        .max_depth(1) // flat directory
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts"]
            .contains(&ext.to_ascii_lowercase().as_str())
        {
            continue;
        }

        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            // Try to extract code: "SSIS-123" or "SSIS-123-v2"
            let code = if let Some(pos) = stem.rfind("-v") {
                // Multi-version: "SSIS-123-v2" -> "SSIS-123"
                &stem[..pos]
            } else {
                stem
            };

            if let Some(normalized) = normalize_code(code) {
                video_by_code
                    .entry(normalized)
                    .or_insert_with(Vec::new)
                    .push(path.to_path_buf());
            }
        }
    }

    // Step 3: Build migration plan
    let mut works = Vec::new();
    let mut matched_videos = 0;

    for (code, nfo_path) in &nfo_by_code {
        let video_paths = video_by_code.get(code).cloned().unwrap_or_default();
        matched_videos += video_paths.len();

        works.push(MigrationWorkPlan {
            code: code.clone(),
            nfo_path: nfo_path.clone(),
            video_paths,
            target_dir: target_dir.join(code),
        });
    }

    let total_nfos = nfo_by_code.len();
    let unmatched_nfos = works.iter().filter(|w| w.video_paths.is_empty()).count();

    Ok(MigrationPlan {
        works,
        total_nfos,
        matched_videos,
        unmatched_nfos,
    })
}

/// Execute a migration plan: create target directories, copy NFO, move videos.
/// Returns the number of works successfully migrated.
pub fn execute_migration(plan: &MigrationPlan) -> Result<usize> {
    let mut migrated = 0;

    for work in &plan.works {
        // Skip works with no videos (NFO-only)
        if work.video_paths.is_empty() {
            continue;
        }

        // Create target directory
        fs::create_dir_all(&work.target_dir)
            .with_context(|| format!("Failed to create directory {:?}", work.target_dir))?;

        // Copy NFO to target/<code>/<code>.nfo
        let nfo_target = work.target_dir.join(format!("{}.nfo", work.code));
        fs::copy(&work.nfo_path, &nfo_target).with_context(|| {
            format!(
                "Failed to copy NFO {:?} -> {:?}",
                work.nfo_path, nfo_target
            )
        })?;

        // Move videos to target/<code>/<code>.ext or <code>-v2.ext
        for (idx, video_path) in work.video_paths.iter().enumerate() {
            let ext = video_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mp4");
            let target_name = if work.video_paths.len() == 1 {
                format!("{}.{}", work.code, ext)
            } else {
                format!("{}-v{}.{}", work.code, idx + 1, ext)
            };
            let video_target = work.target_dir.join(target_name);
            fs::rename(video_path, &video_target).with_context(|| {
                format!(
                    "Failed to move video {:?} -> {:?}",
                    video_path, video_target
                )
            })?;
        }

        migrated += 1;
    }

    Ok(migrated)
}

// ===== Unified migration (drives off a scanned ResourcePool) =====

/// Build a unified migration plan from a scanned resource pool. Every work that
/// has a NFO (and at least the NFO itself) becomes a self-contained target
/// directory under `target_dir/<code>/`. Counts aggregate over all works.
pub fn plan_unified_migration(pool: &ResourcePool, target_dir: &Path) -> UnifiedMigrationPlan {
    let mut works = Vec::new();
    let mut total_videos = 0usize;
    let mut total_images = 0usize;

    for work in &pool.works {
        // Only works defined by a NFO are migratable.
        if work.nfo_path.is_none() {
            continue;
        }
        total_videos += work.videos.len();
        total_images += image_count(work);
        works.push(UnifiedMigrationWorkPlan {
            code: work.code.clone(),
            nfo_path: work.nfo_path.clone(),
            videos: work.videos.clone(),
            poster: work.poster.clone(),
            fanart: work.fanart.clone(),
            thumb: work.thumb.clone(),
            screenshots: work.screenshots.clone(),
            gifs: work.gifs.clone(),
            target_dir: target_dir.join(&work.code),
        });
    }

    UnifiedMigrationPlan {
        total_works: works.len(),
        total_videos,
        total_images,
        works,
    }
}

/// Execute a unified migration plan: create each work directory, copy the NFO,
/// move videos (with -vN naming for multi-version), and copy artwork so every
/// work directory is self-contained. Returns the number of works migrated.
pub fn execute_unified_migration(plan: &UnifiedMigrationPlan) -> Result<usize> {
    let mut migrated = 0;

    for work in &plan.works {
        fs::create_dir_all(&work.target_dir)
            .with_context(|| format!("Failed to create directory {:?}", work.target_dir))?;

        // 1. Copy NFO -> target/<code>/<code>.nfo
        if let Some(nfo_path) = &work.nfo_path {
            let nfo_target = work.target_dir.join(format!("{}.nfo", work.code));
            fs::copy(nfo_path, &nfo_target).with_context(|| {
                format!("Failed to copy NFO {:?} -> {:?}", nfo_path, nfo_target)
            })?;
        }

        // 2. Move videos -> target/<code>/<code>.ext or <code>-vN.ext
        for (idx, video) in work.videos.iter().enumerate() {
            let ext = video
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mp4");
            let name = if work.videos.len() == 1 {
                format!("{}.{}", work.code, ext)
            } else {
                format!("{}-v{}.{}", work.code, idx + 1, ext)
            };
            let target = work.target_dir.join(name);
            fs::rename(video, &target)
                .with_context(|| format!("Failed to move video {:?} -> {:?}", video, target))?;
        }

        // 3. Copy artwork into the work dir (self-contained library).
        if let Some(poster) = &work.poster {
            copy_into(poster, &work.target_dir, "poster")?;
        }
        if let Some(fanart) = &work.fanart {
            copy_into(fanart, &work.target_dir, "fanart")?;
        }
        if let Some(thumb) = &work.thumb {
            copy_into(thumb, &work.target_dir, "thumb")?;
        }
        for shot in &work.screenshots {
            copy_into(shot, &work.target_dir.join("screenshots"), "screenshot")?;
        }
        for gif in &work.gifs {
            copy_into(gif, &work.target_dir, "preview")?;
        }

        migrated += 1;
    }

    Ok(migrated)
}

/// Count artwork resources on a pooled work (for plan summary).
fn image_count(work: &crate::domain::PooledWork) -> usize {
    let mut n = 0;
    if work.poster.is_some() {
        n += 1;
    }
    if work.fanart.is_some() {
        n += 1;
    }
    if work.thumb.is_some() {
        n += 1;
    }
    n + work.screenshots.len() + work.gifs.len()
}

/// Copy `src` into `dir` preserving its original file name; `label` is only a
/// fallback when the source has no file name. The destination directory is
/// created on demand.
fn copy_into(src: &Path, dir: &Path, label: &str) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("Failed to create directory {:?}", dir))?;
    let file_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(label)
        .to_string();
    let target = dir.join(&file_name);
    fs::copy(src, &target)
        .with_context(|| format!("Failed to copy image {:?} -> {:?}", src, target))?;
    Ok(())
}

/// Incremental sync: copy a pooled work's resources into its primary-library
/// directory, but ONLY when the resource lives outside the primary dir (i.e.
/// it is a new/gap resource that needs to be consolidated). Resources already
/// inside the primary dir are skipped (no re-copy). Returns the number of files
/// copied. Used by `incremental_sync_from_pool`.
pub fn sync_work_into_primary(
    work: &crate::domain::PooledWork,
    work_dir: &Path,
    primary_dir: &Path,
) -> Result<usize> {
    let mut copied = 0usize;

    // NFO
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

    // Videos (move semantics NOT used here — copy so the original pool stays
    // intact; the primary dir is the read authority).
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

    // Artwork
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

/// True when `path` is located inside `base` (case-insensitive on Windows).
fn is_under(path: &Path, base: &Path) -> bool {
    let path_str = path.to_string_lossy().to_ascii_lowercase();
    let base_str = base.to_string_lossy().to_ascii_lowercase();
    path_str.starts_with(&base_str)
}
