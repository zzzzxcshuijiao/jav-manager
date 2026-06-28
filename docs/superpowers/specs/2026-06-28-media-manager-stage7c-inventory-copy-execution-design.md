# 阶段 7C：存量整理复制执行设计

## 背景

阶段 7A/7B/7B.1 已经能从真实存量目录中扫描视频、NFO、图片和 GIF，按番号聚合资源，并生成 `resolution.execution_plan.actions`。用户真实导出的 `inventory-20260628-091139.json` 显示：

- 识别 464 部作品。
- 434 部进入 `auto_ready`，且安全执行计划无冲突。
- 30 部需要人工确认，主要是缺视频或多视频。
- 原始候选动作仍存在大量 `target_duplicate`，但这些重复已经被隔离在候选动作区。

阶段 7C 的目标是把“可自动整理”的安全计划落地成真实文件操作，但第一版必须降低破坏性：只复制到整理目标目录，不删除、不移动、不覆盖源文件。

## 目标

- 新增存量整理执行器，唯一输入为 `InventoryPreviewReport` 中每个作品的 `resolution.execution_plan.actions`。
- 只执行 `resolution.bucket == auto_ready` 且 `execution_plan.ready == true` 的作品。
- 本阶段仅支持 `copy` 模式：复制源文件到目标目录，源文件保持不变。
- 执行前进行全量预检，任何源文件缺失、目标已存在、目标越界、动作冲突或非 ready 作品都会阻止执行。
- 真实复制使用临时文件写入和原子重命名，避免留下半写入目标。
- 若复制过程中发生运行时错误，尽量删除本轮已经创建的目标文件，并返回可展示的执行报告。
- 前端在盘点页提供“复制整理”入口、忙碌状态、完成摘要和最近执行明细。
- 测试仍只使用临时目录和假文件，不依赖真实 H/G 盘资源。

## 非目标

- 不移动、不删除源文件。
- 不清理原散乱目录。
- 不执行 `needs_review`、`blocked`、`asset_candidate`。
- 不提供手动勾选单个资源的人工配对。
- 不写入 SQLite 库记录；本阶段只做文件系统复制执行报告。
- 不运行 Tauri GUI、WebView2 或真实 `media-manager.exe`。

## 执行模型

新增 `inventory_execution` Rust core 模块。

### 请求

`InventoryExecutionRequest`：

- `mode: InventoryExecutionMode`：阶段 7C 只接受 `copy`。
- `selected_codes: Vec<String>`：为空时执行当前报告中所有 auto-ready 作品；非空时只执行指定番号，且指定作品必须 auto-ready。

### 报告

`InventoryExecutionReport`：

- `mode`
- `started_at`
- `finished_at`
- `requested_works`
- `executed_works`
- `skipped_works`
- `planned_actions`
- `copied_actions`
- `failed_actions`
- `rolled_back_actions`
- `bytes_copied`
- `logs`

`InventoryExecutionActionLog`：

- `code`
- `kind`
- `from_path`
- `to_path`
- `status`
- `message`
- `bytes`

### 预检

执行前先构建 `PlannedInventoryAction` 列表，并验证：

- 报告必须有 `archive_root`。
- 如果执行全部作品，报告的 `works` 明细不能被截断。
- 指定番号必须存在。
- 指定作品必须是 `auto_ready` 且 `execution_plan.ready == true`。
- 动作必须来自 `execution_plan.actions`，不读取 `work.actions`。
- 每个动作必须有 `to_path`。
- 每个动作不能有 `conflict`。
- 源路径必须存在且是文件。
- 目标路径不能存在。
- 同一批次内不能有重复目标路径。
- 目标路径必须位于 `archive_root` 下。
- 目标父目录会通过 canonical path 复核；Windows junction / Unix symlink 解析到 `archive_root` 外时拒绝执行。

预检失败时不复制任何文件。

### 复制

每个动作按顺序执行：

1. 创建目标父目录。
2. 用 `OpenOptions::create_new(true)` 创建同目录临时文件，流式复制并计算 SHA-256。
3. 校验复制后的字节数与源文件大小一致。
4. 用 no-clobber 方式提交最终目标：优先 hard link，fallback 仍用 create-new copy，不用可覆盖目标的 rename。
5. 记录 `copied` 日志。

如果任一步失败：

- 当前临时文件会被删除。
- 本轮已复制的目标文件会在 bytes + SHA-256 仍匹配时尽量删除。
- 报告记录失败日志和 rollback 数。
- 源文件始终不删除。

## 前端

盘点页在已有按钮区新增“复制整理”：

- 只在有盘点结果、后端可用、存在 auto-ready 作品、且当前没有盘点/导出/执行操作时可用。
- 点击前用浏览器确认框确认目标目录和 auto-ready 数量。
- 执行期间按钮 disabled，并显示“复制中”。
- 完成后状态行显示执行摘要。
- 页面显示最近一次执行报告，包括总动作、失败、回滚和前若干条日志。

## 风险与边界

- 复制模式会占用额外磁盘空间；这是本阶段为保护原始库存选择的成本。
- 如果用户在执行后不重新盘点，界面仍显示旧报告；前端完成后提示用户可重新盘点验证目标状态。
- 如果 `archive_root` 设置成测试目录，执行会复制到测试目录；正式整理前必须用真实目标目录重新盘点。
- 如果真实报告的作品明细被截断，执行全部作品会被拒绝，避免用户误以为已经处理全部结果。

## 验证

后端：

- ready 作品复制视频、NFO 和图片到目标目录，源文件保留。
- 非 ready 作品不会被全量执行选中。
- 指定非 ready 作品会被拒绝，且没有目标文件生成。
- 目标路径已存在时预检拒绝，且不会先复制其他动作。
- 被篡改到 `archive_root` 外的目标路径会被拒绝。

前端：

- API/DTO 覆盖 `InventoryExecutionReport`。
- viewModel 格式化复制执行摘要。
- App 盘点页在预览后显示复制整理入口，执行期间有 loading/disabled 状态，完成后展示摘要。

验证命令：

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory_execution -j 1
npm test -- src/viewModel.test.ts src/App.inventory.test.tsx
npx tsc --noEmit
npm run build
```

最终门禁：

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```
