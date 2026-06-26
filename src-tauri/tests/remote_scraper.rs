use media_manager::remote_scraper::{
    parse_json_ld_metadata, RemoteMetadataHttpClient, RemoteScraperConfig, RemoteScraperSettings,
    RemoteScraperSource, RemoteScraperSourceSettings,
};
use media_manager::{pipeline::ScraperSource, provider::MetadataProvider};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct FakeHttpClient {
    response: Result<String, String>,
    urls: Arc<Mutex<Vec<String>>>,
}

impl FakeHttpClient {
    fn ok(response: &str) -> Self {
        Self {
            response: Ok(response.to_string()),
            urls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn failing() -> Self {
        Self {
            response: Err("network down".to_string()),
            urls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn last_url(&self) -> String {
        self.urls.lock().unwrap().last().unwrap().clone()
    }
}

impl RemoteMetadataHttpClient for FakeHttpClient {
    fn get_text(&self, url: &str) -> anyhow::Result<String> {
        self.urls.lock().unwrap().push(url.to_string());
        match &self.response {
            Ok(response) => Ok(response.clone()),
            Err(error) => Err(anyhow::anyhow!(error.clone())),
        }
    }
}

fn movie_html(title: &str) -> String {
    format!(
        r#"<script type="application/ld+json">{{"@type":"Movie","name":"{}","actor":"Actor A"}}</script>"#,
        title
    )
}

#[test]
fn remote_scraper_settings_defaults_are_safe_and_disabled() {
    let settings = RemoteScraperSettings::default();

    assert!(!settings.enabled);
    assert_eq!(settings.timeout_ms, 8000);
    assert!(settings.user_agent.contains("media-manager"));
    assert!(settings.proxy_url.is_none());
    assert!(settings.include_example_fallback);
    assert_eq!(settings.sources.len(), 3);
    assert!(settings.sources.iter().all(|source| !source.enabled));
}

#[test]
fn remote_scraper_settings_normalize_sources_and_reject_invalid_enabled_source() {
    let settings = RemoteScraperSettings {
        enabled: true,
        timeout_ms: 5000,
        user_agent: " media-manager-test ".to_string(),
        proxy_url: Some("   ".to_string()),
        include_example_fallback: false,
        sources: vec![
            RemoteScraperSourceSettings {
                id: " javdb ".to_string(),
                enabled: true,
                search_url_template: "https://example.test/search?q={code}".to_string(),
                min_confidence: 0.91,
            },
            RemoteScraperSourceSettings {
                id: "javdb".to_string(),
                enabled: true,
                search_url_template: "https://duplicate.test/{code}".to_string(),
                min_confidence: 0.5,
            },
        ],
    };

    let normalized = settings.normalized().unwrap();

    assert_eq!(normalized.user_agent, "media-manager-test");
    assert_eq!(normalized.proxy_url, None);
    assert_eq!(normalized.sources.len(), 1);
    assert_eq!(normalized.sources[0].id, "javdb");
    assert_eq!(normalized.sources[0].min_confidence, 0.91);

    let invalid = RemoteScraperSettings {
        enabled: true,
        sources: vec![RemoteScraperSourceSettings {
            id: "bad".to_string(),
            enabled: true,
            search_url_template: "https://example.test/search".to_string(),
            min_confidence: 0.8,
        }],
        ..RemoteScraperSettings::default()
    };

    let error = invalid.normalized().unwrap_err();
    assert!(error.to_string().contains("{code}"));
}

#[test]
fn json_ld_movie_fixture_parses_remote_metadata() {
    let html = r#"
        <html><head>
          <script type="application/ld+json">
          {
            "@context": "https://schema.org",
            "@type": "Movie",
            "name": "中文标题 ABP-700",
            "alternateName": "Original ABP-700",
            "description": "剧情简介",
            "actor": [{"name":"Actor A"}, {"name":"Actor B"}],
            "genre": ["Drama", "Sample"],
            "productionCompany": {"name":"Studio A"},
            "director": {"name":"Director A"},
            "datePublished": "2026-06-26",
            "image": "https://example.test/cover.jpg"
          }
          </script>
        </head></html>
    "#;

    let metadata = parse_json_ld_metadata("fixture", "ABP-700", html, 0.82)
        .unwrap()
        .unwrap();

    assert_eq!(metadata.provider, "fixture");
    assert_eq!(metadata.normalized_code, "ABP-700");
    assert_eq!(metadata.title, "中文标题 ABP-700");
    assert_eq!(
        metadata.original_title,
        Some("Original ABP-700".to_string())
    );
    assert_eq!(metadata.summary, Some("剧情简介".to_string()));
    assert_eq!(metadata.actors, vec!["Actor A", "Actor B"]);
    assert_eq!(metadata.genres, vec!["Drama", "Sample"]);
    assert_eq!(metadata.studio, Some("Studio A".to_string()));
    assert_eq!(metadata.director, Some("Director A".to_string()));
    assert_eq!(metadata.release_date, Some("2026-06-26".to_string()));
    assert_eq!(
        metadata.cover_url,
        Some("https://example.test/cover.jpg".to_string())
    );
    assert_eq!(metadata.confidence, 0.82);
}

#[test]
fn json_ld_graph_finds_first_video_object() {
    let html = r#"
        <script type="application/ld+json">
        {
          "@graph": [
            {"@type":"BreadcrumbList","name":"ignore"},
            {"@type":"VideoObject","name":"Graph Title","actor":"Actor C","genre":"Genre C"}
          ]
        }
        </script>
    "#;

    let metadata = parse_json_ld_metadata("graph", "ABP-701", html, 0.8)
        .unwrap()
        .unwrap();

    assert_eq!(metadata.title, "Graph Title");
    assert_eq!(metadata.actors, vec!["Actor C"]);
    assert_eq!(metadata.genres, vec!["Genre C"]);
}

#[test]
fn missing_json_ld_returns_none() {
    let metadata = parse_json_ld_metadata("fixture", "ABP-702", "<html></html>", 0.8).unwrap();

    assert!(metadata.is_none());
}

#[test]
fn malformed_json_ld_returns_error() {
    let html = r#"<script type="application/ld+json">{"@type":"Movie","name":</script>"#;

    let error = parse_json_ld_metadata("fixture", "ABP-703", html, 0.8).unwrap_err();

    assert!(error.to_string().contains("invalid JSON-LD"));
}

#[test]
fn remote_scraper_source_builds_encoded_url_and_uses_client() {
    let client = FakeHttpClient::ok(&movie_html("Remote Title"));
    let config =
        RemoteScraperConfig::new("fixture", "https://example.test/search?q={code}", 0.81).unwrap();
    let source = RemoteScraperSource::new(config, client.clone());

    let metadata = source.lookup_remote("ABP 704").unwrap().unwrap();

    assert_eq!(client.last_url(), "https://example.test/search?q=ABP%20704");
    assert_eq!(metadata.title, "Remote Title");
    assert_eq!(metadata.actors, vec!["Actor A"]);
}

#[test]
fn remote_scraper_config_requires_code_placeholder() {
    let error =
        RemoteScraperConfig::new("fixture", "https://example.test/search", 0.8).unwrap_err();

    assert!(error.to_string().contains("{code}"));
}

#[test]
fn remote_scraper_source_propagates_http_errors() {
    let config = RemoteScraperConfig::new("fixture", "https://example.test/{code}", 0.8).unwrap();
    let source = RemoteScraperSource::new(config, FakeHttpClient::failing());

    let error = source.lookup_remote("ABP-705").unwrap_err();

    assert!(error.to_string().contains("network down"));
}

#[test]
fn remote_scraper_source_implements_scraper_source() {
    let client = FakeHttpClient::ok(&movie_html("Pipeline Title"));
    let config = RemoteScraperConfig::new("fixture", "https://example.test/{code}", 0.9).unwrap();
    let source = RemoteScraperSource::new(config, client);

    let metadata = ScraperSource::lookup(&source, "ABP-706").unwrap().unwrap();

    assert_eq!(metadata.source, "fixture");
    assert_eq!(metadata.normalized_code, "ABP-706");
    assert_eq!(metadata.title, "Pipeline Title");
    assert_eq!(metadata.actors, vec!["Actor A"]);
    assert_eq!(metadata.cover_path, None);
}

#[test]
fn remote_scraper_source_implements_metadata_provider() {
    let client = FakeHttpClient::ok(&movie_html("Provider Title"));
    let config = RemoteScraperConfig::new("fixture", "https://example.test/{code}", 0.83).unwrap();
    let source = RemoteScraperSource::new(config, client);

    let metadata = MetadataProvider::lookup(&source, "ABP-707", "ABP-707.mp4")
        .unwrap()
        .unwrap();

    assert_eq!(metadata.provider, "fixture");
    assert_eq!(metadata.title_zh, Some("Provider Title".to_string()));
    assert_eq!(metadata.confidence, 0.83);
    assert_eq!(metadata.actors, vec!["Actor A"]);
}
