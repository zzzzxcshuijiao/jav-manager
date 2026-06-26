# Stage 5B Frontend Service Client Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate the automatic pipeline settings panel to a service-first daemon client that discovers the Stage 5A REST service and falls back to the existing Tauri command bridge.

**Architecture:** Add a tiny Rust discovery command that reads `control-service.json` from the app data directory, then add a pure TypeScript `daemonClient` that chooses REST or command bridge per operation. `src/api.ts` keeps the public API shape stable while routing only daemon methods through the new client, and `src/App.tsx` shows the selected control channel.

**Tech Stack:** Rust/Tauri command layer, React/TypeScript, Vitest, existing Stage 5A REST DTOs, existing Tauri command bridge.

---

## File Structure

- Modify `src-tauri/src/control_service.rs`
  - Add discovery filename/path/read helpers for app-data discovery.
- Modify `src-tauri/src/commands.rs`
  - Add and register `get_control_service_discovery`.
- Modify `src-tauri/tests/control_service.rs`
  - Cover missing, valid, and corrupt discovery files.
- Create `src/daemonClient.ts`
  - Pure TypeScript daemon control client with injected command/fetch/discovery dependencies.
- Create `src/daemonClient.test.ts`
  - Unit tests for REST success, command fallback, business error handling, and resolve routing.
- Modify `src/api.ts`
  - Export `ControlServiceDiscovery`, `DaemonControlChannel`, `getControlServiceDiscovery`, `getDaemonControlChannel`, and route daemon methods through the new client.
- Modify `src/App.tsx`
  - Track and display daemon control channel; disable run-once while paused.
- Modify `.ai_state/tasks.md`, `.ai_state/progress.md`, `.ai_state/reviews/sprint-6.md`, `HANDOFF.md`.

## Task 1: Rust Discovery Helpers and Command

**Files:**
- Modify: `src-tauri/src/control_service.rs`
- Modify: `src-tauri/src/commands.rs`
- Test: `src-tauri/tests/control_service.rs`

- [ ] **Step 1: Write failing discovery helper tests**

Append to `src-tauri/tests/control_service.rs`:

```rust
use media_manager::control_service::{
    control_service_discovery_path, read_control_service_discovery, ControlServiceDiscovery,
    CONTROL_SERVICE_DISCOVERY_FILE,
};

#[test]
fn discovery_file_helpers_read_missing_valid_and_corrupt_files() {
    let tmp = tempfile::tempdir().unwrap();
    let discovery_path = control_service_discovery_path(tmp.path());

    assert_eq!(discovery_path, tmp.path().join(CONTROL_SERVICE_DISCOVERY_FILE));
    assert!(read_control_service_discovery(&discovery_path).unwrap().is_none());

    let discovery = ControlServiceDiscovery {
        service: "media-manager-control".to_string(),
        host: "127.0.0.1".to_string(),
        port: 45123,
        base_url: "http://127.0.0.1:45123".to_string(),
        token: "stage5-token".to_string(),
        pid: 123,
        created_at: "2026-06-26T00:00:00Z".to_string(),
    };
    std::fs::write(&discovery_path, serde_json::to_string(&discovery).unwrap()).unwrap();

    assert_eq!(read_control_service_discovery(&discovery_path).unwrap(), Some(discovery));

    std::fs::write(&discovery_path, "{not-json").unwrap();
    assert!(read_control_service_discovery(&discovery_path).is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service discovery_file_helpers_read_missing_valid_and_corrupt_files -j 1
```

Expected: FAIL because the discovery helper functions and constant do not exist.

- [ ] **Step 3: Implement helpers and Tauri command**

In `src-tauri/src/control_service.rs`, add:

```rust
use std::path::{Path, PathBuf};

pub const CONTROL_SERVICE_DISCOVERY_FILE: &str = "control-service.json";

/// Return the canonical Stage 5B discovery file path under the app data dir.
pub fn control_service_discovery_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CONTROL_SERVICE_DISCOVERY_FILE)
}

/// Read the service discovery document if it exists. Missing files mean that
/// no daemon service has advertised itself yet; malformed files are errors.
pub fn read_control_service_discovery(path: &Path) -> Result<Option<ControlServiceDiscovery>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}
```

If `PathBuf` is already imported, change the import to:

```rust
use std::path::{Path, PathBuf};
```

In `src-tauri/src/commands.rs`, update imports:

