# 接手说明 — media-manager 重构

> 给下一个环境的开发者 / codex：从这份文件开始读。

## 项目

media-manager：Tauri(壳) + React(UI) + Rust(核心/SQLite/管线) 的本地媒体库。重构目标 = **下载 → 自动归档 → 整理 → 分类移动 → 浏览播放** 的端到端、尽量零干预管线。分 5 个阶段实施。

## 当前进度

**阶段 1（数据模型扩展）已完成并通过 review。**

**阶段 2（自动管线 Rust 核心）已实现并验证。** 当前工作分支：`codex/stage2-auto-pipeline`。
阶段 2 提供纯 Rust、无 WebView2 的核心：完成文件判定 → 番号识别 → 多源刮削记录 → 自包含归档布局/执行/回滚 → SQLite 入库，以及 holding / exception / pipeline_runs 路由。

**阶段 3（无头守护核心）已实现并验证。**
阶段 3 提供纯 Rust、无 WebView2 的 daemon core：从 SQLite settings 读取 `source_roots` / `archive_root` / `resource_pool_dirs`，扫描完成视频，维护内存队列，支持 `status` / `pause` / `resume` / `scan_now` / `process_next` / `run_once`，并调用阶段 2 `AutoPipeline` 完成归档、搁置、异常路由和失败计数。

**阶段 4（控制接口与前端连线）已实现并验证。**
阶段 4 采用方案 A：新增 Tauri 命令桥和现有设置页“自动管线”页签，把阶段 3 的无头 core 暴露为前端可消费的同步操作：状态、暂停/恢复、运行一轮、搁置区、异常队列、最近运行记录。阶段 4 仍不做 HTTP/WebSocket、托盘、自启或真实网络 scraper。

**阶段 5A（本地控制服务基座）已实现。**
阶段 5A 新增纯 Rust loopback REST 控制服务基座：发现文件、Bearer token、Origin 校验、health、status、pause/resume、run-once、holding、exceptions、resolve exception、runs。服务测试使用随机本地端口、临时 SQLite、临时媒体目录和假文件，不依赖真实资源。阶段 5A 仍不做 WebSocket、托盘、自启、真实网络 scraper 或前端 HTTP client 迁移。

**阶段 5B（前端服务客户端迁移）已实现并验证。**
阶段 5B 新增 discovery Tauri command 与纯 TypeScript daemon client。设置页“自动管线”优先通过 app-data discovery + loopback REST 调用阶段 5A 服务；服务缺失、不可达、鉴权失败或响应形状异常时回退阶段 4 Tauri command bridge。服务返回业务错误时不回退，避免重复执行或掩盖真实服务错误。UI 会显示控制通道，并在暂停状态禁用“运行一轮”。阶段 5B 仍不启动服务进程、不做托盘/自启/WebSocket、不接真实网络 scraper。

**阶段 5C（控制服务宿主与生命周期）已实现并验证。**
阶段 5C 新增 app-owned control service host。Tauri setup 在 app data / SQLite 初始化后会尝试启动 loopback REST 服务，写入 `control-service.json`，并把 handle 存进 `AppState`；启动失败不阻断应用，前端仍可 fallback 到 command bridge。`AppState` 释放时会 shutdown 服务并删除 discovery。控制服务的 metadata provider 开关改为按请求读取 SQLite 当前值，避免启动快照过期。阶段 5C 仍不做托盘/自启/WebSocket/真实网络 scraper。

验证已通过：

- `cargo test --manifest-path src-tauri/Cargo.toml --test data_model`
- `cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline`
- `cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_control -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test control_service_host -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`
- `npm test`
- `npx tsc --noEmit`
- `npm run build`
- `cargo run --manifest-path src-tauri/Cargo.toml --example stage2_smoke -j 1`
- `cargo run --manifest-path src-tauri/Cargo.toml --example stage3_daemon_smoke -j 1`

