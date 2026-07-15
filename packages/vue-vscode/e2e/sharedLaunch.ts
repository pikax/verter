/**
 * Shared VS Code launch primitives for the e2e runners.
 *
 * Both the fixture-matrix runner (`runTests.ts`) and the DX extension-host driver
 * (`dx/dxLauncher.ts`) need the same low-level launch plumbing: locating the built
 * `verter-lsp` binary, copying it somewhere a rebuild cannot lock, and resolving a
 * runnable VS Code executable (with the Windows `bin/code.cmd` CLI fix). This module
 * is the single home for that plumbing so the two runners cannot drift apart.
 *
 * It performs no work at import time and reads no DX-specific environment.
 */
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { spawnSync } from "child_process";

export { readE2eEnv } from "../src/e2eEnv";

/**
 * Find the `verter-lsp` binary reachable from the extension path.
 * Searches `target/{debug,release}` walking upward to the monorepo root, then
 * `dist/` and `bin/` inside the extension path. Returns `undefined` if not found.
 */
export function findLspBinary(extensionPath: string): string | undefined {
  const ext = process.platform === "win32" ? ".exe" : "";
  const binaryName = `verter-lsp${ext}`;

  // Walk upward to find the monorepo root's target/ directory.
  let dir = extensionPath;
  for (let i = 0; i < 5; i++) {
    for (const profile of ["debug", "release"]) {
      const candidate = path.join(dir, "target", profile, binaryName);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
    dir = path.dirname(dir);
  }

  const distPath = path.join(extensionPath, "dist", binaryName);
  if (fs.existsSync(distPath)) {
    return distPath;
  }

  const binPath = path.join(extensionPath, "bin", binaryName);
  if (fs.existsSync(binPath)) {
    return binPath;
  }

  return undefined;
}

/**
 * Copy the LSP binary to a temp directory to prevent file-locking issues.
 * On Windows a running `.exe` is locked and can't be overwritten by `cargo build`,
 * so the binary is copied off the source path. On other platforms the original
 * path is kept to avoid location-sensitive startup issues with ad-hoc signed debug
 * binaries. Returns the path to use, or `undefined` when no source is found.
 */
export function copyLspBinaryToTemp(extensionPath: string): string | undefined {
  const sourcePath = findLspBinary(extensionPath);
  if (!sourcePath) {
    console.warn("Warning: LSP binary not found — tests will use PATH fallback");
    return undefined;
  }

  if (process.platform !== "win32") {
    console.log(`LSP binary using source path: ${sourcePath}`);
    return sourcePath;
  }

  const tempDir = path.join(os.tmpdir(), `verter-e2e-bin-${process.pid}`);
  fs.mkdirSync(tempDir, { recursive: true });

  const destPath = path.join(tempDir, "verter-lsp.exe");
  fs.copyFileSync(sourcePath, destPath);

  console.log(`LSP binary copied: ${sourcePath} → ${destPath}`);
  return destPath;
}

/**
 * VS Code 1.111+ changed its binary layout: `Code.exe` is a Node.js launcher that
 * does not accept CLI flags like `--disable-extensions`. The CLI entry point is
 * `bin/code.cmd`. This rewrites the executable path to the CLI entry point when it
 * exists; otherwise the path is returned unchanged. Pure — `existsSync` is injected.
 */
export function applyWindowsCliPathFix(
  execPath: string,
  existsSync: (p: string) => boolean = fs.existsSync,
): string {
  const cliPath = path.resolve(execPath, "../bin/code.cmd");
  return existsSync(cliPath) ? cliPath : execPath;
}

/** Injectable dependencies for {@link resolveVscodeExecutablePath} (for tests). */
export interface ResolveVscodeOptions {
  /** Pre-existing host executable to use instead of downloading (validated before use). */
  explicitExecutablePath?: string;
  /** Downloads + unzips VS Code, returning the executable path. */
  download?: (version: string) => Promise<string>;
  /** Platform override (defaults to `process.platform`). */
  platform?: NodeJS.Platform;
  /** `fs.existsSync` override (defaults to the real one). */
  existsSync?: (p: string) => boolean;
}

/**
 * Resolve the VS Code host executable for extension tests. The downloader is lazily
 * imported so unit tests can inject a fake and never touch the network or
 * `@vscode/test-electron`. Do not substitute `bin/code.cmd` here: that CLI can attach
 * to an existing window and exit zero without executing `--extensionTestsPath`.
 */
export async function resolveVscodeExecutablePath(
  version: string,
  opts: ResolveVscodeOptions = {},
): Promise<string> {
  if (opts.explicitExecutablePath) {
    const exists = opts.existsSync ?? fs.existsSync;
    if (!exists(opts.explicitExecutablePath)) {
      throw new Error(
        `Configured VS Code executable does not exist: ${opts.explicitExecutablePath}`,
      );
    }
    return opts.explicitExecutablePath;
  }

  const download =
    opts.download ??
    (async (v: string) => (await import("@vscode/test-electron")).downloadAndUnzipVSCode(v));

  return download(version);
}

export interface SynchronousCommandResult {
  status: number | null;
  stdout?: string;
  stderr?: string;
  error?: Error;
}

export type SynchronousCommandRunner = (
  command: string,
  args: string[],
  options: {
    encoding: "utf8";
    timeout: number;
    shell: boolean;
    windowsHide: boolean;
  },
) => SynchronousCommandResult;

export interface ProvisionVsCodeExtensionOptions {
  /** Output from `resolveCliArgsFromVSCodeExecutablePath`. */
  cliArgs: string[];
  /** Marketplace `publisher.id@version` or an explicit VSIX path. */
  extension: string;
  extensionsDir: string;
  userDataDir: string;
  timeoutMs?: number;
  platform?: NodeJS.Platform;
  run?: SynchronousCommandRunner;
}

/**
 * Merge settings into the exact isolated VS Code user profile used by an E2E host.
 * This runs before launch so extensions observe required lifecycle settings during
 * their first activation rather than through a racy post-activation transition.
 */
export function writeVsCodeUserSettings(
  userDataDir: string,
  settings: Record<string, unknown>,
): void {
  const userDir = path.join(userDataDir, "User");
  const settingsPath = path.join(userDir, "settings.json");
  fs.mkdirSync(userDir, { recursive: true });

  let current: Record<string, unknown> = {};
  if (fs.existsSync(settingsPath)) {
    const parsed: unknown = JSON.parse(fs.readFileSync(settingsPath, "utf8"));
    if (parsed === null || Array.isArray(parsed) || typeof parsed !== "object") {
      throw new Error(`VS Code user settings must be a JSON object: ${settingsPath}`);
    }
    current = parsed as Record<string, unknown>;
  }

  fs.writeFileSync(
    settingsPath,
    `${JSON.stringify({ ...current, ...settings }, null, 2)}\n`,
    "utf8",
  );
}

/**
 * Install an E2E dependency into the exact isolated profile used by the test host.
 * Platform bootstrap arguments (notably macOS's Electron-as-Node CLI entry point)
 * are preserved, while profile arguments supplied by the library are replaced so
 * installation and execution cannot accidentally target different profiles.
 */
export function provisionVsCodeExtension(opts: ProvisionVsCodeExtensionOptions): void {
  const [command, ...rawArgs] = opts.cliArgs;
  if (!command) throw new Error("VS Code CLI resolution returned no executable");

  const args = rawArgs.filter(
    (arg) => !arg.startsWith("--extensions-dir=") && !arg.startsWith("--user-data-dir="),
  );
  args.push(
    `--extensions-dir=${opts.extensionsDir}`,
    `--user-data-dir=${opts.userDataDir}`,
    "--install-extension",
    opts.extension,
    "--force",
  );

  const run: SynchronousCommandRunner =
    opts.run ??
    ((executable, commandArgs, options) => {
      const result = spawnSync(executable, commandArgs, options);
      return {
        status: result.status,
        stdout: result.stdout || undefined,
        stderr: result.stderr || undefined,
        error: result.error,
      };
    });
  const result = run(command, args, {
    encoding: "utf8",
    timeout: opts.timeoutMs ?? 180_000,
    shell: (opts.platform ?? process.platform) === "win32",
    windowsHide: true,
  });

  if (result.error || result.status !== 0) {
    const detail = [result.error?.message, result.stderr, result.stdout]
      .filter((value): value is string => Boolean(value?.trim()))
      .join("\n");
    throw new Error(
      `Failed to provision VS Code extension ${opts.extension}` +
        (result.status === null ? "" : ` (exit ${result.status})`) +
        (detail ? `:\n${detail}` : ""),
    );
  }
}
