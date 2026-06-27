import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

const tempRoots: string[] = [];

/**
 * Create a disposable directory for feedback script integration tests.
 */
function createTempRoot(): string {
  const root = mkdtempSync(join(tmpdir(), "media-manager-feedback-"));
  tempRoots.push(root);
  return root;
}

/**
 * Build a stable Windows environment for PowerShell child processes.
 */
function powerShellEnvironment(overrides: NodeJS.ProcessEnv = {}): NodeJS.ProcessEnv {
  return {
    ...process.env,
    SystemRoot: process.env.SystemRoot ?? "C:\\Windows",
    windir: process.env.windir ?? process.env.SystemRoot ?? "C:\\Windows",
    ...overrides,
  };
}

/**
 * Run the feedback collector with heavyweight verification disabled.
 */
function runFeedbackCollector(
  outputDirectory: string,
  missingAppDataPath: string,
  extraArguments: string[] = [],
  env: NodeJS.ProcessEnv = powerShellEnvironment(),
): string {
  const systemRoot = env.SystemRoot ?? "C:\\Windows";
  const powerShell = join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe");

  return execFileSync(
    powerShell,
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      "scripts/collect-feedback.ps1",
      "-OutputDirectory",
      outputDirectory,
      "-AppDataPath",
      missingAppDataPath,
      "-SkipTests",
      "-SkipBuild",
      ...extraArguments,
    ],
    { cwd: process.cwd(), encoding: "utf8", env, stdio: ["ignore", "pipe", "pipe"] },
  );
}

/**
 * Run the feedback collector with tests enabled and build disabled.
 */
function runFeedbackCollectorWithoutTools(outputDirectory: string, missingAppDataPath: string): string {
  const env = powerShellEnvironment({
    PATH: join(process.env.SystemRoot ?? "C:\\Windows", "System32"),
  });
  const systemRoot = env.SystemRoot ?? "C:\\Windows";
  const powerShell = join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe");

  return execFileSync(
    powerShell,
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      "scripts/collect-feedback.ps1",
      "-OutputDirectory",
      outputDirectory,
      "-AppDataPath",
      missingAppDataPath,
      "-SkipBuild",
      "-SkipRustTests",
    ],
    { cwd: process.cwd(), encoding: "utf8", env, stdio: ["ignore", "pipe", "pipe"] },
  );
}

afterEach(() => {
  for (const root of tempRoots.splice(0)) {
    rmSync(root, { recursive: true, force: true });
  }
});

describe("collect-feedback script", () => {
  it("creates a zip package and manifest when optional app data is missing", () => {
    const root = createTempRoot();
    const outputDirectory = join(root, "feedback");
    const missingAppDataPath = join(root, "missing-app-data");

    const output = runFeedbackCollector(outputDirectory, missingAppDataPath);

    expect(output).toContain("Feedback package:");
    const entries = readdirSync(outputDirectory);
    const runDirectories = entries.filter((entry) => entry.startsWith("feedback-") && !entry.endsWith(".zip"));
    const archives = entries.filter((entry) => entry.startsWith("feedback-") && entry.endsWith(".zip"));
    expect(runDirectories).toHaveLength(1);
    expect(archives).toHaveLength(1);

    const runDirectory = join(outputDirectory, runDirectories[0]);
    const manifestPath = join(runDirectory, "manifest.json");
    expect(existsSync(manifestPath)).toBe(true);

    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    const commandNames = manifest.commands.map((command: { name: string }) => command.name);
    expect(manifest.schemaVersion).toBe(1);
    expect(manifest.options.skipTests).toBe(true);
    expect(manifest.options.skipBuild).toBe(true);
    expect(manifest.appData.available).toBe(false);
    expect(commandNames).toContain("git status");
    expect(commandNames).toContain("git diff names");
    expect(commandNames).toContain("git diff stat");
    expect(existsSync(join(runDirectory, "summary.md"))).toBe(true);
    const gitLog = readFileSync(join(runDirectory, "commands", "git-log.txt"), "utf8");
    const gitDiffNames = readFileSync(join(runDirectory, "commands", "git-diff-names.txt"), "utf8");
    expect(gitLog).toContain("git.exe -c i18n.logOutputEncoding=utf-8 log --oneline -10");
    expect(gitLog).toContain("新增一键反馈包脚本");
    expect(gitDiffNames).toContain("git.exe -c core.safecrlf=false diff --name-only");
    expect(readFileSync(join(runDirectory, "app-data", "app-data-status.txt"), "utf8")).toContain(
      "App data path was not found",
    );
  });

  it("records skipped verification commands when optional npm is missing", () => {
    const root = createTempRoot();
    const outputDirectory = join(root, "feedback");
    const missingAppDataPath = join(root, "missing-app-data");

    runFeedbackCollectorWithoutTools(outputDirectory, missingAppDataPath);

    const runDirectory = join(
      outputDirectory,
      readdirSync(outputDirectory).find((entry) => entry.startsWith("feedback-") && !entry.endsWith(".zip")) ?? "",
    );
    const manifest = JSON.parse(readFileSync(join(runDirectory, "manifest.json"), "utf8"));
    const statuses = new Map(manifest.commands.map((command: { name: string; status: string }) => [command.name, command.status]));
    expect(statuses.get("npm test")).toBe("skipped");
    expect(statuses.has("cargo test")).toBe(false);
  });

  it("keeps the default feedback output directory out of git status", () => {
    expect(readFileSync(".gitignore", "utf8")).toMatch(/^feedback\/$/m);
  });
});