```rust
use crate::control_service::{
    control_service_discovery_path, read_control_service_discovery, ControlServiceDiscovery,
};
```

Add command near daemon commands:

```rust
/// Read the Stage 5A control service discovery document from app data.
#[tauri::command]
pub fn get_control_service_discovery(
    app: tauri::AppHandle,
) -> Result<CommandResult<Option<ControlServiceDiscovery>>, String> {
    let app_data = app.path().app_data_dir().map_err(|error| error.to_string())?;
    let discovery_path = control_service_discovery_path(&app_data);
    Ok(CommandResult {
        data: read_control_service_discovery(&discovery_path)
            .map_err(|error| error.to_string())?,
    })
}
```

Register it in `build_app()` immediately after `get_metadata_provider_enabled`:

```rust
get_control_service_discovery,
```

- [ ] **Step 4: Run focused tests**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --test control_service -j 1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/control_service.rs src-tauri/src/commands.rs src-tauri/tests/control_service.rs
git commit -m "新增阶段5B控制服务发现命令"
```

## Task 2: Pure TypeScript Daemon Client

**Files:**
- Create: `src/daemonClient.ts`
- Create: `src/daemonClient.test.ts`

- [ ] **Step 1: Write failing daemon client tests**

Create `src/daemonClient.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";
import { createDaemonControlClient, type ControlServiceDiscovery } from "./daemonClient";

const discovery: ControlServiceDiscovery = {
  service: "media-manager-control",
  host: "127.0.0.1",
  port: 45123,
  base_url: "http://127.0.0.1:45123",
  token: "stage5-token",
  pid: 123,
  created_at: "2026-06-26T00:00:00Z",
};

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

describe("daemon control client", () => {
  it("falls back to command bridge when discovery is missing", async () => {
    const command = vi.fn(async (name: string) => {
      expect(name).toBe("get_daemon_status");
      return { state: "Idle", configured: true, source_roots: [], archive_root: null, asset_roots: [], queued: 0, processed: 0, open_exceptions: 0, holding_items: 0, recent_runs: 0, metadata_source: "disabled" };
    });
    const fetchImpl = vi.fn();
    const client = createDaemonControlClient({ command, fetchImpl, getDiscovery: async () => null });

    const status = await client.getStatus();

    expect(status.state).toBe("Idle");
    expect(fetchImpl).not.toHaveBeenCalled();
    expect(client.getChannel()).toBe("command");
  });

  it("uses REST when discovery and health are available", async () => {
    const command = vi.fn();
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ ok: true, data: { service: "media-manager-control", status: "ok" } }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, data: { state: "Paused", configured: true, source_roots: [], archive_root: null, asset_roots: [], queued: 0, processed: 0, open_exceptions: 0, holding_items: 0, recent_runs: 0, metadata_source: "example" } }));
    const client = createDaemonControlClient({ command, fetchImpl, getDiscovery: async () => discovery });

    const status = await client.getStatus();

    expect(status.state).toBe("Paused");
    expect(fetchImpl).toHaveBeenLastCalledWith("http://127.0.0.1:45123/v1/status", expect.objectContaining({
      method: "GET",
      headers: expect.objectContaining({ Authorization: "Bearer stage5-token" }),
    }));
    expect(command).not.toHaveBeenCalled();
    expect(client.getChannel()).toBe("service");
  });

  it("does not fall back when the service returns a business error", async () => {
    const command = vi.fn();
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ ok: true, data: { service: "media-manager-control", status: "ok" } }))
      .mockResolvedValueOnce(jsonResponse({ ok: false, error: "示例元数据源未开启" }, 500));
    const client = createDaemonControlClient({ command, fetchImpl, getDiscovery: async () => discovery });

    await expect(client.runOnce()).rejects.toThrow("示例元数据源未开启");
    expect(command).not.toHaveBeenCalled();
    expect(client.getChannel()).toBe("service");
  });

  it("falls back to command bridge when the service is unreachable", async () => {
    const command = vi.fn(async (name: string) => {
      expect(name).toBe("pause_daemon");
      return { state: "Paused", configured: true, source_roots: [], archive_root: null, asset_roots: [], queued: 0, processed: 0, open_exceptions: 0, holding_items: 0, recent_runs: 0, metadata_source: "disabled" };
    });
    const fetchImpl = vi.fn().mockRejectedValue(new Error("connection refused"));
    const client = createDaemonControlClient({ command, fetchImpl, getDiscovery: async () => discovery });

    const status = await client.pause();

    expect(status.state).toBe("Paused");
    expect(command).toHaveBeenCalledOnce();
    expect(client.getChannel()).toBe("command");
  });

  it("maps resolve exception to REST path or command bridge", async () => {
    const command = vi.fn(async () => true);
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ ok: true, data: { service: "media-manager-control", status: "ok" } }))
      .mockResolvedValueOnce(jsonResponse({ ok: true, data: { id: 7, object_path: "x", kind: "ScrapeFailed", evidence_json: "{}", status: "Resolved" } }));
    const client = createDaemonControlClient({ command, fetchImpl, getDiscovery: async () => discovery });

    await expect(client.resolveException(7, "Resolved")).resolves.toBe(true);
    expect(fetchImpl).toHaveBeenLastCalledWith("http://127.0.0.1:45123/v1/exceptions/7/resolve", expect.objectContaining({ method: "POST" }));
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
npm test -- src/daemonClient.test.ts
```

Expected: FAIL because `src/daemonClient.ts` does not exist.

- [ ] **Step 3: Implement daemon client**

Create `src/daemonClient.ts`:

```ts
import type {
  DaemonControlStatus,
  DaemonRunOnceReport,
  ExceptionEntry,
  ExceptionStatus,
  HoldingEntry,
  PipelineRun,
} from "./api";

