# 阶段 3：无头守护核心设计

## 目标

阶段 3 把阶段 2 已验证的 `AutoPipeline` 装进一个可测试的后台核心里。这个核心必须能在 Codex 环境中运行和验证，不启动 Tauri、不启动 WebView2、不创建托盘窗口、不绑定 HTTP/WebSocket，也不依赖真实媒体资源。

它的定位是：先把“后台 worker 的业务内核”做稳，后续阶段再把它暴露给前端或本地控制接口。

## 范围

本阶段要做：

- 从 SQLite settings 读取 daemon 配置：
  - `source_roots`：下载完成后待扫描的入口目录；
  - `archive_root`：自包含媒体库归档目录；
  - `resource_pool_dirs`：本地图片、gif、截图等资源目录。
- 扫描配置的 source roots，找出视频候选文件。
- 在处理前复用阶段 2 的完成判定：
  - 跳过非视频文件；
  - 跳过同名 `.aria2` 控制文件仍存在的文件；
  - 两次快照的大小和 mtime 稳定后才认为可处理。
- 用确定性顺序把完成文件加入内存队列。
- 从队列取文件，调用阶段 2 `AutoPipeline` 处理。
- 维护内存中的 daemon 状态：
  - `idle`
  - `scanning`
  - `processing`
  - `paused`
  - `error`
- 提供纯 Rust 控制门面：
  - `status`
  - `pause`
  - `resume`
  - `scan_now`
  - `process_next`
  - `run_once`
- 提供自包含测试和 CLI-only smoke，全部使用 `tempfile`。

本阶段不做：

- 不做 Tauri GUI、托盘图标、WebView2、`media-manager.exe`。
- 不做长期进程管理、Windows 开机自启、Windows Service。
- 不做 HTTP/WebSocket、token、Origin 校验、端口发现。
- 不做真实 FANZA/JavBus/JavDB scraper，不访问网络。
- 不做 aria2 JSON-RPC。本阶段只保留本地 `.aria2` 文件启发式，RPC 轮询后续再接。
- 不重接前端命令。`commands.rs` 仍是阶段 4 工作，除非阶段 3 测试明确需要抽一个共享 helper。

## 架构

新增一个纯 Rust 模块 `src-tauri/src/daemon.rs`。它只负责编排，不重新实现阶段 2 已有能力。

数据流：

```text
Repository settings
      |
      v
DaemonConfig::load(repo)
      |
      v
HeadlessDaemon::scan_now()
      |
      v
CompletionSnapshot 两次采样 -> CompletedFile
      |
      v
HeadlessDaemon 内存队列
      |
      v
AutoPipeline::process_completed_file()
      |
      v
SQLite: works / file_versions / scrape_jobs / pipeline_runs / holding / exceptions
```

阶段 3 的 daemon core 默认同步执行。测试可以一步一步调用方法并断言精确状态。后续要做常驻进程、定时器、文件监听、HTTP API、托盘，都可以包在这个 core 外面，不改变管线语义。

## 新增模块

创建：

- `src-tauri/src/daemon.rs`

职责：

- 配置加载和校验；
- source roots 扫描；
- 完成文件的双快照采样策略；
- 内存队列管理；
- 暂停、恢复、状态转换；
- 调用 `AutoPipeline`；
- 聚合本轮扫描和处理报告。

禁止事项：

- 不 import `tauri`；
- 不创建窗口；
- 不绑定 socket；
- 不启动永久后台线程；
- 不访问真实外部站点。

## 公开类型

计划新增这些类型。字段可在实现时按 Rust 可见性做微调，但职责不能变。

```rust
pub struct DaemonConfig {
    pub source_roots: Vec<PathBuf>,
    pub archive_root: PathBuf,
    pub asset_roots: Vec<PathBuf>,
}

pub enum DaemonState {
    Idle,
    Scanning,
    Processing,
    Paused,
    Error,
}

pub struct DaemonStatus {
    pub state: DaemonState,
    pub queued: usize,
    pub processed: usize,
    pub last_error: Option<String>,
}

pub struct QueuedFile {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub file_hash: Option<String>,
}

pub struct ScanReport {
    pub scanned_files: usize,
    pub queued_files: usize,
    pub skipped_files: usize,
}

pub struct ProcessReport {
    pub processed: usize,
    pub archived: usize,
    pub holding: usize,
    pub exceptions: usize,
    pub failed: usize,
}

pub struct RunOnceReport {
    pub scan: ScanReport,
    pub process: ProcessReport,
}

pub struct HeadlessDaemon<'a> {
    pub repo: &'a Repository,
    pub config: DaemonConfig,
    pub scrapers: ScrapeCoordinator<'a>,
}
```

## 控制语义

### `DaemonConfig::load(repo)`

- 读取 `source_roots`、`archive_root`、`resource_pool_dirs`。
- `archive_root` 未配置时返回明确错误。
- 允许 `source_roots` 为空，这样状态接口可以表达“daemon 空闲但未配置”。
- 不负责创建目录。扫描时遇到不存在的 root 只跳过。

### `HeadlessDaemon::status()`

- 返回当前内存状态、队列长度、已处理数量、最后错误。
- 不读写 SQLite。

### `pause()`

- 状态切到 `Paused`。
- 不清空队列。
- 暂停时调用 `scan_now` 或 `process_next` 不做文件系统工作。

### `resume()`

- 从 `Paused` 回到 `Idle`。
- 保留已经排队的文件。

### `scan_now()`

