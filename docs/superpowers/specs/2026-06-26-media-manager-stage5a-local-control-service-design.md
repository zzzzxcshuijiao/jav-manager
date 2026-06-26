# 阶段 5A 本地控制服务基座设计

阶段 5A 采用方案 A：**纯 Rust loopback REST 控制服务基座**。目标不是一次性完成托盘、自启、WebSocket、真实网络 scraper 或长期文件监听，而是把阶段 4 的同步 Tauri 命令桥抽象成可独立运行、可测试、可被前端或未来托盘进程消费的本地控制接口。

## 1. 背景

阶段 1 到阶段 4 已经完成数据模型、自动管线核心、无头守护核心和设置页控制面。阶段 4 的控制层仍依赖 Tauri command 调用：前端可以运行一轮、暂停/恢复、看搁置区、异常队列和 pipeline_runs，但控制接口还没有服务化。

全局设计要求最终 daemon 监听 `127.0.0.1:<port>`，前端通过本地 HTTP/WebSocket 控制它，并且所有写操作经 daemon 串行化进入 SQLite。阶段 5A 先实现这个方向的最小可验证基座：loopback REST + token + 端口发现文件 + 已有 daemon 控制能力。

## 2. 目标

- 新增纯 Rust 本地控制服务，不启动 Tauri GUI、WebView2 或托盘。
- 只绑定 loopback 地址，禁止非 loopback 绑定。
- 启动时生成或接收高熵 token，所有业务 API 都要求 `Authorization: Bearer <token>`。
- 启动后写入端口发现文件，包含 host、port、token、pid、created_at，供前端或未来托盘发现。
- 提供 REST API：
  - `GET /health`：不要求 token，只返回服务活性和版本信息。
  - `GET /v1/status`：读取 daemon 状态摘要。
  - `POST /v1/pause`：暂停自动管线。
  - `POST /v1/resume`：恢复自动管线。
  - `POST /v1/run-once`：执行一次“扫描 + drain 队列”。
  - `GET /v1/holding`：列出搁置区。
  - `GET /v1/exceptions`：列出异常队列。
  - `POST /v1/exceptions/{id}/resolve`：把异常标记为 resolved。
  - `GET /v1/runs`：列出最近 pipeline_runs。
- 所有业务响应沿用阶段 4 的 DTO 语义，避免前端 5B 迁移时改两套模型。
- 集成测试使用临时 SQLite、临时目录和假视频，不依赖真实 H:/ 或 G:/ 资源。

## 3. 非目标

- 不实现 WebSocket 推送；阶段 5B/5C 再做实时进度流。
- 不实现托盘菜单、自启、Windows Service 或后台安装器。
- 不接入真实 FANZA/JavBus/JavDB 网络 scraper。
- 不改变阶段 4 设置页 UI 调用路径；前端迁移到 HTTP client 留到 5B。
- 不让 Codex 启动 Tauri GUI、WebView2、`media-manager.exe` 或默认 `cargo run`。

## 4. 方案比较

### 方案 A：标准库 TCP + 极小 HTTP 解析器

优点是依赖最少、测试快、不会引入 async runtime 和额外框架迁移。阶段 5A 的 API 很窄，完全可以用 `TcpListener`、短连接和 `serde_json` 完成。缺点是 HTTP 解析能力有限，只适合作为 loopback 控制 API 的基座，后续 WebSocket 或复杂中间件需要迁移。

### 方案 B：直接引入 Axum/Tokio

优点是接近最终服务形态，后续 WebSocket、middleware、tower 测试会更自然。缺点是阶段 5A 会同时引入 async runtime、依赖版本、shutdown、state clone、测试 harness 等复杂度，容易把本阶段变成框架落地而不是控制契约验证。

### 方案 C：继续只用 Tauri commands

优点是改动小。缺点是没有服务化进展，不能验证端口发现、token、loopback 限制，也无法为托盘 daemon 或未来移动端/远程客户端铺路。

推荐方案 A。5A 的关键价值是把控制契约和安全边界先跑通，保持实现小而可测；当 5B/5C 需要 WebSocket 或更复杂生命周期时，再用这些已固定的 DTO 和测试迁移到底层框架。

## 5. 架构

新增 `src-tauri/src/control_service.rs`，职责是本地控制服务本身：

