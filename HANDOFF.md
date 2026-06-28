# 接手说明 — media-manager 重构

> 给下一个环境的开发者 / codex：从这份文件开始读。

## 项目

media-manager：Tauri(壳) + React(UI) + Rust(核心/SQLite/管线) 的本地媒体库。重构目标 = **下载 → 自动归档 → 整理 → 分类移动 → 浏览播放** 的端到端、尽量零干预管线。原定 1~5 阶段已闭环，当前继续推进后续可编码阶段。

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

**阶段 6A（远程 scraper adapter 骨架）已实现并验证。**
阶段 6A 新增远程 metadata scraper adapter 骨架：HTTP client 注入、URL 模板和 percent encoding、HTML JSON-LD Movie/VideoObject fixture parser、`RemoteMetadata` 中间模型，以及旧 ingest `MetadataProvider` / 自动管线 `ScraperSource` 双接口转换。验证只使用 fake HTTP client 和 fixture，不访问真实 FANZA/JavBus/JavDB，也不改变 `ExamplePipelineScraper` 默认路径。阶段 6A 仍不做真实 HTTP client、站点特定 parser、代理/cookie/登录/重试、封面下载或 UI 设置。

**阶段 6B（aria2 RPC 集成骨架）已实现并验证。**
阶段 6B 新增纯 Rust aria2 JSON-RPC client、fake transport 测试、标准库 HTTP POST transport、完成任务 selected 视频文件提取，以及 daemon `scan_aria2_gid` 显式入口。验证使用 fake transport、临时 TCP server 和临时媒体文件，不依赖真实 aria2 进程、真实下载任务或真实媒体盘。阶段 6B 仍不做 aria2 配置 UI、持久化 endpoint、GID 来源、常驻轮询、WebSocket 通知、下载任务管理或真实网络 scraper。

**阶段 6C（aria2 配置与轮询入口）已实现并验证。**
阶段 6C 新增持久化 aria2 RPC settings、tracked GIDs、run-once 前置 aria2 轮询、poll report 聚合、Tauri 设置命令、前端“自动管线”里的 aria2 配置表单，以及 run-once 摘要里的 aria2 统计。验证仍使用 fake transport、临时 TCP server、临时媒体文件和纯前端 fixture，不依赖真实 aria2 进程、真实下载任务或真实媒体盘。阶段 6C 仍不做 aria2 进程管理、下载任务创建/暂停/恢复、常驻后台轮询、WebSocket/callback 或真实网络 scraper。

**阶段 6D（真实 scraper adapter 可验证接入）已实现并验证。**
阶段 6D 新增远程 scraper settings、真实 HTTPS-capable HTTP client、javdb/javbus/fanza adapter registry、checked-in HTML fixtures、run-once 远程 source 接入、Tauri 设置命令、前端“自动管线”里的远程刮削器配置表单。验证使用 fake client、本地 TCP server、临时 SQLite 和 fixture HTML，不访问真实 scraper 站点，不处理登录/cookie/验证码/反爬，也不下载封面。默认远程 scraper disabled，example fallback 默认保留；用户显式启用远程 source 后，run-once 会优先尝试远程 metadata，再按配置 fallback。

**阶段 6E（日志与诊断系统）已实现并验证。**
阶段 6E 新增本地结构化 JSONL 诊断日志、日志 tail、简单轮转、脱敏诊断快照导出、Tauri 诊断命令、关键命令边界事件记录，以及前端“自动管线”里的诊断日志/导出诊断块。日志写入 best-effort，不阻断保存配置或 run-once；诊断导出默认写到 app data 的 `diagnostics/diagnostics-*.json`，日志位于 app data 的 `logs/media-manager.jsonl`。验证使用临时目录、临时 SQLite 和假数据，不依赖真实资源；导出会脱敏 secret/token/password/authorization/cookie/proxy credentials，不导出媒体、NFO、封面或缩略图内容。

**阶段 6F（一键测试与配置自检）已实现并验证。**
阶段 6F 新增本地一键自检：后端 `self_check` 模块会在 app data 下创建隔离 `self-check/<timestamp>/` 沙盒、独立 SQLite、假视频和临时归档目录，运行自动管线核心确认 `MMT-001.mp4` 可归档；同时输出控制服务、真实目录、元数据源、aria2、远程 scraper、诊断系统的 pass/warn/fail 配置健康检查。前端“自动管线”页新增“一键自检”按钮和结果面板，所有操作有 loading/disabled/status 反馈。自检不连接真实 aria2，不访问真实 scraper 站点，不创建下载任务，不写真实媒体库。

**阶段 7A（存量资源盘点与整理预览）已实现并验证。**
阶段 7A 从下载入口转向用户当前真实痛点：已有视频、NFO、图片、GIF 和信息文件散乱在多个目录/盘。新增 `inventory` 只读扫描器，可扫描多个根目录，按番号聚合视频/NFO/poster/fanart/thumb/screenshot/GIF/其他资源，生成 `archive_root/CODE/` 目标路径预览，标记缺 NFO、缺视频、多视频、多 NFO、疑似重复、番号冲突、NFO 解析失败、目标已存在和目标重复等风险。扫描会对重复/父子 root 下的同一文件去重，避免假多视频冲突；孤儿资源可在“全部/孤儿资源”筛选下直接看到路径和告警，明细最多返回 1000 条但 summary 保留全量计数。前端“自动管线”页新增“存量整理预览”面板，支持多根目录输入、全状态过滤、summary、孤儿资源、资源证据、warnings 和 action target 预览。阶段 7A 不写 SQLite、不创建归档目录、不移动/复制/删除真实资源。

