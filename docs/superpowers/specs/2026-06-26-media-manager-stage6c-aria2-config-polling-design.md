# 阶段 6C aria2 配置与轮询入口设计

## 背景

阶段 6B 已经完成 aria2 JSON-RPC 的底层桥梁：`Aria2Client` 能通过可注入 transport 调用 `aria2.tellStatus`，`Aria2Status::completed_selection` 能从完成任务中提取 selected 且已完成的本地视频文件，`HeadlessDaemon::scan_aria2_gid` 能把给定 GID 的完成文件放入现有自动管线队列。

但 6B 仍停在“调用方必须手里已有 GID 和 client”的骨架层。真实使用时还缺三件事：

- aria2 endpoint / secret / timeout / 是否启用需要持久化到 SQLite；
- 用户或外部回调发现的 GID 需要有一个可配置来源；
- “运行一轮”需要能先轮询配置里的 aria2 GID，再继续原来的目录启发式扫描和入库处理。

阶段 6C 的目标是把这些连接起来，让实际环境可以配置 aria2 并手动触发一次轮询验证，同时仍然保持 Codex 验证自包含。

## 目标

- 在 `app_settings` 中持久化 aria2 配置：启用开关、host、port、path、secret、timeout、poll interval、tracked GIDs。
- 新增 `Aria2Settings` / `Aria2PollReport`，把配置、轮询结果和 UI 展示字段标准化。
- 在 daemon core 中新增“按已配置 GID 轮询一次”的入口，复用阶段 6B `scan_aria2_gid` 内部逻辑和队列去重。
- 修改 command/control-service 的 `run-once` 路径：若 aria2 enabled 且有 tracked GIDs，先 poll aria2，再做原目录扫描，然后处理队列。
- 新增 Tauri commands 和前端 API，用于读取/保存 aria2 配置。
- 在设置页“自动管线”里增加紧凑 aria2 配置区，可保存 endpoint/secret/GID 列表；“运行一轮”作为手动轮询入口。
- 所有测试仍使用 fake transport、临时目录、临时 SQLite，不依赖真实 aria2、真实 H:/ 视频或 WebView2。

## 非目标

- 不启动、停止或管理 aria2 进程。
- 不添加下载任务、不暂停/恢复下载、不做限速和下载队列管理。
- 不实现 WebSocket 通知、`on-bt-download-complete` 回调或常驻后台线程。
- 不自动发现 aria2 active/waiting/stopped 全量任务；本阶段只消费用户配置或外部写入的 tracked GIDs。
- 不把 secret 迁移到系统凭据库；本地 app settings 存储足够支撑当前单机本地使用，后续远程模式再升级。

## 方案对比

### 方案 A：只持久化 endpoint，仍手动传 GID

优点是改动最小；缺点是前端和 daemon 仍不知道该轮询哪些任务，真实环境验证价值有限。

### 方案 B：持久化 endpoint + tracked GIDs + run-once 前置轮询（推荐）

优点是把 6B 的底层能力接进现有“运行一轮”工作流，用户不需要新的后台线程就能验证 aria2 完成任务是否进入自动管线。风险可控，因为 GID 来源明确、轮询次数有限、失败可汇总到状态栏。

### 方案 C：直接实现常驻轮询线程

优点最接近最终产品；缺点是会同时引入生命周期、并发、退避、热更新和退出清理，容易扩大阶段范围，也会让 Codex 验证更复杂。

阶段 6C 采用方案 B。

## 架构

### 配置模型

新增 `Aria2Settings`，字段：

- `enabled: bool`
- `host: String`
- `port: u16`
- `path: String`
- `secret: Option<String>`
- `timeout_ms: u64`
- `poll_interval_secs: u64`
- `tracked_gids: Vec<String>`

默认值：

- disabled；
- host `127.0.0.1`；
- port `6800`；
- path `/jsonrpc`；
- timeout `5000`；
- poll interval `30`；
- tracked GIDs 为空。

`Aria2Settings::endpoint()` 转换到阶段 6B 已有 `Aria2RpcEndpoint`。`Repository` 只负责 JSON 持久化到 `app_settings.aria2_settings`，不新增表。

### 轮询模型

新增 `Aria2PollReport`，字段：

- `enabled`
- `attempted_gids`
- `completed_gids`
- `queued_files`
- `skipped_files`
- `failed_gids`
- `errors`

daemon core 新增：