说明：Windows 当前环境并行 `cargo test` 曾因页面文件不足触发 `os error 1455` / rlib mmap 失败；用 `-j 1` 单作业完整通过。现有 `resource_pool.rs` 有历史 warning（unreachable pattern / unused role），非阶段 2 新增失败。

## 接手步骤（新电脑）

```bash
# 1. 从 bundle 还原仓库（含全部历史 + 分支）
git clone media-manager.bundle media-manager
cd media-manager
git checkout codex/stage2-auto-pipeline

# 2.（可选）解压 VibeCoding 状态：踩坑记录 lessons.md / 进度 tasks.md
tar -xzf media-manager-ai-state.tar.gz   # 在仓库根解出 .ai_state/

# 3. 验证阶段 4（轻量，不需要任何真实媒体资源）
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm ci
npm test
npx tsc --noEmit
npm run build
cargo run --manifest-path src-tauri/Cargo.toml --example stage2_smoke -j 1
cargo run --manifest-path src-tauri/Cargo.toml --example stage3_daemon_smoke -j 1
```

**关键：验证环境是自包含的。** `cargo test`、`npm test`、`stage1_smoke`、`stage2_smoke`、`stage3_daemon_smoke` 都用临时目录(`tempdir`)或纯前端 fixture，**不依赖 H:/ 真实视频或 G:/ 图片库**。开发与测试全程不需要真实媒体资源——它们只在用户实际运行 daemon 处理真实库时才需要。

## 必读文档（按顺序）

1. **`AGENTS.md`** — 硬约束，必须先读：
   - 禁止在非交互 agent 会话里跑 `tauri dev` / 默认 `cargo run` / `media-manager.exe`（启动 WebView2 会崩 host）。验证只用 `cargo test` + `npm run build` + `tsc --noEmit`；阶段 smoke 这种 CLI-only example 可用 `cargo run --example ...`。
   - 编辑含中文的源文件用 `apply_patch` / node:fs，**不要** PowerShell `Set-Content`（GBK 控制台会损坏 UTF-8）。
2. **`docs/superpowers/specs/2026-06-25-media-manager-refactor-design.md`** — 全局设计：架构(后台 daemon + 前端 + SQLite)、自动管线、数据模型、深色沉浸 UI 六页、异常处理、刮削器、服务部署、测试策略、复用 vs 砍掉代码清单、5 阶段划分、后续(aria2 下载集成 / NAS 移动端浏览)。
3. **`docs/superpowers/plans/2026-06-25-media-manager-refactor-stage1-data-model.md`** — 阶段 1 的 plan（已完成，可作 TDD 风格参考）。
4. **`docs/superpowers/plans/2026-06-25-media-manager-refactor-stage2-auto-pipeline.md`** — 阶段 2 的 plan（已完成，含 TDD 步骤与验收）。
5. **`docs/superpowers/specs/2026-06-25-media-manager-stage3-headless-daemon-design.md`** — 阶段 3 无头守护核心设计（已完成）。
6. **`docs/superpowers/plans/2026-06-25-media-manager-refactor-stage3-headless-daemon.md`** — 阶段 3 plan（已完成，含 TDD 步骤与验收）。
7. **`docs/superpowers/specs/2026-06-25-media-manager-stage4-control-ui-design.md`** — 阶段 4 控制接口与前端连线设计（已完成）。
8. **`docs/superpowers/plans/2026-06-25-media-manager-refactor-stage4-control-ui.md`** — 阶段 4 plan（已完成，含 TDD 步骤与验收）。
9. **`docs/superpowers/specs/2026-06-26-media-manager-stage5a-local-control-service-design.md`** — 阶段 5A 本地控制服务基座设计（已完成）。
10. **`docs/superpowers/plans/2026-06-26-media-manager-refactor-stage5a-local-control-service.md`** — 阶段 5A plan（已完成，含 TDD 步骤与验收）。
11. **`docs/superpowers/specs/2026-06-26-media-manager-stage5b-frontend-service-client-design.md`** — 阶段 5B 前端服务客户端迁移设计（已完成）。
12. **`docs/superpowers/plans/2026-06-26-media-manager-refactor-stage5b-frontend-service-client.md`** — 阶段 5B plan（已完成，含 TDD 步骤与验收）。
13. **`docs/superpowers/specs/2026-06-26-media-manager-stage5c-control-service-host-design.md`** — 阶段 5C 控制服务宿主与生命周期设计（已完成）。
14. **`docs/superpowers/plans/2026-06-26-media-manager-refactor-stage5c-control-service-host.md`** — 阶段 5C plan（已完成，含 TDD 步骤与验收）。