**阶段 7B（配对规则与人工复核预览）已实现并验证。** 当前工作分支：`codex/stage7b-pairing-rules`。
阶段 7B 在 7A 只读盘点结果上增加可解释判定层：每个作品新增 `resolution`（复核队列、主视频、主 NFO、推荐动作、理由、风险、阻断原因、置信度）和 `resource_roles`（主/副/疑似重复视频、主/副 NFO、poster/fanart/thumb/screenshot/GIF/image/other）。summary 新增 `auto_ready` / `needs_review` / `blocked`，前端新增可自动整理、需人工确认、阻断、素材候选、状态和孤儿筛选；盘点页可单独填写“整理目标目录”，并展示配对建议和资源角色。新增 `export_inventory_report_command`，把当前盘点 report JSON 导出到 app data 的 `inventory-reports/` 目录，便于真实环境反馈。阶段 7B 仍然只读：不写 SQLite inventory task、不创建目标目录、不移动/复制/删除媒体/NFO/图片。

**阶段 7B.1（盘点执行计划安全化）已实现并验证。** 当前工作分支：`codex/stage7b1-execution-plan`。
阶段 7B.1 修正真实盘点 JSON 暴露出的关键边界：`InventoryWorkPreview.actions` 是候选动作预览，可能包含多 NFO、多 poster、多截图等 `target_duplicate` 诊断信息，不能直接给 7C 执行。每个作品的 `resolution` 新增 `execution_plan`，其中 `actions` 只包含主视频、主 NFO，以及每个目标路径唯一的已选择资源动作；`ready` / `conflicts` / `notes` 用于判断是否能进入自动执行。`auto_ready` 现在以 `execution_plan.ready == true` 为准；多 NFO / 多 poster 可通过安全计划去重，多视频/疑似重复视频仍进入复核，目标已存在/番号冲突/NFO 解析失败进入阻断。前端详情页新增“安全执行计划”，原始动作区改名为“候选动作预览”。阶段 7B.1 仍然只读：不写 SQLite inventory task、不创建目标目录、不移动/复制/删除媒体/NFO/图片。

**阶段 7C（存量复制整理执行）已实现并验证。** 当前工作分支：`codex/stage7c-inventory-execution`。
阶段 7C 将 7B.1 的安全执行计划落地为真实文件复制，但仍是保守的 copy-only：只复制 `resolution.execution_plan.actions` 到整理目标目录，不移动、不删除源文件，不覆盖已存在目标。后端新增 `inventory_execution` core 和 `execute_inventory_plan` command；执行前会拒绝非 auto-ready、截断 full report、缺源文件、目标已存在、批内重复目标、动作冲突、目标路径越界，以及 Windows junction / Unix symlink 指向 archive root 外的目标父目录。复制使用 create-new 临时文件、SHA-256 校验和 no-clobber 落盘；失败回滚前会验证 bytes + SHA-256，避免误删外部替换文件。前端盘点页新增“复制整理”按钮、确认弹窗、loading/disabled、状态摘要和最近执行日志；截断报告会禁用复制并提示缩小入口目录后重新盘点。阶段 7C 不写 SQLite inventory task，不执行人工复核项，不做移动/删除源文件。

**阶段 7D（低空间存量整理执行）已实现并验证。** 当前工作分支：`codex/stage7d-low-space-execution`。
阶段 7D 把 7C 的默认真实执行入口改成适合大视频库的低空间模式：前端“复制整理”改为“低空间整理”，显式调用 `low_space`；视频动作不复制内容，而是在同一文件系统内对源视频创建硬链接；NFO、poster、fanart、thumb、screenshot、GIF、image、other 等小资源继续走 7C 的临时文件 + no-clobber 复制流程。执行报告新增 `linked_actions` / `bytes_linked` 与 `linked` 日志状态，摘要会区分“硬链接视频”和“复制小文件”。硬链接失败不会 fallback 成复制视频，避免真实媒体库意外占满磁盘；源文件仍不删除、不移动，不写 SQLite，不执行人工复核项。

**M1（存量资源集中迁移）设计已固化。** 当前工作分支：`codex/m1-inventory-centralized-migration`。
M1 是用户真实需求的主线修正：不再把默认目标停留在复制或硬链接，而是把散落在多个目录的视频、NFO、图片、GIF 等资源，按既定 `archive_root/CODE/` 布局直接集中迁移。设计要求同卷直接移动；跨卷逐文件复制、校验、提交目标后删除源文件；每个跨卷文件迁移前检查目标卷剩余空间；目标已存在永不覆盖；源文件只在目标校验成功后删除。M1 仍以 `resolution.execution_plan.actions` 为唯一自动执行输入，不执行需人工复核项、不处理孤儿资源、不写 SQLite。下一步应先写 TDD 实施计划，再进入后端执行器和前端入口开发。

