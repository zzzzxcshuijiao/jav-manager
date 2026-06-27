# 阶段 7B：配对规则与人工复核预览设计

## 背景

阶段 7A 已经把散乱存量资源扫描出来，并能按番号聚合视频、NFO、图片、GIF 与孤儿资源。真实环境测试证明这个方向能覆盖用户当前痛点：已有资源散落在多个目录，且规模可能达到数百个作品、上万组素材候选和数千个孤儿资源。

但 7A 的结果仍偏“清单”：用户能看到资源，却还不能快速判断哪些可以直接整理、哪些需要人工确认、为什么某个资源被判为多 NFO、多视频、素材候选或疑似重复。阶段 7B 的目标是给 7A report 加一层可解释的配对判定和复核工作流。

## 目标

- 继续保持只读：不移动、不复制、不删除、不重命名真实文件。
- 在盘点页直接配置本次整理目标目录，避免只能去设置页改全局归档根目录。
- 为每个番号组输出可解释的配对判定：
  - 推荐主视频。
  - 推荐主 NFO。
  - 资源角色与选择理由。
  - 是否可自动整理。
  - 是否需要人工确认。
  - 阻断原因和风险原因。
- 把盘点结果分成更可操作的复核队列：可自动整理、需人工确认、多视频、多 NFO、素材候选、番号冲突、疑似重复、孤儿资源。
- 在前端详情页显示“为什么这么判”，让用户能检查推荐是否可信。
- 支持导出当前 JSON 盘点结果，方便真实环境反馈和后续分析。

## 非目标

- 不执行真实整理动作；真实移动/复制/冲突确认留到 7C。
- 不持久化人工合并/拆分决策；本阶段只做只读复核与导出。
- 不写 SQLite inventory task 表，不污染正式作品库。
- 不做远程 scraper、不补 NFO、不下载图片。
- 不启动 Tauri GUI、WebView2、默认 `cargo run`、`media-manager.exe` 或 in-app browser。
- 不重新设计 7A scanner；只在现有 `InventoryPreviewReport` 基础上补判定层。

## 方案选择

采用方案 A：规则解释 + 可复核队列 + JSON 导出。

理由：

- 用户当前最需要的是理解真实盘点结果，而不是立刻批量移动文件。
- 真实目录规模较大，先让判定结果可解释，能降低 7C 执行整理的风险。
- 不持久化手动决策可以避免提前引入复杂表结构，同时仍能通过导出结果收集真实样本。
- 7A 已经有 scanner、聚合、目标路径预览和状态标签；7B 可以复用这些边界，增量风险可控。

## 数据模型

阶段 7B 在 `src-tauri/src/inventory.rs` 中新增判定 DTO，并挂到现有 `InventoryWorkPreview`。

### `InventoryReviewBucket`

作品级复核队列：

- `auto_ready`：推荐可以自动整理。
- `needs_review`：存在风险，需要人工确认。
- `blocked`：存在明确阻断，不能自动整理。
- `asset_candidate`：没有视频/NFO anchor 的素材候选。

孤儿资源仍保留在 `InventoryPreviewReport.orphans`，前端提供 `orphan` 筛选入口，但 orphan 不挂到 `InventoryWorkPreview.resolution`，因为它不属于任何番号组。

### `InventoryResolution`

每个番号组的配对判定摘要：

- `bucket`: 所属复核队列。
- `primary_video`: 推荐主视频路径，可为空。
- `primary_nfo`: 推荐主 NFO 路径，可为空。
- `recommended`: 简短中文结论。
- `reasons`: 判定理由列表。
- `warnings`: 风险提示列表。
- `blockers`: 阻断原因列表。
- `confidence`: `high | medium | low`。

### `InventoryResourceRole`

每个资源的角色解释：

- `path`: 原始路径。
- `role`: `primary_video | secondary_video | duplicate_video | primary_nfo | secondary_nfo | poster | fanart | thumb | screenshot | gif | image | other`。
- `reason`: 为什么分到这个角色。
- `selected`: 是否为推荐主资源。
- `needs_review`: 是否需要用户确认。

`InventoryWorkPreview` 增加：

- `resolution: InventoryResolution`
- `resource_roles: Vec<InventoryResourceRole>`

`InventorySummary` 增加：

- `auto_ready`
- `needs_review`
- `blocked`

这些计数按完整结果计算，不受 1000 条明细截断影响。

## 配对规则

### 主视频选择

优先级：

1. 文件名 stem 是裸番号，例如 `IPX-159.mp4`。
2. 明确分段第一段，例如 `IPX-159-CD1.mp4`、`IPX-159-part1.mkv`。
3. 同番号组内体积最大的视频。
4. 路径排序兜底，保证结果稳定。

多视频时不直接阻断，但进入 `needs_review`。如果多个视频体积相同，标记 `duplicate_video` 和 `duplicate_candidate`，进入 `needs_review`。

### 主 NFO 选择

优先级：

1. NFO `<num>` 与当前番号一致。
2. 文件名 stem 是裸番号。
3. 与主视频同目录。
4. 路径排序兜底。

多 NFO 时进入 `needs_review`。如果 NFO 解析失败或 NFO `<num>` 与路径/文件名证据冲突，进入 `blocked`。

### 自动可整理判定

满足以下条件时进入 `auto_ready`：