- 递归扫描 `source_roots`。
- 只把稳定完成的视频加入队列。
- 尽量用 canonical path 去重，无法 canonicalize 时退回原始 path。
- 只排队，不处理。
- 不写 `works`、`holding`、`exceptions`、`pipeline_runs`。

### `process_next()`

- 从队列弹出一个文件并调用阶段 2 `AutoPipeline`。
- 根据 `PipelineOutcome` 更新计数。
- 如果 `AutoPipeline` 返回 I/O 或写入类操作错误，增加 failed，记录 `last_error`。阶段 2 已负责把证据写进 `pipeline_runs.status = "failed"`。

### `run_once()`

- 先调用 `scan_now`。
- 然后持续处理队列，直到队列为空或 daemon 被暂停。
- 返回 `RunOnceReport`，同时包含本轮扫描排队数和处理计数，方便 CLI smoke 与后续控制接口展示单轮运行摘要。

## 完成采样策略

阶段 2 已经实现 `CompletionSnapshot::capture` 和 `is_heuristically_complete`。阶段 3 只负责“两次采样之间等多久”。

为了测试快且稳定，设计一个可配置策略：

```rust
pub struct CompletionPolicy {
    pub sample_delay: Duration,
}
```

生产默认值可以是非零延迟。测试和 smoke 使用 `Duration::ZERO`。

阶段 3 必须复用阶段 2 的完成判定，不再写第二套完成规则。

## Scraper 边界

阶段 3 由调用方传入 `ScrapeCoordinator<'a>`。

daemon 不知道真实 scraper。测试和 smoke 用确定性的假 scraper：

- 对一个指定番号返回成功；
- 对其他番号返回 `None`，用于触发 `ScrapeFailed`。

这样阶段 3 不需要网络、代理、第三方站点，也不会因为站点波动导致验证失败。

## 存储边界

阶段 3 默认不新增表，除非实现时证明必须新增。

使用现有存储：

- `app_settings`：配置；
- `pipeline_runs`：运行和失败证据；
- 阶段 2 的 `works`、`file_versions`、`scrape_jobs`、`holding`、`exceptions`。

队列只放内存，不持久化。崩溃恢复、durable queue replay 等到控制接口和进程生命周期明确后再做。

## 测试策略

新增测试文件：

- `src-tauri/tests/daemon.rs`

全部测试使用 `tempfile`，不依赖 `H:/`、`G:/`、真实视频库或网络。

必须覆盖：

- 配置能读取 `source_roots`、`archive_root`、`resource_pool_dirs`；
- 未配置 `archive_root` 时返回明确错误；
- scan 会排队稳定视频，跳过 `.aria2` 未完成文件；
- scan 会跳过非视频文件和不存在的 root；
- pause 会阻止 scan/process，resume 后恢复；
- `process_next` 能通过 `AutoPipeline` 归档一个排队文件；
- 刮削失败进入 `exceptions`；
- 无番号文件进入 `holding`；
- 操作失败体现为 failed 状态，不进入内容异常；
- `run_once` 处理混合临时 inbox，返回确定计数。

新增 CLI-only smoke：

- `src-tauri/examples/stage3_daemon_smoke.rs`

smoke 行为：

- 创建临时 inbox/assets/archive/db；
- 写入 Repository settings；
- 创建 `HeadlessDaemon`；
- 执行 `run_once`；
- 打印确定输出：

```text
stage3_daemon_smoke=completed
queued=3
archived=1
holding=1
exceptions=1
failed=0
no_real_resources_required=true
```

验证命令：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1
cargo test --manifest-path src-tauri/Cargo.toml -j 1
cargo run --manifest-path src-tauri/Cargo.toml --example stage3_daemon_smoke -j 1
```

当前 Windows Codex 环境中，完整 `cargo test` 并行编译曾触发页面文件 / mmap 错误 `os error 1455`，所以完整验证默认带 `-j 1`。

## 失败处理

- 缺 `archive_root`：配置错误，不扫描。
- source root 不存在：跳过并计入 skipped，不报错。
- 存在 `.aria2` 控制文件：跳过，不写 SQLite。
- 内容异常：阶段 2 写 `exceptions`，daemon 增加 exception 计数。
- 搁置：阶段 2 写 `holding`，daemon 增加 holding 计数。
- 文件复制、移动、写 NFO、SQLite 写入等操作失败：阶段 2 完成 `pipeline_runs.status = "failed"`，daemon 增加 failed 计数并记录 `last_error`。

## 安全

阶段 3 不引入本地网络服务，所以不实现 token、Origin、端口选择。

但方法级控制门面要保持清晰，后续阶段可以把同一组操作暴露成 Tauri 命令或 loopback HTTP API，并补鉴权。

## 风险与应对

- 队列重复：尽量 canonicalize path 去重，失败时用原始 path 去重。
- 扫描后源文件继续变化：阶段 2 的 `CompletedFile` 快照和 staged move 校验仍会兜底。
- source root 很大：本阶段优先正确性，递归扫描即可；文件系统 watcher 留到后续。
- 长期进程生命周期复杂：本阶段明确不做，因为 Codex 环境不能安全验证托盘、GUI、自启。

## 自审

- 占位扫描：没有未完成标记或未定义任务。
- 范围检查：只覆盖 headless daemon core；HTTP/WebSocket、托盘、自启、真实 scraper 都明确延后。
- 一致性检查：数据流复用阶段 2 `AutoPipeline`、`CompletionSnapshot` 和 Repository settings，没有引入第二套管线模型。
