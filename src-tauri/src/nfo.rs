//! Structured parser for Kodi / Jav-style `.nfo` movie metadata.
//!
//! The crate's older scanner had ad-hoc NFO field extraction spread inline
//! across `local_metadata_for`. This module is the first-class replacement:
//! it turns an NFO document into a flat `ParsedNfoDocument` that downstream
//! tasks can map onto the domain model. Parsing is intentionally regex-based
//! (matching the rest of the codebase) rather than pulling in an XML crate.
//!
//! Two ordering rules hold throughout, and callers depend on them:
//!   1. Every extracted text value is CDATA-stripped FIRST, then
//!      entity-decoded. Decoding before CDATA stripping would corrupt the
//!      `]]>` boundary if it were ever entity-escaped.
//!   2. `tags` and `genres` stay separate Vecs; merging is the caller's job.
//!
//! Public API is fixed: Tasks 2-5 build on these exact signatures.

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// One rating contribution from a single source, parsed out of the NFO's
/// nested `<ratings>` block or the legacy top-level `<rating>` element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedRatingSource {
    pub source: String,
    pub value: f32,
    pub max: i32,
    pub votes: Option<i64>,
    pub is_default: bool,
}

/// Flat, lossless view of every NFO field this app cares about. Option fields
/// are None only when the tag is absent or empty after stripping; Vec fields
/// are empty when no instances are present (never None).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParsedNfoDocument {
    pub source_code: Option<String>,
    pub title: Option<String>,
    pub original_title: Option<String>,
    pub outline: Option<String>,
    pub summary: Option<String>,
    pub runtime_minutes: Option<i64>,
    pub year: Option<i32>,
    pub sets: Vec<String>,
    pub studio: Option<String>,
    pub label: Option<String>,
    pub tags: Vec<String>,
    pub genres: Vec<String>,
    pub release_date: Option<String>,
    pub cover_url: Option<String>,
    pub poster_path: Option<String>,
    pub thumb_path: Option<String>,
    pub fanart_path: Option<String>,
    pub website: Option<String>,
    pub mpaa: Option<String>,
    pub rating_sources: Vec<ParsedRatingSource>,
}

/// Trims surrounding whitespace, then strips a matching `<![CDATA[` ... `]]>`
/// pair only when BOTH markers are present. Non-CDATA values pass through
/// unchanged (still trimmed). A stray leading marker without a trailing one
/// is left alone so we never silently drop content.
pub fn strip_cdata(value: &str) -> String {
    let trimmed = value.trim();
    const START: &str = "<![CDATA[";
    const END: &str = "]]>";
    if trimmed.starts_with(START) && trimmed.ends_with(END) && trimmed.len() >= START.len() + END.len() {
        trimmed[START.len()..trimmed.len() - END.len()].to_string()
    } else {
        trimmed.to_string()
    }
}

