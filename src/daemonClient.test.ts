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
      return {
        state: "Idle",
        configured: true,
        source_roots: [],
        archive_root: null,
        asset_roots: [],
        queued: 0,
        processed: 0,
        open_exceptions: 0,
        holding_items: 0,
        recent_runs: 0,
        metadata_source: "disabled",
      };
    });
    const fetchImpl = vi.fn();
    const client = createDaemonControlClient({
      command,
      fetchImpl,
      getDiscovery: async () => null,
    });

    const status = await client.getStatus();

    expect(status.state).toBe("Idle");
    expect(fetchImpl).not.toHaveBeenCalled();
    expect(client.getChannel()).toBe("command");
  });

  it("uses REST when discovery and health are available", async () => {
    const command = vi.fn();
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(
        jsonResponse({
          ok: true,
          data: { service: "media-manager-control", status: "ok" },
        }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          ok: true,
          data: {
            state: "Paused",
            configured: true,
            source_roots: [],
            archive_root: null,
            asset_roots: [],
            queued: 0,
            processed: 0,
            open_exceptions: 0,
            holding_items: 0,
            recent_runs: 0,
            metadata_source: "example",
          },
        }),
      );
    const client = createDaemonControlClient({
      command,
      fetchImpl,
      getDiscovery: async () => discovery,
    });

    const status = await client.getStatus();

    expect(status.state).toBe("Paused");
    expect(fetchImpl).toHaveBeenLastCalledWith(
      "http://127.0.0.1:45123/v1/status",
      expect.objectContaining({
        method: "GET",
        headers: expect.objectContaining({ Authorization: "Bearer stage5-token" }),
      }),
    );
    expect(command).not.toHaveBeenCalled();
    expect(client.getChannel()).toBe("service");
  });

  it("does not fall back when the service returns a business error", async () => {
    const command = vi.fn();
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(
        jsonResponse({
          ok: true,
          data: { service: "media-manager-control", status: "ok" },
        }),
      )
      .mockResolvedValueOnce(jsonResponse({ ok: false, error: "示例元数据源未开启" }, 500));
    const client = createDaemonControlClient({
      command,
      fetchImpl,
      getDiscovery: async () => discovery,
    });

    await expect(client.runOnce()).rejects.toThrow("示例元数据源未开启");
    expect(command).not.toHaveBeenCalled();
    expect(client.getChannel()).toBe("service");
  });

  it("falls back to command bridge when the service is unreachable", async () => {
    const command = vi.fn(async (name: string) => {
      expect(name).toBe("pause_daemon");
      return {
        state: "Paused",
        configured: true,
        source_roots: [],
        archive_root: null,
        asset_roots: [],
        queued: 0,
        processed: 0,
        open_exceptions: 0,
        holding_items: 0,
        recent_runs: 0,
        metadata_source: "disabled",
      };
    });
    const fetchImpl = vi.fn().mockRejectedValue(new Error("connection refused"));
    const client = createDaemonControlClient({
      command,
      fetchImpl,
      getDiscovery: async () => discovery,
    });

    const status = await client.pause();

    expect(status.state).toBe("Paused");
    expect(command).toHaveBeenCalledOnce();
    expect(client.getChannel()).toBe("command");
  });

  it("maps resolve exception to REST path or command bridge", async () => {
    const command = vi.fn(async () => true);
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(
        jsonResponse({
          ok: true,
          data: { service: "media-manager-control", status: "ok" },
        }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          ok: true,
          data: {
            id: 7,
            object_path: "x",
            kind: "ScrapeFailed",
            evidence_json: "{}",
            status: "Resolved",
          },
        }),
      );
    const client = createDaemonControlClient({
      command,
      fetchImpl,
      getDiscovery: async () => discovery,
    });

    await expect(client.resolveException(7, "Resolved")).resolves.toBe(true);
    expect(fetchImpl).toHaveBeenLastCalledWith(
      "http://127.0.0.1:45123/v1/exceptions/7/resolve",
      expect.objectContaining({ method: "POST" }),
    );
  });
});
