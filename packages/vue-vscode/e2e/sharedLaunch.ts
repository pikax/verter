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
  /** Downloads + unzips VS Code, returning the executable path. */
  download?: (version: string) => Promise<string>;
  /** Platform override (defaults to `process.platform`). */
  platform?: NodeJS.Platform;
  /** `fs.existsSync` override (defaults to the real one). */
  existsSync?: (p: string) => boolean;
}

/**
 * Resolve a runnable VS Code executable path for the requested version, applying
 * the Windows CLI path fix. The downloader is lazily imported so unit tests can
 * inject a fake and never touch the network or `@vscode/test-electron`.
 */
export async function resolveVscodeExecutablePath(
  version: string,
  opts: ResolveVscodeOptions = {},
): Promise<string> {
  const platform = opts.platform ?? process.platform;
  const existsSync = opts.existsSync ?? fs.existsSync;
  const download =
    opts.download ??
    (async (v: string) => (await import("@vscode/test-electron")).downloadAndUnzipVSCode(v));

  const execPath = await download(version);
  return platform === "win32" ? applyWindowsCliPathFix(execPath, existsSync) : execPath;
}
