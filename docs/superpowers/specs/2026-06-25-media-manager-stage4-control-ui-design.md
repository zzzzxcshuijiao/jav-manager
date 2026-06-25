# 阶段 4：控制接口与前端连线设计

阶段 4 采用方案 A：**Tauri 命令桥 + 最小 UI 连线**。目标不是一次性完成最终托盘 daemon、HTTP/WebSocket 或六页 UI 重写，而是把阶段 3 已验证的无头核心接到现有桌面前端，让用户能在设置页看到自动管线状态、触发一次受控运行、查看异常队列、查看搁置区和运行记录。

## 为什么这样切

阶段 1 已经补齐 SQLite 模型，阶段 2 已经实现自动管线核心，阶段 3 已经把管线包成纯 Rust 的 `HeadlessDaemon`。现在最小的下一步是把这些能力变成前端可消费的命令契约。

直接做 `127.0.0.1` HTTP/WebSocket 会提前引入端口发现、token、Origin 校验、服务生命周期和常驻线程。直接重写最终六页 UI 也会让前端改动过宽，而且 Codex 环境不能启动 Tauri/WebView2 做可视回归。因此阶段 4 保持同步命令式控制面：可验证、可回退、能给阶段 5 的真实服务化留下接口形状。

## 范围

阶段 4 做：

- 新增一个后端命令门面，把阶段 3 的 daemon core 暴露给前端。
- 修复 `src-tauri/src/commands.rs` 中前端命令路径的 `parse_watch_status`，支持阶段 1 新增状态。
- 前端 `api.ts` 补齐自动管线 DTO、命令 wrapper 和观看状态 union。
- 设置页新增“自动管线”页签，展示配置状态、运行状态、搁置区、异常队列、最近运行记录。
- 所有可感知耗时操作都显示 loading：按钮禁用、文案切换、状态栏提示。
- 测试继续使用临时目录和假文件，不依赖真实 H:/、G:/ 资源。

阶段 4 不做：

- 不启动 Tauri GUI、WebView2、`tauri dev`、默认 `cargo run` 或 `media-manager.exe`。
- 不做常驻托盘 daemon、开机自启、Windows Service、后台线程调度或文件系统 watcher。
- 不做 HTTP/WebSocket、token、Origin、端口发现。
- 不接入真实 FANZA/JavBus/JavDB 网络 scraper。
- 不重写最终六页沉浸式 UI。
- 不让前端直接写 SQLite；写操作继续通过 Tauri command 串行进入 Rust。

## 后端设计

新增 `src-tauri/src/daemon_control.rs`，它只负责命令层适配，不重写扫描、识别、归档或入库逻辑。`src-tauri/src/commands.rs` 继续保留 `build_app()` 和现有命令注册，只在 invoke handler 中注册新的 daemon 命令，并在 `AppState` 中增加少量控制状态。

命令层暴露这些接口：

- `get_daemon_status`
  - 读取配置是否完整、当前暂停标记、最近一次错误、累计处理数。
  - 同时返回当前 SQLite 中的 open exception 数、holding 数、最近 pipeline run 摘要数。
- `pause_daemon` / `resume_daemon`
  - 阶段 4 的暂停是同步命令的“下一次运行闸门”，不是正在运行任务的硬取消。
  - 如果 `run_daemon_once` 已经开始，本阶段不会中途打断文件复制或 SQLite 写入。
- `run_daemon_once`
  - 加载 `DaemonConfig`，构造 `HeadlessDaemon`，执行一次 `scan -> process queue`。
  - 返回阶段 3 的 `RunOnceReport`。
  - 若暂停中，直接返回空 report，并保持 UI 可解释的 paused 状态。
- `list_holding_entries`
  - 返回 `Repository::list_holding()`。
- `list_exception_entries`
  - 返回 `Repository::list_exceptions()`，前端默认突出 `Open`。
- `resolve_exception_entry`
  - 调 `Repository::resolve_exception(id, status)`，只允许 `Ignored` 或 `Resolved`。
- `list_pipeline_runs`
  - 返回 `Repository::list_pipeline_runs()`，前端只展示最近若干条。

`HeadlessDaemon` 当前借用 `Repository` 和 `ScrapeCoordinator`，不适合在 `AppState` 里长期保存。因此阶段 4 不保存 daemon 实例，也不暴露跨命令持久队列；产品 UI 只提供“运行一轮”这一种处理动作。后续阶段如果引入常驻 daemon，再把相同 DTO 搬到 HTTP/WebSocket 或托盘控制层。

## Scraper 适配

阶段 2 的自动管线依赖 `pipeline::ScraperSource`，旧入库页使用的是 `provider::MetadataProvider`，两者不是同一个 trait。阶段 4 新增一个本地、无网络的命令层 adapter：

- `ExamplePipelineScraper`
  - 仅在“示例元数据源”开启时使用。
  - 根据番号生成确定性 `ScrapedWorkMetadata`，不访问网络，不下载图片。
  - 用途是验证阶段 2/3/4 串联：扫描、归档、写 NFO、入库、写运行记录。
- 元数据源未开启时，`run_daemon_once` 返回明确错误，不把真实文件批量送进“刮削失败”异常队列。

