# Stage 6D Real Scraper Adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect a configurable real remote scraper adapter into the existing run-once automatic pipeline while keeping all Codex verification fixture-based and self-contained.

**Architecture:** Add scraper settings and source presets in `remote_scraper.rs`, persist them as one `app_settings` JSON value, and build a command-layer owned scraper list for `ScrapeCoordinator`. The real HTTP client sits behind the existing `RemoteMetadataHttpClient` trait, so production can fetch HTTPS while tests use fake clients or a local TCP server.

**Tech Stack:** Rust, SQLite/rusqlite, serde JSON settings, ureq synchronous HTTP client, Tauri commands, React/TypeScript, Vitest.

---

## File Structure

- Modify `src-tauri/Cargo.toml`: add the synchronous HTTPS-capable HTTP client dependency.
- Modify `src-tauri/src/remote_scraper.rs`: add settings DTOs, normalization, error taxonomy, HTTP client, source presets, parser dispatch, and helper builders.
- Modify `src-tauri/src/storage.rs`: persist `RemoteScraperSettings` under `app_settings.remote_scraper_settings`.
- Modify `src-tauri/src/daemon_control.rs`: add `ConfiguredPipelineScrapers`, keep example default, and inject remote sources when settings are enabled.
- Modify `src-tauri/src/commands.rs`: preload remote settings into `AppState`, add get/configure commands, and register them.
- Modify `src-tauri/tests/remote_scraper.rs`: cover settings normalization, HTTP client behavior, and preset parser fixtures.
- Modify `src-tauri/tests/core_behaviour.rs` or `src-tauri/tests/daemon_control.rs`: cover repository persistence and run-once remote priority.
- Modify `src/api.ts`: add remote scraper settings DTOs and command wrappers.
- Modify `src/viewModel.ts` and `src/viewModel.test.ts`: add a compact summary formatter for remote scraper settings.
- Modify `src/App.tsx`: add state, load/save handlers, and the remote scraper settings panel.
- Modify `src/styles.css`: share the existing compact settings panel layout with the remote scraper block.
- Modify `.ai_state/tasks.md`, `.ai_state/progress.md`, `.ai_state/design.md`, `.ai_state/reviews/sprint-11.md`, `.ai_state/lessons.md`, and `HANDOFF.md` as tasks complete.

## Task 1: Remote Scraper Settings Persistence

**Files:**
- Modify: `src-tauri/src/remote_scraper.rs`
- Modify: `src-tauri/src/storage.rs`
- Modify: `src-tauri/tests/remote_scraper.rs`
- Modify: `src-tauri/tests/core_behaviour.rs`

- [ ] **Step 1: Write failing settings tests**

Add tests to `src-tauri/tests/remote_scraper.rs`:

```rust
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
```

Add a persistence test to `src-tauri/tests/core_behaviour.rs`:

```rust
#[test]
fn repository_persists_remote_scraper_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("library.sqlite")).unwrap();
    repo.migrate().unwrap();
    let settings = RemoteScraperSettings {
        enabled: true,
        timeout_ms: 9000,
        user_agent: "media-manager-test".to_string(),
        proxy_url: Some("http://127.0.0.1:8888".to_string()),
        include_example_fallback: false,
        sources: vec![RemoteScraperSourceSettings {
            id: "javdb".to_string(),
            enabled: true,
            search_url_template: "https://example.test/search?q={code}".to_string(),
            min_confidence: 0.88,
        }],
    };

    let saved = repo.set_remote_scraper_settings(&settings).unwrap();
    let loaded = repo.get_remote_scraper_settings().unwrap();

    assert_eq!(saved, loaded);
    assert_eq!(loaded.sources[0].id, "javdb");
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6c'
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper remote_scraper_settings -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test core_behaviour repository_persists_remote_scraper_settings -j 1
```

Expected: compile fails because `RemoteScraperSettings`, `RemoteScraperSourceSettings`, and repository methods do not exist.

- [ ] **Step 3: Implement settings DTOs**

In `src-tauri/src/remote_scraper.rs`, add serde imports and DTOs:

```rust
use serde::{Deserialize, Serialize};

const DEFAULT_REMOTE_SCRAPER_TIMEOUT_MS: u64 = 8000;
const DEFAULT_REMOTE_SCRAPER_USER_AGENT: &str = "media-manager/0.1 local scraper";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteScraperSettings {
    pub enabled: bool,
    pub timeout_ms: u64,
    pub user_agent: String,
    pub proxy_url: Option<String>,
    pub include_example_fallback: bool,
    pub sources: Vec<RemoteScraperSourceSettings>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteScraperSourceSettings {
    pub id: String,
    pub enabled: bool,
    pub search_url_template: String,
    pub min_confidence: f32,
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
            search_url_template: "https://www.dmm.co.jp/digital/videoa/-/list/search/=/?searchstr={code}".to_string(),
            min_confidence: 0.82,
        },
    ]
}
```

Add normalization methods:

```rust
impl RemoteScraperSettings {
    /// Return a validated copy safe to persist and use for scraper construction.
    pub fn normalized(&self) -> Result<Self> {
        if self.timeout_ms == 0 {
            return Err(anyhow!("remote scraper timeout_ms must be greater than zero"));
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
        let mut seen = std::collections::HashSet::new();
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
        Ok(normalized.sources.into_iter().filter(|source| source.enabled).collect())
    }
}

impl RemoteScraperSourceSettings {
    /// Return a trimmed source config and validate the URL template contract.
    pub fn normalized(&self) -> Result<Self> {
        let id = self.id.trim().to_ascii_lowercase();
        if id.is_empty() {
            return Err(anyhow!("remote scraper source id is required"));
        }
        let search_url_template = self.search_url_template.trim().to_string();
        if self.enabled {
            RemoteScraperConfig::new(&id, &search_url_template, self.min_confidence)?;
        }
        if !(0.0..=1.0).contains(&self.min_confidence) {
            return Err(anyhow!("remote scraper min_confidence must be between 0 and 1"));
        }
        Ok(Self {
            id,
            enabled: self.enabled,
            search_url_template,
            min_confidence: self.min_confidence,
        })
    }
}
```

- [ ] **Step 4: Implement repository methods**

In `src-tauri/src/storage.rs`, import the settings type:

```rust
use crate::remote_scraper::RemoteScraperSettings;
```

Add methods beside `get_aria2_settings`:

```rust
    /// Persist normalized remote scraper settings as one JSON app_settings value.
    pub fn set_remote_scraper_settings(
        &self,
        settings: &RemoteScraperSettings,
    ) -> Result<RemoteScraperSettings> {
        let normalized = settings.normalized()?;
        self.set_setting(
            "remote_scraper_settings",
            &serde_json::to_string(&normalized)?,
        )?;
        Ok(normalized)
    }

    /// Read remote scraper settings, returning safe disabled defaults when absent.
    pub fn get_remote_scraper_settings(&self) -> Result<RemoteScraperSettings> {
        let Some(value) = self.get_setting("remote_scraper_settings")? else {
            return Ok(RemoteScraperSettings::default());
        };
        let settings: RemoteScraperSettings = serde_json::from_str(&value)?;
        settings.normalized()
    }
```

- [ ] **Step 5: Run focused tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6c'
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper remote_scraper_settings -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test core_behaviour repository_persists_remote_scraper_settings -j 1
```

Expected: both focused tests pass.

- [ ] **Step 6: Update state and commit**

Update `.ai_state/tasks.md` Task 1 checkbox and append:

```markdown
- 2026-06-26: 阶段 6D Task 1 完成；新增远程 scraper settings 默认值/归一化/SQLite JSON 持久化，focused tests 通过。
```

Commit:

```powershell
git add src-tauri/src/remote_scraper.rs src-tauri/src/storage.rs src-tauri/tests/remote_scraper.rs src-tauri/tests/core_behaviour.rs
git commit -m "持久化阶段6D远程刮削器配置"
```

## Task 2: Real HTTP Client and Error Taxonomy

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/remote_scraper.rs`
- Modify: `src-tauri/tests/remote_scraper.rs`

- [ ] **Step 1: Write failing HTTP client tests**

Add local TCP helper tests to `src-tauri/tests/remote_scraper.rs`:

```rust
#[test]
fn http_remote_metadata_client_sends_user_agent_and_reads_body() {
    let server = LocalHttpServer::new(
        "HTTP/1.1 200 OK\r\nContent-Length: 49\r\nContent-Type: text/html\r\n\r\n<html><body>fixture response from local server</body></html>",
    );
    let client = HttpRemoteMetadataClient::new(
        std::time::Duration::from_millis(1000),
        "media-manager-test".to_string(),
        None,
    )
    .unwrap();

    let body = client.get_text(&server.url("/sample?q=ABP-001")).unwrap();

    assert!(body.contains("fixture response"));
    let request = server.request();
    assert!(request.starts_with("GET /sample?q=ABP-001 HTTP/1.1"));
    assert!(request.contains("User-Agent: media-manager-test"));
}

#[test]
fn http_remote_metadata_client_classifies_non_success_status() {
    let server = LocalHttpServer::new("HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n");
    let client = HttpRemoteMetadataClient::new(
        std::time::Duration::from_millis(1000),
        "media-manager-test".to_string(),
        None,
    )
    .unwrap();

    let error = client.get_text(&server.url("/limited")).unwrap_err();

    assert!(error.to_string().contains("HTTP status 429"));
}
```

Add the helper in the same test file:

```rust
struct LocalHttpServer {
    address: std::net::SocketAddr,
    request: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl LocalHttpServer {
    fn new(response: &'static str) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let request = std::sync::Arc::new(std::sync::Mutex::new(None));
        let request_clone = std::sync::Arc::clone(&request);
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0_u8; 4096];
            let size = std::io::Read::read(&mut stream, &mut buffer).unwrap();
            *request_clone.lock().unwrap() = Some(String::from_utf8_lossy(&buffer[..size]).to_string());
            std::io::Write::write_all(&mut stream, response.as_bytes()).unwrap();
        });
        Self { address, request, handle: Some(handle) }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.address, path)
    }

    fn request(&self) -> String {
        self.handle.as_ref();
        self.request.lock().unwrap().clone().unwrap_or_default()
    }
}

impl Drop for LocalHttpServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6c'
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper http_remote_metadata_client -j 1
```

Expected: compile fails because `HttpRemoteMetadataClient` does not exist.

- [ ] **Step 3: Add dependency**

In `src-tauri/Cargo.toml`, add:

```toml
ureq = "2"
```

- [ ] **Step 4: Implement HTTP client and errors**

In `src-tauri/src/remote_scraper.rs`, add:

```rust
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RemoteScraperError {
    #[error("remote scraper not found: HTTP status {status}")]
    NotFound { status: u16 },
    #[error("remote scraper HTTP status {status}")]
    HttpStatus { status: u16 },
    #[error("remote scraper network error: {message}")]
    Network { message: String },
    #[error("remote scraper transport error: {message}")]
    Transport { message: String },
    #[error("remote scraper parser error: {message}")]
    Parser { message: String },
}

#[derive(Debug, Clone)]
pub struct HttpRemoteMetadataClient {
    timeout: Duration,
    user_agent: String,
    proxy_url: Option<String>,
}

impl HttpRemoteMetadataClient {
    /// Create a real HTTP client for remote metadata pages.
    pub fn new(timeout: Duration, user_agent: String, proxy_url: Option<String>) -> Result<Self> {
        if timeout.is_zero() {
            return Err(anyhow!("remote scraper timeout must be greater than zero"));
        }
        if user_agent.trim().is_empty() {
            return Err(anyhow!("remote scraper user_agent is required"));
        }
        Ok(Self {
            timeout,
            user_agent: user_agent.trim().to_string(),
            proxy_url: proxy_url.and_then(|value| {
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() { None } else { Some(trimmed) }
            }),
        })
    }

    /// Build the production client from persisted remote scraper settings.
    pub fn from_settings(settings: &RemoteScraperSettings) -> Result<Self> {
        let normalized = settings.normalized()?;
        Self::new(
            Duration::from_millis(normalized.timeout_ms),
            normalized.user_agent,
            normalized.proxy_url,
        )
    }

    fn agent(&self) -> Result<ureq::Agent> {
        let mut builder = ureq::AgentBuilder::new().timeout(self.timeout);
        if let Some(proxy_url) = &self.proxy_url {
            builder = builder.proxy(ureq::Proxy::new(proxy_url)?);
        }
        Ok(builder.build())
    }
}

impl RemoteMetadataHttpClient for HttpRemoteMetadataClient {
    fn get_text(&self, url: &str) -> Result<String> {
        let response = self
            .agent()?
            .get(url)
            .set("User-Agent", &self.user_agent)
            .call()
            .map_err(classify_ureq_error)?;
        response
            .into_string()
            .map_err(|error| RemoteScraperError::Transport { message: error.to_string() }.into())
    }
}

fn classify_ureq_error(error: ureq::Error) -> anyhow::Error {
    match error {
        ureq::Error::Status(status, _) if status == 404 || status == 410 => {
            RemoteScraperError::NotFound { status }.into()
        }
        ureq::Error::Status(status, _) => RemoteScraperError::HttpStatus { status }.into(),
        ureq::Error::Transport(error) => RemoteScraperError::Network {
            message: error.to_string(),
        }
        .into(),
    }
}
```

- [ ] **Step 5: Make 404/410 return `Ok(None)` from scraper source**

In `RemoteScraperSource::lookup_remote`, wrap `get_text`:

```rust
        let html = match self.client.get_text(&url) {
            Ok(html) => html,
            Err(error) => {
                if matches!(
                    error.downcast_ref::<RemoteScraperError>(),
                    Some(RemoteScraperError::NotFound { .. })
                ) {
                    return Ok(None);
                }
                return Err(error);
            }
        };
```

- [ ] **Step 6: Run focused tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6c'
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper http_remote_metadata_client -j 1
```

Expected: HTTP client tests pass.

- [ ] **Step 7: Update state and commit**

Append progress:

```markdown
- 2026-06-26: 阶段 6D Task 2 完成；新增真实 HTTP client、User-Agent/timeout/proxy 配置面和错误分类，本地 TCP focused tests 通过。
```

Commit:

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/remote_scraper.rs src-tauri/tests/remote_scraper.rs
git commit -m "实现阶段6D真实HTTP刮削客户端"
```

## Task 3: Source Registry and Fixture Parsers

**Files:**
- Modify: `src-tauri/src/remote_scraper.rs`
- Modify: `src-tauri/tests/remote_scraper.rs`
- Create: `src-tauri/tests/fixtures/remote_scraper/javdb_movie.html`
- Create: `src-tauri/tests/fixtures/remote_scraper/javbus_movie.html`
- Create: `src-tauri/tests/fixtures/remote_scraper/fanza_movie.html`

- [ ] **Step 1: Add fixture files**

Create `src-tauri/tests/fixtures/remote_scraper/javdb_movie.html`:

```html
<html><head><script type="application/ld+json">
{"@type":"Movie","name":"JavDB Fixture ABP-600","alternateName":"ABP-600 Original","actor":[{"name":"Actor J"}],"genre":["Drama"],"productionCompany":{"name":"Studio J"},"datePublished":"2026-06-26","image":"https://example.test/javdb.jpg"}
</script></head></html>
```

Create `src-tauri/tests/fixtures/remote_scraper/javbus_movie.html`:

```html
<html><head><script type="application/ld+json">
{"@type":"VideoObject","name":"JavBus Fixture ABP-601","actor":"Actor B","genre":["Sample"],"director":{"name":"Director B"},"image":{"url":"https://example.test/javbus.jpg"}}
</script></head></html>
```

Create `src-tauri/tests/fixtures/remote_scraper/fanza_movie.html`:

```html
<html><head><script type="application/ld+json">
{"@graph":[{"@type":"BreadcrumbList","name":"ignore"},{"@type":"Movie","name":"FANZA Fixture ABP-602","actor":[{"name":"Actor F"}],"genre":"Genre F","productionCompany":{"name":"Studio F"}}]}
</script></head></html>
```

- [ ] **Step 2: Write failing preset tests**

