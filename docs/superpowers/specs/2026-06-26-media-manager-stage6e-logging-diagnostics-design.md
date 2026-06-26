# Stage 6E Logging and Diagnostics Design

## Goal

阶段 6E 为 media-manager 增加一套本地优先的日志与诊断系统，让后续真实环境运行 daemon、aria2 轮询、远程刮削器和控制服务时，可以在不接入外部服务、不依赖真实媒体资源的前提下定位问题。

本阶段的完成物：

- Rust 后端提供结构化诊断日志写入、读取最近日志、导出诊断快照。
- 关键操作写入统一诊断事件，而不是继续把业务表当日志系统使用。
- 诊断导出只包含可定位问题的摘要和最近事件，不导出媒体文件，不暴露 token、secret、密码或带凭据的代理地址。
- 前端设置页提供诊断入口：刷新最近日志、导出诊断快照，并保留明确 loading/disabled/status 反馈。
- 所有测试使用临时目录、fake 数据和本地文件，不访问真实 H:/、G:/、aria2 或 scraper 站点。

## Current State

项目当前已有业务记录，但没有统一日志系统：

- `pipeline_runs` 记录单个文件自动管线的步骤和终态。
- `scrape_jobs` 记录刮削尝试、来源、错误和关联 pipeline run。
- `exceptions` / `holding` 记录需要人工处理的队列。
- `DaemonControlRuntime.last_error` 保存最近 daemon 错误。
- 前端 `status` 只显示当前页面反馈，刷新后不会保留。

这些记录能说明业务结果，但缺少跨模块时间线：例如一次“运行一轮”开始、读取配置、aria2 轮询、远程刮削配置、控制服务启动失败、导出诊断等事件目前无法按时间串起来。

## Chosen Approach

采用方案 A：项目自有轻量 JSONL 诊断日志。

每条日志是一行 JSON，写入 app data 下的 `logs/media-manager.jsonl`。后端通过 `DiagnosticsWriter` 追加事件、读取尾部事件、在文件达到上限时做简单轮转。诊断快照通过 Tauri command 生成一个 JSON 文件，默认写入 app data 下的 `diagnostics/diagnostics-<timestamp>.json`。

选择该方案的原因：

- 不增加新依赖，适合当前 Rust/Tauri 核心库的保守风格。
- 日志文件独立于 SQLite；即使数据库迁移、锁或损坏出现问题，仍然可以保留启动和命令层事件。
- JSONL 可测试、可人工检查、可后续接入 `tracing` 或导入工具。
- 和现有 `pipeline_runs` / `scrape_jobs` 互补：业务表记录结果，诊断日志记录过程。

未选方案：

- `tracing` + rolling appender：长期标准，但本阶段需要引入和穿透更多框架配置，收益不如先建立诊断契约。
- SQLite 日志表：查询方便，但数据库异常时日志也可能不可用；同时会扩大业务库职责。

## Architecture

### Backend Components

新增 `src-tauri/src/diagnostics.rs`，负责诊断系统的核心逻辑。

主要类型：

- `DiagnosticLevel`：`Info` / `Warn` / `Error`。
- `DiagnosticLogEntry`：包含 `timestamp`、`level`、`target`、`message`、`context`。
- `DiagnosticLogRequest`：前端可传入 `limit`，后端限制最大返回数量。
- `DiagnosticSnapshot`：一次导出的诊断快照，包含生成时间、运行摘要、设置摘要、最近业务记录和最近日志。
- `DiagnosticExportResult`：返回导出文件路径、日志条数、pipeline run 数、scrape job 数。
- `DiagnosticsWriter`：持有日志路径和轮转配置，提供 `append`、`tail`、`export_snapshot`。

`AppState` 新增 `diagnostics: Mutex<Option<DiagnosticsWriter>>`。Tauri setup 在 app data 可用后初始化：

1. 创建 `logs/` 和 `diagnostics/` 目录。
2. 初始化 writer。
3. 写入 `app.setup` 事件，记录 repository 打开和控制服务启动结果。

命令层通过轻量 helper 写日志，日志失败不应让业务命令失败。诊断系统用于定位问题，不能变成主流程的单点失败。

### Event Targets

本阶段写入以下目标事件：

- `app.setup`：应用数据目录、SQLite 初始化、控制服务启动结果。
- `settings.aria2`：保存 aria2 配置结果，包含 enabled、host、port、tracked gid 数，secret 脱敏。
- `settings.remote_scraper`：保存远程刮削配置结果，包含 enabled、source 数、proxy 是否配置，proxy 凭据脱敏。
- `daemon.run_once`：运行一轮开始、完成、失败，包含 scan/aria2/process 摘要和错误。
- `daemon.control`：pause/resume 结果。
- `diagnostics.export`：诊断快照导出成功或失败。