这意味着阶段 4 可以在真实目录上测试“文件被扫描和管线可运行”，但刮削质量仍是示例数据。真正可用于长期整理真实库的网络 scraper 留到后续阶段接入。

## DTO 契约

后端返回的 DTO 使用 `serde` 可序列化结构，字段名保持 snake_case，让 Tauri 到 TypeScript 的映射直观。

核心类型：

- `DaemonControlStatus`
  - `state`: `"Idle" | "Scanning" | "Processing" | "Paused" | "Error"`
  - `configured`: `boolean`
  - `source_roots`: `string[]`
  - `archive_root`: `string | null`
  - `asset_roots`: `string[]`
  - `queued`: `number`
  - `processed`: `number`
  - `last_error`: `string | null`
  - `open_exceptions`: `number`
  - `holding_items`: `number`
  - `recent_runs`: `number`
  - `metadata_source`: `"example" | "disabled"`
- `RunOnceReport`
  - `scan.scanned_files`
  - `scan.queued_files`
  - `scan.skipped_files`
  - `process.processed`
  - `process.archived`
  - `process.holding`
  - `process.exceptions`
  - `process.failed`
- `HoldingEntry`
  - 与 domain 结构一致：`id/path/file_name/size_bytes/reason/created_at`
- `ExceptionEntry`
  - 与 domain 结构一致：`id/object_path/kind/evidence_json/status/created_at/resolved_at`
- `PipelineRun`
  - 与 domain 结构一致：`id/file_path/started_at/finished_at/steps_json/status/error`

TypeScript 中 `WatchStatus` 必须扩展为：

- `Unwatched`
- `WantToWatch`
- `Watching`
- `Watched`
- `OnHold`
- `Favorite`

## 前端设计

阶段 4 沿用现有 `App.tsx` 工作台，不做大拆分。设置页增加一个 `daemon` 页签，中文名为“自动管线”。

“自动管线”页签包含四块：

1. 状态栏
   - 展示当前状态、来源目录数量、归档根目录、资源池目录数量、示例元数据源是否启用。
   - 配置不完整时展示具体缺项，例如“缺少归档根目录”。
2. 操作区
   - “刷新状态”
   - “运行一轮”
   - “暂停”
   - “恢复”
   - 所有按钮在对应请求进行中禁用并切换文案。
3. 待人工处理
   - 搁置区展示文件名、原因、大小、路径。
   - 异常队列展示类型、状态、对象路径、简短证据。
   - 异常支持“标记已解决”“忽略”两个命令。
4. 最近运行
   - 展示最近 pipeline run 的文件名、状态、错误、开始/结束时间。
   - `steps_json` 先以简短文本摘要展示，不在阶段 4 做复杂时间线组件。

设置页进入时自动刷新一次 daemon 状态和三组列表；运行一轮、暂停/恢复、解决异常后刷新相关数据。全局状态栏继续显示最近一次操作结果。

## 错误处理

- repository 不可用：命令返回 `"repository is not available"`，前端显示“后端仓库未就绪”。
- 缺 archive root：`get_daemon_status` 返回 `configured=false`，`run_daemon_once` 返回明确错误。
- 元数据源未开启：`run_daemon_once` 返回“示例元数据源未开启，阶段 4 不会用空 scraper 处理真实文件”。
- 文件系统或 SQLite 失败：沿用阶段 2/3 行为，写 `pipeline_runs.status = failed`，命令返回 report 或错误，前端刷新最近运行记录。
- 异常解析失败：`evidence_json` 原样显示短文本，不阻塞列表渲染。

## 测试策略

Rust：

- 给命令层 adapter 写单元测试，验证示例 scraper 能生成 `ScrapedWorkMetadata`。
- 给 `parse_watch_status` 补回归测试，覆盖 `WantToWatch`、`Watching`、`OnHold`。
- 给 repository 列表命令的纯 helper 写测试，使用 `tempfile` SQLite。
- 继续跑 `cargo test --manifest-path src-tauri/Cargo.toml -j 1`。

前端：

- `api.ts` 类型扩展后跑 `npx tsc --noEmit`。
- 给新增的格式化 helper 写 Vitest，例如 holding reason、exception kind、pipeline status 标签。
- 跑 `npm test` 和 `npm run build`。
- 不使用 Codex in-app browser，不做 WebView2 截图验证。视觉检查由用户在自己的桌面浏览器或 Tauri 环境执行。

## 交付标准

阶段 4 完成时应满足：

- 前端能编译，TypeScript 知道所有新观看状态和 daemon DTO。
- 设置页能看到自动管线状态、搁置区、异常队列、最近运行。
- “运行一轮”有完整 loading 和结果反馈，不依赖真实资源即可通过自动化测试验证命令路径。
- `commands.rs` 的观看状态解析不再把阶段 1 新状态降级为 `Unwatched`。
- 所有验证命令通过，且没有启动任何 WebView2/Tauri GUI。

## 自查

- 没有把 HTTP/WebSocket、托盘、自启或真实网络 scraper 偷渡进阶段 4。
- 后端命令边界与阶段 3 的同步 core 匹配，没有设计跨命令持久队列。
- UI 改动被限制在设置页和 API/type/view-model helper，不做无关重构。
- 真实资源不是测试前提；阶段 4 的自动化验证仍可在临时目录中完成。
