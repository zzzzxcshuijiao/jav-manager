//! Unified resource pool scanner.
//!
//! Every configured directory is treated as an opaque pool of resources. The
//! scanner walks them recursively, classifies files by extension, extracts a
//! normalized code from each file name, and aggregates everything keyed by
//! code. NFOs are the authority that defines a work (no NFO => the work does
//! not exist for rebuild/migration purposes); videos and images attach to a
//! work when their file name carries the same code.
//!
//! This replaces the old split between "source roots (NFO+video co-located)"
//! and "external image library (code-named)" so the user no longer has to tell
//! the app which directory holds which resource type.

use crate::domain::PooledWork;
use crate::domain::ResourcePool;
use crate::identifier::normalize_code;
use crate::nfo::parse_nfo_document;
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts",
];
const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];
const GIF_EXT: &str = "gif";

/// Walk every root, classify each file, and aggregate by normalized code.
/// NFOs define works; videos/images attach to a work when their stem encodes
/// the same code. Files whose name carries no parseable code are counted as
/// orphans but otherwise ignored.
///
/// Two passes: (1) collect every NFO so every code's work entry exists; (2)
/// attach videos/images. This avoids ordering sensitivity — a video seen
/// before its NFO during the directory walk would otherwise be mis-counted as
/// an orphan.
pub fn scan_resource_pool(roots: &[PathBuf]) -> Result<ResourcePool> {
    // code -> PooledWork, insertion-ordered (first NFO sighting).
    let mut works: HashMap<String, PooledWork> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut total_nfos: usize = 0;
    let mut total_videos: usize = 0;
    let mut total_images: usize = 0;
    let mut orphan_videos: usize = 0;
    let mut orphan_images: usize = 0;

    // Pending resources collected in pass 1, attached in pass 2 once all NFOs
    // are known. (code, kind, path) — defers attachment so walk order does not
    // matter (a video seen before its NFO is still matched).
    let mut pending: Vec<(String, FileKind, PathBuf)> = Vec::new();

    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root).follow_links(false) {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e.to_ascii_lowercase(),
                None => continue,
            };
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };

            // NFO: parse and extract code from the document's <num>/source_code.
            if ext == "nfo" {
                let Ok(text) = fs::read_to_string(path) else {
                    continue;
                };
                let Ok(document) = parse_nfo_document(&text) else {
                    continue;
                };
                let Some(raw_code) = document.source_code.as_ref() else {
                    continue;
                };
                let Some(code) = normalize_code(raw_code) else {
                    continue;
                };
                total_nfos += 1;
                let work = works.entry(code.clone()).or_insert_with(|| {
                    order.push(code.clone());
                    PooledWork::new(code.clone())
                });
                if work.nfo_path.is_none() {
                    work.nfo_path = Some(path.to_path_buf());
                }
                continue;
            }

            // Video / image / gif: defer attachment until all NFOs are known.
            let Some(kind) = file_kind(&ext) else {
                continue;
            };
            let Some(code) = code_from_stem(stem) else {
                // No parseable code in the file name → orphan (counted, ignored).
                match kind {
                    FileKind::Video => orphan_videos += 1,
                    FileKind::Image | FileKind::Gif => orphan_images += 1,
                }
                continue;
            };
            match kind {
                FileKind::Video => total_videos += 1,
                FileKind::Image | FileKind::Gif => total_images += 1,
            }
            pending.push((code, kind, path.to_path_buf()));
        }
    }

    // Pass 2: attach each pending resource to its work if an NFO defined that
    // code; otherwise it is an orphan.
    for (code, kind, path) in pending {
        let Some(work) = works.get_mut(&code) else {
            match kind {
                FileKind::Video => orphan_videos += 1,
                FileKind::Image | FileKind::Gif => orphan_images += 1,
            }
            continue;
        };
        attach_resource(work, kind, &path);
    }

    let works: Vec<PooledWork> = order
        .into_iter()
        .filter_map(|code| works.remove(&code))
        .collect();

    // Sort each work's videos so multi-version ordering is stable (-v2 after v1).
    let mut works = works;
    for work in &mut works {
        work.videos.sort_by(|a, b| natural_stem_cmp(a, b));
    }

    Ok(ResourcePool {
        works,
        total_nfos,
        total_videos,
        total_images,
        orphan_videos,
        orphan_images,
    })
}

