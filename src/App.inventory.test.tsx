// @vitest-environment happy-dom
import React from "react";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { api } from "./api";
import type { InventoryExecutionReport, InventoryPreviewReport } from "./api";

/** Creates a deferred promise so tests can observe App loading state before it settles. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((innerResolve, innerReject) => {
    resolve = innerResolve;
    reject = innerReject;
  });
  return { promise, resolve, reject };
}

/** Updates a controlled text field through the native setter so React receives the input event. */
function setTextFieldValue(field: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(field), "value")?.set;
  setter?.call(field, value);
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

/** Returns the first button whose visible text contains the requested label. */
function buttonContaining(label: string): HTMLButtonElement {
  const button = Array.from(document.querySelectorAll("button")).find((candidate) =>
    candidate.textContent?.includes(label)
  );
  if (!(button instanceof HTMLButtonElement)) {
    throw new Error(`Button not found: ${label}`);
  }
  return button;
}

/** Minimal Stage 7B inventory report used to verify App-side preview/export wiring. */
function makeInventoryReport(): InventoryPreviewReport {
  return {
    generated_at: "2026-06-28T12:00:00Z",
    roots: ["D:\\inventory-inbox"],
    archive_root: "D:\\inventory-archive",
    summary: {
      total_files: 3,
      works: 1,
      asset_candidates: 0,
      auto_ready: 1,
      needs_review: 0,
      blocked: 0,
      ready: 1,
      missing_nfo: 0,
      missing_video: 0,
      multi_video: 0,
      multi_nfo: 0,
      code_conflict: 0,
      duplicate_candidate: 0,
      orphans: 0
    },
    works: [
      {
        code: "IPX-201",
        statuses: ["ready"],
        resources: [],
        target_dir: "D:\\inventory-archive\\IPX-201",
        actions: [],
        resolution: {
          bucket: "auto_ready",
          primary_video: "D:\\inventory-inbox\\IPX-201.mp4",
          primary_nfo: "D:\\inventory-inbox\\IPX-201.nfo",
          recommended: "可自动整理",
          reasons: ["主视频和主 NFO 已匹配"],
          warnings: [],
          blockers: [],
          confidence: "high",
          execution_plan: {
            ready: true,
            actions: [
              {
                from_path: "D:\\inventory-inbox\\IPX-201.mp4",
                to_path: "D:\\inventory-archive\\IPX-201\\IPX-201.mp4",
                kind: "video",
                conflict: null
              }
            ],
            conflicts: [],
            notes: ["已从重复目标候选中选择安全动作"]
          }
        },
        resource_roles: []
      }
    ],
    asset_candidates: [],
    orphans: [],
    warnings: [],
    truncated: false
  };
}

/** Minimal M1 execution report used to verify centralized migration UI wiring. */
function makeInventoryExecutionReport(): InventoryExecutionReport {
  return {
    report_path: "C:\\Users\\A\\AppData\\Roaming\\local.media-manager\\inventory-reports\\inventory-execution.json",
    mode: "move",
    started_at: "2026-06-28T12:01:00Z",
    finished_at: "2026-06-28T12:02:00Z",
    requested_works: 1,
    executed_works: 1,
    skipped_works: 0,
    planned_actions: 1,
    linked_actions: 0,
    copied_actions: 0,
    moved_actions: 1,
    failed_actions: 0,
    rolled_back_actions: 0,
    rollback_failed_actions: 0,
    same_volume_actions: 1,
    cross_volume_actions: 0,
    space_blocked_actions: 0,
    bytes_linked: 0,
    bytes_copied: 0,
    bytes_moved: 5,
    logs: [
      {
        code: "IPX-201",
        kind: "video",
        from_path: "D:\\inventory-inbox\\IPX-201.mp4",
        to_path: "D:\\inventory-archive\\IPX-201\\IPX-201.mp4",
        status: "moved",
        message: "same_volume_rename",
        bytes: 5
      }
    ]
  };
}

