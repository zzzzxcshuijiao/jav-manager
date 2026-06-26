use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::Value;

/// Normalized metadata returned by a remote scraper before conversion to app DTOs.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteMetadata {
    pub provider: String,
    pub normalized_code: String,
    pub title: String,
    pub original_title: Option<String>,
    pub summary: Option<String>,
    pub actors: Vec<String>,
    pub genres: Vec<String>,
    pub studio: Option<String>,
    pub director: Option<String>,
    pub release_date: Option<String>,
    pub cover_url: Option<String>,
    pub confidence: f32,
}

/// Parse the first Movie or VideoObject JSON-LD block from a remote HTML page.
pub fn parse_json_ld_metadata(
    source_name: &str,
    normalized_code: &str,
    html: &str,
    min_confidence: f32,
) -> Result<Option<RemoteMetadata>> {
    for raw_json in extract_json_ld_blocks(html) {
        let value: Value = serde_json::from_str(&raw_json)
            .map_err(|error| anyhow!("invalid JSON-LD: {error}"))?;
        if let Some(object) = find_media_object(&value) {
            let Some(title) = string_field(object, "name") else {
                return Ok(None);
            };
            return Ok(Some(RemoteMetadata {
                provider: source_name.to_string(),
                normalized_code: normalized_code.to_string(),
                title,
                original_title: string_field(object, "alternateName"),
                summary: string_field(object, "description"),
                actors: names_field(object.get("actor")),
                genres: strings_field(object.get("genre")),
                studio: first_name(object.get("productionCompany")),
                director: first_name(object.get("director")),
                release_date: string_field(object, "datePublished"),
                cover_url: image_url(object.get("image")),
                confidence: min_confidence,
            }));
        }
    }
    Ok(None)
}

fn extract_json_ld_blocks(html: &str) -> Vec<String> {
    let re = Regex::new(
        r#"(?is)<script[^>]+type\s*=\s*["']application/ld\+json["'][^>]*>(?P<body>.*?)</script>"#,
    )
    .expect("valid JSON-LD script regex");
    re.captures_iter(html)
        .filter_map(|capture| {
            capture
                .name("body")
                .map(|body| body.as_str().trim().to_string())
        })
        .collect()
}

fn find_media_object(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    match value {
        Value::Object(object) if is_media_object(object) => Some(object),
        Value::Object(object) => object.get("@graph").and_then(find_media_object),
        Value::Array(items) => items.iter().find_map(find_media_object),
        _ => None,
    }
}

fn is_media_object(object: &serde_json::Map<String, Value>) -> bool {
    match object.get("@type") {
        Some(Value::String(kind)) => kind == "Movie" || kind == "VideoObject",
        Some(Value::Array(kinds)) => kinds.iter().any(|kind| {
            matches!(kind, Value::String(text) if text == "Movie" || text == "VideoObject")
        }),
        _ => false,
    }
}

fn string_field(object: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(value_to_string)
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        _ => None,
    }
}

fn names_field(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(name_from_value).collect(),
        Some(single) => name_from_value(single).into_iter().collect(),
        None => Vec::new(),
    }
}

fn strings_field(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(value_to_string).collect(),
        Some(single) => value_to_string(single).into_iter().collect(),
        None => Vec::new(),
    }
}

fn first_name(value: Option<&Value>) -> Option<String> {
    names_field(value).into_iter().next()
}

fn name_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Value::Object(object) => string_field(object, "name"),
        _ => None,
    }
}

fn image_url(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Some(Value::Array(items)) => items.iter().find_map(|item| image_url(Some(item))),
        Some(Value::Object(object)) => string_field(object, "url"),
        _ => None,
    }
}