export type DaemonControlChannel = "service" | "command" | "none";

export interface ControlServiceDiscovery {
  service: string;
  host: string;
  port: number;
  base_url: string;
  token: string;
  pid: number;
  created_at: string;
}

type CommandExecutor = <T>(name: string, args?: Record<string, unknown>) => Promise<T>;
type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

type Envelope<T> = { ok: true; data: T } | { ok: false; error: string };

interface ClientDeps {
  command: CommandExecutor;
  fetchImpl?: FetchLike;
  getDiscovery: () => Promise<ControlServiceDiscovery | null>;
}

interface Route<TCommand, TService = TCommand> {
  method: "GET" | "POST";
  path: string;
  commandName: string;
  commandArgs?: Record<string, unknown>;
  mapServiceData?: (value: TService) => TCommand;
}

export function createDaemonControlClient(deps: ClientDeps) {
  let channel: DaemonControlChannel = "none";
  const fetchImpl = deps.fetchImpl ?? fetch;

  async function call<TCommand, TService = TCommand>(route: Route<TCommand, TService>): Promise<TCommand> {
    const discovery = await safeDiscovery(deps.getDiscovery);
    if (!discovery) {
      return callCommand<TCommand>(route);
    }
    const serviceReady = await health(discovery, fetchImpl).catch(() => false);
    if (!serviceReady) {
      return callCommand<TCommand>(route);
    }
    try {
      const result = await callService<TService>(discovery, fetchImpl, route.method, route.path);
      channel = "service";
      return route.mapServiceData ? route.mapServiceData(result) : (result as unknown as TCommand);
    } catch (error) {
      if (isFallbackError(error)) {
        return callCommand<TCommand>(route);
      }
      channel = "service";
      throw error;
    }
  }

  async function callCommand<T>(route: Route<T>): Promise<T> {
    const result = await deps.command<T>(route.commandName, route.commandArgs);
    channel = "command";
    return result;
  }

  return {
    getChannel: () => channel,
    getStatus: () => call<DaemonControlStatus>({ method: "GET", path: "/v1/status", commandName: "get_daemon_status" }),
    pause: () => call<DaemonControlStatus>({ method: "POST", path: "/v1/pause", commandName: "pause_daemon" }),
    resume: () => call<DaemonControlStatus>({ method: "POST", path: "/v1/resume", commandName: "resume_daemon" }),
    runOnce: () => call<DaemonRunOnceReport>({ method: "POST", path: "/v1/run-once", commandName: "run_daemon_once_command" }),
    listHolding: () => call<HoldingEntry[]>({ method: "GET", path: "/v1/holding", commandName: "list_holding_entries" }),
    listExceptions: () => call<ExceptionEntry[]>({ method: "GET", path: "/v1/exceptions", commandName: "list_exception_entries" }),
    resolveException: (id: number, status: Exclude<ExceptionStatus, "Open">) => call<boolean>({
      method: "POST",
      path: `/v1/exceptions/${id}/resolve`,
      commandName: "resolve_exception_entry_command",
      commandArgs: { id, status },
      mapServiceData: () => true,
    }),
    listRuns: () => call<PipelineRun[]>({ method: "GET", path: "/v1/runs", commandName: "list_pipeline_runs" }),
  };
}

