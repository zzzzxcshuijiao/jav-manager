# 阶段 6A 远程 Scraper Adapter 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增一个不访问真实站点的远程 scraper adapter 骨架，用 fake HTTP client 和 HTML/JSON fixture 验证真实 scraper 未来需要的边界。

**Architecture:** 新建 `remote_scraper` Rust 模块，先解析 HTML 中 JSON-LD，再通过 `RemoteScraperSource<C>` 同时实现旧 ingest `MetadataProvider` 和自动管线 `ScraperSource`。HTTP 获取通过 trait 注入，阶段 6A 不引入真实网络 client。

**Tech Stack:** Rust 2021, serde_json, regex, anyhow, existing `ProviderMetadata` / `ScrapedWorkMetadata` / `MetadataProvider` / `ScraperSource`, `cargo test --manifest-path src-tauri/Cargo.toml -j 1`.

---

## 文件结构

- Create `src-tauri/src/remote_scraper.rs`
  - 远程 scraper 中间模型、HTTP client trait、URL 模板、JSON-LD parser、adapter trait 实现。
- Modify `src-tauri/src/lib.rs`
  - 导出 `pub mod remote_scraper;`。
- Create `src-tauri/tests/remote_scraper.rs`
  - 覆盖 parser、URL 构建、fake client、`MetadataProvider` / `ScraperSource` 双接口。
- Modify `.ai_state/tasks.md`, `.ai_state/progress.md`, `.ai_state/reviews/sprint-9.md`, `.ai_state/lessons.md`, `HANDOFF.md`
  - 记录 Sprint 9 状态、验证和后续项。

---

### Task 1: JSON-LD Parser

**Files:**
- Create: `src-tauri/tests/remote_scraper.rs`
- Create: `src-tauri/src/remote_scraper.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 写 parser 红灯测试**

创建 `src-tauri/tests/remote_scraper.rs`：

```rust
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
    assert_eq!(metadata.original_title, Some("Original ABP-700".to_string()));
    assert_eq!(metadata.summary, Some("剧情简介".to_string()));
    assert_eq!(metadata.actors, vec!["Actor A", "Actor B"]);
    assert_eq!(metadata.genres, vec!["Drama", "Sample"]);
    assert_eq!(metadata.studio, Some("Studio A".to_string()));
    assert_eq!(metadata.director, Some("Director A".to_string()));
    assert_eq!(metadata.release_date, Some("2026-06-26".to_string()));
    assert_eq!(metadata.cover_url, Some("https://example.test/cover.jpg".to_string()));
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
```

- [ ] **Step 2: 跑红灯测试**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper -j 1
```

Expected: FAIL，`media_manager::remote_scraper` 不存在。

- [ ] **Step 3: 实现最小 parser**

实现 `src-tauri/src/remote_scraper.rs`：

```rust
use crate::domain::{ProviderMetadata, ScrapedWorkMetadata};
use anyhow::{anyhow, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
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
```

同文件补齐 private helpers：

```rust
fn extract_json_ld_blocks(html: &str) -> Vec<String> {
    let re = Regex::new(
        r#"(?is)<script[^>]+type\s*=\s*["']application/ld\+json["'][^>]*>(?P<body>.*?)</script>"#,
    )
    .expect("valid JSON-LD script regex");
    re.captures_iter(html)
        .filter_map(|capture| capture.name("body").map(|body| body.as_str().trim().to_string()))
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
```

在 `src-tauri/src/lib.rs` 添加：

```rust
pub mod remote_scraper;
```