验证已通过：

- `cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory_preview -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test data_model`
- `cargo test --manifest-path src-tauri/Cargo.toml --test aria2_rpc -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test auto_pipeline`
- `cargo test --manifest-path src-tauri/Cargo.toml --test daemon -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_control -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test control_service_host -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test remote_scraper -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test diagnostics -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml --test self_check -j 1`
- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`
- `npm test`
- `npx tsc --noEmit`
- `npm run build`
- `cargo run --manifest-path src-tauri/Cargo.toml --example stage2_smoke -j 1`
- `cargo run --manifest-path src-tauri/Cargo.toml --example stage3_daemon_smoke -j 1`

阶段 7D 最终额外通过：

- `cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1`（8 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`（245 tests）
- `npm test`（65 tests）
- `npx tsc --noEmit`
- `npm run build`

阶段 7A 最终 reviewer follow-up 之后额外通过：

- `cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1`（15 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory_preview -j 1`（5 tests）
- `npx tsc --noEmit`
- `npm test`（54 tests）
- `npm run build`

orphan 明细上限 follow-up 之后额外通过：

- `npx tsc --noEmit`
- `npm test -- src/viewModel.test.ts`（45 tests）
- `npm test -- src/daemonClient.test.ts src/viewModel.test.ts`（52 tests）
- `npm run build`

阶段 7B 通过：

- `cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1`（24 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory_export_command_writes_report_json_under_app_data -j 1`（1 test）
- `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory_preview -j 1`（5 tests）
- `npm test -- src/App.inventory.test.tsx`（2 tests）
- `npm test`（60 tests）
- `npx tsc --noEmit`
- `npm run build`
- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`（234 tests）

阶段 7B.1 通过：

- `cargo test --manifest-path src-tauri/Cargo.toml --test inventory execution_plan -j 1`（3 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1`（28 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory_preview -j 1`（5 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory_export_command_writes_report_json_under_app_data -j 1`（1 test）
- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`（238 tests）
- `npm test -- src/viewModel.test.ts src/App.inventory.test.tsx`（52 tests）
- `npm test`（61 tests）
- `npx tsc --noEmit`
- `npm run build`

阶段 7C 通过：

- `cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1`（7 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1`（28 tests）
- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`（245 tests）
- `npm test -- src/viewModel.test.ts src/App.inventory.test.tsx`（55 tests）
- `npm test`（65 tests）
- `npx tsc --noEmit`
- `npm run build`

说明：Windows 当前环境并行 `cargo test` 曾因页面文件不足触发 `os error 1455` / rlib mmap 失败；用 `-j 1` 单作业曾完整通过。阶段 7A follow-up 清理无关 `cargo fmt` 噪音后复跑完整 `cargo test --manifest-path src-tauri/Cargo.toml -j 1`，当前会话在编译 examples/tests 时再次触发 `os error 1455`。orphan 明细上限修复后，focused inventory Rust re-run 也被同一 `os error 1455` 挡在编译阶段；full `npm test` 被 child Windows PowerShell OOM/internal loading error 挡在 `scripts/collect-feedback.test.ts`。这些失败未进入业务断言。新环境接手时建议先释放/扩展页面文件，再重跑完整 `cargo test --manifest-path src-tauri/Cargo.toml -j 1` 和 `npm test`。现有 `resource_pool.rs` 有历史 warning（unreachable pattern / unused role），非阶段 7A 新增失败。

## 接手步骤（新电脑）

```bash
# 1. 从 bundle 还原仓库（含全部历史 + 分支）
git clone media-manager.bundle media-manager
cd media-manager
git checkout codex/stage2-auto-pipeline

# 2.（可选）解压 VibeCoding 状态：踩坑记录 lessons.md / 进度 tasks.md
tar -xzf media-manager-ai-state.tar.gz   # 在仓库根解出 .ai_state/