#[derive(Clone, Copy)]
enum FileKind {
    Video,
    /// Static image. Its poster/fanart/thumb/screenshot role is inferred from the stem.
    Image,
    Gif,
}

#[derive(Clone, Copy)]
enum ImageRole {
    Poster,
    Fanart,
    Thumb,
    Screenshot,
    Generic,
}

/// Classify supported resource extensions; unsupported files are ignored by the pool scanner.
fn file_kind(ext: &str) -> Option<FileKind> {
    if VIDEO_EXTS.contains(&ext) {
        return Some(FileKind::Video);
    }
    if ext == GIF_EXT {
        return Some(FileKind::Gif);
    }
    if IMAGE_EXTS.contains(&ext) {
        return Some(FileKind::Image);
    }
    None
}

/// Attach a classified resource file to a work, picking the right field by
/// inspecting the file-name suffix (e.g. `SSIS-123-poster.jpg`).
fn attach_resource(work: &mut PooledWork, kind: FileKind, path: &Path) {
    match kind {
        FileKind::Video => work.videos.push(path.to_path_buf()),
        FileKind::Gif => work.gifs.push(path.to_path_buf()),
        FileKind::Image => {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            let role = image_role_from_stem(&stem);
            match role {
                ImageRole::Poster => {
                    work.poster.get_or_insert_with(|| path.to_path_buf());
                }
                ImageRole::Fanart => {
                    work.fanart.get_or_insert_with(|| path.to_path_buf());
                }
                ImageRole::Thumb => {
                    work.thumb.get_or_insert_with(|| path.to_path_buf());
                }
                ImageRole::Screenshot => work.screenshots.push(path.to_path_buf()),
                ImageRole::Generic => {
                    // Bare code-named image (e.g. SSIS-123.jpg) → treat as poster.
                    work.poster.get_or_insert_with(|| path.to_path_buf());
                }
            }
        }
    }
}

/// Infer an image's role from its stem suffix (after the code). Recognized
/// scraper suffixes: `-poster`, `-fanart`, `-thumb`, `-shot`/`-screenshot`.
fn image_role_from_stem(stem: &str) -> ImageRole {
    if stem.ends_with("-poster") || stem.ends_with("-ps") {
        return ImageRole::Poster;
    }
    if stem.ends_with("-fanart") || stem.ends_with("-pl") {
        return ImageRole::Fanart;
    }
    if stem.ends_with("-thumb") {
        return ImageRole::Thumb;
    }
    if stem.ends_with("-shot") || stem.ends_with("-screenshot") {
        return ImageRole::Screenshot;
    }
    ImageRole::Generic
}

/// Extract a normalized code from a resource file stem. Strips a trailing
/// version/CD segment first so `SSIS-123-v2` / `SSIS-123-cd2` collapse to
/// `SSIS-123`. Returns None when no code can be parsed.
fn code_from_stem(stem: &str) -> Option<String> {
    let stripped = strip_version_segment(stem).unwrap_or_else(|| stem.to_string());
    normalize_code(&stripped)
}

/// Strip a trailing version/CD/disc/part segment: `-v2`, `-cd2`, `_disc3`, `part1`.
fn strip_version_segment(stem: &str) -> Option<String> {
    let lower = stem.to_ascii_lowercase();
    let cut = ["-v", "-cd", "_cd", "-disc", "_disc", "-part", "_part"]
        .iter()
        .find_map(|seg| {
            lower.rfind(seg).filter(|&pos| {
                // Segment must be followed by digits only until end.
                let tail = &lower[pos + seg.len()..];
                !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit())
            })
        });

    match cut {
        Some(pos) => Some(stem[..pos].to_string()),
        None => None,
    }
}

