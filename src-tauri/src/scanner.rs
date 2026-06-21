use crate::domain::{CodeConflictEvidence, IngestItem, ProviderMetadata};
use crate::domain::{IngestDecision, ReviewReason};
use crate::identifier::extract_code_from_text;
use anyhow::Result;
use crate::nfo::{parse_nfo_document, ParsedNfoDocument};
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

pub struct Scanner;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaInfo {
    pub duration_seconds: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codec: Option<String>,
}

impl Scanner {
    pub fn scan_sources(roots: &[PathBuf]) -> Result<Vec<IngestItem>> {
        let mut items = Vec::new();
        for root in roots {
            if !root.exists() {
                continue;
            }

            for entry in WalkDir::new(root).follow_links(false) {
                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                if !entry.file_type().is_file() || !is_video_file(path) {
                    continue;
                }

                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_string();
                let path_code = extract_code_from_path(path);
                let nfo_code = extract_code_from_local_nfo(path);
                let has_code_conflict = path_code.is_some() && nfo_code.is_some() && path_code != nfo_code;
                let code_conflict = match (&path_code, &nfo_code) {
                    (Some(path_code), Some(nfo_code)) if path_code != nfo_code => find_local_nfo(path).map(|nfo_path| {
                        CodeConflictEvidence {
                            path_code: path_code.clone(),
                            nfo_code: nfo_code.clone(),
                            nfo_path,
                        }
                    }),
                    _ => None,
                };
                let normalized_code = path_code.or(nfo_code);
                let media_info = if should_probe_media_file(metadata.len()) {
                    probe_media_info(path).unwrap_or_default()
                } else {
                    MediaInfo::default()
                };
                let local_metadata = local_metadata_for(path, normalized_code.as_deref());
                let mut review_reasons = Vec::new();
                if normalized_code.is_none() {
                    review_reasons.push(ReviewReason::MissingCode);
                }
                if has_code_conflict {
                    review_reasons.push(ReviewReason::CodeConflict);
                }
                items.push(IngestItem {
                    id: None,
                    job_id: None,
                    source_root: root.clone(),
                    path: path.to_path_buf(),
                    file_name,
                    size_bytes: metadata.len(),
                    duration_seconds: media_info.duration_seconds,
                    width: media_info.width,
                    height: media_info.height,
                    codec: media_info.codec,
                    normalized_code,
                    confidence: 0.82,
                    decision: IngestDecision::NeedsReview,
                    review_reasons,
                    code_conflict,
                    metadata: local_metadata,
                    candidate_work_id: None,
                    file_hash: sample_file_fingerprint(path).ok(),
                });
            }
        }

        mark_duplicate_file_candidates(&mut items);
        Ok(items)
    }
}

fn probe_media_info(path: &Path) -> Result<MediaInfo> {
    let Some(ffprobe) = ffprobe_executable() else {
        return Ok(MediaInfo::default());
    };
    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,codec_name,width,height,duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()?;

    if !output.status.success() {
        return Ok(MediaInfo::default());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_ffprobe_media_info(&text)
}

fn ffprobe_executable() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("FFPROBE_PATH") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }

    let bundled = PathBuf::from(r"D:\Program Files\ffmpeg-20191206-b66a800-win64-static\bin\ffprobe.exe");
    if bundled.exists() {
        return Some(bundled);
    }

    Some(PathBuf::from("ffprobe"))
}

