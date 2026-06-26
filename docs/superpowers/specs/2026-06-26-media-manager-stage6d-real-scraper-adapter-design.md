# 阶段 6D 真实 scraper adapter 可验证接入设计

## 背景

阶段 6A 已经把远程 scraper 抽象成 `RemoteMetadataHttpClient`、`RemoteScraperSource` 和 JSON-LD fixture parser；阶段 6C 已经让“运行一轮”可以消费 aria2 完成任务并进入自动管线。但当前自动管线的元数据来源仍然是 `ExamplePipelineScraper`，真实环境里只能验证“文件能被归档”，不能验证“远程站点元数据能被拉取、解析并优先用于入库”。

阶段 6D 的目标是在不引入 live-site 测试依赖的前提下，把真实 HTTP client、可持久化的远程 scraper 设置、站点 adapter 注册表和前端配置入口接进现有 run-once 工作流。用户在实际桌面环境里启用后，可以看到远程 scraper 是否被调用、是否命中、失败原因是什么；Codex 验证仍然只依赖 fake client、fixture HTML、临时 SQLite 和本地 TCP server。

## 目标

- 新增持久化 `RemoteScraperSettings`，保存总开关、超时、User-Agent、可选代理字段、示例 fallback 开关和源列表。
- 新增真实 HTTP client，实现现有 `RemoteMetadataHttpClient` trait，支持真实 `http://` / `https://` 页面获取、超时、User-Agent 和明确错误分类。
- 新增站点 adapter 注册层，把 `javdb`、`javbus`、`fanza` 作为可配置 source id 暴露出来；每个 adapter 有独立 parser 入口和 fixture 测试。
- 修改 command 层 run-once：默认行为保持 example scraper；远程设置启用后，按配置顺序把真实远程 source 加入 `ScrapeCoordinator`。
- 新增 Tauri commands 和前端 API，用于读取/保存远程 scraper 设置。
- 在设置页“自动管线”增加远程 scraper 配置区，显示启用状态、User-Agent、超时、源列表和保存反馈。
- 所有测试不访问真实 scraper 站点，不依赖真实媒体盘、不启动 WebView2。

## 非目标

- 不保证当前 javdb / javbus / fanza live HTML 一定能解析；6D 的可验证契约是 adapter 边界、HTTP 获取、配置和 fixture parser。
- 不实现登录、cookie 采集、验证码、反爬绕过或任何规避访问控制的逻辑。
- 不在 Codex 测试中访问真实成人站点、真实代理或真实用户资源。
- 不下载封面图片；`cover_url` 继续保留在远程中间模型，`ScrapedWorkMetadata.cover_path` 仍为 `None`。
- 不做后台常驻 scraper 队列、重试调度、缓存失效策略或批量补元数据。
- 不替换已有 provider / pipeline trait；阶段 6D 只在现有边界后面增加真实实现。

## 方案对比

### 方案 A：只新增标准库 HTTP client

优点是依赖最少；缺点是 Rust 标准库没有 TLS，真实站点大多是 HTTPS，只能支持很有限的实际验证。这个方案更像 transport 练习，不足以支撑“实际环境可测”。

### 方案 B：新增轻量 HTTPS client + 保持 fixture-first 测试（推荐）

使用轻量同步 HTTP client 依赖实现 `RemoteMetadataHttpClient`，真实运行时支持 HTTPS、超时和 User-Agent；测试仍通过 fake client 和本地 HTTP server 覆盖请求形状，不触网。风险是多一个依赖，但边界集中在 `remote_scraper.rs`，且不会影响默认禁用路径。

### 方案 C：直接接入 Tauri/WebView 或浏览器抓取

优点是能处理更多动态页面；缺点是会触发本项目明确禁止的 WebView2 路径，也容易滑向登录态和反爬绕过。本阶段不采用。

阶段 6D 采用方案 B。

## 架构

### 设置模型

新增 `RemoteScraperSettings`：

- `enabled: bool`
- `timeout_ms: u64`
- `user_agent: String`
- `proxy_url: Option<String>`
- `include_example_fallback: bool`
- `sources: Vec<RemoteScraperSourceSettings>`

新增 `RemoteScraperSourceSettings`：

- `id: String`
- `enabled: bool`
- `search_url_template: String`
- `min_confidence: f32`

默认值：

- 总开关 disabled；
- timeout `8000`；
- User-Agent 使用明确的 media-manager 标识；
- proxy 为空；
- `include_example_fallback = true`，保持阶段 4/6C 默认行为；
- 内置 `javdb` / `javbus` / `fanza` 三个 disabled source，模板可编辑。

`Repository` 继续用 `app_settings` JSON 存储，不新增表。保存时会 trim、去重 source id、拒绝空 User-Agent、拒绝 timeout 为 0、拒绝 enabled source 缺少 `{code}`。

### HTTP client

新增 `HttpRemoteMetadataClient`，实现 `RemoteMetadataHttpClient`：

- 使用同步 HTTPS-capable client；
- 每次请求设置 User-Agent；
- 应用超时；
- 2xx 才读取 body；
- 非 2xx 返回 `RemoteScraperError::HttpStatus`；
- 连接、DNS、TLS、超时类错误返回 `RemoteScraperError::Network`；
- body 转文本失败返回 `RemoteScraperError::Transport`。

`proxy_url` 字段先作为配置面保留。实现层只有在依赖 API 可直接安全配置时才启用；否则保存但不默认使用，并在设计偏离时更新本文档。

