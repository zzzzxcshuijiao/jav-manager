# 阶段 6F：一键测试与配置自检设计

## 背景

阶段 6E 已经具备结构化诊断日志和诊断快照，但真实桌面验证仍然需要用户手工准备目录、假视频、aria2 GID、远程 scraper 配置，并且多个可选能力的错误会挤在同一个“最近错误”里。最近实测暴露了三个体验问题：

- 自动归档本身已经成功，但 aria2 或远程 scraper 的可选错误容易让用户误以为“归档失败”。
- 远程 scraper 与旧的“示例元数据源”开关存在理解成本，用户不容易判断当前到底有没有可用元数据源。
- 验证基础链路需要手工造 `inbox/archive` 和假视频，步骤重复且容易漏。

阶段 6F 的目标是提供一个可重复、低风险、面向排障的一键入口：不用真实媒体库、不依赖真实 aria2、不访问真实站点，也能证明“当前程序核心归档链路可用”，并把实际配置问题以分项结果展示出来。

## 目标

- 后端新增自检核心，输出结构化 `pass/warn/fail` 检查项和总状态。
- 新增一键沙盒归档验证：自动创建临时沙盒目录和假视频，运行阶段 3/4 自动管线核心，确认可以归档。
- 新增配置健康检查：控制服务、真实配置目录、元数据源、aria2 设置、远程 scraper 设置、诊断系统。
- 前端“自动管线”页新增“一键自检”按钮和分项结果列表，所有操作有 loading、disabled、status 反馈。
- 修正元数据源可用性判定：远程 scraper 或示例 fallback 可用时，不应被旧 `metadata_provider_enabled` 开关误判为完全不可用。

## 非目标

- 不启动 Tauri GUI、WebView2、`tauri dev`、默认 `cargo run` 或 `media-manager.exe`。
- 不访问真实 JavDB/JavBus/FANZA，不验证 live HTML 兼容性。
- 不创建、暂停、恢复或管理 aria2 下载任务。
- 不自动发现全部 aria2 GID，不做常驻后台轮询或 WebSocket。
- 不修改用户真实来源目录、归档目录、资源池目录，也不向真实媒体库写入沙盒作品。
- 不上传诊断，不导出媒体/NFO/图片内容。

## 用户体验

入口位于“设置 → 自动管线”的状态区域，按钮文案为“**一键自检**”。点击后：

1. 按钮禁用并显示“自检中”。
2. 全局状态显示“正在执行自动管线自检...”。
3. 成功返回后显示总评，例如：
   - `自检通过：沙盒归档链路可用，诊断系统可用。`
   - `自检有警告：沙盒归档可用，但 aria2 启用后没有跟踪 GID。`
   - `自检失败：沙盒归档未完成，请查看失败项。`
4. 面板展示每个检查项的状态、标题、说明和建议动作。

结果不要求用户复制路径。沙盒路径可以在详情中保留，供排障使用。

## 后端架构

新增 `src-tauri/src/self_check.rs`，职责边界如下：

- 定义 DTO：
  - `SelfCheckSeverity`: `Pass | Warn | Fail`
  - `SelfCheckItem`: `id/title/severity/message/action`
  - `SelfCheckReport`: `generated_at/overall/checks/sandbox`
  - `SelfCheckSandboxSummary`: `root/inbox/archive/video_path/archived_path/pipeline_status`
- 提供 `run_pipeline_self_check(app_data_dir, repo, control_service_status, daemon_status, diagnostics)`。
- 提供纯函数检查配置：
  - 控制服务是否运行。
  - 来源目录/归档目录是否配置。
  - 元数据源是否可用。
  - aria2 配置是否合理。
  - 远程 scraper 配置是否合理。
  - 诊断日志是否可写/可读。
- 提供隔离沙盒归档：
  - 在 `app_data_dir/self-check/<timestamp>/` 下创建独立 `library.sqlite`、`inbox/`、`archive/`、`assets/`。
  - 写入 `MMT-001.mp4` 假视频。
  - 使用 `ExamplePipelineScraper` 和禁用 aria2 的临时设置运行 `run_daemon_once_with_transports`。
  - 验证 `archive/MMT-001/MMT-001.mp4` 存在，且 pipeline run 状态为 `archived`。