- `HeadlessDaemon::poll_aria2_once<T: Aria2Transport>(&mut self, settings: &Aria2Settings, transport: T) -> Result<Aria2PollReport>`

行为：

- daemon paused 时返回空 report，不访问 aria2；
- settings disabled 时返回 `enabled=false` 的空 report；
- enabled 但 tracked GIDs 为空时返回 `enabled=true` 且 attempted 为 0；
- 对每个 GID 创建同一个 `Aria2Client`，调用 6B 的 GID 扫描内部逻辑；
- 未完成任务不是错误，计入 attempted，queued 为 0；
- 单个 GID RPC 错误计入 `failed_gids` 和 `errors`，继续处理后续 GID，并把 daemon `last_error` / state 标记为 Error；
- 成功入队的文件继续走现有 queue 去重。

### run-once 集成

`daemon_control::run_daemon_once` 调整为：

1. 仍先检查 paused 和 metadata provider enabled；
2. 从 SQLite 读取 `Aria2Settings`；
3. 创建 `HeadlessDaemon`；
4. 调用 `daemon.poll_aria2_once(&settings, HttpAria2Transport)`；
5. 再调用 `daemon.run_once()` 做目录扫描和队列处理；
6. 把 aria2 report 放进 `RunOnceReport.aria2`，前端摘要显示 aria2 尝试/入队/失败。

这不会影响未配置 aria2 的用户：默认 disabled，原 run-once 行为保持不变。

### 前端配置

设置页“自动管线”增加 aria2 配置区：

- 启用开关；
- host、port、path、timeout、poll interval；
- secret 输入框；
- tracked GIDs 多行文本；
- 保存按钮有 loading/disabled/status 反馈。

“运行一轮”仍是唯一手动执行按钮。aria2 enabled 时，运行摘要会包含 aria2 轮询结果；disabled 时不显示额外噪音。

## 数据流

```text
设置页保存 aria2 配置
  -> configure_aria2_settings Tauri command
  -> Repository::set_aria2_settings(app_settings JSON)

运行一轮
  -> frontend daemon client
  -> REST /v1/run-once 或 command bridge
  -> daemon_control::run_daemon_once
  -> Repository::get_aria2_settings
  -> HeadlessDaemon::poll_aria2_once
  -> Aria2Client::tell_status(gid)
  -> CompletedFile 入队
  -> HeadlessDaemon::run_once
  -> AutoPipeline 归档 / 搁置 / 异常
```

## 错误处理

- 配置解析失败：返回明确错误，不静默重置，避免覆盖用户配置。
- host/path 为空、port 为 0、timeout 为 0：保存时拒绝。
- tracked GIDs 保存时 trim、去空、去重，避免重复轮询。
- 单个 GID RPC 失败：写入 `Aria2PollReport.errors`，daemon `last_error` 可见；不回退为目录扫描，也不吞掉鉴权/网络错误。
- aria2 disabled：不访问网络，不影响目录启发式扫描。
- aria2 enabled 但无 GID：不是错误，方便用户先保存 endpoint。

## 测试策略

Rust：

- `core_behaviour.rs`：`Repository::get_aria2_settings` 默认值、持久化、trim/去重、非法配置拒绝。
- `aria2_rpc.rs`：`Aria2Settings::endpoint` 转换和默认值。
- `daemon.rs`：fake transport 验证 `poll_aria2_once` 对多个 GID 聚合 queued/skipped/errors，paused/disabled 不访问 transport。
- `daemon_control.rs` 或 `control_service.rs`：`run_daemon_once` 在 aria2 enabled 时先 poll tracked GID，再处理队列；disabled 时保持旧行为。

前端：

- `viewModel.test.ts`：运行摘要包含 aria2 轮询结果。
- `daemonClient.test.ts`：`RunOnceReport` shape 扩展不破坏 service-first fallback。
- `tsc` / `npm test` / `npm run build` 覆盖 UI 类型与构建。

完整 gate：

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

## 验收

- 默认配置下，现有 run-once 行为不变。
- 保存 aria2 配置后重开 Repository 能读回同样配置。
- tracked GIDs 会被去空、去重。
- fake aria2 完成任务可以通过 run-once 入队并进入现有自动管线处理。
- aria2 RPC 错误在 report/status 中可见，不会创建内容异常。
- 前端设置页能保存 aria2 配置，并在运行摘要中显示 aria2 轮询结果。
- Codex 验证不依赖真实 aria2 或 WebView2。
