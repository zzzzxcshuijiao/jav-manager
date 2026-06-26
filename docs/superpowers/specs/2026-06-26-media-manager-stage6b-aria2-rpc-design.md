# 阶段 6B aria2 RPC 集成骨架设计

## 背景

阶段 2 已经实现 `Aria2TaskSnapshot` 和完成判定：`status == "complete"` 且 `completed_length == total_length`。阶段 3 daemon 目前只通过本地目录启发式判断完成：视频文件稳定、可读、同名 `.aria2` 控制文件不存在。这个兜底能在没有 aria2 的环境里开发，但还没有真正消费 aria2 的下载状态。

阶段 6B 的目标是补上 aria2 JSON-RPC 到自动管线之间的可测试桥梁，让后续可以从“aria2 下载任务完成”直接得到 `CompletedFile` 候选，再交给现有 daemon / `AutoPipeline` 处理。

参考契约来自 aria2 官方手册：

- JSON-RPC HTTP 路径是 `/jsonrpc`。
- method-level secret 作为第一个参数，格式为 `token:<secret>`。
- `aria2.tellStatus` 可返回 `gid`、`status`、`totalLength`、`completedLength`、`files` 等字段。
- 字段值多为字符串，`files[].selected` 表示多文件任务中该文件是否被选中。
- HTTP JSON-RPC 不提供通知，WebSocket 才有通知；本阶段先做轮询式客户端，WebSocket 留到后续。

官方文档：https://aria2.github.io/manual/en/html/aria2c.html#rpc-interface

## 目标

- 新增纯 Rust `aria2` 模块，封装 JSON-RPC 请求、响应解析、错误语义和完成文件提取。
- 支持注入 transport，用假 transport 测试 RPC 逻辑，不依赖真实 aria2 进程、真实下载目录或网络环境。
- 支持真实 HTTP POST transport 的最小实现，目标是本机 aria2 默认端口或用户配置端点；测试使用临时 TCP server。
- 将 aria2 返回的 task status 转为阶段 2 已有 `Aria2TaskSnapshot`，复用 `is_aria2_complete`。
- 从完成任务的 `files` 中提取已选中、已完成、存在于本地、且是视频文件的 `CompletedFile`。
- 给 daemon 增加一个显式的“从 aria2 GID 扫描完成文件”的可测试入口，后续真实轮询器或 UI 配置可以复用。

## 非目标

- 不启动或管理 aria2 进程。
- 不新增下载任务、暂停/恢复下载、限速、队列管理或 UI 下载面板。
- 不做 WebSocket 通知、`onBtDownloadComplete` 回调或常驻后台轮询线程。
- 不要求真实 H:/ 视频、真实 G:/ 图片库或真实 aria2 服务参与测试。
- 不改变阶段 3 的目录启发式兜底；未配置 aria2 时仍然按现有扫描逻辑工作。

## 方案对比

### 方案 A：只做 DTO 和假 transport

优点是最小、风险低；缺点是没有真实 HTTP 边界，后面仍要补一轮传输层和错误处理。

### 方案 B：DTO + 注入 transport + 最小 HTTP POST transport（推荐）

优点是既能用假 transport 精准测试 JSON-RPC，又能用临时 TCP server 验证真实 HTTP 请求格式和响应解析；真实环境只需配置 aria2 endpoint 就可以试。复杂度仍可控，因为只支持 POST `/jsonrpc` 和 JSON body。

### 方案 C：直接做 daemon 常驻轮询器

优点是更接近最终形态；缺点是会同时引入定时器、生命周期、配置热更新和错误退避，容易扩大阶段 6B 范围。

本阶段采用方案 B。

## 架构

新增 `src-tauri/src/aria2.rs`，职责集中在 aria2 RPC 和下载完成文件提取：