async function safeDiscovery(getDiscovery: () => Promise<ControlServiceDiscovery | null>) {
  try {
    return await getDiscovery();
  } catch {
    return null;
  }
}

async function health(discovery: ControlServiceDiscovery, fetchImpl: FetchLike) {
  const response = await fetchImpl(`${discovery.base_url}/health`, { method: "GET" });
  if (!response.ok) return false;
  const envelope = (await response.json()) as Envelope<{ service: string; status: string }>;
  return envelope.ok && envelope.data.service === "media-manager-control";
}

async function callService<T>(discovery: ControlServiceDiscovery, fetchImpl: FetchLike, method: "GET" | "POST", path: string): Promise<T> {
  let response: Response;
  try {
    response = await fetchImpl(`${discovery.base_url}${path}`, {
      method,
      headers: { Authorization: `Bearer ${discovery.token}` },
    });
  } catch {
    throw new FallbackToCommandError();
  }
  if (response.status === 401 || response.status === 403) {
    throw new FallbackToCommandError();
  }
  const envelope = (await response.json()) as Envelope<T>;
  if (!envelope.ok) {
    throw new Error(envelope.error);
  }
  return envelope.data;
}

class FallbackToCommandError extends Error {}

function isFallbackError(error: unknown) {
  return error instanceof FallbackToCommandError;
}
```

- [ ] **Step 4: Run daemon client tests**

Run:

```powershell
npm test -- src/daemonClient.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/daemonClient.ts src/daemonClient.test.ts
git commit -m "新增阶段5B自动管线服务客户端"
```

## Task 3: API Integration

**Files:**
- Modify: `src/api.ts`
- Test: `src/daemonClient.test.ts`

- [ ] **Step 1: Write failing API-level test for command names**

Extend `src/daemonClient.test.ts` with a command fallback case for all daemon methods:

```ts
it("uses the existing Tauri command names for every fallback method", async () => {
  const command = vi.fn(async () => []);
  const client = createDaemonControlClient({ command, fetchImpl: vi.fn(), getDiscovery: async () => null });

  await client.listHolding();
  await client.listExceptions();
  await client.listRuns();

  expect(command.mock.calls.map((call) => call[0])).toEqual([
    "list_holding_entries",
    "list_exception_entries",
    "list_pipeline_runs",
  ]);
});
```

- [ ] **Step 2: Run test**

Run:

```powershell
npm test -- src/daemonClient.test.ts
```

Expected: PASS after Task 2; if it fails, fix command names before touching `api.ts`.

- [ ] **Step 3: Wire `src/api.ts`**

At the top of `src/api.ts`, add:

```ts
import { createDaemonControlClient, type ControlServiceDiscovery, type DaemonControlChannel } from "./daemonClient";
export type { ControlServiceDiscovery, DaemonControlChannel } from "./daemonClient";
```

After `command<T>()`, add:

```ts
const daemonClient = createDaemonControlClient({
  command,
  getDiscovery: () => command<ControlServiceDiscovery | null>("get_control_service_discovery"),
});
```

Change daemon methods in `api`:

```ts
  getControlServiceDiscovery() {
    return command<ControlServiceDiscovery | null>("get_control_service_discovery");
  },
  getDaemonControlChannel() {
    return daemonClient.getChannel();
  },
  getDaemonStatus() {
    return daemonClient.getStatus();
  },
  pauseDaemon() {
    return daemonClient.pause();
  },
  resumeDaemon() {
    return daemonClient.resume();
  },
  runDaemonOnce() {
    return daemonClient.runOnce();
  },
  listHoldingEntries() {
    return daemonClient.listHolding();
  },
  listExceptionEntries() {
    return daemonClient.listExceptions();
  },
  resolveExceptionEntry(id: number, status: Exclude<ExceptionStatus, "Open">) {
    return daemonClient.resolveException(id, status);
  },
  listPipelineRuns() {
    return daemonClient.listRuns();
  },
```

- [ ] **Step 4: Run TypeScript check and tests**

Run:

```powershell
npm test -- src/daemonClient.test.ts
npx tsc --noEmit
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/api.ts src/daemonClient.test.ts
git commit -m "接入阶段5B自动管线API路由"
```

## Task 4: Settings UI Channel Display and Paused Run Guard

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/viewModel.ts`
- Modify: `src/viewModel.test.ts`

