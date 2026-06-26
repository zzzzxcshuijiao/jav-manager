use media_manager::remote_scraper::parse_json_ld_metadata;

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