- `Aria2RpcEndpoint`：host、port、path、secret、timeout。
- `Aria2Transport` trait：输入 endpoint 和 JSON body，输出 JSON body 字符串。
- `HttpAria2Transport`：标准库 `TcpStream` POST 实现，只支持 loopback / 用户配置 host 的 HTTP，不引入新依赖。
- `Aria2Client<T>`：构建 `aria2.tellStatus` 请求、调用 transport、解析 JSON-RPC response。
- `Aria2Status` / `Aria2File`：贴近 aria2 返回形状，保留字符串字段并提供安全数值转换。
- `Aria2CompletedSelection`：完成任务中可交给管线处理的本地视频文件集合。

现有 `pipeline.rs` 保持拥有完成判定函数：

- `Aria2Status::to_task_snapshot()` 转换成 `pipeline::Aria2TaskSnapshot`。
- `aria2` 模块调用 `pipeline::is_aria2_complete()` 判定任务完成。

`daemon.rs` 增加显式入口，不改变默认 `scan_now()` 行为：

- `scan_aria2_gid(&mut self, client: &Aria2Client<_>, gid: &str) -> Result<ScanReport>`
- 当 aria2 task 未完成时返回 `queued_files = 0`，不写 SQLite。
- 当完成时，将 selected + completed + 本地视频文件转成 `CompletedFile` 并复用现有 `queue_completed_file` 去重。
- 路径不存在、非视频、未选中、单文件未完成都计入 skipped。

## 数据流

```text
aria2 gid
  -> Aria2Client::tell_status(gid)
  -> JSON-RPC POST /jsonrpc
  -> Aria2Status
  -> Aria2TaskSnapshot + is_aria2_complete
  -> selected completed files
  -> CompletedFile::from_path
  -> HeadlessDaemon queue
  -> process_next / run_once
  -> AutoPipeline
```

阶段 6B 不把 aria2 任务持久化到 SQLite。GID 来源可以是后续 UI、配置、外部回调或轮询器；本阶段只把“给定 GID，能否拿到完成文件并入队”做扎实。

## 错误处理

- HTTP 连接失败、非 2xx 响应、响应缺失 body、JSON 解析失败：返回明确 `anyhow` 错误，调用方可把 daemon 状态置为 Error。
- JSON-RPC `error` 字段：返回包含 code/message 的错误，不 fallback 到目录启发式，以免掩盖 aria2 配置或鉴权问题。
- aria2 数值字段解析失败：视为 RPC 数据错误。
- 未完成任务：不是错误，返回空完成集合。
- 完成任务没有可处理视频：不是错误，返回 skipped 计数，后续可在 UI 展示诊断。
- `selected == "false"` 的文件跳过，避免处理用户未选择下载的 BT 子文件。

## 测试策略

全部测试自包含：

- `src-tauri/tests/aria2_rpc.rs`
  - secret 参数被放在第一个 `params` 元素。
  - `tellStatus` 成功响应能解析字符串数值和 files。
  - JSON-RPC error 转为错误。
  - 完成任务只提取 selected、completed、本地存在的视频文件。
  - 未完成任务不提取文件。
  - 临时 TCP server 验证 HTTP transport 会 POST `/jsonrpc`，带 JSON body，并能读取 response。
- `src-tauri/tests/daemon.rs`
  - `scan_aria2_gid` 对完成任务入队。
  - 重复 `scan_aria2_gid` 不重复入队。
  - 未完成任务不入队、不报错。

完整 gate 仍然使用：

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH
cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

## 实施顺序

1. 新增 aria2 RPC 单元测试和模块骨架，先覆盖请求构建、响应解析和错误语义。
2. 实现假 transport 驱动的 `Aria2Client::tell_status`。
3. 实现完成文件提取，复用 `is_aria2_complete` 和 `CompletedFile::from_path`。
4. 实现最小 HTTP POST transport，并用临时 TCP server 测试。
5. 在 daemon 增加 `scan_aria2_gid`，复用 queue 去重和 `ScanReport`。
6. 更新 `.ai_state`、`HANDOFF.md`、review / lessons，并提交。

## 验收

- 不依赖真实 aria2、真实媒体盘或 WebView2。
- aria2 RPC 契约有独立测试覆盖。
- daemon 可以从一个 fake aria2 完成任务入队 `CompletedFile`。
- 原有目录启发式扫描行为不回退。
- 全量 Rust / 前端验证通过。