深层模块不在本阶段直接依赖 logger。阶段 6E 先在命令边界获得可用时间线，后续真实 daemon 常驻化时再决定是否把 logger 注入 `daemon`、`aria2`、`remote_scraper` 内部。

### Diagnostic Snapshot

导出快照为单个 JSON 文件，不打包 zip，不包含媒体内容。

快照内容：

- `generated_at`：导出时间。
- `app_data_dir`：app data 路径。
- `control_service`：当前 app-owned 控制服务状态。
- `settings`：source root 数、archive root 是否配置、metadata provider 是否启用、aria2/remote scraper 脱敏摘要。
- `daemon`：当前 daemon status 摘要。
- `recent_pipeline_runs`：最近 10 条。
- `recent_scrape_jobs`：最近 20 条。
- `open_exceptions`：打开状态异常数量和最近 20 条。
- `holding_items`：搁置数量和最近 20 条。
- `recent_logs`：最近 200 条诊断日志。

数量限制固定在后端，避免前端误操作导出过大文件。

### Redaction Rules

所有日志上下文和诊断快照在写入或导出前统一脱敏。

规则：

- key 包含 `secret`、`token`、`password`、`authorization`、`cookie` 时，非空值写为 `"***"`。
- aria2 secret 只记录是否配置，不记录原值。
- 代理 URL 如包含 `user:password@host`，导出时改为 `scheme://***@host`；无凭据代理只保留 host/path。
- 不导出媒体文件内容、NFO 内容、封面图片或缩略图。
- 路径默认保留，因为本地诊断需要知道哪个文件失败；未来如果要分享给他人，可在后续阶段增加“路径匿名化导出”模式。

## Frontend Design

设置页“自动管线”页签新增“诊断”块，沿用现有 `daemon-panel` 的紧凑工具界面。

状态与操作：

- “刷新日志”：调用 `get_diagnostic_log_tail({ limit: 80 })`，按钮显示 loading 并禁用。
- “导出诊断”：调用 `export_diagnostics_snapshot()`，按钮显示 loading 并禁用，完成后状态行显示导出路径和摘要。
- 最近日志列表：按时间倒序或后端返回顺序展示 `timestamp / level / target / message`，错误级别可使用现有警示色。

前端新增类型和 helper：

- `DiagnosticLevel`
- `DiagnosticLogEntry`
- `DiagnosticExportResult`
- `formatDiagnosticLogLine`
- `formatDiagnosticExportSummary`

普通浏览器 demo 模式不模拟诊断导出；没有 Tauri 后端时显示现有 backend warning 和状态反馈即可。

## Testing Strategy

Rust tests：

- `DiagnosticsWriter` 在临时目录创建 JSONL 文件并能读取尾部日志。
- `tail(limit)` 限制返回数量，limit 为 0 或过大时使用后端上限。
- 日志轮转在小尺寸配置下可触发，旧文件保留且新文件继续写入。
- 脱敏规则覆盖 secret/token/password/header/proxy URL。
- 快照 helper 使用临时 SQLite repository 造假 pipeline/scrape/exception/holding/settings，导出 JSON 文件并确认不含 secret 原文。
- 命令层 helper 在 diagnostics 未初始化时不阻断业务返回。

Frontend tests：

- `formatDiagnosticLogLine` 对 Info/Warn/Error 生成稳定中文文本。
- `formatDiagnosticExportSummary` 展示路径、日志数和业务摘要。
- API 类型通过 `npx tsc --noEmit`。

Full safe gate：

- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`
- `npm test`
- `npx tsc --noEmit`
- `npm run build`

仍然不运行 `tauri dev`、`cargo run`、`media-manager.exe` 或 in-app browser。

## Non-Goals

- 不实现实时日志 streaming、WebSocket 或后台 tail。
- 不上传日志，不接入外部 observability 服务。
- 不导出 zip 包，不引入压缩依赖。
- 不实现路径匿名化分享模式。
- 不把深层 `daemon` / `aria2` / `remote_scraper` 全量改造成 logger-aware。
- 不解析或导出真实媒体、NFO、封面、缩略图内容。

## Acceptance Criteria

- 后端能在 app data 下写入结构化 JSONL 日志。
- 设置保存、daemon 运行一轮、pause/resume、控制服务 setup、诊断导出会产生可读事件。
- 前端能刷新最近日志并导出诊断快照，所有长操作有 loading/disabled/status 反馈。
- 导出的 JSON 不包含 aria2 secret、token、password、Authorization、Cookie 或代理 URL 明文凭据。
- 所有新增验证都使用临时目录和假数据，另一台没有真实资源的机器也能运行。
- 安全验证命令全部通过。