/// Decode the small set of XML entities NFO scrapers actually emit. `&amp;`
/// is decoded last so a literal `&amp;lt;` (meaning the text `&lt;`) is not
/// double-expanded into a bare `<`.
fn decode_xml_entities(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Normalize an extracted tag value: CDATA-strip first, then entity-decode.
/// Applied uniformly to every text field so CDATA / entity handling cannot
/// drift between call sites.
fn normalize_text(value: &str) -> String {
    decode_xml_entities(&strip_cdata(value))
}

/// Parse a `<runtime>` value into whole minutes.
///
/// First match wins:
///   1. `HH:MM:SS` (three colon-separated parts) -> `h*60 + m`, rounding
///      seconds up at 30s so `02:15:00` -> 135.
///   2. Pure integer -> itself (minutes).
///   3. `<n>分` / `<n> min` / `<n> minutes` after normalizing the
///      traditional `分鐘` / `分鍾` glyphs down to `分`.
///   4. Anything else -> None.
pub fn parse_runtime_minutes(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    // (a) HH:MM:SS — exactly three numeric colon-separated parts.
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() == 3 {
        let h: i64 = parts[0].trim().parse().ok()?;
        let m: i64 = parts[1].trim().parse().ok()?;
        let s: i64 = parts[2].trim().parse().ok()?;
        return Some(h * 60 + m + if s >= 30 { 1 } else { 0 });
    }

    // (b) Pure integer minutes.
    if let Ok(minutes) = trimmed.parse::<i64>() {
        return Some(minutes);
    }

    // (c) Suffixed number: normalize traditional minute glyphs, then peel a
    // trailing 分 / min / minutes unit before parsing the digits.
    let normalized = trimmed.replace("分鐘", "分").replace("分鍾", "分");
    // Also fold the simplified-Chinese 分钟 (U+949F) — the dominant modern
    // scraper form — down to the bare 分 unit handled below.
    let normalized = normalized.replace("分钟", "分");
    let lower = normalized.to_ascii_lowercase();
    let stripped = lower
        .trim_end_matches("分")
        .trim_end_matches("min")
        .trim_end_matches("minutes")
        .trim();
    if let Ok(minutes) = stripped.parse::<i64>() {
        return Some(minutes);
    }

    None
}

/// Extract the inner text of the first `<tag ...>...</tag>` occurrence (the
/// opening tag may carry attributes). Returns the normalized value, or None
/// when the tag is absent or empty after normalization.
fn extract_tag(text: &str, tag: &str) -> Option<String> {
    let pattern = format!(r"(?is)<{tag}[^>]*>(.*?)</{tag}>");
    let caps = Regex::new(&pattern).ok()?.captures(text)?;
    let value = normalize_text(caps.get(1)?.as_str());
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Every non-empty text value of `<tag ...>...</tag>` in document order.
fn extract_all_tags(text: &str, tag: &str) -> Vec<String> {
    let pattern = format!(r"(?is)<{tag}[^>]*>(.*?)</{tag}>");
    let Ok(re) = Regex::new(&pattern) else {
        return Vec::new();
    };
    re.captures_iter(text)
        .filter_map(|c| {
            let value = normalize_text(c.get(1)?.as_str());
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        })
        .collect()
}

/// Pull a `name="value"` attribute out of an opening tag's source slice.
/// Returns the attribute value with surrounding double quotes stripped.
fn extract_attribute(opening_tag: &str, name: &str) -> Option<String> {
    // Leading \b so `name=` can't match inside `username=` / `maxlength=`.
    let pattern = format!(r#"(?i)\b{name}\s*=\s*"([^"]*)""#);
    let caps = Regex::new(&pattern).ok()?.captures(opening_tag)?;
    Some(caps.get(1)?.as_str().to_string())
}

/// Collect all `<set>` values. A set may be bare text (`<set>Name</set>`) or
/// contain a `<name>` child (`<set><name>Name</name></set>`); prefer the
/// `<name>` child when present. Order-preserving dedup.
fn extract_sets(text: &str) -> Vec<String> {
    let set_re = Regex::new(r"(?is)<set\b[^>]*>(.*?)</set>").unwrap();
    let name_re = Regex::new(r"(?is)<name>(.*?)</name>").unwrap();
    let mut out = Vec::new();
    for caps in set_re.captures_iter(text) {
        let inner = caps.get(1).unwrap().as_str();
        let value = match name_re.captures(inner) {
            Some(c) => normalize_text(c.get(1).unwrap().as_str()),
            None => normalize_text(inner),
        };
        if value.is_empty() || out.contains(&value) {
            continue;
        }
        out.push(value);
    }
    out
}

/// Parse the nested `<ratings><rating ...>...</rating></ratings>` block, plus
/// the legacy top-level `<rating>` fallback, into `ParsedRatingSource`s.
///
/// When a nested `<ratings>` block exists, only its children are emitted
/// (the legacy top-level element is ignored in that case, matching Kodi's
/// precedence). Without a nested block, a single top-level `<rating>V</rating>`
/// (or `<rating><value>V</value>...</rating>`) yields one default source.
fn parse_rating_sources(nfo_text: &str) -> Vec<ParsedRatingSource> {
    let ratings_block_re = Regex::new(r"(?is)<ratings>(.*?)</ratings>").unwrap();
    let rating_re = Regex::new(r"(?is)<rating\b([^>]*)>(.*?)</rating>").unwrap();
    let value_re = Regex::new(r"(?is)<value>(.*?)</value>").unwrap();
    let votes_re = Regex::new(r"(?is)<votes>(.*?)</votes>").unwrap();

    let mut out = Vec::new();

    if let Some(block_caps) = ratings_block_re.captures(nfo_text) {
        let block = block_caps.get(1).unwrap().as_str();
        for rating_caps in rating_re.captures_iter(block) {
            let attrs = rating_caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let inner = rating_caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let source = extract_attribute(attrs, "name").unwrap_or_else(|| "nfo".to_string());
            let max = extract_attribute(attrs, "max")
                .and_then(|v| v.trim().parse::<i32>().ok())
                .unwrap_or(10);
            let is_default = extract_attribute(attrs, "default")
                .map(|v| v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            let value = value_re
                .captures(inner)
                .and_then(|c| normalize_text(c.get(1).unwrap().as_str()).parse::<f32>().ok())
                .unwrap_or(0.0);
            let votes = votes_re
                .captures(inner)
                .and_then(|c| normalize_text(c.get(1).unwrap().as_str()).parse::<i64>().ok());
            out.push(ParsedRatingSource {
                source,
                value,
                max,
                votes,
                is_default,
            });
        }
        return out;
    }

    // Legacy top-level <rating ...>V | <value>V</value></rating> with an
    // optional sibling top-level <votes>.
    if let Some(rating_caps) = rating_re.captures(nfo_text) {
        let attrs = rating_caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let inner = rating_caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let value = value_re
            .captures(inner)
            .or_else(|| {
                Regex::new(r"(?s)^\s*([0-9.]+)\s*$")
                    .ok()?
                    .captures(inner.trim())
            })
            .and_then(|c| normalize_text(c.get(1).unwrap().as_str()).parse::<f32>().ok())
            .unwrap_or(0.0);
        let max = extract_attribute(attrs, "max")
            .and_then(|v| v.trim().parse::<i32>().ok())
            .unwrap_or(10);
        let votes = votes_re
            .captures(nfo_text)
            .and_then(|c| normalize_text(c.get(1).unwrap().as_str()).parse::<i64>().ok());
        out.push(ParsedRatingSource {
            source: "nfo".to_string(),
            value,
            max,
            votes,
            is_default: true,
        });
    }

    out
}

/// Parse a Kodi/Jav-style `.nfo` document into a flat `ParsedNfoDocument`.
///
/// Missing tags leave their fields None / empty. Regex extraction never
/// throws, so this returns Ok for any input; Err is reserved for genuinely
/// unreachable parser state so callers can `?` it uniformly.
pub fn parse_nfo_document(xml: &str) -> Result<ParsedNfoDocument> {
    let mut doc = ParsedNfoDocument::default();

    doc.source_code = extract_tag(xml, "num");
    doc.title = extract_tag(xml, "title");
    doc.original_title = extract_tag(xml, "originaltitle")
        .or_else(|| extract_tag(xml, "original_title"));
    doc.outline = extract_tag(xml, "outline");
    doc.summary = extract_tag(xml, "plot").or_else(|| extract_tag(xml, "summary"));
    doc.runtime_minutes = extract_tag(xml, "runtime")
        .as_deref()
        .and_then(parse_runtime_minutes);
    doc.year = extract_tag(xml, "year").and_then(|y| y.trim().parse::<i32>().ok());
    doc.sets = extract_sets(xml);
    doc.studio = extract_tag(xml, "studio").or_else(|| extract_tag(xml, "maker"));
    doc.label = extract_tag(xml, "label").or_else(|| extract_tag(xml, "publisher"));
    doc.tags = extract_all_tags(xml, "tag");
    doc.genres = extract_all_tags(xml, "genre");
    doc.release_date = extract_tag(xml, "premiered")
        .or_else(|| extract_tag(xml, "releasedate"))
        .or_else(|| extract_tag(xml, "release"));
    doc.cover_url = extract_tag(xml, "cover");
    doc.poster_path = extract_tag(xml, "poster");
    doc.thumb_path = extract_tag(xml, "thumb");
    doc.fanart_path = extract_tag(xml, "fanart");
    doc.website = extract_tag(xml, "website");
    doc.mpaa = extract_tag(xml, "mpaa");
    doc.rating_sources = parse_rating_sources(xml);

    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_cdata_returns_inner_text_without_markers() {
        assert_eq!(strip_cdata("<![CDATA[TheLifeErotic.19.06.20-Rope 2]]>"), "TheLifeErotic.19.06.20-Rope 2");
        assert_eq!(strip_cdata("plain text"), "plain text");
    }

    #[test]
    fn parse_runtime_minutes_handles_multiple_formats() {
        assert_eq!(parse_runtime_minutes("134"), Some(134));
        assert_eq!(parse_runtime_minutes("9分鍾"), Some(9));
        assert_eq!(parse_runtime_minutes("120 min"), Some(120));
        assert_eq!(parse_runtime_minutes("9分钟"), Some(9));
        assert_eq!(parse_runtime_minutes("02:15:00"), Some(135));
        assert_eq!(parse_runtime_minutes("garbage"), None);
    }

    #[test]
    fn parse_nfo_document_extracts_rich_fields_and_nested_ratings() {
        let xml = r#"<movie>
  <title><![CDATA[TheLifeErotic.19.06.20-Rope 2]]></title>
  <originaltitle><![CDATA[TheLifeErotic.19.06.20-Rope 2]]></originaltitle>
  <outline><![CDATA[]]></outline>
  <plot><![CDATA[Plot text]]></plot>
  <num>TheLifeErotic.19.06.20</num>
  <runtime>9分鍾</runtime>
  <year>2019</year>
  <set>The Life Erotic</set>
  <studio>Metartnetwork</studio>
  <label>The Life Erotic</label>
  <premiered>2019-06-20</premiered>
  <poster>poster.jpg</poster>
  <thumb>thumb.jpg</thumb>
  <fanart>fanart.jpg</fanart>
  <cover>https://example.com/cover.jpg</cover>
  <website>https://javdb.com/v/0yqx7</website>
  <tag>中文字幕</tag>
  <genre>H264</genre>
  <ratings><rating name="javdb" max="5" default="true"><value>4.0</value><votes>2</votes></rating></ratings>
</movie>"#;
        let parsed = parse_nfo_document(xml).expect("parse should succeed");
        assert_eq!(parsed.title.as_deref(), Some("TheLifeErotic.19.06.20-Rope 2"));
        assert_eq!(parsed.summary.as_deref(), Some("Plot text"));
        assert_eq!(parsed.source_code.as_deref(), Some("TheLifeErotic.19.06.20"));
        assert_eq!(parsed.runtime_minutes, Some(9));
        assert_eq!(parsed.year, Some(2019));
        assert_eq!(parsed.sets, vec!["The Life Erotic"]);
        assert_eq!(parsed.studio.as_deref(), Some("Metartnetwork"));
        assert_eq!(parsed.label.as_deref(), Some("The Life Erotic"));
        assert_eq!(parsed.tags, vec!["中文字幕"]);
        assert_eq!(parsed.genres, vec!["H264"]);
        assert_eq!(parsed.rating_sources, vec![ParsedRatingSource {
            source: "javdb".to_string(),
            value: 4.0,
            max: 5,
            votes: Some(2),
            is_default: true,
        }]);
    }

    #[test]
    fn parse_nfo_document_falls_back_to_legacy_top_level_rating() {
        let xml = r#"<movie>
  <num>ABC-123</num>
  <rating max="10">8.2</rating>
  <votes>150</votes>
</movie>"#;
        let parsed = parse_nfo_document(xml).expect("parse should succeed");
        assert_eq!(parsed.rating_sources.len(), 1);
        let r = &parsed.rating_sources[0];
        assert_eq!(r.source, "nfo");
        assert!((r.value - 8.2).abs() < f32::EPSILON);
        assert_eq!(r.max, 10);
        assert_eq!(r.votes, Some(150));
        assert!(r.is_default);
    }
}
