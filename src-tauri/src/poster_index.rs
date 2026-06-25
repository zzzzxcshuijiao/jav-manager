//! External image-library index keyed by code.
//!
//! Three flat libraries are scanned into one case-insensitive code -> image
//! map that the rebuild step uses to backfill covers, screenshots, and GIF
//! previews for works whose own NFO has no usable artwork:
//!
//!   * Poster directory     - files named `<code>.<ext>`
//!   * ScreenShot directory - files named `<code>-shot.<ext>` (suffix optional)
//!   * GIF directory        - files named `<code>.gif` (non-code names skipped)

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One resolved image set keyed by code.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PosterEntry {
    /// Poster image path (e.g. Poster/ABP-601.jpg) when present.
    pub poster: Option<PathBuf>,
    /// Screenshot image path (e.g. ScreenShot/ABP-601-shot.jpg).
    pub screenshot: Option<PathBuf>,
    /// Animated preview path (e.g. AAAAA/ABP-601.gif).
    pub gif: Option<PathBuf>,
}

impl PosterEntry {
    /// True when at least one of poster/screenshot/gif is available.
    pub fn has_any(&self) -> bool {
        self.poster.is_some() || self.screenshot.is_some() || self.gif.is_some()
    }
}

/// Which directory slot a scanned image belongs to.
#[derive(Clone, Copy)]
enum ImageKind {
    Poster,
    Screenshot,
    Gif,
}

/// In-memory code -> image-set index. Built once from a directory scan and
/// queried cheaply during rebuild. The map is keyed by the UPPERCASE
/// normalized code so lookups are case-insensitive.
#[derive(Debug, Clone, Default)]
pub struct PosterIndex {
    entries: HashMap<String, PosterEntry>,
}

impl PosterIndex {
    /// Build an index by scanning the three flat image directories. Any
    /// directory may be missing (the index simply has no entries for that
    /// kind). Non-image files and names that are not a code are skipped.
    pub fn scan(poster_dir: &Path, screenshot_dir: &Path, gif_dir: &Path) -> Self {
        let mut entries: HashMap<String, PosterEntry> = HashMap::new();
        index_dir(&mut entries, poster_dir, ImageKind::Poster);
        index_dir(&mut entries, screenshot_dir, ImageKind::Screenshot);
        index_dir(&mut entries, gif_dir, ImageKind::Gif);
        PosterIndex { entries }
    }

    /// Empty index with no directories.
    pub fn empty() -> Self {
        PosterIndex {
            entries: HashMap::new(),
        }
    }

    /// Build an index from a scanned resource pool. Each pooled work's
    /// poster/screenshot/gif (matched by code across all scanned dirs) is
    /// funneled into the same code-keyed lookup that the rebuild pipeline uses.
    /// This is the bridge that lets the existing `persist_grouped_rebuild` image
    /// backfill consume the unified resource pool without changes.
    pub fn from_pool(pool: &crate::domain::ResourcePool) -> Self {
        let mut entries: HashMap<String, PosterEntry> = HashMap::new();
        for work in &pool.works {
            let key = work.code.to_ascii_uppercase();
            let entry = entries.entry(key).or_default();
            if entry.poster.is_none() {
                entry.poster = work.poster.clone();
            }
            if entry.screenshot.is_none() {
                entry.screenshot = work.screenshots.first().cloned();
            }
            if entry.gif.is_none() {
                entry.gif = work.gifs.first().cloned();
            }
        }
        PosterIndex { entries }
    }

    /// Number of distinct codes with at least one image.
    pub fn len(&self) -> usize {
        self.entries.values().filter(|e| e.has_any()).count()
    }

    /// True when no codes are indexed.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Look up the image entry for a code (case-insensitive).
    pub fn get(&self, code: &str) -> Option<&PosterEntry> {
        self.entries.get(&code.to_ascii_uppercase())
    }
}

