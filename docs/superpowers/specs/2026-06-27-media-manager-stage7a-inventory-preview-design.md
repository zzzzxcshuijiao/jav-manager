# 阶段 7A：存量资源盘点与整理预览设计

## 背景

阶段 6A 到 6F 打通了“新下载文件进入自动管线”的基础能力：aria2 接入骨架、远程 scraper、诊断日志、一键自检和沙盒归档。但真实使用反馈表明，当前更核心的问题不是继续增强下载入口，而是已有存量资源本身已经散乱：

- 视频、NFO、封面、剧照、GIF 可能分布在不同盘、不同目录、不同命名规则下。
- 部分资源有 NFO 但找不到视频，部分视频没有 NFO，部分图片只有番号文件名。
- 用户希望最终整理为“每番号一个目录”，但第一步必须只读预览，不能直接移动或删除真实文件。

因此阶段 7A 从 aria2 下载接入转向存量整理：先盘点、聚合、分类和生成整理预览，让用户知道“现在有什么、能整理成什么、风险在哪里”。

## 目标

- 新增只读 Inventory Scanner，扫描用户选择的多个根目录。
- 按番号聚合视频、NFO、图片、GIF 和其他可识别资源。
- 生成“每番号一个目录”的整理预览计划，但不移动、不复制、不删除、不重命名。
- 给每个作品打状态标签：可整理、缺 NFO、缺视频、多版本、多 NFO、番号冲突、孤儿资源。
- 前端提供存量整理预览面板：扫描按钮、汇总统计、状态过滤、作品详情和目标目录预览。
- 所有扫描操作必须有 loading/disabled/status 反馈，符合项目操作反馈原则。

## 非目标

- 不执行真实文件移动、复制、删除或重命名。
- 不覆盖已有归档目录，不清理重复资源。
- 不访问真实 scraper 站点，不下载封面，不补全远程 metadata。
- 不接 aria2 自动发现，不处理新下载任务。
- 不启动 Tauri GUI、WebView2、默认 `cargo run`、`media-manager.exe` 或 in-app browser。
- 不要求用户先把存量资源手工整理到同一目录结构。

## 目标目录模型

阶段 7A 的预览目标固定为：

```text
archive_root\
  CODE\
    CODE.ext
    CODE.nfo
    poster.jpg
    fanart.jpg
    thumb.jpg
    screenshots\
    gifs\
```

阶段 7A 只生成这个目标结构的预览，不创建目录也不移动文件。后续 7C 才会在用户确认后执行整理。

## 输入来源

阶段 7A 新增“存量扫描根目录”概念，由前端命令参数传入；本阶段不持久化扫描根目录，只保留在本次页面状态里。它和现有配置关系如下：

- `archive_root`：继续使用现有归档根目录，作为整理目标根。
- `source_roots`：现有自动管线来源目录可作为默认扫描候选，但不强制。
- `resource_pool_dirs`：现有资源池目录也可作为默认扫描候选。
- 用户可以额外添加任意存量目录，例如下载盘、图片盘、NFO 输出目录。

扫描根目录之间不区分“视频目录”或“图片目录”。所有根都按统一资源池处理，降低配置负担。

## 后端架构

新增 `src-tauri/src/inventory.rs`，职责边界：

- 定义 DTO：
  - `InventoryResourceKind`: `video | nfo | poster | fanart | thumb | screenshot | gif | other`
  - `InventoryResource`: 原始路径、文件名、kind、大小、推断番号、证据来源。
  - `InventoryWorkPreview`: 番号、资源集合、状态标签、目标目录、预览动作。
  - `InventoryPreviewAction`: `from_path/to_path/kind/conflict`，只描述计划，不执行。
  - `InventorySummary`: 总文件数、识别作品数、可整理数、缺 NFO、缺视频、多版本、多 NFO、冲突、孤儿资源。
  - `InventoryPreviewReport`: generated_at、roots、archive_root、summary、works、orphans、warnings。
- 扫描多个 root，递归读取文件元信息。
- 识别资源类型和番号。
- 聚合为作品预览。
- 生成目标路径预览。
- 记录无法识别或无法访问的资源为 warning/orphan，不中断整次扫描。

阶段 7A 不写 SQLite。原因是当前目标是“临时盘点和预览”，先避免把大量不确定的候选资源污染正式库。后续如果用户确认这个扫描结果有价值，7B/7C 再设计持久化任务表。

## 番号识别规则

识别优先级：

1. NFO `<num>` / `source_code`，置信度最高。
2. 文件名 stem 中的番号。
3. 父目录名中的番号。
4. 图片 scraper 常见后缀去除后的 stem，例如 `IPX-159-poster.jpg`、`IPX-159-fanart.jpg`。

如果同一个资源从多个位置提取到不同番号，标记为 `code_conflict`，不自动归入可整理。