- 至少一个视频。
- 至少一个 NFO。
- 无 `code_conflict`。
- 无 `nfo_parse_error`。
- 无 action `conflict`。
- 无疑似重复视频。
- 主视频和主 NFO 都能明确选出。

否则按风险进入 `needs_review` 或 `blocked`。

### 素材候选

只有图片/GIF/其他资源，没有视频和 NFO anchor 的番号组进入 `asset_candidate`。它们不计入 `missing_video` 的人工整理压力，但可在 7B 前端单独查看和导出，供后续人工补配。

### 孤儿资源

无法识别番号的资源继续进入 `orphans`。7B 不尝试用模糊标题、目录语义或远程 scraper 反推番号，避免误配。

## 前端设计

盘点页保留 7A 的主结构，但补以下能力：

- 在“入口目录”下增加“整理目标目录”输入框。
- 增加“选择目标目录”按钮；桌面后端可用时走系统目录选择器，不可用时保留手填路径。
- `generateInventoryPreview` 调用 `api.previewInventory(roots, inventoryArchiveRoot)`，不再固定传 `null`。
- 汇总卡片增加：
  - 可自动整理。
  - 需人工确认。
  - 阻断。
- 筛选器增加：
  - 可自动整理。
  - 需人工确认。
  - 阻断。
  - 多视频。
  - 多 NFO。
  - 素材候选。
  - 番号冲突。
  - 疑似重复。
  - 孤儿资源。
- 详情页在资源和动作预览前新增“配对判定”区：
  - 推荐结论。
  - 主视频。
  - 主 NFO。
  - 判定理由。
  - 风险提示。
  - 阻断原因。
- 资源列表显示角色标签和理由，例如“主视频”“备选 NFO”“疑似重复视频”。
- 增加“导出 JSON”按钮，导出当前 `InventoryPreviewReport` 到本地文件。

所有长操作必须符合项目操作反馈原则：按钮 disabled，显示进行中文案，状态栏同步反馈。

## JSON 导出

新增 Tauri command：

- `export_inventory_report(report: InventoryPreviewReport) -> InventoryExportResult`

导出路径放在 app data 下：

```text
%APPDATA%\local.media-manager\inventory-reports\inventory-YYYYMMDD-HHMMSS.json
```

导出内容是当前内存中的 report JSON，包含 roots、archive_root、summary、works、asset_candidates、orphans、warnings、resolution 和 resource_roles。summary 始终代表完整扫描计数；如果 7A/7B report 因结果过大被截断，导出的 works、asset_candidates 和 orphans 明细也只包含当前返回的前 1000 项。

安全边界：

- 导出仅写 JSON 报告，不复制媒体/NFO/图片内容。
- 报告会包含本地路径；这是本地诊断用途。跨机器分享前由用户自行判断是否需要匿名化。
- 如果没有 report，前端不允许导出。

CSV 不进入 7B 主线。原因是 CSV 会丢失嵌套证据和角色解释，真实反馈分析优先需要结构化 JSON。

## 错误处理

- 目标目录为空：仍允许扫描，但 `to_path` 为空，resolution 降为 `needs_review`，提示未设置整理目标。
- 目标路径已存在：action 标记 `target_exists`，resolution 进入 `blocked`。
- 批内目标重复：action 标记 `target_duplicate`，resolution 进入 `blocked`。
- 多视频：进入 `needs_review`，除非存在同尺寸疑似重复，增加 duplicate warning。
- 多 NFO：进入 `needs_review`，如果有 code conflict 或 parse error 则进入 `blocked`。
- 导出失败：前端保留当前 report，状态栏显示失败原因，不清空盘点结果。
- report 被截断：summary 保持全量，导出当前返回的明细；后续分页/持久化任务表再解决全量明细导出。

## 测试策略

后端测试：

- 主视频选择：裸番号优先、CD1 优先、体积最大兜底、路径稳定兜底。
- 主 NFO 选择：NFO `<num>` 一致优先、裸番号优先、同目录优先。
- `auto_ready`：干净的单视频 + 单 NFO + 图片组合。
- `needs_review`：多视频、多 NFO、素材候选、缺 NFO、缺视频。
- `blocked`：番号冲突、NFO 解析失败、目标已存在、批内目标重复。
- resource role：主/副视频、疑似重复、主/副 NFO、图片角色都有解释。
- JSON 导出：写入 app data 下 inventory-reports，不写媒体内容。

前端测试：

- `viewModel` 格式化 review bucket、confidence、resolution summary。
- filter helper 能按 resolution bucket 和旧 status 同时筛选。
- `formatInventorySummary` 包含 auto_ready、needs_review、blocked。
- 导出按钮在无 report 时禁用，有 report 时调用 API 并格式化路径反馈。

验证命令：

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1
cargo test --manifest-path src-tauri/Cargo.toml commands::tests::inventory -j 1
npm test -- src/viewModel.test.ts
npx tsc --noEmit
npm run build
```

完整 gate 在环境允许时继续使用：

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

## 阶段边界

- 7B 完成后，用户能用真实目录跑盘点、看推荐判定、定位需要人工确认的项目，并导出 JSON 给 Codex 分析。
- 7C 才开始设计真实整理执行，包括移动/复制策略、二次确认、冲突处理、日志和失败恢复。
- 7D 再处理整理后的浏览库体验，例如封面墙、播放入口、筛选和作品详情。
