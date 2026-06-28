# 阶段 7B.1：盘点执行计划安全化设计

## 背景

阶段 7B 已经为存量盘点结果增加了配对判定、资源角色、复核队列和 JSON 导出。用户用真实媒体库导出的报告显示：识别本身有效，但大量作品的原始 `actions` 存在 `target_duplicate`。这些重复主要来自多个 NFO、多个 poster、多个截图或多来源图片都会预览到同一个目标路径。

这暴露出一个阶段边界问题：`actions` 是候选动作明细，适合解释和诊断；它不能直接作为 7C 的真实移动/复制输入。阶段 7B.1 的目标是把“候选动作”与“可执行动作”分开，让后续真实执行只消费经过选择、去重和冲突复核的安全计划。

## 目标

- 保持盘点只读：不移动、不复制、不删除、不重命名真实文件。
- 新增每个作品的 `execution_plan`，作为 7C 可消费的安全动作草案。
- `execution_plan.actions` 只包含确定选中的主视频、主 NFO 和每个目标路径唯一的资源动作。
- `execution_plan.conflicts` 汇总会阻止自动执行的原因，例如目标已存在、缺少目标目录、缺主视频、缺主 NFO、存在多视频或疑似重复视频。
- `auto_ready` 必须以 `execution_plan.ready == true` 为准；不能再因为原始 `actions` 有可解释的候选重复而误判为可自动整理。
- 前端详情页明确区分“安全执行计划”和“候选动作预览”，避免用户把候选动作误认为将来会全量执行。
- 用真实 JSON 暴露出的场景补回归：多 NFO / 多 poster 的作品不能让 `auto_ready` 带着重复目标动作。

## 非目标

- 不执行真实整理动作；真实执行仍留给 7C。
- 不持久化人工确认结果。
- 不实现批量选择、手动覆盖或资源拖拽配对。
- 不取消原始 `actions` 字段；它仍用于诊断、解释和查看候选冲突。
- 不新增 SQLite 表。

## 数据模型

新增 `InventoryExecutionPlan`：

- `ready: bool`：是否可作为自动整理草案。
- `actions: Vec<InventoryPreviewAction>`：已经去重和筛选后的安全动作。
- `conflicts: Vec<String>`：阻止自动执行的原因。
- `notes: Vec<String>`：非阻断说明，例如“已从重复目标中选择 1 个资源”。

`InventoryResolution` 增加：

- `execution_plan: InventoryExecutionPlan`

保持兼容：

- `InventoryWorkPreview.actions` 仍代表候选动作预览。
- `InventoryResourceRole` 仍代表资源解释。

## 规则

### 选入执行计划

- 主视频：只选 `resolution.primary_video`。
- 主 NFO：只选 `resolution.primary_nfo`。
- 其他资源：按现有候选动作顺序遍历，每个目标路径只选第一个动作。
- 没有目标目录时，动作仍保留 `to_path = null`，但计划不 ready。
- `target_duplicate` 只要来自未选中的候选重复，不进入安全动作的 conflict。
- `target_exists` 必须保留为执行冲突。

### 自动 ready

只有同时满足以下条件，`execution_plan.ready = true` 且 bucket 为 `auto_ready`：

- 有主视频。
- 有主 NFO。
- 没有 `CodeConflict` / `NfoParseError`。
- 没有 `MultiVideo` / `DuplicateCandidate`。
- 安全动作非空。
- 安全动作没有 conflict。
- 安全动作都有目标路径。

多 NFO 不再自动阻断；如果能选出主 NFO，其他 NFO 留在候选动作和资源角色中复核。多 poster、重复截图等资源同理，只保留一个安全动作，其余保留为候选明细。

### 复核和阻断

- 多视频和疑似重复视频进入 `needs_review`，因为真实执行前需要确认分段、版本或重复。
- 缺主视频、缺主 NFO、未配置目标目录进入 `needs_review`。
- 番号冲突、NFO 解析失败、目标已存在进入 `blocked`。
- 只含素材的组继续为 `asset_candidate`。

## 前端

- 详情页新增“安全执行计划”区，展示 ready 状态、动作数量、conflicts 和 notes。
- 原“整理动作预览”改名为“候选动作预览”，明确它不是执行清单。
- viewModel 增加 `formatInventoryExecutionPlanSummary`，用于列表/详情展示。

## 测试策略

后端：

- 多 NFO + 多 poster 时，原始 `actions` 可以有 `target_duplicate`，但 `execution_plan.actions` 不能有 `target_duplicate`，且可进入 `auto_ready`。
- 多视频时，即便选出主视频，也必须 `needs_review`，`execution_plan.ready = false`。
- 目标已存在时，`execution_plan.conflicts` 包含目标已存在，bucket 为 `blocked`。
- summary 的 `auto_ready` 按 `execution_plan.ready` 统计。

前端：

- DTO 包含 `execution_plan`。
- `formatInventoryExecutionPlanSummary` 能格式化 ready / conflict / no-target 状态。
- App 详情页展示安全计划，并把原动作区标成候选动作预览。

## 验证

不启动 Tauri GUI、WebView2、默认 `cargo run` 或 `media-manager.exe`。

使用：

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml --test inventory -j 1
npm test -- src/viewModel.test.ts src/App.inventory.test.tsx
npx tsc --noEmit
npm run build
```

环境允许时再跑完整：

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
```