- [ ] **Step 1: Write failing viewModel test**

In `src/viewModel.test.ts`, add imports:

```ts
formatDaemonChannel,
```

Add to the daemon helper test:

```ts
expect(formatDaemonChannel("service")).toBe("本地服务");
expect(formatDaemonChannel("command")).toBe("命令桥");
expect(formatDaemonChannel("none")).toBe("未连接");
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
npm test -- src/viewModel.test.ts
```

Expected: FAIL because `formatDaemonChannel` does not exist.

- [ ] **Step 3: Implement channel formatting**

In `src/viewModel.ts`, import type:

```ts
import type { DaemonControlChannel } from "./api";
```

Add:

```ts
export function formatDaemonChannel(channel: DaemonControlChannel) {
  if (channel === "service") return "本地服务";
  if (channel === "command") return "命令桥";
  return "未连接";
}
```

- [ ] **Step 4: Update `src/App.tsx`**

Import `DaemonControlChannel` type and `formatDaemonChannel`.

Add state:

```ts
const [daemonChannel, setDaemonChannel] = useState<DaemonControlChannel>("none");
```

In `loadDaemonPanelData`, after state lists are set:

```ts
setDaemonChannel(api.getDaemonControlChannel());
```

In the daemon panel status line, append:

```tsx
 · 控制通道 ${formatDaemonChannel(daemonChannel)}
```

Change the run button disabled condition to include paused:

```tsx
disabled={daemonBusy !== null || !daemonStatus?.configured || daemonStatus?.state === "Paused"}
```

- [ ] **Step 5: Run frontend tests and typecheck**

Run:

```powershell
npm test
npx tsc --noEmit
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx src/viewModel.ts src/viewModel.test.ts
git commit -m "显示阶段5B自动管线控制通道"
```

## Task 5: Verification, Review, HANDOFF

**Files:**
- Modify: `HANDOFF.md`
- Modify: `.ai_state/tasks.md`
- Modify: `.ai_state/progress.md`
- Create: `.ai_state/reviews/sprint-6.md`
- Modify: `.ai_state/lessons.md`
- Modify: `.ai_state/project.json`

- [ ] **Step 1: Update local state**

Set `.ai_state/tasks.md`:

```markdown
# Sprint 6 Tasks - 阶段 5B 前端服务客户端迁移

- [x] 固化阶段 5B 中文设计
- [x] 编写阶段 5B implementation plan
- [x] Task 1: Rust discovery helper 和 Tauri command
- [x] Task 2: TypeScript daemon service client
- [x] Task 3: `src/api.ts` 自动管线方法接入 service-first 路由
- [x] Task 4: 设置页显示控制通道并禁用暂停态运行
- [x] Task 5: 全量验证、评审和交接
```

- [ ] **Step 2: Run full verification**

Run:

```powershell
$env:PATH='C:\Users\DELL\.cargo\bin;' + $env:PATH; cargo test --manifest-path src-tauri/Cargo.toml -j 1
npm test
npx tsc --noEmit
npm run build
```

Expected all pass.

- [ ] **Step 3: Review**

Run:

```powershell
git diff --check 801dd4a..HEAD
git diff --stat 801dd4a..HEAD
$env:npm_config_cache='.npm-cache'; npx ecc-agentshield scan
```

Record ECC as not applicable if the installed CLI still scans global `.claude` only.

- [ ] **Step 4: Update HANDOFF**

Add Stage 5B deliverables:

```markdown
**阶段 5B（前端服务客户端迁移）已实现并验证。**
阶段 5B 新增 discovery command 与 TypeScript daemon client。自动管线设置页优先通过 discovery + REST 调用阶段 5A 服务；服务缺失、不可达或鉴权失败时回退 Tauri command bridge。UI 显示控制通道，并在暂停状态禁用“运行一轮”。
```

- [ ] **Step 5: Commit docs**

```bash
git add HANDOFF.md
git commit -m "更新阶段5B交接说明"
```

## Self-Review

- Spec coverage: Rust discovery, TS client, API integration, UI channel, fallback rules, verification all have tasks.
- Placeholder scan: checked for unfinished markers and none remain.
- Type consistency: `ControlServiceDiscovery`, `DaemonControlChannel`, command names, and REST paths match the Stage 5B spec.
- Boundary check: no service process startup, no Tauri GUI launch, no WebSocket, no tray/autostart, no real scraper.