如果 NFO `<num>` 与文件名番号不同，优先使用 NFO 番号，但该作品标记 `code_conflict`，前端必须展示冲突证据。

## 资源分类

视频扩展名沿用现有视频识别集合：

```text
mp4, mkv, avi, mov, wmv, flv, webm, m4v, ts
```

图片扩展名：

```text
jpg, jpeg, png, webp
```

GIF 单独归类。

图片角色根据文件名后缀推断：

- poster：`poster`、`cover`、`ps`、裸番号图片。
- fanart：`fanart`、`background`、`pl`。
- thumb：`thumb`、`thumbnail`。
- screenshot：`shot`、`screenshot`、`sample`、带序号的剧照。

无法识别角色但能识别番号的图片归入 generic image，前端显示为“其他图片”，不阻断预览。

## 聚合与状态

每个番号生成一个 `InventoryWorkPreview`。状态可以多个并存：

- `ready`：至少有一个视频，且至少有一个 NFO 或可用本地/远程 fallback metadata 线索。
- `missing_nfo`：有视频但没有 NFO。
- `missing_video`：有 NFO/图片/GIF，但没有视频。
- `multi_video`：同一番号有多个视频，可能是多版本或分集。
- `multi_nfo`：同一番号有多个 NFO，需要后续选择主 NFO。
- `code_conflict`：NFO、文件名、目录名之间提取到不同番号。
- `duplicate_candidate`：多个视频 fingerprint 暂未计算时先按相同 stem/大小/路径相似度标记候选。
- `orphan`：无法识别番号或无法归到任何作品。

阶段 7A 只分类，不自动裁决。多版本、多 NFO、冲突处理留给 7B。

## 预览动作

每个资源生成目标路径，但只作为预览：

- 主视频：`archive_root/CODE/CODE.ext`
- 多视频：`archive_root/CODE/CODE-v2.ext`、`CODE-v3.ext`
- NFO：`archive_root/CODE/CODE.nfo`
- poster：`archive_root/CODE/poster.ext`
- fanart：`archive_root/CODE/fanart.ext`
- thumb：`archive_root/CODE/thumb.ext`
- screenshot：`archive_root/CODE/screenshots/<original-name>`
- gif：`archive_root/CODE/gifs/<original-name>`
- 其他图片：`archive_root/CODE/images/<original-name>`

如果目标路径已经存在，预览动作标记 `target_exists`，不覆盖。阶段 7A 不尝试解决覆盖冲突。

## 前端设计

在现有设置页或新增“存量整理”区域提供：

- 扫描根目录列表。
- 归档根目录提示，复用现有 archive root。
- “生成整理预览”按钮。
- 扫描状态行：正在扫描的 root、已发现文件数、耗时。
- 汇总卡片：
  - 识别作品
  - 可整理
  - 缺 NFO
  - 缺视频
  - 多版本
  - 冲突
  - 孤儿资源
- 状态过滤 tabs 或 segmented control。
- 作品列表：
  - 番号
  - 状态标签
  - 视频/NFO/图片数量
  - 目标目录
- 作品详情：
  - 原始资源路径
  - 推断番号证据
  - 目标路径预览
  - 冲突说明

界面必须明确展示“预览，不会移动文件”。

## 错误处理

- root 不存在：记录 warning，继续扫描其他 root。
- root 无权限：记录 warning，继续扫描其他 root。
- 单个文件读取 metadata 失败：记录 orphan/warning，不中断扫描。
- NFO 解析失败：资源仍保留为 NFO 文件，作品标记 `nfo_parse_error`。
- archive root 未配置：扫描仍可执行，但不生成 `to_path`，summary 标记需要先配置归档根目录。
- 扫描结果过大：后端完整计算 summary，但返回作品详情最多 1000 个，并附加 `truncated` warning；分页和持久化任务表留给后续阶段。

## 测试策略

后端测试：

- `inventory` scanner 使用 `tempdir` 构造混合目录。
- 覆盖视频、NFO、poster、fanart、thumb、screenshot、gif 的跨目录聚合。
- 覆盖视频无 NFO、NFO 无视频、图片无视频、多视频、多 NFO。
- 覆盖 NFO `<num>` 与文件名冲突。
- 覆盖目标路径预览和 `target_exists` 标记。
- 覆盖 root 缺失不失败。

前端测试：

- `viewModel` 格式化 summary/status 标签。
- API DTO 类型通过 `npx tsc --noEmit`。
- UI 状态至少覆盖 loading/disabled/status 文案。

完整验证：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

## 后续阶段边界

- 7B：配对规则与冲突处理，包括多 NFO 主从选择、多视频版本判断、手动合并/拆分。
- 7C：执行整理，支持移动/复制策略、冲突确认、日志和失败回滚。
- 7D：整理后的浏览库体验，围绕作品详情、封面墙、筛选和播放。
- aria2 自动发现降级为后续入口增强，不进入 7A 主线。
