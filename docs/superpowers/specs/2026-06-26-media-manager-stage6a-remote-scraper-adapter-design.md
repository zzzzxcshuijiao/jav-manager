# 阶段 6A 远程 Scraper Adapter 骨架设计

## 背景

阶段 2~5 已经把“完成文件 → 番号识别 → 刮削 → 归档 → 入库 → 控制接口 → 前端控制通道”串起来，但元数据仍然由 `ExamplePipelineScraper` 生成。阶段 6B 已经补上 aria2 RPC 完成信号骨架；如果下载侧继续推进，而元数据侧仍只有 example scraper，真实文件最终还是会因为刮削不足进入异常队列。

阶段 6A 的目标不是直接抓真实站点，也不是在 Codex 里访问成人站点验证。目标是把远程 scraper 的工程边界先做出来：HTTP 获取可注入、解析器可单测、失败语义稳定、输出能复用现有 `ScraperSource` 和旧 ingest `MetadataProvider` 形状。后续添加 FANZA/JavBus/JavDB 具体 adapter 时，只需要补 URL 策略和源特定 parser。

## 目标

- 新增纯 Rust 远程 metadata scraper 模块，作为真实 scraper 的 adapter 骨架。
- 支持注入 HTTP client，测试用 fake client，不依赖真实网络、代理、登录态或站点可用性。
- 用 fixture 覆盖 HTML 中 JSON-LD metadata 解析，先支持通用 Movie / VideoObject 结构。
- 输出中间模型 `RemoteMetadata`，再显式转换为：
  - `ProviderMetadata`：服务旧 ingest / review 路径；
  - `ScrapedWorkMetadata`：服务阶段 2 自动管线 `ScraperSource`。
- `RemoteScraperSource<C>` 同时实现 `MetadataProvider` 和 `ScraperSource`，这样旧入口和自动管线不需要两套远程适配器。
- 明确错误语义：HTTP 失败返回 error，解析不到匹配作品返回 `Ok(None)`，解析到低质量数据按字段缺失处理而不是 panic。

## 非目标

- 不访问真实 FANZA/JavBus/JavDB。
- 不承诺当前任何第三方站点 HTML 结构。
- 不下载封面图片到本地资源池；`ScrapedWorkMetadata.cover_path` 暂时为 `None`，远程封面 URL 只保留在 `ProviderMetadata.cover_url`。
- 不做代理、cookie、登录、重试、限速、robots 策略或站点反爬处理。
- 不新增 UI 设置、多源排序或 scraper 开关；现有 `metadata_provider_enabled` 仍只控制 example / disabled。
- 不替换 `ExamplePipelineScraper` 的默认运行路径，避免实际环境误触真实网络。

## 方案对比

### 方案 A：直接写 FANZA/JavDB 站点 parser

优点是看起来最接近真实使用；缺点是站点结构、登录/区域限制、网络稳定性都不适合在 Codex 环境验证，容易把阶段目标变成“追网站变化”。

### 方案 B：先写通用远程 adapter + JSON-LD parser（推荐）

优点是工程边界完整：HTTP、解析、转换、错误语义和管线接入都能用 fixture 测清楚；后续站点 parser 可以渐进加入。缺点是还不能立刻抓真实站点。

### 方案 C：继续只保留 ExamplePipelineScraper

优点是零风险；缺点是自动管线永远缺真实 metadata 入口，后续任何下载完成信号都会卡在元数据侧。

本阶段采用方案 B。

## 架构

新增 `src-tauri/src/remote_scraper.rs`：

- `RemoteMetadataHttpClient`
  - trait，方法 `get_text(&self, url: &str) -> anyhow::Result<String>`。
  - 测试用 fake client 记录 URL 并返回 fixture。
  - 真实 HTTP 实现暂不加入，避免本阶段引入 TLS / 代理 / 站点策略；后续可用 `ureq` / `reqwest` 或已有运行时接入。
- `RemoteScraperConfig`
  - `source_name`
  - `search_url_template`，包含 `{code}` 占位符。
  - `min_confidence`
- `RemoteMetadata`
  - provider、normalized_code、title、original_title、summary、actors、genres、studio、director、release_date、cover_url、confidence。
- `RemoteScraperSource<C>`
  - 持有 config 和 client。
  - `lookup_remote(normalized_code)`：构造 URL → 拉取 HTML → 解析 fixture → 返回 `Option<RemoteMetadata>`。
  - 实现 `MetadataProvider`：转为 `ProviderMetadata`。
  - 实现 `ScraperSource`：转为 `ScrapedWorkMetadata`，`cover_path = None`。
- `parse_json_ld_metadata(source_name, normalized_code, html, min_confidence)`
  - 从 `<script type="application/ld+json">` 中提取 JSON。
  - 支持单对象、数组、`@graph` 三种常见形状。
  - 接受 `@type` 为 `Movie` 或 `VideoObject` 的对象。
  - title 来源优先 `name`，original_title 可用 `alternateName`。
  - actor/director/productionCompany 支持字符串、对象、数组三种形状。
  - genre 支持字符串或数组。
  - date 使用 `datePublished`。
  - image 支持字符串、数组或对象 URL。

## 数据流

```text
normalized_code
  -> RemoteScraperSource::lookup_remote
  -> RemoteScraperConfig::build_url
  -> RemoteMetadataHttpClient::get_text
  -> parse_json_ld_metadata
  -> RemoteMetadata
  -> ProviderMetadata 或 ScrapedWorkMetadata
  -> IngestEngine / ScrapeCoordinator
```

## 错误处理

- URL 模板缺 `{code}`：创建 config 时返回 error。
- `normalized_code` 为空：lookup 返回 error。
- HTTP client error：向上返回 error，让 `ScrapeCoordinator` 写 scrape_jobs failure。
- HTML 没有 JSON-LD 或没有 Movie / VideoObject：返回 `Ok(None)`，让多源 fallback 继续。
- JSON-LD 存在但 JSON 解析失败：返回 error，避免把站点结构损坏误判为“没找到”。
- 解析出对象但缺 title：返回 `Ok(None)`，因为当前管线至少需要标题落库。

## 测试策略

新增 `src-tauri/tests/remote_scraper.rs`：

- JSON-LD Movie fixture 能解析 title、original title、summary、actors、genres、studio、director、release date、cover URL。
- 数组 / `@graph` fixture 能找到第一个 Movie / VideoObject。
- 缺失 JSON-LD 返回 `None`。
- JSON-LD 语法损坏返回 error。
- `RemoteScraperSource` 会按模板构造 URL，并把 normalized code URL-encode。
- `RemoteScraperSource` 作为 `ScraperSource` 返回 `ScrapedWorkMetadata`，`cover_path = None`。
- `RemoteScraperSource` 作为 `MetadataProvider` 返回 `ProviderMetadata`，保留 `cover_url` 和 confidence。
- HTTP error 会透传。

完整 gate 仍然使用：

```powershell
$ErrorActionPreference='Stop'
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

## 验收

- 不依赖真实网络或真实 scraper 站点。
- 远程 adapter 边界同时适配旧 ingest 和自动管线。
- JSON-LD 解析行为有 fixture 测试，失败语义清楚。
- 现有 `ExamplePipelineScraper` 默认路径不改变。
- 全量验证通过。