Add tests:

```rust
#[test]
fn source_specific_parsers_read_checked_in_fixtures() {
    let javdb = include_str!("fixtures/remote_scraper/javdb_movie.html");
    let javbus = include_str!("fixtures/remote_scraper/javbus_movie.html");
    let fanza = include_str!("fixtures/remote_scraper/fanza_movie.html");

    let javdb_metadata = parse_javdb_metadata("ABP-600", javdb, 0.86).unwrap().unwrap();
    let javbus_metadata = parse_javbus_metadata("ABP-601", javbus, 0.87).unwrap().unwrap();
    let fanza_metadata = parse_fanza_metadata("ABP-602", fanza, 0.88).unwrap().unwrap();

    assert_eq!(javdb_metadata.provider, "javdb");
    assert_eq!(javdb_metadata.title, "JavDB Fixture ABP-600");
    assert_eq!(javbus_metadata.provider, "javbus");
    assert_eq!(javbus_metadata.director, Some("Director B".to_string()));
    assert_eq!(fanza_metadata.provider, "fanza");
    assert_eq!(fanza_metadata.studio, Some("Studio F".to_string()));
}

#[test]
fn remote_scraper_source_dispatches_parser_by_source_id() {
    let client = FakeHttpClient::ok(include_str!("fixtures/remote_scraper/javdb_movie.html"));
    let config = RemoteScraperConfig::new("javdb", "https://example.test/{code}", 0.86).unwrap();
    let source = RemoteScraperSource::new(config, client);

    let metadata = source.lookup_remote("ABP-600").unwrap().unwrap();

    assert_eq!(metadata.provider, "javdb");
    assert_eq!(metadata.title, "JavDB Fixture ABP-600");
}
```

- [ ] **Step 3: Run tests and verify they fail**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6c'
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper source_specific_parsers -j 1
```

Expected: compile fails because parser entry points do not exist.

- [ ] **Step 4: Implement parser dispatch**

In `src-tauri/src/remote_scraper.rs`, add:

```rust
/// Parse one remote page using the parser associated with the source id.
pub fn parse_remote_source_metadata(
    source_name: &str,
    normalized_code: &str,
    html: &str,
    min_confidence: f32,
) -> Result<Option<RemoteMetadata>> {
    match source_name.trim().to_ascii_lowercase().as_str() {
        "javdb" => parse_javdb_metadata(normalized_code, html, min_confidence),
        "javbus" => parse_javbus_metadata(normalized_code, html, min_confidence),
        "fanza" => parse_fanza_metadata(normalized_code, html, min_confidence),
        other => parse_json_ld_metadata(other, normalized_code, html, min_confidence),
    }
}

/// Parse a JavDB page fixture into normalized remote metadata.
pub fn parse_javdb_metadata(
    normalized_code: &str,
    html: &str,
    min_confidence: f32,
) -> Result<Option<RemoteMetadata>> {
    parse_json_ld_metadata("javdb", normalized_code, html, min_confidence)
}

/// Parse a JavBus page fixture into normalized remote metadata.
pub fn parse_javbus_metadata(
    normalized_code: &str,
    html: &str,
    min_confidence: f32,
) -> Result<Option<RemoteMetadata>> {
    parse_json_ld_metadata("javbus", normalized_code, html, min_confidence)
}

/// Parse a FANZA page fixture into normalized remote metadata.
pub fn parse_fanza_metadata(
    normalized_code: &str,
    html: &str,
    min_confidence: f32,
) -> Result<Option<RemoteMetadata>> {
    parse_json_ld_metadata("fanza", normalized_code, html, min_confidence)
}
```

Change `lookup_remote` to call dispatch:

```rust
        parse_remote_source_metadata(
            &self.config.source_name,
            normalized_code,
            &html,
            self.config.min_confidence,
        )
```

- [ ] **Step 5: Run focused tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6c'
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper source_specific_parsers -j 1
cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper remote_scraper_source_dispatches_parser_by_source_id -j 1
```

Expected: parser tests pass.

- [ ] **Step 6: Update state and commit**

Append progress:

```markdown
- 2026-06-26: 阶段 6D Task 3 完成；新增 javdb/javbus/fanza parser dispatch 和 checked-in HTML fixtures，focused tests 通过。
```

Commit:

```powershell
git add src-tauri/src/remote_scraper.rs src-tauri/tests/remote_scraper.rs src-tauri/tests/fixtures/remote_scraper
git commit -m "接入阶段6D站点刮削器适配层"
```

## Task 4: Run-Once Integration With Configured Remote Sources

**Files:**
- Modify: `src-tauri/src/remote_scraper.rs`
- Modify: `src-tauri/src/daemon_control.rs`
- Modify: `src-tauri/tests/daemon_control.rs`

- [ ] **Step 1: Write failing daemon_control test**

Add a fake remote client and test to `src-tauri/tests/daemon_control.rs`:

```rust
#[derive(Clone)]
struct CommandRemoteClient {
    html: String,
}

impl RemoteMetadataHttpClient for CommandRemoteClient {
    fn get_text(&self, _url: &str) -> anyhow::Result<String> {
        Ok(self.html.clone())
    }
}

#[test]
fn run_once_uses_configured_remote_scraper_before_example_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, archive, _assets) = configured_repo(&tmp);
    let video = inbox.join("ABP-600.mp4");
    std::fs::write(&video, b"stable video bytes").unwrap();
    repo.set_remote_scraper_settings(&RemoteScraperSettings {
        enabled: true,
        include_example_fallback: true,
        sources: vec![RemoteScraperSourceSettings {
            id: "javdb".to_string(),
            enabled: true,
            search_url_template: "https://example.test/search?q={code}".to_string(),
            min_confidence: 0.86,
        }],
        ..RemoteScraperSettings::default()
    })
    .unwrap();
    let mut runtime = DaemonControlRuntime::default();
    let html = r#"<script type="application/ld+json">{"@type":"Movie","name":"Remote Pipeline Title","actor":"Actor R"}</script>"#;

    let report = run_daemon_once_with_transports(
        &repo,
        &mut runtime,
        true,
        CommandAria2Transport { response: "{\"jsonrpc\":\"2.0\",\"id\":\"media-manager-tell-status\",\"result\":{\"gid\":\"none\",\"status\":\"active\",\"totalLength\":\"1\",\"completedLength\":\"0\",\"files\":[]}}".to_string() },
        CommandRemoteClient { html: html.to_string() },
    )
    .unwrap();

    assert_eq!(report.process.archived, 1);
    let works = repo.list_works().unwrap();
    assert_eq!(works[0].title, "Remote Pipeline Title");
    assert!(archive.join("ABP-600").join("ABP-600.mp4").exists());
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6c'
cargo test --manifest-path src-tauri/Cargo.toml --test daemon_control run_once_uses_configured_remote_scraper_before_example_fallback -j 1
```

Expected: compile fails because `run_daemon_once_with_transports` and imports do not exist.

- [ ] **Step 3: Add remote source builder**

In `src-tauri/src/remote_scraper.rs`, add:

```rust
/// Build enabled remote scraper sources from normalized settings and a reusable client.
pub fn build_remote_scraper_sources<C>(
    settings: &RemoteScraperSettings,
    client: C,
) -> Result<Vec<Box<dyn crate::pipeline::ScraperSource>>>
where
    C: RemoteMetadataHttpClient + Clone + 'static,
{
    let mut sources: Vec<Box<dyn crate::pipeline::ScraperSource>> = Vec::new();
    for source in settings.enabled_sources()? {
        let config = RemoteScraperConfig::new(
            &source.id,
            &source.search_url_template,
            source.min_confidence,
        )?;
        sources.push(Box::new(RemoteScraperSource::new(config, client.clone())));
    }
    Ok(sources)
}
```

- [ ] **Step 4: Add command-layer configured scraper owner**

In `src-tauri/src/daemon_control.rs`, import:

```rust
use crate::remote_scraper::{
    build_remote_scraper_sources, HttpRemoteMetadataClient, RemoteMetadataHttpClient,
};
```

Add:

```rust
/// Owns scraper instances long enough to lend them to `ScrapeCoordinator`.
pub struct ConfiguredPipelineScrapers {
    sources: Vec<Box<dyn ScraperSource>>,
}

impl ConfiguredPipelineScrapers {
    /// Build production scraper sources from repository settings.
    pub fn from_repository(repo: &Repository, metadata_enabled: bool) -> Result<Self> {
        let settings = repo.get_remote_scraper_settings()?;
        let client = HttpRemoteMetadataClient::from_settings(&settings)?;
        Self::with_remote_client(&settings, metadata_enabled, client)
    }

    /// Build scraper sources with an injected client for tests.
    pub fn with_remote_client<C>(
        settings: &crate::remote_scraper::RemoteScraperSettings,
        metadata_enabled: bool,
        remote_client: C,
    ) -> Result<Self>
    where
        C: RemoteMetadataHttpClient + Clone + 'static,
    {
        if !metadata_enabled {
            bail!("元数据源未开启，自动管线不会用空 scraper 处理真实文件");
        }
        let normalized = settings.normalized()?;
        let mut sources = build_remote_scraper_sources(&normalized, remote_client)?;
        if normalized.include_example_fallback {
            sources.push(Box::new(ExamplePipelineScraper));
        }
        if sources.is_empty() {
            bail!("没有可用元数据源，自动管线不会处理真实文件");
        }
        Ok(Self { sources })
    }

    /// Borrow scraper sources for the pipeline coordinator.
    pub fn coordinator(&self) -> ScrapeCoordinator<'_> {
        ScrapeCoordinator {
            sources: self.sources.iter().map(|source| source.as_ref()).collect(),
        }
    }
}
```

- [ ] **Step 5: Wire run-once through injectable transports**

Change `run_daemon_once_with_aria2_transport`:

```rust
pub fn run_daemon_once_with_aria2_transport<T: Aria2Transport>(
    repo: &Repository,
    runtime: &mut DaemonControlRuntime,
    metadata_enabled: bool,
    aria2_transport: T,
) -> Result<RunOnceReport> {
    let remote_settings = repo.get_remote_scraper_settings()?;
    let remote_client = HttpRemoteMetadataClient::from_settings(&remote_settings)?;
    run_daemon_once_with_transports(
        repo,
        runtime,
        metadata_enabled,
        aria2_transport,
        remote_client,
    )
}

pub fn run_daemon_once_with_transports<T, C>(
    repo: &Repository,
    runtime: &mut DaemonControlRuntime,
    metadata_enabled: bool,
    aria2_transport: T,
    remote_client: C,
) -> Result<RunOnceReport>
where
    T: Aria2Transport,
    C: RemoteMetadataHttpClient + Clone + 'static,
{
    if runtime.paused {
        return Ok(RunOnceReport::default());
    }
    let remote_settings = repo.get_remote_scraper_settings()?;
    let scrapers =
        ConfiguredPipelineScrapers::with_remote_client(&remote_settings, metadata_enabled, remote_client)?;
    let config = DaemonConfig::load(repo)?;
    let coordinator = scrapers.coordinator();
    let mut daemon = HeadlessDaemon::with_completion_policy(
        repo,
        config,
        coordinator,
        CompletionPolicy {
            sample_delay: Duration::ZERO,
        },
    );
    let aria2_settings = repo.get_aria2_settings()?;
    let aria2 = daemon.poll_aria2_once(&aria2_settings, aria2_transport)?;
    match daemon.run_once() {
        Ok(mut report) => {
            report.aria2 = aria2;
            runtime.processed += report.process.processed;
            runtime.last_error = daemon.status().last_error;
            Ok(report)
        }
        Err(error) => {
            runtime.last_error = Some(error.to_string());
            Err(error)
        }
    }
}
```