describe("inventory page wiring", () => {
  let root: Root | null = null;
  let container: HTMLDivElement | null = null;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    vi.spyOn(api, "getSourceRoots").mockResolvedValue([]);
    vi.spyOn(api, "getArchiveRoot").mockResolvedValue("D:\\stored-archive");
    vi.spyOn(api, "getMetadataProviderEnabled").mockResolvedValue(false);
    vi.spyOn(api, "listArchiveActionLogs").mockResolvedValue([]);
    vi.spyOn(api, "getLatestIngestJob").mockResolvedValue(null);
    vi.spyOn(api, "listWorks").mockResolvedValue([]);
    vi.spyOn(api, "getPosterDirs").mockResolvedValue({ poster_dir: null, screenshot_dir: null, gif_dir: null });
    vi.spyOn(api, "getResourcePoolDirs").mockResolvedValue([]);
    vi.spyOn(api, "getPrimaryLibraryDir").mockResolvedValue(null);
    vi.spyOn(api, "getOrCreateThumbnail").mockResolvedValue(null);

    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    if (root) {
      act(() => root?.unmount());
    }
    container?.remove();
    root = null;
    container = null;
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    Reflect.deleteProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT");
    vi.restoreAllMocks();
  });

  it("passes the inventory target root to preview and disables fields while scanning", async () => {
    const report = makeInventoryReport();
    const pendingPreview = deferred<InventoryPreviewReport>();
    const previewSpy = vi.spyOn(api, "previewInventory").mockReturnValue(pendingPreview.promise);

    await act(async () => {
      root?.render(<App />);
    });
    await act(async () => {
      buttonContaining("盘点").click();
    });

    const rootsField = document.querySelector(".inventory-roots-field textarea");
    const targetField = document.querySelector(".inventory-roots-field input");
    expect(rootsField).toBeInstanceOf(HTMLTextAreaElement);
    expect(targetField).toBeInstanceOf(HTMLInputElement);
    expect((targetField as HTMLInputElement).placeholder).toBe("D:\\mm-7a-test\\archive");

    await act(async () => {
      setTextFieldValue(rootsField as HTMLTextAreaElement, "D:\\inventory-inbox");
      setTextFieldValue(targetField as HTMLInputElement, "D:\\inventory-archive");
      buttonContaining("开始盘点").click();
    });

    expect(previewSpy).toHaveBeenCalledWith(["D:\\inventory-inbox"], "D:\\inventory-archive");
    expect((rootsField as HTMLTextAreaElement).disabled).toBe(true);
    expect((targetField as HTMLInputElement).disabled).toBe(true);

    await act(async () => {
      pendingPreview.resolve(report);
      await pendingPreview.promise;
    });
  });

  it("exports the current inventory report after preview succeeds", async () => {
    const report = makeInventoryReport();
    vi.spyOn(api, "previewInventory").mockResolvedValue(report);
    const exportSpy = vi.spyOn(api, "exportInventoryReport").mockResolvedValue({
      path: "C:\\Users\\A\\AppData\\Roaming\\local.media-manager\\inventory-reports\\inventory.json",
      works: 1,
      asset_candidates: 0,
      orphans: 0
    });

    await act(async () => {
      root?.render(<App />);
    });
    await act(async () => {
      buttonContaining("盘点").click();
    });

    const rootsField = document.querySelector(".inventory-roots-field textarea") as HTMLTextAreaElement;
    const targetField = document.querySelector(".inventory-roots-field input") as HTMLInputElement;
    await act(async () => {
      setTextFieldValue(rootsField, "D:\\inventory-inbox");
      setTextFieldValue(targetField, "D:\\inventory-archive");
      buttonContaining("开始盘点").click();
    });

    await act(async () => {
      buttonContaining("导出 JSON").click();
    });

    expect(exportSpy).toHaveBeenCalledWith(report);
  });

  it("shows the selected safe execution plan separately from raw candidate actions", async () => {
    const report = makeInventoryReport();
    vi.spyOn(api, "previewInventory").mockResolvedValue(report);

    await act(async () => {
      root?.render(<App />);
    });
    await act(async () => {
      buttonContaining("盘点").click();
    });

    const rootsField = document.querySelector(".inventory-roots-field textarea") as HTMLTextAreaElement;
    const targetField = document.querySelector(".inventory-roots-field input") as HTMLInputElement;
    await act(async () => {
      setTextFieldValue(rootsField, "D:\\inventory-inbox");
      setTextFieldValue(targetField, "D:\\inventory-archive");
      buttonContaining("开始盘点").click();
    });

    expect(document.body.textContent).toContain("安全执行计划");
    expect(document.body.textContent).toContain("安全计划可执行：1 个动作。");
    expect(document.body.textContent).toContain("候选动作预览");
  });

  it("executes the current safe inventory plan with centralized migration feedback", async () => {
    const report = makeInventoryReport();
    const executionReport = makeInventoryExecutionReport();
    vi.spyOn(api, "previewInventory").mockResolvedValue(report);
    const pendingExecution = deferred<InventoryExecutionReport>();
    const executeSpy = vi.spyOn(api, "executeInventoryPlan").mockReturnValue(pendingExecution.promise);
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);

    await act(async () => {
      root?.render(<App />);
    });
    await act(async () => {
      buttonContaining("盘点").click();
    });

    const rootsField = document.querySelector(".inventory-roots-field textarea") as HTMLTextAreaElement;
    const targetField = document.querySelector(".inventory-roots-field input") as HTMLInputElement;
    await act(async () => {
      setTextFieldValue(rootsField, "D:\\inventory-inbox");
      setTextFieldValue(targetField, "D:\\inventory-archive");
      buttonContaining("开始盘点").click();
    });

    await act(async () => {
      buttonContaining("集中迁移").click();
    });

    expect(executeSpy).toHaveBeenCalledWith(report, [], "move");
    const confirmText = String(confirmSpy.mock.calls[0]?.[0]);
    expect(confirmText).toContain("成功后源路径不再保留");
    expect(confirmText).toContain("跨盘会逐文件复制校验后删除源文件");
    expect(buttonContaining("迁移中").disabled).toBe(true);

    await act(async () => {
      pendingExecution.resolve(executionReport);
      await pendingExecution.promise;
    });

    expect(document.body.textContent).toContain("集中迁移完成：作品 1/1，迁移 1/1，失败 0，回滚 0，同盘 1，跨盘 0");
    expect(document.body.textContent).toContain("inventory-execution.json");
    expect(document.body.textContent).toContain("已迁移");
    expect(document.body.textContent).toContain("IPX-201");
  });

  it("blocks centralized migration when the inventory report details are truncated", async () => {
    const report = makeInventoryReport();
    report.truncated = true;
    report.summary.works = 1001;
    vi.spyOn(api, "previewInventory").mockResolvedValue(report);

    await act(async () => {
      root?.render(<App />);
    });
    await act(async () => {
      buttonContaining("盘点").click();
    });

    const rootsField = document.querySelector(".inventory-roots-field textarea") as HTMLTextAreaElement;
    const targetField = document.querySelector(".inventory-roots-field input") as HTMLInputElement;
    await act(async () => {
      setTextFieldValue(rootsField, "D:\\inventory-inbox");
      setTextFieldValue(targetField, "D:\\inventory-archive");
      buttonContaining("开始盘点").click();
    });

    expect(buttonContaining("集中迁移").disabled).toBe(true);
    expect(document.body.textContent).toContain("报告明细已截断，不能集中迁移全部作品");
  });

  it("allows centralized migration when only non-work inventory details are truncated", async () => {
    const report = makeInventoryReport();
    report.truncated = true;
    report.summary.asset_candidates = 1001;
    vi.spyOn(api, "previewInventory").mockResolvedValue(report);

    await act(async () => {
      root?.render(<App />);
    });
    await act(async () => {
      buttonContaining("盘点").click();
    });

    const rootsField = document.querySelector(".inventory-roots-field textarea") as HTMLTextAreaElement;
    const targetField = document.querySelector(".inventory-roots-field input") as HTMLInputElement;
    await act(async () => {
      setTextFieldValue(rootsField, "D:\\inventory-inbox");
      setTextFieldValue(targetField, "D:\\inventory-archive");
      buttonContaining("开始盘点").click();
    });

    expect(buttonContaining("集中迁移").disabled).toBe(false);
    expect(document.body.textContent).not.toContain("报告明细已截断，不能集中迁移全部作品");
  });
});