# 3. 验证当前分支（轻量，不需要任何真实媒体资源）
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
15. **`docs/superpowers/specs/2026-06-26-media-manager-stage6a-remote-scraper-adapter-design.md`** — 阶段 6A 远程 scraper adapter 骨架设计（已完成）。
16. **`docs/superpowers/plans/2026-06-26-media-manager-refactor-stage6a-remote-scraper-adapter.md`** — 阶段 6A plan（已完成，含 TDD 步骤与验收）。
17. **`docs/superpowers/specs/2026-06-26-media-manager-stage6b-aria2-rpc-design.md`** — 阶段 6B aria2 RPC 集成骨架设计（已完成）。
18. **`docs/superpowers/plans/2026-06-26-media-manager-refactor-stage6b-aria2-rpc.md`** — 阶段 6B plan（已完成，含 TDD 步骤与验收）。
19. **`docs/superpowers/specs/2026-06-26-media-manager-stage6c-aria2-config-polling-design.md`** — 阶段 6C aria2 配置与轮询入口设计（已完成）。
20. **`docs/superpowers/plans/2026-06-26-media-manager-refactor-stage6c-aria2-config-polling.md`** — 阶段 6C plan（已完成，含 TDD 步骤与验收）。
21. **`docs/superpowers/specs/2026-06-26-media-manager-stage6d-real-scraper-adapter-design.md`** — 阶段 6D 真实 scraper adapter 可验证接入设计（已完成）。
22. **`docs/superpowers/plans/2026-06-26-media-manager-refactor-stage6d-real-scraper-adapter.md`** — 阶段 6D plan（已完成，含 TDD 步骤与验收）。
23. **`docs/superpowers/specs/2026-06-26-media-manager-stage6e-logging-diagnostics-design.md`** — 阶段 6E 日志与诊断系统设计（已完成）。
24. **`docs/superpowers/plans/2026-06-26-media-manager-refactor-stage6e-logging-diagnostics.md`** — 阶段 6E plan（已完成，含 TDD 步骤与验收）。
25. **`docs/superpowers/specs/2026-06-27-media-manager-stage6f-self-check-design.md`** — 阶段 6F 一键测试与配置自检设计（已完成）。
26. **`docs/superpowers/plans/2026-06-27-media-manager-stage6f-self-check.md`** — 阶段 6F plan（已完成，含 TDD 步骤与验收）。
27. **`docs/superpowers/specs/2026-06-27-media-manager-stage7a-inventory-preview-design.md`** — 阶段 7A 存量资源盘点与整理预览设计（已完成）。
28. **`docs/superpowers/plans/2026-06-27-media-manager-stage7a-inventory-preview.md`** — 阶段 7A plan（已完成，含 TDD 步骤、评审记录和验收）。
29. **`docs/superpowers/specs/2026-06-28-media-manager-stage7b-pairing-rules-design.md`** — 阶段 7B 配对规则与人工复核预览设计（已完成）。
30. **`docs/superpowers/plans/2026-06-28-media-manager-stage7b-pairing-rules.md`** — 阶段 7B plan（已完成，含 TDD 步骤、评审记录和验收）。
31. **`docs/superpowers/specs/2026-06-28-media-manager-stage7b1-safe-execution-plan-design.md`** — 阶段 7B.1 盘点执行计划安全化设计（已完成）。
32. **`docs/superpowers/plans/2026-06-28-media-manager-stage7b1-safe-execution-plan.md`** — 阶段 7B.1 plan（已完成，含 TDD 步骤与验收）。
33. **`docs/superpowers/specs/2026-06-28-media-manager-stage7c-inventory-copy-execution-design.md`** — 阶段 7C 存量复制整理执行设计（已完成）。
34. **`docs/superpowers/plans/2026-06-28-media-manager-stage7c-inventory-copy-execution.md`** — 阶段 7C plan（已完成，含 TDD 步骤与验收）。
35. **`docs/superpowers/specs/2026-06-28-media-manager-stage7d-low-space-inventory-execution-design.md`** — 阶段 7D 低空间存量整理执行设计（已完成）。
36. **`docs/superpowers/plans/2026-06-28-media-manager-stage7d-low-space-inventory-execution.md`** — 阶段 7D plan（已完成，含 TDD 步骤与验收）。
37. **`docs/superpowers/specs/2026-06-28-media-manager-m1-centralized-inventory-migration-design.md`** — M1 存量资源集中迁移设计（当前主线，下一步写实施计划）。

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

## 阶段 6B 交付物

- 新增 `src-tauri/src/aria2.rs`：`Aria2RpcEndpoint`、`Aria2Transport`、`HttpAria2Transport`、`Aria2Client`、`Aria2Status`、`Aria2File`、`Aria2CompletedSelection`。
- `aria2.tellStatus` 请求会把 secret 作为 `params[0] = "token:<secret>"`，并请求 `gid`、`status`、`totalLength`、`completedLength`、`files`。
- JSON-RPC `error` 会返回明确错误；缺失 result、非法数字字段、HTTP 非 2xx 也会报错。
- `Aria2Status::completed_selection` 只提取 task complete 且 file selected、file completed、本地存在、是视频文件的 `CompletedFile`。
- `HttpAria2Transport` 使用标准库 `TcpStream` POST `/jsonrpc`，测试用临时 TCP server 验证请求形状，不依赖真实 aria2。
- `HeadlessDaemon::scan_aria2_gid` 作为显式入口，复用现有 queue / known key 去重；未完成 task 返回空 `ScanReport`，不写 SQLite。
- 新增 `src-tauri/tests/aria2_rpc.rs`，覆盖 secret 参数、字符串字段解析、JSON-RPC error、完成文件筛选和 HTTP transport。
- `src-tauri/tests/daemon.rs` 新增 aria2 GID 扫描回归，覆盖完成入队、重复不重复、未完成不报错。

## 阶段 6C 交付物