- [ ] **Step 6: Run focused tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6c'
cargo test --manifest-path src-tauri/Cargo.toml --test daemon_control -j 1
```

Expected: daemon_control suite passes, including old example behavior.

- [ ] **Step 7: Update state and commit**

Append progress:

```markdown
- 2026-06-26: 阶段 6D Task 4 完成；run-once 通过配置构建远程 scraper，默认 example 行为保持不变，远程 source enabled 时优先命中。
```

Commit:

```powershell
git add src-tauri/src/remote_scraper.rs src-tauri/src/daemon_control.rs src-tauri/tests/daemon_control.rs
git commit -m "接入阶段6D运行一轮远程刮削器"
```

## Task 5: Commands, API, and Frontend Settings Panel

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/api.ts`
- Modify: `src/viewModel.ts`
- Modify: `src/viewModel.test.ts`
- Modify: `src/App.tsx`
- Modify: `src/styles.css`

- [ ] **Step 1: Write failing command and frontend tests**

Add command unit tests near existing command settings tests in `src-tauri/src/commands.rs`:

```rust
#[test]
fn remote_scraper_settings_command_normalizes_without_repository() {
    let state = AppState::default();
    let response = configure_remote_scraper_settings(
        RemoteScraperSettings {
            enabled: true,
            user_agent: " media-manager-test ".to_string(),
            sources: vec![RemoteScraperSourceSettings {
                id: "javdb".to_string(),
                enabled: true,
                search_url_template: "https://example.test/{code}".to_string(),
                min_confidence: 0.8,
            }],
            ..RemoteScraperSettings::default()
        },
        tauri::State::from(&state),
    )
    .unwrap();

    assert!(response.data.enabled);
    assert_eq!(response.data.user_agent, "media-manager-test");
}
```

Add `src/viewModel.test.ts` test:

```ts
it("formats remote scraper settings summary", () => {
  expect(formatRemoteScraperSettingsSummary({
    enabled: true,
    timeout_ms: 8000,
    user_agent: "media-manager-test",
    proxy_url: null,
    include_example_fallback: true,
    sources: [
      { id: "javdb", enabled: true, search_url_template: "https://example.test/{code}", min_confidence: 0.82 },
      { id: "javbus", enabled: false, search_url_template: "https://example.test/{code}", min_confidence: 0.82 }
    ]
  })).toBe("已启用 · 1 个远程源 · 保留示例 fallback");
});
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6c'
cargo test --manifest-path src-tauri/Cargo.toml --lib remote_scraper_settings_command_normalizes_without_repository -j 1
npm test -- src/viewModel.test.ts
```

Expected: compile/type failures because commands and formatter do not exist.

- [ ] **Step 3: Add commands**

In `src-tauri/src/commands.rs`, import:

```rust
use crate::remote_scraper::RemoteScraperSettings;
```

Add `remote_scraper_settings: Mutex<RemoteScraperSettings>` to `AppState`, default it, and add commands:

```rust
#[tauri::command]
pub fn configure_remote_scraper_settings(
    settings: RemoteScraperSettings,
    state: State<'_, AppState>,
) -> Result<CommandResult<RemoteScraperSettings>, String> {
    let normalized = if let Some(repo) = state
        .repository
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        repo.set_remote_scraper_settings(&settings)
            .map_err(|error| error.to_string())?
    } else {
        settings.normalized().map_err(|error| error.to_string())?
    };
    *state
        .remote_scraper_settings
        .lock()
        .map_err(|error| error.to_string())? = normalized.clone();
    Ok(CommandResult { data: normalized })
}

#[tauri::command]
pub fn get_remote_scraper_settings(
    state: State<'_, AppState>,
) -> Result<CommandResult<RemoteScraperSettings>, String> {
    Ok(CommandResult {
        data: state
            .remote_scraper_settings
            .lock()
            .map_err(|error| error.to_string())?
            .clone(),
    })
}
```

Register both commands in `generate_handler!`, and preload settings in setup:

```rust
            let remote_scraper_settings = repo
                .get_remote_scraper_settings()
                .map_err(|error| error.to_string())?;
            *state
                .remote_scraper_settings
                .lock()
                .map_err(|error| error.to_string())? = remote_scraper_settings;
```

- [ ] **Step 4: Add TypeScript DTOs and API wrappers**

In `src/api.ts`, add:

