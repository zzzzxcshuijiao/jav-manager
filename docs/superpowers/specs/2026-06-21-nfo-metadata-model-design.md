 # NFO 全维度元数据建模与集中查询（后端先行）

 - 状态：已确认（等待 writing-plans）
 - 日期：2026-06-21
 - 范围：把 526 个本地 NFO 的全部维度落库，并提供集中查询能力
 - 不在本范围：迁移归档修复、增量扫描、缩略图优化、分页性能

 ## 背景

 现有数据库以「入库任务 + 归档移动」为中心建模，NFO 只被当作视频的附属物顺带读取，
 且只提取了约 60% 的字段。实测三个解析缺陷直接影响数据质量：

 1. `<![CDATA[...]]>` 不处理，`extract_xml_tag` 把字面量 `![CDATA[` 与 `]]` 当文本存进
    title/summary，CinMingle 批量污染。
 2. `runtime` 在 NFO 中有 `134`、`9分鍾`、`120 min` 等多种形态，当前 0 处读取。
 3. `rating` 是浮点且可嵌套 `<ratings><rating name="javdb" max="5">`，当前 schema 用
    `INTEGER(0-10)` 存不下。

 NFO 分布：`H:/AV` 42 个 + `H:\CineMingle-1.3.0\JAV_output` 484 个 = 526 个（递归统计）。
 字段分布（120 样本统计，几乎 100% 出现）：
 `title / originaltitle / sorttitle / num / premiered / releasedate / release / year /
  runtime / plot / outline / actor / set / studio / maker / label / director / tag /
  genre / rating / criticrating / votes / ratings(嵌套) / cover / poster / thumb /
  fanart / website / mpaa / customrating`。

 完全没落库的字段：runtime、year、votes、criticrating、label/publisher、set、tag（与
 genre 分离的那份）、outline、website、poster/thumb/fanart、嵌套 ratings。

 ## 目标

 把 NFO 中的维度尽可能完整保存进数据库，支持按演员/标签/系列/厂牌/发行反向查询作品，
 多 CD 同作品合并为一条 work。

 ## 数据模型（方案 B：规范化关系模型）

 ### 三个跨切面决策

 1. 统一 tag：废弃 `works.genres_json`，NFO `<tag>` 与 `<genre>` 合并写入统一的
    `work_tags`（`tags.name` 自动去重）。
 2. 非番号格式分开：`normalized_code` 仅存「标准番号」（匹配
    `[A-Z]{2,10}-\d{2,6}`），新增 `source_code`（原始 `<num>` 原样存）与 `code_kind`
    （`standard` / `nonstandard`）。scanner 不再对非标准格式硬 normalize。
 3. 多 CD 合并：同一 `<num>` 的多个 NFO 合并为一个 work，每个 NFO 对应一条
    `file_versions`。归并键是 `source_code`。

 ### 新增/改造的表

 ```sql
 -- works：废弃 rating(INTEGER)、genres_json；新增多个标量列
 -- 保留 app_settings、archive_action_logs
 ALTER TABLE works
   ADD COLUMN source_code TEXT;
 ALTER TABLE works
   ADD COLUMN code_kind TEXT NOT NULL DEFAULT 'standard';  -- 'standard' | 'nonstandard'
 ALTER TABLE works
   ADD COLUMN runtime_minutes INTEGER;
 ALTER TABLE works
   ADD COLUMN year INTEGER;
 ALTER TABLE works
   ADD COLUMN label_id INTEGER REFERENCES labels(id);
 ALTER TABLE works
   ADD COLUMN set_id INTEGER REFERENCES sets(id);
 ALTER TABLE works
   ADD COLUMN website TEXT;
 ALTER TABLE works
   ADD COLUMN mpaa TEXT;
 ALTER TABLE works
   ADD COLUMN outline TEXT;
 ALTER TABLE works
   ADD COLUMN poster_path TEXT;
 ALTER TABLE works
   ADD COLUMN thumb_path TEXT;
 ALTER TABLE works
   ADD COLUMN fanart_path TEXT;
 ALTER TABLE works
   ADD COLUMN criticrating REAL;
 ALTER TABLE works
   ADD COLUMN rating_value REAL;
 ALTER TABLE works
   ADD COLUMN rating_max INTEGER;
 ALTER TABLE works
   ADD COLUMN rating_votes INTEGER;
 ALTER TABLE works
   ADD COLUMN has_video INTEGER NOT NULL DEFAULT 1;  -- 0/1 布尔
 -- 旧 rating INTEGER 列的值搬迁到 rating_value 后删除（迁移逻辑见第 5 节）
 -- 旧 genres_json 列保留但不再写入（避免破坏性 DROP COLUMN 在旧 SQLite 上的兼容问题）
 ```

 ```sql
 CREATE TABLE IF NOT EXISTS tags (
   id INTEGER PRIMARY KEY AUTOINCREMENT,
   name TEXT NOT NULL UNIQUE,
   created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
 );
 CREATE TABLE IF NOT EXISTS work_tags (
   work_id INTEGER NOT NULL REFERENCES works(id) ON DELETE CASCADE,
   tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
   PRIMARY KEY (work_id, tag_id)
 );

 CREATE TABLE IF NOT EXISTS sets (
   id INTEGER PRIMARY KEY AUTOINCREMENT,
   name TEXT NOT NULL UNIQUE,
   created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
 );
 CREATE TABLE IF NOT EXISTS work_sets (
   work_id INTEGER NOT NULL REFERENCES works(id) ON DELETE CASCADE,
   set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
   PRIMARY KEY (work_id, set_id)
 );

 CREATE TABLE IF NOT EXISTS labels (
   id INTEGER PRIMARY KEY AUTOINCREMENT,
   name TEXT NOT NULL UNIQUE,
   created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
 );
 CREATE TABLE IF NOT EXISTS studios (
   id INTEGER PRIMARY KEY AUTOINCREMENT,
   name TEXT NOT NULL UNIQUE,
   created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
 );

 CREATE TABLE IF NOT EXISTS work_ratings (
   work_id INTEGER NOT NULL REFERENCES works(id) ON DELETE CASCADE,
   source TEXT NOT NULL,           -- 'nfo' / 'javdb' / 'dmm' / ...
   value REAL NOT NULL,
   max INTEGER NOT NULL,
   votes INTEGER,
   PRIMARY KEY (work_id, source)
 );
 ```

 不动的表：`actors / actor_names / work_actors / file_versions / ingest_items /
 ingest_jobs`。

 ### 决策细化

 - studio 与 label 各建一表：studio/maker=制作方，label/publisher=发行线。
 - `works.rating` INTEGER→REAL 为破坏性变更；现有唯一 work（ABP-525）rating=NULL，实际
   无损。迁移脚本把旧 INTEGER 列值搬进新 REAL 列后删除旧列。
 - 多 CD 合并归并键 `source_code`；跨来源同 num（H:/AV 与 CinMingle 同一 num）算同一 work。

 ## NFO 解析修复（第 2 节）

 引入结构化 NFO 解析器，取代零散的 `extract_xml_tag` 调用。

 ### CDATA 修复

 所有字段值统一剥 CDATA 壳：`<![CDATA[实际内容]]>` → `实际内容`。优先级：
 CDATA 剥壳 > 实体解码，顺序不可反。无 CDATA 的值不变。

 ### 字段提取规则

 | 字段 | NFO 来源 | 归一化逻辑 |
 |---|---|---|
 | source_code | `<num>` | 原样存，trim，CDATA 剥壳 |
 | normalized_code | 派生 | 跑 `normalize_code(source_code)`，成功→standard，失败→nonstandard |
 | code_kind | 派生 | 见上 |
 | title | `<title>` | CDATA 剥壳 |
 | original_title | `<originaltitle>` | CDATA 剥壳 |
 | outline | `<outline>` | 与 plot 分离；CDATA 剥壳；空→None |
 | summary | `<plot>` | CDATA 剥壳；空→None |
 | runtime_minutes | `<runtime>` | 单位归一化（见下） |
 | year | `<year>` | parse INTEGER，失败→None |
 | release_date | `<premiered>` → `<releasedate>` → `<release>` | 取第一个非空 |
 | director | `<director>` | CDATA 剥壳；空→None |
 | website | `<website>` | 原样 |
 | mpaa | `<mpaa>` | 原样 |
 | cover_url | `<cover>` | URL 或相对路径 |
 | poster_path/thumb_path/fanart_path | `<poster>/<thumb>/<fanart>` | 相对 NFO 目录解析 |
 | actors | `<actor><name>` | 补 CDATA；空/未知过滤 |
 | tags | `<tag>` ∪ `<genre>` 去重 | 合并写入 work_tags |
 | sets | `<set><name>`（可多值） | 写入 work_sets |
 | studio | `<studio>` → `<maker>` 回退 | 写 studios |
 | label | `<label>` → `<publisher>` 回退 | 写 labels |
 | rating | 见下 | 复杂，单独处理 |

 ### runtime 单位归一化

 规则顺序（依次尝试，首个匹配成功即用，全失败→None）：

 1. `HH:MM:SS` 或 `MM:SS` → 折算分钟
 2. 纯数字 → 假定分钟
 3. 「数字 + 中文分」（分钟/分鍾/分） → 取数字部分
 4. 「数字 + min/minutes」 → 取数字部分
 5. 其他/空 → None

 每个分支一个 TDD 测试。

 ### rating 提取（优先级）

 1. 若有嵌套 `<ratings><rating name="X" max="M"><value>V</value><votes>N</votes>`：
    存 `work_ratings` 多行（一个源一行）。works 主表冗余的「默认评分」按以下优先级取：
    `default="true"` 的源 > 嵌套第一个 > 无嵌套时用顶层 `<rating>` + `<votes>`。
 2. 否则用顶层 `<rating>V</rating>` + `<criticrating>C</criticrating>` + `<votes>N`：
    works.rating_value=V，criticrating=C，rating_votes=N。rating_max 推断：有 criticrating
    且明显大于 rating → max=10；否则按 5 或 10 猜。同时写一条 `work_ratings(source='nfo', ...)`。

 不在本 spec 判断「哪个评分更权威」，先把数据存全。

 ### clean_tags

 保留 scanner.rs 现有 `clean_genres` 的清洗逻辑，但改名 `clean_tags`，在 tag+genre 合并后
 统一跑一次，过滤纯 ASCII（H264/1080P）与番号前缀（ABP）噪声。

 ## 数据采集工作流（第 3 节）

 ### 双源驱动扫描

 当前 Scanner 只扫视频，NFO 作为附属物被顺带读。改为双源驱动：

 1. 找所有视频文件（现有逻辑）→ video_items
 2. 找所有 .nfo 文件 → nfo_items
 3. 配对：每个 nfo 找同目录同名前缀的视频（nfo stem 去掉可能的 `-cdN` 后缀）
   - 有视频：nfo 元数据挂到 video_item 上
   - 无视频：nfo 单独成为一个 item（元数据有了，文件信息空）

 无视频的 NFO（纯刮削产物）也入库，has_video=0；作品库全显示但页面样式区分。

 ### 多 CD 合并算法（扫描后、入库前）

 1. 按 source_code 分组所有 nfo_items。
 2. 每组选一个「主 NFO」作为 work 元数据来源：
    - 优先 stem 最短（最像主文件）：`IPX-607.nfo` 优于 `IPX-607-C-cd3.nfo`。
    - 同长度选文件名 ASCII 序最小（确定性）。
 3. 主 NFO → work 全部元数据字段。
 4. 组内每个 NFO（含主）→ 一条 file_versions：
    - video 存在：用 video 的 size/duration/codec/hash
    - 无 video：只存 path(指向 nfo)、source_code、size=0

 ### 跨来源同 num 合并

 入库时对每个 source_code 先查 `works WHERE source_code = ?`：
 - 已存在：新 file_versions 挂到现有 work_id，**不覆盖已有非空字段，只补空字段**（保护手动数据）。
 - 不存在：新建 work。

 ## 查询 API + 作品库视图（第 4 节）

 本 spec 只交付后端 API（前端视图留下一个 spec），但 API 形状按未来前端需求设计。

 ### 新增 Tauri 命令（storage.rs 实现）

 ```
 list_tags()                  → [{id, name, work_count}]
 list_sets()                  → [{id, name, work_count}]
 list_studios()               → [{id, name, work_count}]
 list_labels()                → [{id, name, work_count}]
 list_works_by_tag(tag_id)    → [Work]
 list_works_by_set(set_id)    → [Work]
 list_works_by_actor(actor_id)→ [Work]
 list_works_by_studio(studio_id)→ [Work]
 list_works_by_label(label_id)→ [Work]
 get_work_detail(work_id)     → WorkDetail { work, actors, tags, sets, file_versions, ratings }
 list_works(filters)          → [Work]  支持多维度 AND 叠加（tag_id[], actor_id[], ...）
 ```

 `get_work_detail` 是聚合查询，一次性返回 work + 所有关联实体，减少前端往返。
 `list_works(filters)` 支持多维度叠加筛选（tag_id 数组 + actor_id 数组取交集）。

 ### Work 类型扩展（api.ts 对齐）

 - 新增：source_code, code_kind, runtime_minutes, year, release_date, label_id, set_id,
   director, website, mpaa, outline, poster_path, thumb_path, fanart_path,
   rating_value, rating_max, rating_votes, criticrating, has_video
 - tags 从 string[] 变 Tag[]（带 id）

 前端视图（卡片网格、维度面板默认折叠、多维度叠加筛选、无视频作品样式区分）留到下一个 spec。

 ## 迁移脚本与验证（第 5 节）

 ### rebuild_library_from_nfo 命令

 独立于现有 start_scan 的重建命令，避免破坏性操作与 UI 实时扫描冲突。

 ```
 rebuild_library_from_nfo(source_roots) → RebuildReport {
   nfos_scanned, works_created, works_merged, tags_extracted,
   sets_extracted, actors_extracted, file_versions_created,
   errors: [{nfo_path, message}]
 }
 ```

 后端 `Repository::rebuild_library`：

 1. 开事务。
 2. 保留 app_settings、archive_action_logs；清空其余元数据/任务表。
 3. 调用新版 Scanner 双源扫描。
 4. 执行多 CD 合并算法。
 5. 按 source_code 分组，跨来源同 num 先查已有 work。
 6. 逐组写库：主 NFO → works + 关联表；每个 NFO → file_versions。
 7. 提交事务（任一步失败 → 回滚，旧库完整保留）。
 8. 返回 RebuildReport。

 ### preview_rebuild（dry-run）

 两个交付形态：
 - Rust example `preview_rebuild.rs`：命令行快速验证（examining 模式），不写库。
 - UI 按钮（Settings 视图）：二次确认后执行 rebuild，显示 RebuildReport。

 ### UI 入口

 Settings 视图加「重建作品库」按钮，带明确警告文案（清空现有作品数据，配置与历史日志保留）。

 ## 测试策略（TDD）

 后端每个解析分支一个测试：
 - CDATA 剥壳：纯 CDATA / 半 CDATA / 无 CDATA
 - runtime 归一化：纯数字 / 中文分 / 英文 min / HH:MM:SS / 失败
 - rating 嵌套：有嵌套 ratings / 仅顶层 / 都没有
 - 多 CD 合并：同 num 多 NFO → 一 work 多 version
 - 跨来源同 num：两目录同 num → 一 work
 - 非番号 num：code_kind=nonstandard，normalized_code=None
 - 全量重建事务：中途失败 → 旧库不变
 - 多维度叠加查询（AND 逻辑）

 ## 交付边界

 本 spec 只交付后端：schema + NFO 解析 + 多 CD 合并 + rebuild/preview 命令 + dry-run
 example + 查询 API + 后端测试。前端作品库视图（卡片网格、维度面板、叠加筛选交互）留到
 下一个 spec。

 ## 决策记录（全部已确认）

 | 决策 | 选择 |
 |---|---|
 | 数据模型 | 方案 B：规范化关系模型（tag/set/studio/label/ratings 建表） |
 | tag 与 genre | 统一 tag（合并写入 work_tags） |
 | 非番号 num | 分开：source_code 原样存 + code_kind 区分 |
 | 多 CD | 合并到同一 work（多 file_version） |
 | outline/plot | 分开存 |
 | 默认评分优先级 | 嵌套 default=true > 嵌套第一个 > 顶层 |
 | 四类图 | 分开存（cover_url/poster/thumb/fanart 各一列） |
 | 无视频 NFO | 入库，has_video=0，页面样式区分 |
 | 跨来源同 num | 算同一 work；重扫只补空字段不覆盖 |
 | 重扫策略 | 全量重建元数据表，保留 app_settings 与 archive_action_logs |
 | 维度面板 | 默认折叠（前端 spec，此处仅定形） |
 | 多维度叠加筛选 | 支持 AND（前端 spec，此处仅定 API） |
 | 作品库主区 | 卡片网格（前端 spec，此处仅定形） |
 | dry-run | example + UI 按钮都做 |
 | 交付形态 | 后端先行，前端视图留下一 spec |
 -- 旧 rating INTEGER 列：迁移逻辑把值搬进 rating_value 后，因旧 SQLite 对 DROP COLUMN
 -- 支持不一致，物理列保留但不再读写（写入路径全部走 rating_value）。
 -- 旧 genres_json 列同理保留但不再写入，避免破坏性 DROP COLUMN 的兼容问题。
 get_work_detail(work_id)     → WorkDetail { work, actors, tags, sets, file_versions, ratings }
 list_works(filters)          → [Work]  支持多维度 AND 叠加（tag_id[], actor_id[], ...）
 ```

 `get_work_detail` 是聚合查询，一次性返回 work + 所有关联实体，减少前端往返。
 `list_works(filters)` 支持多维度叠加筛选（tag_id 数组 + actor_id 数组取交集）。
 filters 为空对象或缺省时退化为「列出全部」，与现有无参 `list_works()` 行为等价；
 现有 `list_works()` 命令保留向后兼容（等价于 `list_works({})`）。
 rebuild_library_from_nfo(source_roots) → RebuildReport {
   nfos_scanned, works_created, works_merged, tags_extracted,
   sets_extracted, actors_extracted, file_versions_created,
   errors: [{nfo_path, message}]
 }
 ```

 `works_merged` 计数 = 多 NFO 同 num 组数（每个被折叠成单 work 的组 +1）；单 NFO 作品
 不计入。`works_created` = 实际写入的 work 总数（含合并后的）。