- 新增 `Aria2Settings`：默认 loopback endpoint、enabled、secret、timeout、poll interval、tracked GIDs，归一化会 trim、补 `/jsonrpc` 路径前缀、去空 secret、去空/去重 GID。
- `Repository::set_aria2_settings` / `get_aria2_settings` 把 aria2 设置作为 `app_settings.aria2_settings` JSON 持久化；未配置时返回安全默认值。
- 新增 `Aria2PollReport`，记录 enabled、attempted/completed GIDs、queued/skipped files、failed GIDs 和逐 GID error。
- `HeadlessDaemon::poll_aria2_once` 会在单次 run-once 范围内轮询配置的 GID，复用 `Aria2Status::completed_selection` 和现有 daemon queue / known key 去重；单个 GID 失败会记录 report error 并继续处理其他 GID。
- `run_daemon_once` 在目录扫描前读取 SQLite aria2 settings 并调用 `poll_aria2_once`；测试通过 injectable transport 覆盖，不需要真实 aria2 进程。
- `commands.rs` 新增并注册 `configure_aria2_settings` / `get_aria2_settings`，Tauri setup 会把 SQLite settings 预载入 `AppState`。
- `src/api.ts` 新增 `Aria2Settings` / `Aria2PollReport` / `DaemonRunOnceReport.aria2`，并暴露 get/save aria2 settings command wrapper。
- 设置页“自动管线”新增 aria2 配置块：启用、host、port、RPC path、secret、timeout、poll interval、tracked GIDs，保存按钮有 loading/disabled/status 反馈。
- `summarizeRunOnceReport` 会在 run-once 摘要前置 aria2 统计。
- 新增/更新测试：`aria2_rpc`、`core_behaviour`、`daemon`、`daemon_control`、`commands` unit、`daemonClient.test.ts`、`viewModel.test.ts`。

## 阶段 6D 交付物

- 新增 `RemoteScraperSettings` / `RemoteScraperSourceSettings`：默认 disabled、timeout、User-Agent、proxy URL、example fallback、内置 javdb/javbus/fanza source；配置作为 `app_settings.remote_scraper_settings` JSON 持久化。
- 新增 `HttpRemoteMetadataClient`，用 `ureq` 支持真实 HTTP/HTTPS GET、User-Agent、timeout、可选 proxy、HTTP status / network / transport 错误分类；404/410 会转为 scraper miss，其他错误记录为 scrape job 失败原因。
- 新增 source parser dispatch：`parse_javdb_metadata`、`parse_javbus_metadata`、`parse_fanza_metadata` 暂复用 JSON-LD fixture parser，后续站点结构变化可只改对应 adapter。
- 新增 `src-tauri/tests/fixtures/remote_scraper/*.html`，所有 parser/HTTP 测试使用 fake client、本地 TCP server 和 checked-in fixture，不访问真实站点。
- 新增 `build_remote_scraper_sources` 与 `ConfiguredPipelineScrapers`；run-once 会按 SQLite settings 构建远程 source，远程 source enabled 时排在 example fallback 前面。
- `commands.rs` 新增并注册 `configure_remote_scraper_settings` / `get_remote_scraper_settings`，Tauri setup 会把 SQLite settings 预载入 `AppState`。
- `src/api.ts` 新增远程 scraper DTO 和 command wrapper；`src/viewModel.ts` 新增 `formatRemoteScraperSettingsSummary`。
- 设置页“自动管线”新增远程刮削器配置块：总开关、User-Agent、timeout、proxy URL、example fallback、source enable/template/confidence，保存按钮有 loading/disabled/status 反馈。
- 新增/更新测试：`remote_scraper`、`core_behaviour`、`daemon_control`、`commands` unit、`viewModel.test.ts`；完整 gate 通过 `cargo test`、`npm test`、`npx tsc --noEmit`、`npm run build`。

## 阶段 6E 交付物

- 新增 `src-tauri/src/diagnostics.rs`：`DiagnosticLevel`、`DiagnosticLogEntry`、`DiagnosticsWriter`、`DiagnosticSnapshot`、`DiagnosticExportResult`、redaction helper、snapshot builder 和 export helper。
- `DiagnosticsWriter` 写入 app data `logs/media-manager.jsonl`，支持 tail 读取、最大 200 条返回、简单文件轮转和上下文脱敏。
- 诊断快照汇总控制服务状态、daemon 状态、脱敏 settings、最近 pipeline runs、scrape jobs、open exceptions、holding items 和最近日志；导出为 app data `diagnostics/diagnostics-*.json`。
- 快照导出会脱敏 structured context、settings proxy、业务行错误字段、exception evidence JSON、control/daemon last_error 中的 secret/token/password/authorization/cookie/proxy credentials。
- `AppState` 新增 optional diagnostics writer；Tauri setup 初始化 writer 并记录 app/setup/control-service 事件，失败不阻断应用。
- `commands.rs` 新增并注册 `get_diagnostic_log_tail`、`export_diagnostics_snapshot_command`，并记录 aria2 settings、remote scraper settings、pause/resume、run-once start/success/failure 事件。
- `src/api.ts` 新增诊断 DTO 和 API wrapper；`src/viewModel.ts` 新增诊断日志/导出摘要格式化 helper。
- 设置页“自动管线”新增诊断块：刷新日志、导出诊断、最近日志列表，按钮有 loading/disabled/status 反馈。
- 新增 `src-tauri/tests/diagnostics.rs`，覆盖 JSONL append/tail/rotation/redaction、快照导出、业务行敏感字段脱敏；更新 `commands` unit 与 `viewModel.test.ts`。
- 完整 gate 通过 `cargo test --manifest-path src-tauri/Cargo.toml -j 1`、`npm test`、`npx tsc --noEmit`、`npm run build`。