- [ ] **Step 4: 跑绿灯测试**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper -j 1
```

Expected: PASS。

- [ ] **Step 5: Commit Task 1**

```powershell
git add src-tauri/src/lib.rs src-tauri/src/remote_scraper.rs src-tauri/tests/remote_scraper.rs
git commit -m "新增阶段6A远程刮削解析器"
```

---

### Task 2: RemoteScraperSource 和 Fake HTTP Client

**Files:**
- Modify: `src-tauri/src/remote_scraper.rs`
- Modify: `src-tauri/tests/remote_scraper.rs`

- [ ] **Step 1: 写 source adapter 红灯测试**

追加测试：

```rust
use media_manager::remote_scraper::{
    RemoteMetadataHttpClient, RemoteScraperConfig, RemoteScraperSource,
};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct FakeHttpClient {
    response: anyhow::Result<String>,
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
            response: Err(anyhow::anyhow!("network down")),
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
            Err(error) => Err(anyhow::anyhow!(error.to_string())),
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
fn remote_scraper_source_builds_encoded_url_and_uses_client() {
    let client = FakeHttpClient::ok(&movie_html("Remote Title"));
    let config = RemoteScraperConfig::new("fixture", "https://example.test/search?q={code}", 0.81)
        .unwrap();
    let source = RemoteScraperSource::new(config, client.clone());

    let metadata = source.lookup_remote("ABP 704").unwrap().unwrap();

    assert_eq!(client.last_url(), "https://example.test/search?q=ABP%20704");
    assert_eq!(metadata.title, "Remote Title");
    assert_eq!(metadata.actors, vec!["Actor A"]);
}

#[test]
fn remote_scraper_config_requires_code_placeholder() {
    let error = RemoteScraperConfig::new("fixture", "https://example.test/search", 0.8)
        .unwrap_err();

    assert!(error.to_string().contains("{code}"));
}

#[test]
fn remote_scraper_source_propagates_http_errors() {
    let config = RemoteScraperConfig::new("fixture", "https://example.test/{code}", 0.8)
        .unwrap();
    let source = RemoteScraperSource::new(config, FakeHttpClient::failing());

    let error = source.lookup_remote("ABP-705").unwrap_err();

    assert!(error.to_string().contains("network down"));
}
```

- [ ] **Step 2: 跑红灯测试**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper -j 1
```

Expected: FAIL，source/config/client 类型不存在。

- [ ] **Step 3: 实现 config、client trait、source 和 URL encoding**

在 `remote_scraper.rs` 添加：

```rust
/// HTTP boundary used by remote scraper adapters.
pub trait RemoteMetadataHttpClient: Send + Sync {
    /// Fetch a remote HTML document as UTF-8 text.
    fn get_text(&self, url: &str) -> Result<String>;
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
            return Err(anyhow!("remote scraper URL template must contain {code}"));
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
```

- [ ] **Step 4: 跑绿灯测试**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper -j 1
```

Expected: PASS。

- [ ] **Step 5: Commit Task 2**

```powershell
git add src-tauri/src/remote_scraper.rs src-tauri/tests/remote_scraper.rs
git commit -m "实现阶段6A远程刮削客户端边界"
```

---

### Task 3: MetadataProvider 和 ScraperSource 转换

**Files:**
- Modify: `src-tauri/src/remote_scraper.rs`
- Modify: `src-tauri/tests/remote_scraper.rs`

- [ ] **Step 1: 写 trait 转换红灯测试**

追加测试：

```rust
use media_manager::pipeline::ScraperSource;
use media_manager::provider::MetadataProvider;

#[test]
fn remote_scraper_source_implements_scraper_source() {
    let client = FakeHttpClient::ok(&movie_html("Pipeline Title"));
    let config = RemoteScraperConfig::new("fixture", "https://example.test/{code}", 0.9)
        .unwrap();
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
    let config = RemoteScraperConfig::new("fixture", "https://example.test/{code}", 0.83)
        .unwrap();
    let source = RemoteScraperSource::new(config, client);

    let metadata = MetadataProvider::lookup(&source, "ABP-707", "ABP-707.mp4")
        .unwrap()
        .unwrap();

    assert_eq!(metadata.provider, "fixture");
    assert_eq!(metadata.title_zh, Some("Provider Title".to_string()));
    assert_eq!(metadata.confidence, 0.83);
    assert_eq!(metadata.actors, vec!["Actor A"]);
}
```

- [ ] **Step 2: 跑红灯测试**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper -j 1
```

Expected: FAIL，trait impl 不存在。

- [ ] **Step 3: 实现转换和 trait impl**

在 `remote_scraper.rs` 添加：

```rust
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
```

- [ ] **Step 4: 跑绿灯测试**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper -j 1
```

Expected: PASS。

- [ ] **Step 5: Commit Task 3**

```powershell
git add src-tauri/src/remote_scraper.rs src-tauri/tests/remote_scraper.rs
git commit -m "接入阶段6A远程刮削元数据接口"
```

---

### Task 4: 验证、Review 和交接

**Files:**
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/progress.md`
- Create: `.ai_state/reviews/sprint-9.md`
- Modify: `.ai_state/lessons.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: 更新 Sprint 9 tasks**

```markdown
# Sprint 9 Tasks - 阶段 6A 远程 Scraper Adapter 骨架

- [x] 固化阶段 6A 中文设计
- [x] 编写阶段 6A implementation plan
- [x] Task 1: JSON-LD parser
- [x] Task 2: RemoteScraperSource 与 fake HTTP client
- [x] Task 3: MetadataProvider / ScraperSource 转换
- [x] Task 4: 全量验证、评审和交接
```

- [ ] **Step 2: focused verification**

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper -j 1
```

Expected: PASS。

- [ ] **Step 3: full gate**

```powershell
$ErrorActionPreference='Stop'
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
npm test
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
npx tsc --noEmit
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
npm run build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

Expected: all PASS。

- [ ] **Step 4: review / lessons / handoff**

记录：

- 6A 不访问真实站点。
- `RemoteMetadataHttpClient` 是注入边界。
- `RemoteScraperSource` 同时服务旧 ingest 和自动管线。
- 后续真实站点 adapter、真实 HTTP client、封面下载和 UI 设置仍 DEFER。

- [ ] **Step 5: Commit handoff**

```powershell
git add HANDOFF.md
git commit -m "更新阶段6A交接说明"
```

---

## 自审清单

- Spec coverage: 覆盖 HTTP 注入、URL 模板、JSON-LD parser、中间模型、`MetadataProvider`、`ScraperSource`、失败语义和验证。
- Scope guard: 不访问真实 scraper 站点，不新增真实 HTTP client，不改默认 example scraper，不做 UI 设置。
- Type consistency: `RemoteMetadataHttpClient`、`RemoteScraperConfig`、`RemoteScraperSource`、`RemoteMetadata`、`parse_json_ld_metadata` 命名一致。
- Testability: 所有新行为都能通过 fake client 和 HTML fixture 测试。
