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

/**
 * Create a daemon client that prefers the local REST service and preserves the command bridge fallback.
 */
export function createDaemonControlClient(deps: ClientDeps) {
  let channel: DaemonControlChannel = "none";
  const fetchImpl = deps.fetchImpl ?? fetch;

  /**
   * Return the command bridge result and mark the current control channel.
   */
  async function callCommand<T>(route: Route<T>): Promise<T> {
    const result = await deps.command<T>(route.commandName, route.commandArgs);
    channel = "command";
    return result;
  }

  /**
   * Execute one daemon route with service-first routing and conservative fallback.
   */
  async function call<TCommand, TService = TCommand>(
    route: Route<TCommand, TService>,
  ): Promise<TCommand> {
    const discovery = await safeDiscovery(deps.getDiscovery);
    if (!discovery) {
      return callCommand<TCommand>(route);
    }

    const serviceReady = await health(discovery, fetchImpl);
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

  return {
    getChannel: () => channel,
    getStatus: () =>
      call<DaemonControlStatus>({
        method: "GET",
        path: "/v1/status",
        commandName: "get_daemon_status",
      }),
    pause: () =>
      call<DaemonControlStatus>({
        method: "POST",
        path: "/v1/pause",
        commandName: "pause_daemon",
      }),
    resume: () =>
      call<DaemonControlStatus>({
        method: "POST",
        path: "/v1/resume",
        commandName: "resume_daemon",
      }),
    runOnce: () =>
      call<DaemonRunOnceReport>({
        method: "POST",
        path: "/v1/run-once",
        commandName: "run_daemon_once_command",
      }),
    listHolding: () =>
      call<HoldingEntry[]>({
        method: "GET",
        path: "/v1/holding",
        commandName: "list_holding_entries",
      }),
    listExceptions: () =>
      call<ExceptionEntry[]>({
        method: "GET",
        path: "/v1/exceptions",
        commandName: "list_exception_entries",
      }),
    resolveException: (id: number, status: Exclude<ExceptionStatus, "Open">) =>
      call<boolean, ExceptionEntry>({
        method: "POST",
        path: `/v1/exceptions/${id}/resolve`,
        commandName: "resolve_exception_entry_command",
        commandArgs: { id, status },
        mapServiceData: () => true,
      }),
    listRuns: () =>
      call<PipelineRun[]>({
        method: "GET",
        path: "/v1/runs",
        commandName: "list_pipeline_runs",
      }),
  };
}

/**
 * Read discovery safely so a corrupt or unavailable command keeps the fallback path usable.
 */
async function safeDiscovery(getDiscovery: () => Promise<ControlServiceDiscovery | null>) {
  try {
    return await getDiscovery();
  } catch {
    return null;
  }
}

/**
 * Check that the advertised service is reachable and is the expected loopback control service.
 */
async function health(discovery: ControlServiceDiscovery, fetchImpl: FetchLike) {
  try {
    const response = await fetchImpl(`${discovery.base_url}/health`, { method: "GET" });
    if (!response.ok) return false;
    const envelope = (await response.json()) as Envelope<{ service: string; status: string }>;
    return envelope.ok && envelope.data.service === "media-manager-control";
  } catch {
    return false;
  }
}

/**
 * Call one REST route and preserve service business errors instead of masking them with fallback.
 */
async function callService<T>(
  discovery: ControlServiceDiscovery,
  fetchImpl: FetchLike,
  method: "GET" | "POST",
  path: string,
): Promise<T> {
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

  let envelope: Envelope<T>;
  try {
    envelope = (await response.json()) as Envelope<T>;
  } catch {
    throw new FallbackToCommandError();
  }

  if (!envelope.ok) {
    throw new Error(envelope.error);
  }
  return envelope.data;
}

/**
 * Internal marker for failures where the existing Tauri command bridge is still authoritative.
 */
class FallbackToCommandError extends Error {}

/**
 * Distinguish transport/auth/shape failures from service-level business failures.
 */
function isFallbackError(error: unknown) {
  return error instanceof FallbackToCommandError;
}
