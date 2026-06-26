# 阶段 5B 前端服务客户端迁移设计

阶段 5B 采用方案 A：**服务优先、命令桥兜底**。目标是在不启动 Tauri GUI、不引入托盘/自启、不要求真实 daemon 常驻的前提下，把设置页“自动管线”面板迁移到阶段 5A 的 discovery + REST client 形态。没有可用服务时，前端自动退回阶段 4 的 Tauri command bridge，保证当前桌面体验不倒退。

## 1. 背景

阶段 5A 已经提供纯 Rust loopback REST 控制服务基座：服务写 discovery JSON，业务 API 使用 Bearer token，并暴露 status、pause/resume、run-once、holding、exceptions、resolve 和 runs。

阶段 4 UI 仍直接调用 Tauri commands。5B 要把“自动管线”前端调用边界改成服务客户端形状，为未来托盘 daemon、自启和 WebSocket 留出空间。但当前 Codex 环境仍不能运行 Tauri GUI/WebView2，也不能实际启动 `media-manager.exe`。因此 5B 必须可通过 TypeScript 单元测试、Rust 单元测试、`tsc` 和 build 验证。

## 2. 目标

- 前端新增独立 daemon service client，优先读取 discovery 并调用 5A REST API。
- discovery 不由浏览器直接读文件；新增一个很薄的 Tauri command 读取 app data 下的 discovery JSON。
- 当 discovery 缺失、服务不可达、token 失效或健康检查失败时，自动回退到阶段 4 command bridge。
- 当服务返回业务错误（例如 `run-once` 返回 500 且包含“示例元数据源未开启”）时，不回退命令桥，直接把服务错误展示给用户，避免重复执行或掩盖真实服务状态。
- 设置页 UI 保持阶段 4 结构，只增加“控制通道”状态：本地服务 / 命令桥 / 未连接。
- 继续保留所有长耗时操作的 loading/disabled/status 反馈。
- 测试不依赖真实 H:/、G:/、真实服务进程或 Tauri WebView2。

## 3. 非目标

- 不启动或管理控制服务进程。
- 不做托盘、自启、Windows Service、后台文件监听或 WebSocket。
- 不接真实网络 scraper。
- 不迁移全应用 API；5B 只迁移自动管线控制面。
- 不移除阶段 4 Tauri commands；它们作为过渡 fallback 保留。

## 4. 方案比较

### 方案 A：服务优先、命令桥兜底

前端每次自动管线操作先通过 Tauri command 获取 discovery。若 discovery 存在且 `/health` 正常，则调用 REST；若 discovery 缺失或网络不可达，则调用现有 Tauri commands。

优点是最稳：没有常驻 daemon 时 UI 仍可用；有 daemon 时前端路径已经是服务化形态。缺点是 5B 期间同时存在两条控制路径，需要清楚测试 fallback 规则。

### 方案 B：只用 REST，去掉命令桥

优点是架构更干净。缺点是当前没有托盘 daemon 和自启，用户打开桌面 app 时很可能没有 REST 服务，自动管线面板会直接不可用。

### 方案 C：只写 HTTP client，不接 UI

优点是改动小。缺点是阶段 5B 不能体现真实效果，UI 仍在用 command bridge，迁移价值不足。

推荐方案 A。它把服务客户端边界真实接进 UI，同时保留阶段 4 可用性。

## 5. 架构

### 5.1 Rust discovery command

在 `src-tauri/src/control_service.rs` 增加 discovery 文件 helper：

- `CONTROL_SERVICE_DISCOVERY_FILE = "control-service.json"`。
- `control_service_discovery_path(app_data_dir: &Path) -> PathBuf`。
- `read_control_service_discovery(path: &Path) -> Result<Option<ControlServiceDiscovery>>`。

在 `src-tauri/src/commands.rs` 增加 Tauri command：

- `get_control_service_discovery(app: tauri::AppHandle) -> Result<CommandResult<Option<ControlServiceDiscovery>>, String>`。
- 它只读 app data 下的 discovery 文件，不启动服务，不写文件。
- 文件不存在返回 `data: null`，JSON 损坏返回错误。

### 5.2 TypeScript daemon client

新增 `src/daemonClient.ts`，作为纯 TypeScript 单元：

- 输入依赖通过构造函数注入：
  - `command<T>(name, args?)`：现有 Tauri command bridge。
  - `fetch(input, init)`：浏览器 fetch，测试里注入 fake fetch。
  - `getDiscovery()`：默认走 `get_control_service_discovery` command。