## 阶段 1 交付物

- `WatchStatus` +3 变体：`WantToWatch` / `Watching` / `OnHold`
- `works` +2 播放字段：`watch_progress_seconds`、`last_played_at`（由 `set_watch_progress` 写，不走 `upsert_work`）
- 6 张新表 + CRUD：`scrape_jobs`、`exceptions`、`holding`、`pipeline_runs`、`collections`、`work_collections`
- migration 幂等：`CREATE TABLE IF NOT EXISTS` + `ensure_column`，旧库升级不丢数据

## 阶段 2 交付物

- 新增 `src-tauri/src/pipeline.rs`：aria2 完成判定、两次文件快照稳定性判定、番号识别、scraper trait/coordinator、归档布局、staged copy/verify/finalize move、失败回滚、端到端 `AutoPipeline`。
- 新增 pipeline DTO：`CompletedFile`、`ScrapedWorkMetadata`、`ArchiveAsset`、`PipelineStepRecord`、`PipelineOutcome`。
- `scrape_jobs.work_id` 改为 nullable，并新增 `normalized_code` / `object_path` / `pipeline_run_id`，失败刮削不再需要预创建 `works`。
- 新增 `render_scraped_nfo`，成功路径会生成 `<code>.nfo`。
- 成功路径直接入库：`works`、actors、tags、`file_versions`、`scrape_jobs`、`pipeline_runs`。
- 人工路由：无番号 → `holding`；重复 fingerprint / 刮削全失败 → `exceptions`；文件复制/写入/移动失败 → `pipeline_runs.status = failed`，不进内容异常队列。
- 补上阶段 1 defer：`remove_work_from_collection` + `work_collections ON DELETE CASCADE` 回归测试。
- 新增 `src-tauri/tests/auto_pipeline.rs` 与 `src-tauri/examples/stage2_smoke.rs`，全部使用 `tempdir` 假文件。

## 阶段 3 交付物

- 新增 `src-tauri/src/daemon.rs`：`DaemonConfig` / `CompletionPolicy` / `DaemonStatus` / `HeadlessDaemon` / `ScanReport` / `ProcessReport` / `RunOnceReport`。
- `DaemonConfig::load` 从 SQLite settings 读取 `source_roots`、`archive_root`、`resource_pool_dirs`；缺 `archive_root` 时返回明确错误。
- `scan_now` 递归扫描 source roots，跳过不存在 root、非视频文件、`.aria2` 未完成文件，只把完成且稳定的视频加入内存队列。
- 队列按确定性顺序处理，使用 canonical path 去重；重复扫描不会重复排队同一文件。
- `pause` / `resume` / `status` 维护纯内存状态，不启动后台线程、不写 SQLite。
- `process_next` 从队列弹出一个文件，调用阶段 2 `AutoPipeline`，统计 archived / holding / exceptions / failed。
- 操作性失败（例如归档根不可写 / 不是目录）计入 failed，阶段 2 写 `pipeline_runs.status = failed`，不进入内容异常队列。
- `run_once` 执行一次“扫描 → drain 队列”，返回扫描报告 + 处理报告。
- 新增 `src-tauri/tests/daemon.rs` 与 `src-tauri/examples/stage3_daemon_smoke.rs`，全部使用 `tempdir` 假文件和 fake scraper。