## 阶段 6F 交付物

- 新增 `src-tauri/src/self_check.rs`：`SelfCheckSeverity`、`SelfCheckItem`、`SelfCheckSandboxSummary`、`SelfCheckOverall`、`SelfCheckReport`、配置健康检查和沙盒归档 runner。
- 沙盒自检在 app data 下创建隔离 `self-check/<timestamp>/`、独立 `library.sqlite`、`inbox/archive/assets` 和 `MMT-001.mp4`，使用 fake aria2 transport 与 fake remote client 运行自动管线，不读写真实媒体库作品表。
- `daemon_control::metadata_source_available` 统一元数据源可用性判定：旧示例开关、远程 enabled source、example fallback 任一可用即认为自动管线有元数据源。
- `ConfiguredPipelineScrapers` 不再只因旧 `metadata_provider_enabled=false` 拒绝处理；只有远程 source 和 example fallback 都不可用时才报“元数据源未开启”。
- `commands.rs` 新增并注册 `run_pipeline_self_check_command`，命令边界记录 `self_check.run` started/completed/failed 诊断事件。
- `src/api.ts` 新增自检 DTO 和 `runPipelineSelfCheck()`；`src/viewModel.ts` 新增自检 severity/overall/summary 中文格式化。
- 设置页“自动管线”新增“一键自检”按钮和结果面板，展示总评、沙盒路径、每项 pass/warn/fail 和建议动作。
- 新增/更新测试：`src-tauri/tests/self_check.rs`、`src-tauri/tests/daemon_control.rs`、`src-tauri/tests/control_service.rs`、`commands` unit、`src/viewModel.test.ts`。
- 完整 gate 通过 `cargo test --manifest-path src-tauri/Cargo.toml -j 1`、`npm test`、`npx tsc --noEmit`、`npm run build`。

## 阶段 7A 交付物

- 新增 `src-tauri/src/inventory.rs`：Inventory DTO、只读多根目录扫描、资源类型识别、番号 evidence、按番号聚合、状态标签、目标路径 preview、summary 和 truncation。
- `inventory` 识别视频、NFO、poster、fanart、thumb、screenshot、GIF、其他图片/文件；NFO parse/read 失败不会中断扫描，会保留资源并标记 `nfo_parse_error`。
- 目标路径只做预览：主视频 `CODE.ext`、多视频 `CODE-vN.ext`、NFO `CODE.nfo`、poster/fanart/thumb 固定名、screenshots/gifs/images 子目录；不会创建目录或移动文件。
- 目标冲突会标记 `target_exists` 和 `target_duplicate`，组合冲突不会互相覆盖；Windows 下 `.JPG/.jpg` 这类大小写冲突也会被识别。
- 重复/重叠 roots 会按 canonical path 去重；同一番号组内同尺寸多视频会打 `duplicate_candidate`，供 7B 冲突处理继续细化。
- 作品详情和孤儿资源明细各最多返回 1000 条；summary 仍按完整扫描结果计算，过量时 `truncated=true`。
- `commands.rs` 新增并注册 `preview_inventory`，递归扫描通过 `spawn_blocking` 执行；命令参数和 state archive root 都会 trim/空白归一化，空 roots 返回中文错误。
- `src/api.ts` 新增 inventory DTO 与 `previewInventory()`；`src/viewModel.ts` 新增 inventory 状态、summary、action target 格式化，并按 token 处理组合 conflict。
- `src/App.tsx` 设置页“自动管线”新增“存量整理预览”面板：多根目录输入、生成预览按钮、summary、全状态过滤、作品列表、孤儿资源列表、详情资源、evidence、warnings 和 action target。
- `src/styles.css` 新增 inventory panel / summary / filter / list / detail / action / responsive 样式。
- 新增 `src-tauri/tests/inventory.rs`，并扩展 `commands` unit 与 `src/viewModel.test.ts`；复审后 focused Rust、前端 unit/type/build 均通过。最终 orphan 明细截断修复的 Rust 测试在当前 Windows 会话被 `os error 1455` 页面文件限制挡在编译阶段，需要新环境复跑。

## 阶段 7B 交付物

- `src-tauri/src/inventory.rs` 新增 `InventoryReviewBucket`、`InventoryConfidence`、`InventoryResolution`、`InventoryResourceRoleKind`、`InventoryResourceRole`，并为每个番号组输出配对建议和资源角色。
- 主视频选择和 action target 排序共用同一套视频排序，避免“详情解释选 A、动作预览移动 B”；主 NFO 按 `<num>`、裸番号、同目录、路径稳定排序选择。
- summary 新增 `auto_ready`、`needs_review`、`blocked`；target_exists 和番号证据冲突会进入 `blocked`，多视频/多 NFO/疑似重复进入人工复核视角，纯图片/GIF 组进入素材候选。
- `src-tauri/src/commands.rs` 新增并注册 `export_inventory_report_command`，只把当前 report JSON 写到 app data 的 `inventory-reports/`，不写媒体内容。
- `src/api.ts` / `src/viewModel.ts` 新增 7B DTO、review/status/orphan 统一筛选、置信度/角色/复核分桶格式化和导出摘要。
- `src/App.tsx` 的“一键盘点”支持本次整理目标目录、复核分桶筛选、配对建议详情、资源角色展示、扫描中输入禁用和导出 JSON。
- 新增 `src/App.inventory.test.tsx`，通过 `happy-dom` 覆盖 App 层关键接线：目标目录传给 preview、导出当前 report、扫描中禁用输入和 Windows placeholder。
- 完整 gate 通过 `cargo test --manifest-path src-tauri/Cargo.toml -j 1`、`npm test`、`npx tsc --noEmit`、`npm run build`。历史 `resource_pool.rs` warning 仍存在。

