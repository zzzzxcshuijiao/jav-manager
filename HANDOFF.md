# 接手说明 — media-manager 重构

> 给下一个环境的开发者 / codex：从这份文件开始读。

## 项目

media-manager：Tauri(壳) + React(UI) + Rust(核心/SQLite/管线) 的本地媒体库。重构目标 = **下载 → 自动归档 → 整理 → 分类移动 → 浏览播放** 的端到端、尽量零干预管线。分 5 个阶段实施。

## 当前进度

**阶段 1（数据模型扩展）已完成并通过 review。** 9 个 commit 在 `codex/nfo-metadata-model` 分支：`9369fb7..495f99e`。
87 个后端测试全绿，migration 幂等（现有库可无损升级）。

## 接手步骤（新电脑）

```bash
# 1. 从 bundle 还原仓库（含全部历史 + 分支）
git clone media-manager.bundle media-manager
cd media-manager
git checkout codex/nfo-metadata-model

# 2.（可选）解压 VibeCoding 状态：踩坑记录 lessons.md / 进度 tasks.md
tar -xzf media-manager-ai-state.tar.gz   # 在仓库根解出 .ai_state/

# 3. 验证阶段 1（轻量，不需要任何真实媒体资源）
cd src-tauri
cargo test                       # 期望 87 个测试全绿
cargo run --example stage1_smoke # 打印 7 大数据层功能实测结果
cd .. && npm install             # 前端依赖（按需）
```

**关键：验证环境是自包含的。** `cargo test` 和 `stage1_smoke` 都用临时目录(`tempdir`)造假文件，**不依赖 H:/ 真实视频或 G:/ 图片库**。开发与测试全程不需要真实媒体资源——它们只在用户实际运行 daemon 处理真实库时才需要。

## 必读文档（按顺序）

1. **`AGENTS.md`** — 硬约束，必须先读：
   - 禁止在非交互 agent 会话里跑 `tauri dev` / `cargo run` / `media-manager.exe`（启动 WebView2 会崩 host）。验证只用 `cargo test` + `npm run build` + `tsc --noEmit`。
   - 编辑含中文的源文件用 `apply_patch` / node:fs，**不要** PowerShell `Set-Content`（GBK 控制台会损坏 UTF-8）。
2. **`docs/superpowers/specs/2026-06-25-media-manager-refactor-design.md`** — 全局设计：架构(后台 daemon + 前端 + SQLite)、自动管线、数据模型、深色沉浸 UI 六页、异常处理、刮削器、服务部署、测试策略、复用 vs 砍掉代码清单、5 阶段划分、后续(aria2 下载集成 / NAS 移动端浏览)。
3. **`docs/superpowers/plans/2026-06-25-media-manager-refactor-stage1-data-model.md`** — 阶段 1 的 plan（已完成，可作 TDD 风格参考）。

## 阶段 1 交付物

- `WatchStatus` +3 变体：`WantToWatch` / `Watching` / `OnHold`
- `works` +2 播放字段：`watch_progress_seconds`、`last_played_at`（由 `set_watch_progress` 写，不走 `upsert_work`）
- 6 张新表 + CRUD：`scrape_jobs`、`exceptions`、`holding`、`pipeline_runs`、`collections`、`work_collections`
- migration 幂等：`CREATE TABLE IF NOT EXISTS` + `ensure_column`，旧库升级不丢数据

## 留给后续阶段的项（review 时判定 DEFER，不是缺陷）

- `src-tauri/src/commands.rs:1422` 有第二个 `parse_watch_status`（前端命令路径）缺新变体 → **阶段 4 前端连线时修**（否则 UI 发新状态会降级为 Unwatched）
- `remove_work_from_collection` API + `ON DELETE CASCADE` 测试 → **阶段 2 补**
- 几处 `created_at` 只写未读 / `parse_*` 回退不可区分 / `impl Repository` 末尾多余空行 → **阶段 2 顺手清理**

## 下一步

写**阶段 2 plan（自动管线）**：监控 → 识别 → 刮削(多源) → 整理(多版本/图片) → 移动(归档结构) → 入库，以及异常队列 / 搁置区的路由。用 `writing-plans` 技能基于设计文档 §5 拆，沿用阶段 1 的 TDD + `tempdir` 模式（测试造假文件，不依赖真实资源）。阶段 2–5 的划分见设计文档。

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