- `ControlServiceConfig`：监听 host、端口、发现文件路径、token、sqlite 路径、元数据源开关。
- `ControlServiceRuntime`：保存 `Repository`、`DaemonControlRuntime`、token、服务元信息，并串行执行请求。
- `ControlServiceHandle`：测试和未来托盘进程用的启动句柄，提供实际端口、发现文件内容和 shutdown。
- `ControlRequest` / `ControlResponse`：极小 HTTP 解析和 JSON 响应模型。

服务不直接重写阶段 3/4 逻辑，而是调用 `daemon_control` 中已有 helper：

```text
HTTP request
  -> token/origin/path/method validation
  -> ControlServiceRuntime
  -> Repository + DaemonControlRuntime
  -> daemon_control helper
  -> JSON response
```

锁策略保持简单：阶段 5A 的服务线程逐请求串行处理；单个请求持有 repo/runtime 所需锁直到完成。它不是最终高并发服务，但与“daemon 是唯一写入者”的方向一致，也避免 SQLite 并发写入。

## 6. 安全边界

- 服务只允许 `127.0.0.1` 或 `localhost` 配置；其他 host 直接返回配置错误。
- 业务 API 缺 token 返回 `401`；token 错误返回 `403`。
- `Origin` 存在时只允许 `tauri://localhost`、`http://127.0.0.1:*`、`http://localhost:*`；未知 Origin 返回 `403`。
- token 默认由 `SystemTime`、进程 id 和当前线程时间混合生成，再用 SHA-256 编码。阶段 5A 不承诺密码学级密钥管理，但接口形态保留；5C 可替换为系统随机源。
- 发现文件只写入用户指定路径；测试使用临时目录。未来产品路径应放在用户态私有 app config 下。

## 7. REST 契约

所有 JSON 响应统一格式：

```json
{
  "ok": true,
  "data": {}
}
```

错误响应统一格式：

```json
{
  "ok": false,
  "error": "message"
}
```

主要 endpoint：

| Method | Path | Auth | Body | Result |
| --- | --- | --- | --- | --- |
| GET | `/health` | no | none | `{ "service": "media-manager-control", "status": "ok" }` |
| GET | `/v1/status` | yes | none | `DaemonControlStatus` |
| POST | `/v1/pause` | yes | none | `DaemonControlStatus` |
| POST | `/v1/resume` | yes | none | `DaemonControlStatus` |
| POST | `/v1/run-once` | yes | none | `RunOnceReport` |
| GET | `/v1/holding` | yes | none | `HoldingEntry[]` |
| GET | `/v1/exceptions` | yes | none | `Exception[]` |
| POST | `/v1/exceptions/{id}/resolve` | yes | none | `Exception` |
| GET | `/v1/runs` | yes | none | `PipelineRun[]` |

阶段 5A 不支持请求体字段扩展；异常解决固定写 `resolved`。后续人工处理策略如果需要“忽略、重试、改番号、合并”等动作，再新增显式 endpoint。

## 8. 测试策略

Rust 集成测试新增 `src-tauri/tests/control_service.rs`：

- 启动服务时只绑定 loopback，发现文件写入实际端口和 token。
- `/health` 不带 token 成功。
- `/v1/status` 不带 token 返回 `401`，错误 token 返回 `403`，正确 token 返回状态。
- 未开启元数据源时，`POST /v1/run-once` 返回错误并且不处理真实文件，沿用阶段 4 安全保护。
- 开启示例元数据源时，临时目录中的假视频可以通过 `POST /v1/run-once` 入库归档。
- `GET /v1/holding`、`GET /v1/exceptions`、`POST /v1/exceptions/{id}/resolve`、`GET /v1/runs` 能读写 SQLite 中已有队列数据。
- 非 loopback host 配置启动失败。

完整验证仍使用：

- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`
- `npm test`
- `npx tsc --noEmit`
- `npm run build`

## 9. 交付标准

- 5A 设计文档和 implementation plan 均已提交。
- Rust 新增服务模块有单元/集成测试，测试只用临时目录。
- 不运行任何 Tauri GUI、WebView2、默认 `cargo run` 或 `media-manager.exe`。
- 阶段 4 UI 仍可编译，后续 5B 可以基于发现文件和 REST endpoint 迁移前端。
- HANDOFF 和 `.ai_state` 更新到阶段 5A 当前状态。

## 10. 自审

- 无未定占位符。
- 5A 范围只覆盖本地 REST 服务基座，未把 WebSocket、托盘、自启、真实 scraper 偷渡进来。
- 安全模型与全局设计一致：loopback、token、Origin、发现文件。
- 测试策略不依赖真实媒体资源，也不需要启动 WebView2。