/// Natural comparison of two paths by file stem so `SSIS-123-v2` sorts after
/// `SSIS-123` (lexicographic on the stem is sufficient for stable ordering).
fn natural_stem_cmp(a: &Path, b: &Path) -> std::cmp::Ordering {
    let a_stem = a.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let b_stem = b.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    a_stem.cmp(b_stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn nfo(code: &str) -> String {
        format!("<movie><num>{code}</num><title>{code}</title></movie>")
    }

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "resource_pool_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_aggregates_by_code_across_dirs() {
        let nfo_dir = tmp("nfo");
        let video_dir = tmp("video");
        let img_dir = tmp("img");

        touch(&nfo_dir.join("SSIS-123.nfo"), &nfo("SSIS-123"));
        touch(&video_dir.join("SSIS-123.mp4"), "x");
        touch(&img_dir.join("SSIS-123.jpg"), "x");

        let pool = scan_resource_pool(&[nfo_dir, video_dir, img_dir]).unwrap();
        assert_eq!(pool.total_nfos, 1);
        assert_eq!(pool.total_videos, 1);
        assert_eq!(pool.total_images, 1);
        assert_eq!(pool.works.len(), 1);
        let work = &pool.works[0];
        assert_eq!(work.code, "SSIS-123");
        assert!(work.nfo_path.is_some());
        assert_eq!(work.videos.len(), 1);
        assert!(work.poster.is_some());
    }

    #[test]
    fn multi_version_videos_collapse_to_one_code() {
        let dir = tmp("multi");
        touch(&dir.join("IPX-456.nfo"), &nfo("IPX-456"));
        touch(&dir.join("IPX-456.mp4"), "x");
        touch(&dir.join("IPX-456-v2.mkv"), "x");
        touch(&dir.join("IPX-456-cd3.mp4"), "x");

        let pool = scan_resource_pool(&[dir]).unwrap();
        let work = &pool.works[0];
        assert_eq!(work.videos.len(), 3, "all three videos attach to one work");
    }

    #[test]
    fn image_roles_classified_by_suffix() {
        let dir = tmp("roles");
        touch(&dir.join("ABP-601.nfo"), &nfo("ABP-601"));
        touch(&dir.join("ABP-601-poster.jpg"), "x");
        touch(&dir.join("ABP-601-fanart.jpg"), "x");
        touch(&dir.join("ABP-601-shot.jpg"), "x");
        touch(&dir.join("ABP-601.gif"), "x");

        let pool = scan_resource_pool(&[dir]).unwrap();
        let work = &pool.works[0];
        assert!(work.poster.is_some(), "poster by suffix");
        assert!(work.fanart.is_some(), "fanart by suffix");
        assert_eq!(work.screenshots.len(), 1, "screenshot by -shot suffix");
        assert_eq!(work.gifs.len(), 1, "gif");
    }

    #[test]
    fn resources_without_nfo_are_orphans() {
        let dir = tmp("orphan");
        // Video with a code but no matching NFO → orphan video.
        touch(&dir.join("MIDE-872.mp4"), "x");
        // Image with no parseable code → orphan image.
        touch(&dir.join("actor-name.jpg"), "x");

        let pool = scan_resource_pool(&[dir]).unwrap();
        assert_eq!(pool.works.len(), 0, "no NFO means no work");
        assert_eq!(pool.orphan_videos, 1);
        assert_eq!(pool.orphan_images, 1);
    }

    #[test]
    fn bare_code_image_falls_back_to_poster() {
        let dir = tmp("bare");
        touch(&dir.join("SSIS-123.nfo"), &nfo("SSIS-123"));
        touch(&dir.join("SSIS-123.jpg"), "x"); // no role suffix

        let pool = scan_resource_pool(&[dir]).unwrap();
        let work = &pool.works[0];
        assert!(work.poster.is_some(), "bare code image becomes poster");
    }

    #[test]
    fn unsupported_extensions_are_not_counted_as_images() {
        let dir = tmp("unsupported");
        touch(&dir.join("IPX-159.nfo"), &nfo("IPX-159"));
        touch(&dir.join("IPX-159.txt"), "metadata");
        touch(&dir.join("IPX-160.txt"), "orphan");

        let pool = scan_resource_pool(&[dir]).unwrap();
        assert_eq!(pool.total_images, 0, "text files are not image resources");
        assert_eq!(pool.orphan_images, 0, "text files are not orphan images");
        assert!(
            pool.works[0].poster.is_none(),
            "text file does not become poster"
        );
    }
}