### 站点 adapter

新增 source registry：

- `RemoteScraperPreset::JavDb`
- `RemoteScraperPreset::JavBus`
- `RemoteScraperPreset::Fanza`

每个 preset 暴露：

- source id；
- 默认 URL template；
- parser 函数；
- 默认 confidence。

parser 入口暂时复用 JSON-LD 解析能力，并增加轻量 code/title sanity：

- JSON-LD 缺失时返回 `Ok(None)`，表示 not found / unsupported page；
- JSON-LD 损坏时返回 parser error；
- 命中的 metadata 会保留 source id、normalized code、title、actors、genres、studio、director、release date、cover URL；
- confidence 低于 source 设置时返回 `Ok(None)`。

这个注册层的价值是把“具体站点解析策略”从通用 `RemoteScraperSource` 中拆出来，后续如果某个站点需要 DOM/meta fallback，只改对应 parser 和 fixture。

### run-once 集成

新增 `ConfiguredPipelineScrapers` 作为命令层构建器：

1. 读取 `RemoteScraperSettings`；
2. 如果远程 scraper enabled，按 settings 顺序创建 `RemoteScraperSource<HttpRemoteMetadataClient>`；
3. 远程 source 放在 example 前面，让真实 metadata 优先命中；
4. 如果 `include_example_fallback` 为 true，保留 `ExamplePipelineScraper`；
5. 如果没有任何 source，保持现有保护：拒绝处理真实文件，避免空 scraper 把内容误路由。

`ScrapeCoordinator` 本身不需要改成 owning 类型；构建器拥有 boxed sources，并提供 borrowed refs 给 `HeadlessDaemon`。

### 前端配置

设置页“自动管线”新增远程 scraper 配置区：

- 总启用开关；
- User-Agent；
- timeout；
- proxy URL 输入框，文案标明“可选，默认不用”；
- example fallback 开关；
- 三个内置源的启用开关、URL template、min confidence；
- 保存按钮有 loading、disabled 和状态反馈。

UI 不展示使用说明长文，只保留必要标签和状态。真正运行反馈仍通过 run-once 摘要、scrape_jobs 和异常/搁置面板体现。

## 数据流

```text
保存远程 scraper 设置
  -> configure_remote_scraper_settings Tauri command
  -> Repository::set_remote_scraper_settings(app_settings JSON)

运行一轮
  -> frontend daemon client
  -> REST /v1/run-once 或 command bridge
  -> daemon_control::run_daemon_once
  -> Repository::get_remote_scraper_settings
  -> ConfiguredPipelineScrapers
  -> ScrapeCoordinator
  -> RemoteScraperSource::lookup
  -> HttpRemoteMetadataClient::get_text
  -> source-specific parser
  -> AutoPipeline 入库 / 搁置 / 异常
```

## 错误处理

- 配置保存失败：返回明确命令错误，不覆盖旧配置。
- source disabled：完全不参与 run-once。
- URL template 缺 `{code}`：保存时拒绝。
- 网络错误：scrape job 记录具体 source 和错误字符串，coordinator 继续尝试下一个 source。
- HTTP 404 / 410：映射为 not found，允许 fallback。
- HTTP 401 / 403 / 429 / 5xx：记录 HTTP status 错误，允许 fallback，但不伪装成 parser miss。
- parser 无命中：返回 `Ok(None)`，记录 not found。
- parser 损坏：返回 parser error，记录失败原因。
- 所有 source 都失败或无命中：保持阶段 2 语义，进入 scrape-failure exception。

## 测试策略

Rust：

- `remote_scraper.rs` 测默认 settings、归一化、非法配置拒绝。
- `remote_scraper.rs` 用本地 TCP server 验证真实 HTTP client 的 method、path、User-Agent 和非 2xx 错误。
- `remote_scraper.rs` 用 checked-in fixture 覆盖 `javdb` / `javbus` / `fanza` parser 入口。
- `storage` / `core_behaviour` 测 `Repository::set_remote_scraper_settings` 和 `get_remote_scraper_settings`。
- `daemon_control.rs` 用 injectable remote client 或 local server 覆盖远程 source enabled 时优先于 example，disabled 时保持旧行为。
- 完整 Rust gate 仍只跑 `cargo test --manifest-path src-tauri/Cargo.toml -j 1`。

前端：

- `api.ts` 补齐 settings DTO 和 command wrapper 类型。
- `viewModel.test.ts` 覆盖远程 scraper 设置摘要或归一化显示 helper。
- `App.tsx` 配置区保存按钮覆盖 loading/disabled/status 行为；如果纯组件测试成本过高，用 `tsc` 和现有 Vitest helper 覆盖数据转换。
- 完整前端 gate：`npm test`、`npx tsc --noEmit`、`npm run build`。

## 验收

- 默认 settings 下，run-once 行为与阶段 6C 保持一致。
- 保存远程 scraper settings 后，重开 Repository 能读回归一化结果。
- 启用远程 source 后，run-once 会先尝试远程 scraper；fixture / fake 命中时使用远程 metadata 入库。
- HTTP client 能对本地测试 server 发出带 User-Agent 的 GET，并正确处理非 2xx。
- 站点 adapter parser 有独立 fixture 测试，不访问真实站点。
- 前端可以读取、编辑、保存远程 scraper 设置，所有保存操作有 loading/disabled/status 反馈。
- Codex 验证不依赖真实媒体盘、真实 scraper 站点、真实代理、WebView2 或 Tauri GUI。