fn index_dir(entries: &mut HashMap<String, PosterEntry>, dir: &Path, kind: ImageKind) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if !is_image_extension(path.extension().and_then(|e| e.to_str())) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let code = normalize_stem(stem, kind);
        // Skip names that are not a code (forum date IDs, actor names, etc).
        if !is_code(&code) {
            continue;
        }
        let entry = entries.entry(code).or_default();
        match kind {
            ImageKind::Poster => entry.poster = Some(path.clone()),
            ImageKind::Screenshot => entry.screenshot = Some(path.clone()),
            ImageKind::Gif => entry.gif = Some(path.clone()),
        }
    }
}

/// Strip the "-shot" suffix used by the screenshot directory and uppercase the
/// code so the index is case-insensitive.
fn normalize_stem(stem: &str, kind: ImageKind) -> String {
    let trimmed = match kind {
        ImageKind::Screenshot => stem
            .strip_suffix("-shot")
            .or_else(|| stem.strip_suffix("-SHOT"))
            .unwrap_or(stem),
        _ => stem,
    };
    trimmed.to_ascii_uppercase()
}

/// A code looks like LETTERS-DIGITS (e.g. ABP-601). Used to filter out forum
/// date IDs (062715-909) and actor/source names from the GIF directory.
fn is_code(code: &str) -> bool {
    let Some((letters, digits)) = code.split_once('-') else {
        return false;
    };
    !letters.is_empty()
        && letters.chars().all(|c| c.is_ascii_alphabetic())
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
}

fn is_image_extension(ext: Option<&str>) -> bool {
    matches!(
        ext.map(str::to_ascii_lowercase).as_deref(),
        Some("jpg") | Some("jpeg") | Some("png") | Some("webp") | Some("gif")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"x").unwrap();
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "poster_index_{}_{}",
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
    fn scan_builds_case_insensitive_three_source_map() {
        let tmp = tmp_dir("ci");
        let poster = tmp.join("Poster");
        let shot = tmp.join("ScreenShot");
        let gif = tmp.join("Gif");
        touch(&poster.join("ABP-601.jpg"));
        touch(&poster.join("ipx-065.png"));
        touch(&shot.join("ABP-601-shot.jpg"));
        touch(&shot.join("ipx-065-shot.jpg"));
        touch(&gif.join("ABP-601.gif"));
        touch(&shot.join("readme.txt")); // ignored: not an image
        touch(&gif.join("062715-909.gif")); // ignored: not a code

        let index = PosterIndex::scan(&poster, &shot, &gif);
        assert_eq!(index.len(), 2);

        let abp = index.get("abp-601").expect("ABP-601 present");
        assert_eq!(abp.poster.as_ref().unwrap().file_name().unwrap(), "ABP-601.jpg");
        assert_eq!(
            abp.screenshot.as_ref().unwrap().file_name().unwrap(),
            "ABP-601-shot.jpg"
        );
        assert_eq!(abp.gif.as_ref().unwrap().file_name().unwrap(), "ABP-601.gif");

        let ipx = index.get("IPX-065").expect("IPX-065 present");
        assert!(ipx.poster.is_some());
        assert!(ipx.screenshot.is_some());
        assert!(ipx.gif.is_none());
    }

    #[test]
    fn scan_tolerates_missing_directories() {
        let index = PosterIndex::scan(
            &PathBuf::from("/no/such/poster"),
            &PathBuf::from("/no/such/shot"),
            &PathBuf::from("/no/such/gif"),
        );
        assert!(index.is_empty());
        assert!(index.get("ANY-001").is_none());
    }

    #[test]
    fn non_code_names_in_gif_dir_are_skipped() {
        let tmp = tmp_dir("skip");
        let empty = tmp.join("empty");
        let gif = tmp.join("Gif");
        fs::create_dir_all(&empty).unwrap();
        touch(&gif.join("cytherea.gif")); // actor name, not a code
        touch(&gif.join("010414-516.gif")); // forum date id
        touch(&gif.join("MIDE-872.gif")); // real code

        let index = PosterIndex::scan(&empty, &empty, &gif);
        assert_eq!(index.len(), 1);
        assert!(index.get("MIDE-872").is_some());
        assert!(index.get("cytherea").is_none());
    }
}