pub fn parse_ffprobe_media_info(text: &str) -> Result<MediaInfo> {
    let value: Value = serde_json::from_str(text)?;
    let video_stream = value
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| {
            streams.iter().find(|stream| {
                stream
                    .get("codec_type")
                    .and_then(Value::as_str)
                    .map(|kind| kind == "video")
                    .unwrap_or(false)
            })
        });

    let duration_seconds = value
        .get("format")
        .and_then(|format| format.get("duration"))
        .and_then(parse_duration_value)
        .or_else(|| video_stream.and_then(|stream| stream.get("duration")).and_then(parse_duration_value));

    Ok(MediaInfo {
        duration_seconds,
        width: video_stream
            .and_then(|stream| stream.get("width"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        height: video_stream
            .and_then(|stream| stream.get("height"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        codec: video_stream
            .and_then(|stream| stream.get("codec_name"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToString::to_string),
    })
}

fn parse_duration_value(value: &Value) -> Option<u64> {
    value
        .as_str()
        .and_then(|text| text.parse::<f64>().ok())
        .or_else(|| value.as_f64())
        .filter(|duration| duration.is_finite() && *duration >= 0.0)
        .map(|duration| duration.round() as u64)
}

fn mark_duplicate_file_candidates(items: &mut [IngestItem]) {
    let mut hash_counts = HashMap::<String, usize>::new();
    for item in items.iter() {
        if let Some(hash) = item.file_hash.as_ref() {
            *hash_counts.entry(hash.clone()).or_default() += 1;
        }
    }

    for item in items.iter_mut() {
        let Some(hash) = item.file_hash.as_ref() else {
            continue;
        };
        if hash_counts.get(hash).copied().unwrap_or_default() > 1 {
            item.decision = IngestDecision::DuplicateCandidate;
            push_review_reason(item, ReviewReason::DuplicateFile);
        }
    }
}

/// Bounded-memory file fingerprint: hashes file size plus head/mid/tail 1MB
/// regions so multi-GB videos never load into RAM. For files up to 3MB the
/// entire content is hashed, so tiny identical files still match exactly.
/// Results are prefixed `fp:` to distinguish from any future full-hash path.
pub fn sample_file_fingerprint(path: &Path) -> Result<String> {
    const CHUNK_SIZE: usize = 1024 * 1024; // 1 MB per sampled region
    let mut file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    let mut hasher = Sha256::new();
    // Include size first so different-sized files never collide.
    hasher.update(size.to_le_bytes());

    use std::io::{Read, Seek, SeekFrom};
    if size <= (CHUNK_SIZE * 3) as u64 {
        // Small file: hash entire content (degrades to a full hash).
        let mut reader = std::io::BufReader::new(&mut file);
        std::io::copy(&mut reader, &mut hasher)?;
    } else {
        let mut buf = vec![0u8; CHUNK_SIZE];
        file.read_exact(&mut buf)?;
        hasher.update(&buf);
        file.seek(SeekFrom::Start(size / 2))?;
        file.read_exact(&mut buf)?;
        hasher.update(&buf);
        file.seek(SeekFrom::Start(size - CHUNK_SIZE as u64))?;
        file.read_exact(&mut buf)?;
        hasher.update(&buf);
    }

    let digest = hasher.finalize();
    Ok(format!("fp:{digest:x}"))
}

fn push_review_reason(item: &mut IngestItem, reason: ReviewReason) {
    if !item.review_reasons.contains(&reason) {
        item.review_reasons.push(reason);
    }
}

pub fn is_video_file(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "m4v" | "ts" | "flv" | "webm"
    )
}

pub fn should_probe_media_file(size_bytes: u64) -> bool {
    size_bytes >= 1024
}

fn extract_code_from_path(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|name| name.to_str())
        .and_then(extract_code_from_text)
        .or_else(|| {
            path.parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                .and_then(extract_code_from_text)
        })
}

fn local_metadata_for(video_path: &Path, normalized_code: Option<&str>) -> Option<ProviderMetadata> {
    let nfo = find_local_nfo(video_path);
    let cover = find_local_cover(video_path).or_else(|| {
        // Real scraper output often names the cover after the code
        // (e.g. IDBD-815.jpg) rather than a multi-CD video stem.
        normalized_code.and_then(|code| find_image_by_stem(video_path.parent()?, code))
    });
    if nfo.is_none() && cover.is_none() {
        return None;
    }

    let nfo_text = nfo
        .as_ref()
        .and_then(|path| fs::read_to_string(path).ok())
        .unwrap_or_default();
    Some(ProviderMetadata {
        provider: "local".to_string(),
        title_zh: extract_xml_tag(&nfo_text, "title"),
        original_title: extract_xml_tag(&nfo_text, "originaltitle")
            .or_else(|| extract_xml_tag(&nfo_text, "original_title")),
        aliases: vec![],
        summary: extract_xml_tag(&nfo_text, "plot").or_else(|| extract_xml_tag(&nfo_text, "summary")),
        cover_url: cover.map(|path| path.to_string_lossy().to_string()),
        release_date: extract_xml_tag(&nfo_text, "premiered")
            .or_else(|| extract_xml_tag(&nfo_text, "releasedate"))
            .or_else(|| extract_xml_tag(&nfo_text, "release_date")),
        confidence: 0.95,
        actors: extract_actor_names(&nfo_text),
        genres: clean_genres(&extract_all_xml_tags(&nfo_text, "genre")),
        studio: extract_xml_tag(&nfo_text, "studio")
            .or_else(|| extract_xml_tag(&nfo_text, "maker")),
        director: extract_xml_tag(&nfo_text, "director"),
    })
}

fn extract_code_from_local_nfo(video_path: &Path) -> Option<String> {
    let nfo_text = find_local_nfo(video_path).and_then(|path| fs::read_to_string(path).ok())?;
    extract_xml_tag(&nfo_text, "uniqueid")
        .or_else(|| extract_xml_tag(&nfo_text, "num"))
        .and_then(|value| extract_code_from_text(&value))
}

fn find_local_nfo(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem().and_then(|value| value.to_str())?;
    for candidate in [
        parent.join(format!("{stem}.nfo")),
        parent.join("movie.nfo"),
        parent.join("info.nfo"),
    ] {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    fs::read_dir(parent)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.eq_ignore_ascii_case("nfo"))
                .unwrap_or(false)
        })
}