沙盒验证只使用临时 SQLite 和 app data 下的自检目录，不读写用户真实 SQLite 的作品、文件版本或 pipeline 表。真实配置只用于健康检查，不参与沙盒归档执行。

## 元数据源判定

现有 `MetadataSource` 只有 `example/disabled`，阶段 6F 保持前端兼容，但后端新增内部判定函数：

- `metadata_enabled == true`：示例源可用。
- `remote_scraper.enabled == true` 且至少一个 source enabled：远程源可用。
- `remote_scraper.include_example_fallback == true`：示例 fallback 可用。
- 三者都不可用时才视为“没有可用元数据源”。

`ConfiguredPipelineScrapers` 不再只用旧 `metadata_provider_enabled` 拦截真实文件。它会根据远程设置和示例开关构建实际 sources；若结果为空才报错。

## aria2 检查语义

aria2 是可选能力，自检不连接真实 aria2，不把 aria2 不可用当成总失败：

- `enabled == false`：`pass`，说明当前未启用。
- `enabled == true` 且无 tracked GID：`warn`，建议填写真实完成任务 GID 或关闭轮询。
- `enabled == true` 且 secret 配置存在：`warn`，提示确认 aria2 是否真的配置了 RPC secret。
- host/path/timeout 归一化失败：`fail`。

后续若增加真实 RPC 探测，应作为显式“连接测试”按钮，不放进本阶段默认一键自检。

## 远程 scraper 检查语义

远程 scraper 是可选能力，自检不访问真实站点：

- `enabled == false` 且 `include_example_fallback == true`：`pass`，基础归档可用。
- `enabled == true` 但 enabled source 为 0，且 fallback 关闭：`fail`。
- proxy URL scheme 不是 `http://` 或 `https://`：`warn` 或 `fail`，取决于是否会阻断构建 HTTP client。
- enabled source 的 URL template 缺 `{code}`：`fail`。
- enabled source 存在且配置可归一化：`pass`。

## Tauri 命令与日志

`commands.rs` 新增：

- `run_pipeline_self_check_command`

命令会读取 app data、repository、control service status、daemon status 和 diagnostics writer，调用 `self_check` 模块，返回 `SelfCheckReport`。命令边界写诊断日志：

- `self_check.run started`
- `self_check.run completed`
- `self_check.run failed`

日志只记录统计和结果，不记录 secret、token、cookie。

## 前端设计

`src/api.ts` 新增自检 DTO 和 `api.runPipelineSelfCheck()`。

`src/viewModel.ts` 新增：

- `formatSelfCheckOverall`
- `formatSelfCheckSummary`
- `formatSelfCheckSeverity`

`src/App.tsx` 在自动管线页新增：

- `selfCheckReport` state。
- `selfCheckBusy` state。
- `runPipelineSelfCheck()` handler。
- 自检结果列表，放在诊断日志块之前。

结果用现有面板样式延展，新增少量 CSS：通过/警告/失败三个状态颜色即可。

## 错误处理

- 沙盒目录创建失败：总状态 `fail`，返回可读 action。
- 沙盒 run-once 报错：总状态 `fail`，保留错误消息，诊断日志记录 error。
- 真实配置缺少归档根目录：配置项 `warn`，但不阻塞沙盒归档。
- 真实 aria2/远程 scraper 配置有问题：通常为 `warn`，只有配置无法归一化或无任何元数据源时为 `fail`。
- 诊断 writer 不可用：`warn`，不阻塞沙盒归档。

## 测试策略

后端：

- 新增 `src-tauri/tests/self_check.rs`。
- 使用 `tempdir` 创建 app data 和 SQLite。
- 覆盖沙盒自检成功归档、不污染真实 repo、aria2 enabled 无 GID 警告、无元数据源失败、远程 proxy/template 配置检查。
- 覆盖 `ConfiguredPipelineScrapers` 在远程源或 fallback 可用时不被旧开关误拦。

前端：

- `src/viewModel.test.ts` 覆盖自检总评和 severity 格式化。
- `npx tsc --noEmit` 覆盖 DTO/组件类型。

完整验证：

- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`
- `npm test`
- `npx tsc --noEmit`
- `npm run build`

不运行 Tauri GUI、WebView2、默认 `cargo run` 或 in-app browser。