```ts
export interface RemoteScraperSourceSettings {
  id: string;
  enabled: boolean;
  search_url_template: string;
  min_confidence: number;
}

export interface RemoteScraperSettings {
  enabled: boolean;
  timeout_ms: number;
  user_agent: string;
  proxy_url?: string | null;
  include_example_fallback: boolean;
  sources: RemoteScraperSourceSettings[];
}
```

Add wrappers:

```ts
  configureRemoteScraperSettings(settings: RemoteScraperSettings) {
    return command<RemoteScraperSettings>("configure_remote_scraper_settings", { settings });
  },
  getRemoteScraperSettings() {
    return command<RemoteScraperSettings>("get_remote_scraper_settings");
  },
```

- [ ] **Step 5: Add view model formatter**

In `src/viewModel.ts`, add:

```ts
import type { RemoteScraperSettings } from "./api";

export function formatRemoteScraperSettingsSummary(settings: RemoteScraperSettings): string {
  if (!settings.enabled) {
    return "已停用";
  }
  const enabledSources = settings.sources.filter((source) => source.enabled).length;
  const fallback = settings.include_example_fallback ? "保留示例 fallback" : "不使用示例 fallback";
  return `已启用 · ${enabledSources} 个远程源 · ${fallback}`;
}
```

- [ ] **Step 6: Add App state and save handler**

In `src/App.tsx`, import `RemoteScraperSettings`, add default state:

```ts
const defaultRemoteScraperSettings: RemoteScraperSettings = {
  enabled: false,
  timeout_ms: 8000,
  user_agent: "media-manager/0.1 local scraper",
  proxy_url: "",
  include_example_fallback: true,
  sources: [
    { id: "javdb", enabled: false, search_url_template: "https://javdb.com/search?q={code}&f=all", min_confidence: 0.82 },
    { id: "javbus", enabled: false, search_url_template: "https://www.javbus.com/search/{code}", min_confidence: 0.82 },
    { id: "fanza", enabled: false, search_url_template: "https://www.dmm.co.jp/digital/videoa/-/list/search/=/?searchstr={code}", min_confidence: 0.82 }
  ]
};
```

Add state and loader:

```ts
const [remoteScraperSettings, setRemoteScraperSettings] = useState<RemoteScraperSettings>(defaultRemoteScraperSettings);
const [remoteScraperBusy, setRemoteScraperBusy] = useState(false);

function applyRemoteScraperSettings(settings: RemoteScraperSettings) {
  setRemoteScraperSettings({ ...settings, proxy_url: settings.proxy_url ?? "" });
}
```

Update `loadDaemonPanelData` to fetch `api.getRemoteScraperSettings()` and call `applyRemoteScraperSettings`.

Add save handler:

```ts
async function saveRemoteScraperSettings() {
  if (remoteScraperBusy) return;
  setRemoteScraperBusy(true);
  setStatus("正在保存远程刮削器配置...");
  try {
    const saved = await api.configureRemoteScraperSettings({
      ...remoteScraperSettings,
      proxy_url: remoteScraperSettings.proxy_url?.trim() ? remoteScraperSettings.proxy_url : null
    });
    applyRemoteScraperSettings(saved);
    setStatus(`远程刮削器配置已保存：${formatRemoteScraperSettingsSummary(saved)}。`);
  } catch (error) {
    setStatus(`保存远程刮削器配置失败：${String(error)}`);
  } finally {
    setRemoteScraperBusy(false);
  }
}
```

- [ ] **Step 7: Add settings panel UI**

Place this block below the aria2 settings block:

```tsx
<div className="remote-scraper-settings">
  <div className="remote-scraper-settings-head">
    <label>
      <input
        type="checkbox"
        checked={remoteScraperSettings.enabled}
        onChange={(event) => setRemoteScraperSettings({ ...remoteScraperSettings, enabled: event.target.checked })}
      />
      远程刮削器
    </label>
    <button type="button" onClick={saveRemoteScraperSettings} disabled={remoteScraperBusy || !hasBackend}>
      <Settings size={16} /> {remoteScraperBusy ? "保存中" : "保存刮削器"}
    </button>
  </div>
  <div className="remote-scraper-settings-grid">
    <label>
      User-Agent
      <input
        value={remoteScraperSettings.user_agent}
        onChange={(event) => setRemoteScraperSettings({ ...remoteScraperSettings, user_agent: event.target.value })}
      />
    </label>
    <label>
      超时 ms
      <input
        type="number"
        min={1}
        value={remoteScraperSettings.timeout_ms}
        onChange={(event) => setRemoteScraperSettings({ ...remoteScraperSettings, timeout_ms: Number(event.target.value) })}
      />
    </label>
    <label>
      代理 URL
      <input
        value={remoteScraperSettings.proxy_url ?? ""}
        onChange={(event) => setRemoteScraperSettings({ ...remoteScraperSettings, proxy_url: event.target.value })}
      />
    </label>
  </div>
  <label className="remote-scraper-fallback">
    <input
      type="checkbox"
      checked={remoteScraperSettings.include_example_fallback}
      onChange={(event) => setRemoteScraperSettings({ ...remoteScraperSettings, include_example_fallback: event.target.checked })}
    />
    保留示例 fallback
  </label>
  <div className="remote-scraper-source-list">
    {remoteScraperSettings.sources.map((source, index) => (
      <div className="remote-scraper-source-row" key={source.id}>
        <label>
          <input
            type="checkbox"
            checked={source.enabled}
            onChange={(event) => {
              const sources = remoteScraperSettings.sources.map((candidate, sourceIndex) =>
                sourceIndex === index ? { ...candidate, enabled: event.target.checked } : candidate
              );
              setRemoteScraperSettings({ ...remoteScraperSettings, sources });
            }}
          />
          {source.id}
        </label>
        <input
          value={source.search_url_template}
          onChange={(event) => {
            const sources = remoteScraperSettings.sources.map((candidate, sourceIndex) =>
              sourceIndex === index ? { ...candidate, search_url_template: event.target.value } : candidate
            );
            setRemoteScraperSettings({ ...remoteScraperSettings, sources });
          }}
        />
        <input
          type="number"
          min={0}
          max={1}
          step={0.01}
          value={source.min_confidence}
          onChange={(event) => {
            const sources = remoteScraperSettings.sources.map((candidate, sourceIndex) =>
              sourceIndex === index ? { ...candidate, min_confidence: Number(event.target.value) } : candidate
            );
            setRemoteScraperSettings({ ...remoteScraperSettings, sources });
          }}
        />
      </div>
    ))}
  </div>
</div>
```

