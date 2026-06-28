# 阶段 7D：低空间存量整理执行设计

## 背景

阶段 7C 已经能把 `auto_ready` 作品的安全执行计划落地到整理目标目录，但它是 copy-only：视频、NFO、图片和 GIF 都会复制一份。用户真实媒体库的视频文件很大，且真实源目录和整理目标目录大多数位于同一个盘符；继续复制视频会带来不可接受的磁盘空间成本。

阶段 7D 的目标是在保留 7C 安全边界的前提下，把真实执行策略调整为低空间模式：视频不复制内容，而是在同一文件系统内创建硬链接；小资源仍复制，源文件仍不删除、不移动。

## 目标

- 新增低空间执行模式，作为前端默认整理入口。
- 视频动作使用源文件硬链接到目标路径，不流式复制视频内容。
- NFO、poster、fanart、thumb、screenshot、GIF、image、other 仍沿用 7C 的安全复制流程。
- 源文件始终保留；本阶段不移动、不删除原始散乱目录。
- 硬链接失败时不能静默降级为复制视频，必须返回明确错误，避免误占用大量空间。
- 执行报告区分硬链接动作和复制动作，显示硬链接数量、复制数量、失败和回滚。
- 前端“复制整理”改为“低空间整理”，确认弹窗明确说明视频使用硬链接，小资源复制。
- 测试继续使用临时目录和假文件，不依赖真实 H/G 盘资源。

## 非目标

- 不实现移动视频模式。
- 不删除源目录中的旧文件。
- 不写入 SQLite 库记录。
- 不处理 `needs_review`、`blocked`、`asset_candidate` 的人工复核执行。
- 不把跨盘失败自动改成完整复制视频。
- 不运行 Tauri GUI、WebView2、`tauri dev`、默认 `cargo run` 或 `media-manager.exe`。

## 执行策略

沿用 7C 的 `inventory_execution` core 与 `execute_inventory_plan` command，但扩展 `InventoryExecutionMode`：

- `copy`：保留兼容旧模式，全部动作复制。
- `low_space`：默认新模式，视频硬链接，小资源复制。

`InventoryExecutionRequest.mode` 为空时，命令层仍可默认 `copy` 以保持 API 兼容；前端显式传 `low_space`。

## 动作分类

`InventoryResourceKind::Video`：

- 预检仍要求源文件存在、目标不存在、目标位于 `archive_root` 下、批内目标不重复。
- 执行时创建目标父目录，并再次通过 canonical parent 校验 junction/symlink 不越界。
- 使用 `fs::hard_link(source, target)` 直接建立目标硬链接。
- 如果目标已存在、跨文件系统、权限不足、文件系统不支持硬链接或其他错误，返回失败；不 fallback 为复制视频。
- 执行日志状态为 `linked`。
- `bytes_linked` 统计视频源文件大小，但不代表新增磁盘占用。

非视频资源：

- 沿用 7C 的临时文件、create-new、字节数校验和 no-clobber 落盘。
- 执行日志状态为 `copied`。
- `bytes_copied` 统计真实复制的小资源字节数。

## 报告模型

扩展 `InventoryExecutionActionStatus`：

- `linked`
- `copied`
- `failed`
- `rolled_back`

扩展 `InventoryExecutionReport`：

- `linked_actions`
- `copied_actions`
- `failed_actions`
- `rolled_back_actions`
- `bytes_linked`
- `bytes_copied`

`planned_actions` 仍表示计划动作总数。`executed_works` 的判定改为：该作品所有计划动作都成功，且成功状态可以是 `linked` 或 `copied`。

## 回滚策略

执行中若某个动作失败：

- 当前动作若产生临时文件，删除临时文件。
- 本轮已创建目标按倒序回滚。
- 对复制目标，继续使用 7C 的 bytes + SHA-256 校验后删除。
- 对硬链接视频目标，删除目标路径本身；删除硬链接不会删除源文件，只会减少链接计数。
- 回滚日志使用 `rolled_back`，消息说明是“已删除本轮生成的硬链接目标”或“已删除本轮复制目标”。

硬链接目标的误删风险边界：执行器只删除本轮刚创建的 `to_path`，目标创建前已通过 `create_new` 等价的 `fs::hard_link` no-clobber 语义保证目标不存在；若回滚前目标被外部替换，文件大小和源路径身份校验无法跨平台完全统一，本阶段采用保守策略：若目标路径仍是文件且源路径仍存在，则只删除目标路径，不触碰源路径。

## 前端

盘点页按钮和文案调整：

- 按钮：`低空间整理`
- 忙碌态：`整理中`
- 状态行：`正在低空间整理安全执行计划...`
- 确认框：提示“视频创建硬链接，不复制大视频；NFO/图片/GIF 等小资源会复制；源文件不会删除。”
- 完成摘要：`低空间整理完成：作品 X/Y，硬链接 A，复制 B，失败 C，回滚 D，链接视频 M，复制小文件 N。`
- 最近执行日志：`linked` 显示为 `已硬链接`，`copied` 显示为 `已复制`。

## 风险与边界

- 硬链接要求源和目标位于同一文件系统；用户确认真实环境大多数同盘，因此适合作为默认低空间策略。
- 如果用户目标目录在另一块盘，视频硬链接会失败，执行报告会给出错误，不会偷偷复制视频。
- 硬链接不是快捷方式；目标路径和源路径指向同一文件内容。删除其中一个路径不影响另一个路径继续存在，但修改文件内容会影响所有硬链接路径。因此 UI 和文档应把视频视为只读资产。
- 小资源复制仍会占用额外空间，但体积可控，且能让整理目录自包含。

## 验证

后端：

- `low_space` 模式下视频目标与源文件应是硬链接；修改源内容后目标读到相同内容，且源文件仍存在。
- `low_space` 模式下 NFO/图片仍复制；修改源小文件后目标内容不变。
- 视频硬链接失败时，不 fallback 为复制视频，并回滚已创建的小资源/硬链接目标。
- `copy` 模式保持 7C 行为，兼容旧调用。
- 目标已存在、目标越界、非 ready 选择、截断报告等 7C 安全边界继续有效。

前端：

- API 类型接受 `low_space`。
- `formatInventoryExecutionSummary` 正确显示硬链接和复制统计。
- 盘点页调用 `executeInventoryPlan(report, [], "low_space")`，按钮文案、loading、确认框和最近执行日志符合新策略。

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