fn find_local_cover(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem().and_then(|value| value.to_str())?;
    for suffix in ["ps", "poster"] {
        if let Some(path) = find_image_by_stem(parent, &format!("{stem}-{suffix}")) {
            return Some(path);
        }
    }

    for name in [stem, "cover", "poster", "fanart", "folder"] {
        if let Some(path) = find_image_by_stem(parent, name) {
            return Some(path);
        }
    }

    find_keyword_image(parent, &["poster", "ps"])
        .or_else(|| find_keyword_image(parent, &["fanart", "pl"]))
}

fn find_image_by_stem(parent: &Path, stem: &str) -> Option<PathBuf> {
    for extension in ["jpg", "jpeg", "png", "webp"] {
        let candidate = parent.join(format!("{stem}.{extension}"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn find_keyword_image(parent: &Path, keywords: &[&str]) -> Option<PathBuf> {
    fs::read_dir(parent)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            let is_image = path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp"))
                .unwrap_or(false);
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                return false;
            };
            let normalized_stem = stem.to_ascii_lowercase();
            is_image && keywords.iter().any(|keyword| normalized_stem.contains(keyword))
        })
}

fn extract_xml_tag(text: &str, tag: &str) -> Option<String> {
    let pattern = format!(r"(?is)<{tag}[^>]*>(.*?)</{tag}>");
    let captures = Regex::new(&pattern).ok()?.captures(text)?;
    let value = captures.get(1)?.as_str().trim();
    if value.is_empty() {
        None
    } else {
        Some(decode_xml_entities(value))
    }
}

fn decode_xml_entities(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Extracts <actor><name>...</name> values from a Kodi-style NFO, dropping
/// scraper placeholders such as 未知演员 / 未知男優 / empty names.
fn extract_actor_names(nfo_text: &str) -> Vec<String> {
    let block = Regex::new(r"(?is)<actor>(.*?)</actor>").unwrap();
    let name = Regex::new(r"(?is)<name>(.*?)</name>").unwrap();
    let mut out: Vec<String> = Vec::new();
    for caps in block.captures_iter(nfo_text) {
        if let Some(nm) = name.captures(caps.get(1).unwrap().as_str()) {
            let value = decode_xml_entities(nm.get(1).unwrap().as_str().trim());
            if value.is_empty() {
                continue;
            }
            // drop scraper placeholders (unknown actress / actor)
            let lower = value.to_lowercase();
            if lower.contains("未知") || lower == "unknown" || lower == "unknown actor" {
                continue;
            }
            if !out.contains(&value) {
                out.push(value);
            }
        }
    }
    out
}

// --- Library rebuild scan: NFO-first pairing of NFO + video files ---

/// Result of an NFO-first library scan: every parseable `.nfo` under the roots
/// with its paired video, plus (for Task 4+) the raw ingest items. The rebuild
/// pipeline drives off `nfo_documents`; `items` stays empty for now.
#[derive(Debug, Clone, Default)]
pub struct ScannedLibrary {
    pub items: Vec<IngestItem>,
    pub nfo_documents: Vec<ScannedNfoDocument>,
}

/// One scanned NFO and the video paired to it. `source_root` records which
/// root the NFO was found under so a rebuilt file_version can point back to
/// its origin.
#[derive(Debug, Clone)]
pub struct ScannedNfoDocument {
    pub nfo_path: PathBuf,
    pub source_root: PathBuf,
    pub document: ParsedNfoDocument,
    pub paired_video: Option<PathBuf>,
    pub paired_video_size: u64,
}

/// Walk every root, parse each `.nfo`, and pair it with a video in the same
/// directory. A video is paired when its stem matches the NFO stem exactly,
/// or matches after stripping a trailing `-cdN` / `_discN` segment so a
/// multi-CD NFO still finds its base video. Unparseable NFOs are skipped; the
/// rebuild report accounts for them as errors via grouping.
pub fn scan_library_roots(roots: &[PathBuf]) -> Result<ScannedLibrary> {
    let mut nfo_documents = Vec::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root).follow_links(false) {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            let is_nfo = path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("nfo"))
                .unwrap_or(false);
            if !is_nfo {
                continue;
            }
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            let Ok(document) = parse_nfo_document(&text) else {
                continue;
            };
            let (paired_video, paired_video_size) = pair_video_for_nfo(path);
            nfo_documents.push(ScannedNfoDocument {
                nfo_path: path.to_path_buf(),
                source_root: root.clone(),
                document,
                paired_video,
                paired_video_size,
            });
        }
    }
    Ok(ScannedLibrary {
        items: Vec::new(),
        nfo_documents,
    })
}