## 阶段 4 交付物

- 新增 `src-tauri/src/daemon_control.rs`：`DaemonControlRuntime` / `DaemonControlStatus` / `MetadataSource` / `ExamplePipelineScraper`，作为 Tauri 命令层和阶段 3 core 之间的同步控制门面。
- 新增 `src-tauri/tests/daemon_control.rs`：覆盖示例 scraper、状态摘要、元数据源关闭时拒绝处理真实文件、运行一轮归档、holding / exceptions / pipeline_runs 列表与异常解决。
- `src-tauri/src/commands.rs` 注册新命令：`get_daemon_status`、`pause_daemon`、`resume_daemon`、`run_daemon_once_command`、`list_holding_entries`、`list_exception_entries`、`resolve_exception_entry_command`、`list_pipeline_runs`。
- 修复前端命令路径 `parse_watch_status`：`WantToWatch` / `Watching` / `OnHold` 不再降级为 `Unwatched`。
- `src/api.ts` 补齐 daemon DTO、队列 DTO、运行记录 DTO 和命令 wrapper。
- `src/viewModel.ts` / `src/viewModel.test.ts` 新增自动管线中文格式化 helper，Vitest 覆盖状态、异常、搁置、运行摘要和证据截断。
- `src/App.tsx` 设置页新增“自动管线”页签：状态、运行一轮、暂停/恢复、搁置区、异常队列、最近运行；所有操作都有 loading/disabled/status 反馈。
- `src/styles.css` 新增自动管线面板的列表、指标和移动端布局样式。

## 阶段 5A 交付物

- 新增 `src-tauri/src/control_service.rs`：`ControlServiceConfig` / `ControlServiceDiscovery` / `ControlServiceRuntime` / `ControlServiceHandle`，提供标准库 TCP 实现的 loopback REST 服务基座。
- 服务只允许 `127.0.0.1` / `localhost`，业务 API 要求 `Authorization: Bearer <token>`，未知 Origin 返回 403。
- 服务启动后写 discovery JSON，包含 service、host、port、base_url、token、pid、created_at。
- `GET /health` 无需 token，返回服务活性。
- `/v1/status`、`/v1/pause`、`/v1/resume`、`/v1/run-once`、`/v1/holding`、`/v1/exceptions`、`/v1/exceptions/{id}/resolve`、`/v1/runs` 复用阶段 4 `daemon_control` helper。
- `daemon_control::resolve_exception_entry` 现在返回更新后的 `Exception`，Tauri command 仍可忽略该返回值，REST endpoint 用它回传 resolved 状态。
- 新增 `src-tauri/tests/control_service.rs`：覆盖 loopback 拒绝、discovery、health、token/Origin、status/pause/resume、run-once、队列 API、元数据关闭保护、404 和非法异常 id 400。

## 阶段 5B 交付物

- `control_service.rs` 新增 `CONTROL_SERVICE_DISCOVERY_FILE`、`control_service_discovery_path`、`read_control_service_discovery`，固定 app data 下的 `control-service.json` 发现文件路径。
- `commands.rs` 新增并注册 `get_control_service_discovery`，前端通过 Tauri command 读取 discovery，不直接读文件系统。
- 新增 `src/daemonClient.ts`：服务优先、命令桥兜底的自动管线客户端，覆盖 status、pause/resume、run-once、holding、exceptions、resolve、runs。
- 新增 `src/daemonClient.test.ts`：覆盖 discovery 缺失 fallback、REST 成功、业务错误不 fallback、服务不可达 fallback、异常解决 REST 路由和 fallback command 名称。
- `src/api.ts` 保持原自动管线 API 名称，内部迁移到 `daemonClient`，并暴露 `getControlServiceDiscovery` / `getDaemonControlChannel`。
- `src/viewModel.ts` 新增 `formatDaemonChannel`，`src/App.tsx` 设置页显示“控制通道 本地服务/命令桥/未连接”。
- `src/App.tsx` 在 daemon 状态为 `Paused` 时禁用“运行一轮”，避免 UI 允许触发一个会被暂停态拒绝的操作。

