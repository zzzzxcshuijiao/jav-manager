use crate::domain::{ProviderMetadata, ScrapedWorkMetadata};
use anyhow::{anyhow, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

const DEFAULT_REMOTE_SCRAPER_TIMEOUT_MS: u64 = 8000;
const DEFAULT_REMOTE_SCRAPER_USER_AGENT: &str = "media-manager/0.1 local scraper";

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

impl RemoteMetadata {
    /// Convert remote metadata to the ingest provider DTO.
    pub fn to_provider_metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            provider: self.provider.clone(),
            title_zh: Some(self.title.clone()),
            original_title: self.original_title.clone(),
            aliases: vec![self.normalized_code.clone()],
            summary: self.summary.clone(),
            cover_url: self.cover_url.clone(),
            release_date: self.release_date.clone(),
            confidence: self.confidence,
            actors: self.actors.clone(),
            genres: self.genres.clone(),
            studio: self.studio.clone(),
            director: self.director.clone(),
        }
    }

    /// Convert remote metadata to the automatic pipeline DTO.
    pub fn to_scraped_work_metadata(&self) -> ScrapedWorkMetadata {
        ScrapedWorkMetadata {
            source: self.provider.clone(),
            normalized_code: self.normalized_code.clone(),
            title: self.title.clone(),
            original_title: self.original_title.clone(),
            summary: self.summary.clone(),
            actors: self.actors.clone(),
            genres: self.genres.clone(),
            studio: self.studio.clone(),
            director: self.director.clone(),
            release_date: self.release_date.clone(),
            cover_path: None,
        }
    }
}

/// HTTP boundary used by remote scraper adapters.
pub trait RemoteMetadataHttpClient: Send + Sync {
    /// Fetch a remote HTML document as UTF-8 text.
    fn get_text(&self, url: &str) -> Result<String>;
}

/// Persisted remote scraper settings used by the automatic pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteScraperSettings {
    pub enabled: bool,
    pub timeout_ms: u64,
    pub user_agent: String,
    pub proxy_url: Option<String>,
    pub include_example_fallback: bool,
    pub sources: Vec<RemoteScraperSourceSettings>,
}

impl Default for RemoteScraperSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_ms: DEFAULT_REMOTE_SCRAPER_TIMEOUT_MS,
            user_agent: DEFAULT_REMOTE_SCRAPER_USER_AGENT.to_string(),
            proxy_url: None,
            include_example_fallback: true,
            sources: default_remote_scraper_sources(),
        }
    }
}

impl RemoteScraperSettings {
    /// Return a validated copy safe to persist and use for scraper construction.
    pub fn normalized(&self) -> Result<Self> {
        if self.timeout_ms == 0 {
            return Err(anyhow!(
                "remote scraper timeout_ms must be greater than zero"
            ));
        }
        let user_agent = self.user_agent.trim().to_string();
        if user_agent.is_empty() {
            return Err(anyhow!("remote scraper user_agent is required"));
        }
        let proxy_url = self.proxy_url.as_ref().and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
        let mut seen = HashSet::new();
        let mut sources = Vec::new();
        for source in &self.sources {
            let normalized = source.normalized()?;
            if seen.insert(normalized.id.clone()) {
                sources.push(normalized);
            }
        }
        Ok(Self {
            enabled: self.enabled,
            timeout_ms: self.timeout_ms,
            user_agent,
            proxy_url,
            include_example_fallback: self.include_example_fallback,
            sources,
        })
    }

    /// Return enabled sources only when the global remote scraper switch is on.
    pub fn enabled_sources(&self) -> Result<Vec<RemoteScraperSourceSettings>> {
        let normalized = self.normalized()?;
        if !normalized.enabled {
            return Ok(Vec::new());
        }
        Ok(normalized
            .sources
            .into_iter()
            .filter(|source| source.enabled)
            .collect())
    }
}

/// Persisted configuration for one remote scraper source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteScraperSourceSettings {
    pub id: String,
    pub enabled: bool,
    pub search_url_template: String,
    pub min_confidence: f32,
}

impl RemoteScraperSourceSettings {
    /// Return a trimmed source config and validate the URL template contract.
    pub fn normalized(&self) -> Result<Self> {
        let id = self.id.trim().to_ascii_lowercase();
        if id.is_empty() {
            return Err(anyhow!("remote scraper source id is required"));
        }
        if !(0.0..=1.0).contains(&self.min_confidence) {
            return Err(anyhow!(
                "remote scraper min_confidence must be between 0 and 1"
            ));
        }
        let search_url_template = self.search_url_template.trim().to_string();
        if self.enabled {
            RemoteScraperConfig::new(&id, &search_url_template, self.min_confidence)?;
        }
        Ok(Self {
            id,
            enabled: self.enabled,
            search_url_template,
            min_confidence: self.min_confidence,
        })
    }
}