## 阶段 7B.1 交付物

- `src-tauri/src/inventory.rs` 新增 `InventoryExecutionPlan`，挂在 `InventoryResolution.execution_plan`；该计划是后续 7C 唯一允许消费的动作草案。
- `InventoryWorkPreview.actions` 保留为候选动作预览，用于诊断和解释重复目标；不要把它直接用于真实移动/复制。
- 安全计划只选择主视频、主 NFO，以及每个目标路径的第一个资源动作；候选 `target_duplicate` 不会污染 `execution_plan.actions`，但 `target_exists` 会保留并阻断。
- `auto_ready` 改为依赖 `execution_plan.ready`；缺目标目录、多视频、疑似重复视频会进入复核，目标已存在、番号冲突、NFO 解析失败会阻断。
- `src/api.ts` / `src/viewModel.ts` 新增前端 DTO 和 `formatInventoryExecutionPlanSummary`。
- `src/App.tsx` 详情页新增“安全执行计划”，并把原始动作区改名为“候选动作预览”，避免用户误以为所有候选动作都会执行。
- 新增/更新测试覆盖真实反馈场景：多 NFO + 多 poster 候选重复、安全计划去重、多视频复核、目标已存在阻断、App 详情展示。
- 完整 gate 通过 `cargo test --manifest-path src-tauri/Cargo.toml -j 1`、`npm test`、`npx tsc --noEmit`、`npm run build`。历史 `resource_pool.rs` warning 仍存在。

## 阶段 7C 交付物

- 新增 `src-tauri/src/inventory_execution.rs`：copy-only `execute_inventory_report`，DTO 包含 `InventoryExecutionRequest` / `InventoryExecutionReport` / `InventoryExecutionActionLog`。
- 执行器只消费 `resolution.execution_plan.actions`，并有 raw candidate 污染回归测试；`InventoryWorkPreview.actions` 仍只是候选动作预览。
- 全量执行只处理 auto-ready 作品；显式选择非 auto-ready 会被拒绝；截断 report 不能执行全部作品。
- 安全预检会拒绝源缺失、目标已存在、动作冲突、批内重复目标、目标路径越界，以及 Windows junction / Unix symlink 指向 archive root 外。
- 复制使用 create-new 临时文件、SHA-256 校验、no-clobber 落盘；失败回滚只删除 bytes + SHA-256 仍匹配的本轮目标。
- `src-tauri/src/commands.rs` 新增并注册 `execute_inventory_plan`，通过 `spawn_blocking` 执行真实复制。
- `src/api.ts` / `src/viewModel.ts` 新增执行 DTO/API 和 `formatInventoryExecutionSummary`。
- `src/App.tsx` 的“一键盘点”新增“复制整理”按钮、确认弹窗、执行中 disabled/loading、完成状态和最近执行日志；截断报告会禁用复制整理。
- 完整 gate 通过 `cargo test --manifest-path src-tauri/Cargo.toml -j 1`、`npm test`、`npx tsc --noEmit`、`npm run build`。历史 `resource_pool.rs` warning 仍存在。

## 阶段 7D 交付物

- `InventoryExecutionMode` 新增 `low_space`，保留 `copy` 兼容旧调用。
- `InventoryExecutionActionStatus` 新增 `linked`；`InventoryExecutionReport` 新增 `linked_actions` / `bytes_linked`。
- `low_space` 模式下 `InventoryResourceKind::Video` 使用 `fs::hard_link(source, target)` 创建目标硬链接，不调用临时复制流程；硬链接失败会返回“视频硬链接失败”，不会降级为复制视频。
- 非视频资源仍使用 7C 的 create-new 临时文件、字节数校验、SHA-256 和 no-clobber 落盘。
- 失败回滚会倒序删除本轮创建的目标；复制目标继续按 bytes + SHA-256 校验，硬链接目标只删除目标路径本身，不触碰源路径。
- 新增后端回归：`inventory_low_space_execution_hardlinks_video_and_copies_small_assets`，证明修改源视频会同步反映到目标硬链接，而修改源 NFO 不会影响已复制目标。
- 前端类型支持 `low_space` / `linked`，盘点页按钮改为“低空间整理”，确认框说明“视频硬链接，小资源复制，源文件不删除”。
- 执行摘要显示硬链接数量、复制数量、链接视频字节数和复制小文件字节数；最近执行日志显示“已硬链接”。
- 灰色按钮 follow-up：低空间整理现在只在 `summary.works > works.length` 时禁用；如果只是素材候选/孤儿资源明细截断，仍允许执行完整的作品安全计划。
- 完整 gate 通过 `cargo test --manifest-path src-tauri/Cargo.toml -j 1`、`npm test`、`npx tsc --noEmit`、`npm run build`。历史 `resource_pool.rs` warning 仍存在。

