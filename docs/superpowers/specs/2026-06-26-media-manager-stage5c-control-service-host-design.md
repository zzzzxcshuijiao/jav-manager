# 阶段 5C 控制服务宿主与生命周期设计

阶段 5C 采用方案 A：**Tauri 应用内托管控制服务**。目标是在不启动 Tauri GUI、不实现托盘/自启/WebSocket 的前提下，把阶段 5A 的 loopback REST 服务接入真实应用初始化路径，让阶段 5B 的前端 service-first 客户端在实际桌面运行时能走真实 discovery + REST 路径。

## 背景

阶段 5A 已经提供 `ControlServiceRuntime::start()`，能绑定 loopback、写 discovery、提供 REST API。阶段 5B 已经让前端自动管线面板优先读取 discovery 并调用 REST，失败时回退 Tauri command bridge。

当前缺口是：Tauri 应用启动时并不会启动控制服务，`control-service.json` 只会在测试或未来宿主手动调用时出现。因此 5B 的“本地服务”通道在真实桌面环境中还无法自然体现。

## 目标

- Tauri `setup` 初始化 SQLite 后，自动启动 loopback 控制服务。
- 控制服务使用 app data 下的同一个 `library.sqlite`，另开一个 SQLite connection，避免移动或共享 UI command bridge 的 `Repository`。
- discovery 文件固定为 app data 下 `control-service.json`，继续复用阶段 5B helper。
- `AppState` 保存 `ControlServiceHandle`，应用状态释放时停止 listener。
- shutdown 时删除 discovery 文件，避免下次启动前遗留过期 endpoint/token。
- 增加只读宿主状态 command，供前端或诊断工具查看服务是否运行、端口、discovery 路径和最近启动错误。
- 控制服务读取 metadata provider 开关时避免启动时快照过期；运行状态和 run-once 使用 SQLite 中的当前设置。

## 非目标

- 不实现独立托盘 daemon、开机自启、Windows Service 或后台文件监听循环。
- 不实现 WebSocket 或实时进度推送。
- 不接真实网络 scraper。
- 不移除阶段 4 command bridge fallback。
- 不在 Codex 会话中运行 `tauri dev`、默认 `cargo run`、`media-manager.exe` 或 in-app browser。

## 架构

新增 `src-tauri/src/control_service_host.rs`，职责是把“应用数据目录”转换为可启动的服务宿主：

- `ControlServiceHostStatus`：只读状态 DTO，包含 `running`、`host`、`port`、`discovery_path`、`last_error`。
- `build_control_service_config(app_data_dir, metadata_provider_enabled)`：生成 loopback、随机端口、固定 discovery 路径的配置。
- `start_control_service_host(app_data_dir)`：打开并迁移 `library.sqlite`，读取当前 metadata provider 开关，启动 `ControlServiceRuntime`。
- `control_service_host_status(app_data_dir, handle, last_error)`：把当前 handle 和启动错误转换为 command DTO。

`src-tauri/src/control_service.rs` 做两处小扩展：

- `ControlServiceHandle` 记录 discovery path，并在 `shutdown()` 时删除 discovery 文件。
- `ControlServiceRuntime` 在 status/run-once 时从 SQLite 读取当前 `metadata_provider_enabled`，读失败时才退回配置里的启动默认值。

`src-tauri/src/commands.rs` 做宿主接入：

- `AppState` 增加 `control_service: Mutex<Option<ControlServiceHandle>>` 和 `control_service_error: Mutex<Option<String>>`。
- `setup` 完成 repository 初始化后调用 `start_control_service_host(&app_data)`。成功则保存 handle；失败则保存错误但不阻止应用启动，前端仍可通过命令桥 fallback 工作。
- `AppState::drop` 尝试 shutdown handle，确保进程退出时释放 listener 并删除 discovery。
- 新增 command `get_control_service_host_status`，注册到 `generate_handler!`。

前端只做轻量接入：

- `src/api.ts` 增加 `ControlServiceHostStatus` DTO 和 `getControlServiceHostStatus()`。
- 阶段 5B 的自动管线面板继续以 `getDaemonControlChannel()` 显示实际通道；5C 不新增复杂 UI。

## 数据流

```text
Tauri setup
  -> app.path().app_data_dir()
  -> open_repository(app_data/library.sqlite)  // UI command bridge
  -> start_control_service_host(app_data)
       -> Repository::open(app_data/library.sqlite)  // service connection
       -> ControlServiceRuntime::start()
       -> write app_data/control-service.json
  -> AppState.control_service = Some(handle)

Settings 自动管线面板
  -> api.getDaemonStatus()
  -> daemonClient reads get_control_service_discovery()
  -> REST /v1/status when service is alive
  -> fallback Tauri command bridge when service is missing/unhealthy
```

## 错误处理

- 服务启动失败不让 Tauri setup fail；记录 `control_service_error`，前端仍可走 command bridge。
- discovery 文件缺失或损坏仍由阶段 5B 客户端 fallback。
- shutdown 删除 discovery 失败不 panic，只忽略清理错误。
- 服务业务错误仍不 fallback；这是阶段 5B 已定规则。

## 测试策略

- 新增 `src-tauri/tests/control_service_host.rs`：
  - 配置 helper 生成 loopback、随机端口、app-data discovery 路径。
  - `start_control_service_host` 使用 tempdir SQLite 启动服务，写 discovery，`/health` 可访问。
  - `shutdown` 后 discovery 文件被删除。
  - host status 能表达 running / not running / last_error。
- 扩展 `src-tauri/tests/control_service.rs`：
  - metadata provider 开关在服务启动后改动，REST status/run-once 读取 SQLite 当前值，不停留在启动快照。
- 前端只跑 `npm test`、`npx tsc --noEmit`、`npm run build`，不做 WebView2 渲染。

## 验收

- 5C 设计文档和 implementation plan 已提交。
- Rust focused tests 覆盖宿主 helper、discovery 清理和 metadata provider 动态读取。
- 全量 `cargo test --manifest-path src-tauri/Cargo.toml -j 1` 通过。
- `npm test`、`npx tsc --noEmit`、`npm run build` 通过。
- `HANDOFF.md` 更新到阶段 5C。