/// Find the video paired to an NFO in its directory and return its byte size.
/// Exact stem match wins; otherwise a trailing CD/disc/part segment is stripped
/// so `IPX-607-cd2.nfo` can still resolve `IPX-607.mp4`.
fn pair_video_for_nfo(nfo_path: &Path) -> (Option<PathBuf>, u64) {
    let Some(parent) = nfo_path.parent() else {
        return (None, 0);
    };
    let Some(stem) = nfo_path.file_stem().and_then(|s| s.to_str()) else {
        return (None, 0);
    };

    if let Some(video) = find_video_by_stem(parent, stem) {
        return video_with_size(video);
    }
    if let Some(base) = strip_cd_segment(stem) {
        if let Some(video) = find_video_by_stem(parent, &base) {
            return video_with_size(video);
        }
    }
    (None, 0)
}

fn video_with_size(video: PathBuf) -> (Option<PathBuf>, u64) {
    let size = fs::metadata(&video).map(|metadata| metadata.len()).unwrap_or(0);
    (Some(video), size)
}

fn find_video_by_stem(parent: &Path, stem: &str) -> Option<PathBuf> {
    for extension in ["mp4", "mkv", "avi", "mov", "wmv", "m4v", "ts", "flv", "webm"] {
        let candidate = parent.join(format!("{stem}.{extension}"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Strip a trailing `-cdN` / `_cdN` / `discN` / `partN` segment (case
/// insensitive) from a stem, returning the owned base stem when something was
/// actually removed.
fn strip_cd_segment(stem: &str) -> Option<String> {
    let re = Regex::new(r"(?i)[-_]?(cd|disc|part)[-_]?\d+$").ok()?;
    let stripped = re.replace(stem, "");
    (stripped.len() < stem.len() && !stripped.is_empty()).then(|| stripped.into_owned())
}

/// Returns every text value of <tag>...</tag> in document order.
fn extract_all_xml_tags(text: &str, tag: &str) -> Vec<String> {
    let pattern = format!(r"(?is)<{tag}[^>]*>(.*?)</{tag}>");
    let re = match Regex::new(&pattern) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    re.captures_iter(text)
        .filter_map(|c| {
            let v = c.get(1)?.as_str().trim();
            if v.is_empty() {
                None
            } else {
                Some(decode_xml_entities(v))
            }
        })
        .collect()
}

/// Filters codec / resolution / code-prefix noise out of scraped genres so
/// only real categories survive. Real genres are CJK-heavy; pure ASCII tokens
/// like H264 / 1080P / 4K / SSNI are scraper technical noise.
fn clean_genres(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for g in raw {
        let value = g.trim();
        if value.is_empty() {
            continue;
        }
        // drop pure-ASCII tokens (codecs, resolutions, label prefixes)
        if value.is_ascii() {
            continue;
        }
        // drop code-prefix patterns like "ABP", "SSNI-452"
        if Regex::new(r"^[A-Z]{2,6}[-_]?d*$").unwrap().is_match(value) {
            continue;
        }
        if !out.iter().any(|g| g == value) {
            out.push(value.to_string());
        }
    }
    out
}