## 阶段 6A 交付物

- 新增 `src-tauri/src/remote_scraper.rs`：`RemoteMetadata`、`RemoteMetadataHttpClient`、`RemoteScraperConfig`、`RemoteScraperSource`、`parse_json_ld_metadata`。
- `RemoteMetadataHttpClient` 把真实 HTTP 获取隔离在 trait 后面；阶段 6A 测试只用 fake client。
- `RemoteScraperConfig` 要求 URL 模板包含 `{code}`，`build_url` 会对 code 做 percent encoding。
- `parse_json_ld_metadata` 支持 HTML 中 JSON-LD 单对象、数组和 `@graph`，接受 `Movie` / `VideoObject`，提取 title、original title、summary、actors、genres、studio、director、release date、cover URL。
- `RemoteScraperSource` 同时实现旧 ingest `MetadataProvider` 和自动管线 `ScraperSource`。
- `ScrapedWorkMetadata.cover_path` 暂为 `None`；远程封面 URL 只保留在 `ProviderMetadata.cover_url`，封面下载留后续。
- 新增 `src-tauri/tests/remote_scraper.rs`，覆盖 parser、URL 构建、HTTP error、`MetadataProvider` 和 `ScraperSource` 转换。

## 留给后续阶段的项（review 时判定 DEFER，不是缺陷）

- `src-tauri/src/commands.rs` 前端命令路径的 `parse_watch_status` 新变体缺失已在阶段 4 修复，并有 `commands::tests::command_watch_status_parser_accepts_stage1_statuses` 回归测试。
- 几处 `created_at` 只写未读 / `parse_*` 回退不可区分 → 非阻塞清理项，后续碰到相关命令/UI 时再处理。
- 真实站点 live HTML 兼容性仍需用户在实际环境验证；Codex 测试只证明 adapter / HTTP / fixture / run-once 边界，不证明当前 javdb/javbus/fanza 页面一定可解析。
- 登录、cookie、验证码、反爬绕过、真实用户会话复用仍不做，也不应在 Codex 测试中加入。
- 封面下载、远程 metadata 缓存、批量重试、精细多源排序/权重仍是后续阶段工作。
- aria2 进程管理、下载任务创建/暂停/恢复、自动发现所有 aria2 任务、常驻后台轮询、WebSocket/callback 通知仍是后续阶段工作。
- WebSocket、托盘、自启、常驻后台线程仍是后续阶段工作。

## 下一步

当前主线已经切到 **M1：存量资源集中迁移**。下一步不要继续围绕 7D 的硬链接整理扩展，先用 `writing-plans` 为 M1 写 TDD 实施计划，计划输入是 `docs/superpowers/specs/2026-06-28-media-manager-m1-centralized-inventory-migration-design.md`。

如果先做实际环境验证，阶段 7D 仍可作为只增不删的保守验证路径：

1. 用户在自己的交互桌面终端运行 `npm run dev`，用浏览器打开 `http://localhost:1420`；Codex 不启动 WebView2/Tauri GUI。
2. 设置页 → 自动管线 → 可先跑“一键自检”确认基础链路。
3. 在“一键盘点”里每行填一个真实存量根目录，例如下载目录、NFO 输出目录、图片目录。
4. 填写“整理目标目录”，这是低空间整理的目标根目录；正式测试前建议先用同一盘符下的空目录或测试目录。
5. 点击“开始盘点”；期望只生成汇总、复核分桶、作品列表、资源证据、配对建议、安全执行计划、候选动作预览、warnings 和目标路径预览，不移动、不复制、不删除任何文件。
6. 重点看“可自动整理 / 需人工确认 / 阻断 / 素材候选 / 孤儿资源”筛选是否符合真实目录情况，并打开作品详情检查主视频、主 NFO、资源角色、安全执行计划、候选动作重复和阻断原因。
7. 如果 `可自动整理` 大于 0 且报告未截断，可点击“低空间整理”；确认框会提示视频创建硬链接，小资源复制，源文件不删除。期望源视频仍在原位置，目标目录出现按番号整理后的硬链接视频和复制的小资源，并显示最近低空间整理摘要。
8. 整理后重新点击“开始盘点”验证目标状态；如果目标已存在，后续报告应把对应目标冲突识别出来。
9. 点击“导出 JSON”，导出当前 report 到 app data 的 `inventory-reports/`；如果需要后续分析，把导出的 JSON 路径或文件发给 Codex。

如果继续做 Codex 可编码工作，优先进入 M1 实施：新增 `move` 执行模式、同卷直接移动、跨卷逐文件复制校验删除、空间检查、作品级回滚、执行报告和“一键盘点”主按钮切换。迁移结果入库同步、目标库重建、人工复核项执行和旧迁移入口清理放到 M1 之后。仍然不要在 Codex 会话里启动 Tauri GUI 或 WebView2；Codex 验证继续用 `cargo test --manifest-path src-tauri/Cargo.toml -j 1`、`npm test`、`npx tsc --noEmit`、`npm run build`，视觉检查由用户在自己的交互桌面环境运行。

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
