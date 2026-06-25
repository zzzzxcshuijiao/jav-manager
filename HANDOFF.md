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

验证已通过：

- `cargo test --manifest-path src-tauri/Cargo.toml --test data_model`
- `cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline`
- `cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_control -j 1`
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

## 留给后续阶段的项（review 时判定 DEFER，不是缺陷）

- `src-tauri/src/commands.rs` 前端命令路径的 `parse_watch_status` 新变体缺失已在阶段 4 修复，并有 `commands::tests::command_watch_status_parser_accepts_stage1_statuses` 回归测试。
- 几处 `created_at` 只写未读 / `parse_*` 回退不可区分 → 非阻塞清理项，后续碰到相关命令/UI 时再处理。
- 真实网络 scraper（FANZA/JavBus/JavDB 等）尚未接入；阶段 4 只提供无网络 `ExamplePipelineScraper`，用于验证管线串联。
- HTTP/WebSocket、token、Origin、端口发现、托盘、自启、常驻后台线程仍是后续阶段工作。

## 下一步

进入**阶段 5（服务化与真实运行闭环）**：把阶段 4 的同步命令桥演进为真实长期控制层，重点是本地控制接口/托盘/自启/后台生命周期、真实 scraper 接入、异常重试与人工处理闭环。仍然不要在 Codex 会话里启动 Tauri GUI 或 WebView2；Codex 验证继续用 `cargo test --manifest-path src-tauri/Cargo.toml -j 1`、`npm test`、`npx tsc --noEmit`、`npm run build`，视觉检查由用户在自己的桌面环境运行。

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