- [ ] **Step 8: Add CSS by sharing aria2 panel styles**

In `src/styles.css`, extend selectors:

```css
.aria2-settings,
.remote-scraper-settings {
  border: 1px solid #dfe7ec;
  border-radius: 6px;
  background: #fbfdfe;
  padding: 12px;
  display: grid;
  gap: 10px;
}

.aria2-settings-head,
.remote-scraper-settings-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 10px;
}

.aria2-settings-grid,
.remote-scraper-settings-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 10px;
}

.remote-scraper-source-list {
  display: grid;
  gap: 8px;
}

.remote-scraper-source-row {
  display: grid;
  grid-template-columns: 120px minmax(0, 1fr) 90px;
  gap: 8px;
  align-items: center;
}
```

- [ ] **Step 9: Run focused frontend and command tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6c'
cargo test --manifest-path src-tauri/Cargo.toml --lib remote_scraper_settings_command_normalizes_without_repository -j 1
npm test -- src/viewModel.test.ts
npx tsc --noEmit
```

Expected: command test, view model test, and TypeScript pass.

- [ ] **Step 10: Update state and commit**

Append progress:

```markdown
- 2026-06-26: 阶段 6D Task 5 完成；新增远程 scraper Tauri commands、前端 DTO/API、设置页配置块和状态反馈。
```

Commit:

```powershell
git add src-tauri/src/commands.rs src/api.ts src/viewModel.ts src/viewModel.test.ts src/App.tsx src/styles.css
git commit -m "接入阶段6D远程刮削器前端配置"
```

## Task 6: Full Verification, Review, and Handoff

**Files:**
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/progress.md`
- Create: `.ai_state/reviews/sprint-11.md`
- Modify: `.ai_state/lessons.md`
- Modify: `HANDOFF.md`

- [ ] **Step 1: Run full safe verification**

Before cargo, check for stale Rust processes:

```powershell
Get-Process cargo,rustc,link -ErrorAction SilentlyContinue
```

Then run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR='src-tauri/target/stage6c'
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

Expected: all pass. Do not run `tauri dev`, default `cargo run`, `media-manager.exe`, or in-app browser.

- [ ] **Step 2: Self-review the diff**

Run:

```powershell
git diff --stat HEAD
git diff HEAD -- src-tauri/src/remote_scraper.rs src-tauri/src/daemon_control.rs src-tauri/src/commands.rs src/App.tsx
```

Check:

- default remote scraper disabled;
- no live-site tests;
- no cookie/login/captcha logic;
- run-once still refuses empty metadata source;
- all frontend save actions have loading/disabled/status feedback.

- [ ] **Step 3: Request reviewer if tool is available**

Use the configured reviewer/subagent path if available. If reviewer tooling fails again, record the failure and perform mainline self-review in `.ai_state/reviews/sprint-11.md`.

- [ ] **Step 4: Update documents**

Update `.ai_state/tasks.md` so every Sprint 11 task is checked.

Append final progress:

```markdown
- 2026-06-26: 阶段 6D 全量验证通过；远程 scraper 配置、HTTP client、站点 adapter、run-once 集成和前端配置已完成。
```

Add `HANDOFF.md` Stage 6D section:

```markdown
**阶段 6D（真实 scraper adapter 可验证接入）已实现并验证。**
阶段 6D 新增远程 scraper settings、真实 HTTP client、javdb/javbus/fanza adapter registry、run-once 远程 source 接入和前端配置区。测试仍使用 fake client、本地 TCP server 和 HTML fixtures，不访问真实站点、不处理登录/cookie/反爬，也不下载封面。
```

Append a lesson:

```markdown
## [2026-06-26 Sprint 11] 真实 scraper 要把 live-site 风险挡在 fixture 边界外

- **Pattern**: 真实 HTTP client 只实现 transport；站点解析用 source registry + checked-in fixture 测试，run-once 通过配置显式启用后才加入远程 source。
- **Constraint**: Codex 验证不得访问真实 scraper 站点，不做登录、cookie、验证码或反爬绕过；真实环境验证由用户在交互桌面里启用配置后执行。
```

- [ ] **Step 5: Commit final docs**

Commit:

```powershell
git add HANDOFF.md
git commit -m "更新阶段6D交接说明"
```

- [ ] **Step 6: Final status**

Report:

- commit hashes created in Stage 6D;
- exact verification commands and pass/fail status;
- actual-environment note: user can enable remote scraper settings and run one pass, but live-site HTML compatibility is not guaranteed by Codex fixture tests.