- 输出自动管线操作：
  - `getStatus`
  - `pause`
  - `resume`
  - `runOnce`
  - `listHolding`
  - `listExceptions`
  - `resolveException`
  - `listRuns`
- 维护 `channel` 状态：
  - `service`：最近一次成功使用 REST。
  - `command`：最近一次使用 fallback command bridge。
  - `none`：尚未成功调用。

fallback 规则：

- discovery 为 `null`：fallback 到 command。
- `/health` 网络失败、401/403、非 2xx、响应 envelope 非 ok：fallback 到 command。
- 业务 endpoint 网络失败、401/403、服务不可达：fallback 到 command。
- 业务 endpoint 返回 4xx/5xx 且 envelope 中有业务错误：抛出错误，不 fallback。

### 5.3 `src/api.ts` 接入

`src/api.ts` 保留全局 `api` 外形。只有自动管线相关方法改为调用 daemon client：

- `getDaemonStatus`
- `pauseDaemon`
- `resumeDaemon`
- `runDaemonOnce`
- `listHoldingEntries`
- `listExceptionEntries`
- `resolveExceptionEntry`
- `listPipelineRuns`
- 新增 `getDaemonControlChannel`

其余 API 继续走原 command bridge，不做横向迁移。

### 5.4 设置页 UI

`src/App.tsx` 新增 `daemonChannel` state。`loadDaemonPanelData` 完成后读取 `api.getDaemonControlChannel()`，自动管线状态标题显示：

- 本地服务：REST discovery + service path 成功。
- 命令桥：fallback command bridge。
- 未连接：尚未成功读取。

保持现有 loading/disabled 逻辑。顺手修复阶段 4 reviewer 提到的暂停态 UX：`daemonStatus.state === "Paused"` 时禁用“运行一轮”，避免显示“扫描 0 个文件”的误导。

## 6. API 契约

### Tauri command

```ts
api.getControlServiceDiscovery(): Promise<ControlServiceDiscovery | null>
```

DTO：

```ts
interface ControlServiceDiscovery {
  service: "media-manager-control" | string;
  host: string;
  port: number;
  base_url: string;
  token: string;
  pid: number;
  created_at: string;
}
```

### REST envelope

```ts
type ControlServiceEnvelope<T> =
  | { ok: true; data: T }
  | { ok: false; error: string };
```

HTTP client 必须解析 envelope。不能直接假设 body 就是 DTO。

## 7. 测试策略

### TypeScript

新增 `src/daemonClient.test.ts`：

- discovery 缺失时，自动调用 command bridge。
- discovery 存在且 health/status 成功时，使用 REST，并带上 Bearer token。
- REST endpoint 返回业务错误时，不 fallback，直接抛错。
- 网络失败或 token 失败时 fallback command。
- resolve exception 在 REST 下调用 `/v1/exceptions/{id}/resolve`，在 command fallback 下调用 `resolve_exception_entry_command`。

### Rust

在现有 Rust 测试中覆盖 discovery helper：

- discovery 文件不存在返回 `None`。
- discovery 文件存在时能反序列化 `ControlServiceDiscovery`。
- 损坏 JSON 返回错误。

完整验证继续使用：

- `cargo test --manifest-path src-tauri/Cargo.toml -j 1`
- `npm test`
- `npx tsc --noEmit`
- `npm run build`

## 8. 风险与边界

- 5B 不保证真实服务已经启动；fallback 是阶段 5B 的产品保护。
- discovery 文件路径必须与未来 daemon 启动配置统一；5B 先固定为 app data 下 `control-service.json`。
- REST fallback 不能吞掉业务错误，否则可能重复执行 run-once 或隐藏服务真实失败。
- Codex 不做视觉验证；用户后续在真实桌面运行 `npm run dev` / Tauri GUI 时再看 UI。

## 9. 交付标准

- 5B 设计文档和 implementation plan 均已提交。
- Rust discovery command 已注册且有测试。
- TypeScript daemon client 有 fallback 和 error tests。
- 设置页自动管线面板可以显示控制通道，并在暂停态禁用运行按钮。
- 全量验证通过，HANDOFF 更新到阶段 5B。

## 10. 自审

- 无未定占位符。
- 5B 范围只迁移自动管线前端调用边界，不启动 daemon、不做托盘、不做 WebSocket。
- discovery、REST envelope、fallback 规则明确。
- 测试策略不依赖真实服务或真实媒体资源。