/// Return the built-in remote scraper presets, disabled by default.
pub fn default_remote_scraper_sources() -> Vec<RemoteScraperSourceSettings> {
    vec![
        RemoteScraperSourceSettings {
            id: "javdb".to_string(),
            enabled: false,
            search_url_template: "https://javdb.com/search?q={code}&f=all".to_string(),
            min_confidence: 0.82,
        },
        RemoteScraperSourceSettings {
            id: "javbus".to_string(),
            enabled: false,
            search_url_template: "https://www.javbus.com/search/{code}".to_string(),
            min_confidence: 0.82,
        },
        RemoteScraperSourceSettings {
            id: "fanza".to_string(),
            enabled: false,
            search_url_template:
                "https://www.dmm.co.jp/digital/videoa/-/list/search/=/?searchstr={code}"
                    .to_string(),
            min_confidence: 0.82,
        },
    ]
}

/// Configuration for one remote metadata source.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteScraperConfig {
    pub source_name: String,
    pub search_url_template: String,
    pub min_confidence: f32,
}

impl RemoteScraperConfig {
    /// Create a config and require a `{code}` placeholder so lookups stay explicit.
    pub fn new(source_name: &str, search_url_template: &str, min_confidence: f32) -> Result<Self> {
        if !search_url_template.contains("{code}") {
            return Err(anyhow!("remote scraper URL template must contain {{code}}"));
        }
        Ok(Self {
            source_name: source_name.to_string(),
            search_url_template: search_url_template.to_string(),
            min_confidence,
        })
    }

    /// Build a lookup URL for a normalized code.
    pub fn build_url(&self, normalized_code: &str) -> Result<String> {
        if normalized_code.trim().is_empty() {
            return Err(anyhow!("normalized_code is required"));
        }
        Ok(self
            .search_url_template
            .replace("{code}", &percent_encode(normalized_code)))
    }
}

/// Remote scraper implementation shared by ingest and auto-pipeline paths.
#[derive(Debug, Clone)]
pub struct RemoteScraperSource<C> {
    config: RemoteScraperConfig,
    client: C,
}

impl<C: RemoteMetadataHttpClient> RemoteScraperSource<C> {
    /// Create a scraper source from config and an injected HTTP client.
    pub fn new(config: RemoteScraperConfig, client: C) -> Self {
        Self { config, client }
    }

    /// Fetch and parse remote metadata for one normalized code.
    pub fn lookup_remote(&self, normalized_code: &str) -> Result<Option<RemoteMetadata>> {
        let url = self.config.build_url(normalized_code)?;
        let html = self.client.get_text(&url)?;
        parse_json_ld_metadata(
            &self.config.source_name,
            normalized_code,
            &html,
            self.config.min_confidence,
        )
    }
}

impl<C: RemoteMetadataHttpClient> crate::provider::MetadataProvider for RemoteScraperSource<C> {
    fn name(&self) -> &str {
        &self.config.source_name
    }

    fn lookup(
        &self,
        normalized_code: &str,
        _original_file_name: &str,
    ) -> Result<Option<ProviderMetadata>> {
        Ok(self
            .lookup_remote(normalized_code)?
            .map(|metadata| metadata.to_provider_metadata()))
    }
}

impl<C: RemoteMetadataHttpClient> crate::pipeline::ScraperSource for RemoteScraperSource<C> {
    fn name(&self) -> &str {
        &self.config.source_name
    }

    fn lookup(&self, normalized_code: &str) -> Result<Option<ScrapedWorkMetadata>> {
        Ok(self
            .lookup_remote(normalized_code)?
            .map(|metadata| metadata.to_scraped_work_metadata()))
    }
}

/// Parse the first Movie or VideoObject JSON-LD block from a remote HTML page.
pub fn parse_json_ld_metadata(
    source_name: &str,
    normalized_code: &str,
    html: &str,
    min_confidence: f32,
) -> Result<Option<RemoteMetadata>> {
    for raw_json in extract_json_ld_blocks(html) {
        let value: Value =
            serde_json::from_str(&raw_json).map_err(|error| anyhow!("invalid JSON-LD: {error}"))?;
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
        Some(Value::Array(kinds)) => kinds.iter().any(
            |kind| matches!(kind, Value::String(text) if text == "Movie" || text == "VideoObject"),
        ),
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

fn percent_encode(input: &str) -> String {
    input
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}