## 阶段 5C 交付物

- 新增 `src-tauri/src/control_service_host.rs`：`ControlServiceHostStatus`、`build_control_service_config`、`start_control_service_host`、`control_service_host_status`，负责从 app data 启动 app-owned loopback 服务。
- `src-tauri/src/lib.rs` 导出 `control_service_host`。
- `ControlServiceHandle` 现在记录 discovery path，提供 `host()` / `discovery_path()` accessor，`shutdown()` 会删除 discovery 文件。
- `ControlServiceRuntime` 在 `/v1/status` 和 `/v1/run-once` 时动态读取 SQLite 中的 `metadata_provider_enabled`，读失败才回退启动默认值。
- `AppState` 新增 `control_service` 和 `control_service_error`，Tauri setup 成功打开 app data SQLite 后尝试启动服务；启动失败只记录错误，不阻断应用启动。
- `AppState::drop` 会 shutdown 控制服务，释放 listener 并清理 discovery。
- `commands.rs` 新增并注册 `get_control_service_host_status`，用于诊断宿主是否运行、端口、discovery 路径和最近启动错误。
- `src/api.ts` 新增 `ControlServiceHostStatus` 和 `getControlServiceHostStatus()`。
- 新增 `src-tauri/tests/control_service_host.rs`，覆盖 host config、启动、health、status、shutdown 清理 discovery。
- `src-tauri/tests/control_service.rs` 新增动态 metadata provider 回归，并修正 run-once fixture 以 SQLite 设置为准。

## 留给后续阶段的项（review 时判定 DEFER，不是缺陷）

- `src-tauri/src/commands.rs` 前端命令路径的 `parse_watch_status` 新变体缺失已在阶段 4 修复，并有 `commands::tests::command_watch_status_parser_accepts_stage1_statuses` 回归测试。
- 几处 `created_at` 只写未读 / `parse_*` 回退不可区分 → 非阻塞清理项，后续碰到相关命令/UI 时再处理。
- 真实网络 scraper（FANZA/JavBus/JavDB 等）尚未接入；阶段 4 只提供无网络 `ExamplePipelineScraper`，用于验证管线串联。
- WebSocket、托盘、自启、常驻后台线程仍是后续阶段工作。

## 下一步

如果继续做 Codex 可编码工作，建议下一步在两条路线中选一条：**阶段 6A 真实 scraper adapter 骨架**（先做 provider trait 的 HTTP client 注入、HTML/JSON 解析单测、失败语义，不依赖真实网络）或 **阶段 6B aria2 RPC 集成骨架**（先做 RPC DTO、轮询/完成事件转换、假 HTTP 测试）。如果先做实际环境验证，重点检查设置页“自动管线”的控制通道是否变为“本地服务”，以及 app data 下是否生成 `control-service.json`。仍然不要在 Codex 会话里启动 Tauri GUI 或 WebView2；Codex 验证继续用 `cargo test --manifest-path src-tauri/Cargo.toml -j 1`、`npm test`、`npx tsc --noEmit`、`npm run build`，视觉检查由用户在自己的交互桌面环境运行。

## 阶段 1 commit 清单

```
495f99e test: stage 1 end-to-end smoke example
82c4030 fix: remove unused Collection import from data_model test
6add504 feat(storage): collections and work_collections many-to-many
1c64fcc feat(storage): pipeline_runs table and CRUD
c9fed38 feat(storage): holding table and CRUD
726958c feat(storage): exceptions table and CRUD
eda74c5 feat(storage): scrape_jobs table and CRUD
1df3adb feat(storage): persist watch_progress_seconds and last_played_at on works
9369fb7 feat(domain): extend WatchStatus with WantToWatch/Watching/OnHold
```
（其下 `da8417d` 是重构 base：固化了之前 NFO 会话的工作。）
