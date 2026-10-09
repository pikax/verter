import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import {
  acquireVscodeInChildProcess,
  applyWindowsCliPathFix,
  copyLspBinaryToTemp,
  findLspBinary,
  resolveVscodeExecutablePath,
  VSCODE_ACQUISITION_RETRY,
  VSCODE_EXECUTABLE_MARKER,
} from "../sharedLaunch";

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

function tmp(prefix: string): string {
  const d = mkdtempSync(join(tmpdir(), prefix));
  tmps.push(d);
  return d;
}

const BIN = process.platform === "win32" ? "verter-lsp.exe" : "verter-lsp";

describe("findLspBinary", () => {
  it("finds the binary under target/debug from the extension path", () => {
    const ext = tmp("dx-find-");
    const debugDir = join(ext, "target", "debug");
    mkdirSync(debugDir, { recursive: true });
    const expected = join(debugDir, BIN);
    writeFileSync(expected, "binary");
    expect(findLspBinary(ext)).toBe(expected);
  });

  it("walks upward to a monorepo target/release", () => {
    const root = tmp("dx-find-up-");
    const releaseDir = join(root, "target", "release");
    mkdirSync(releaseDir, { recursive: true });
    const expected = join(releaseDir, BIN);
    writeFileSync(expected, "binary");
    // Extension path is a nested package; the search must climb to the root.
    const ext = join(root, "packages", "vue-vscode");
    mkdirSync(ext, { recursive: true });
    expect(findLspBinary(ext)).toBe(expected);
  });

  it("falls back to dist/ then bin/ inside the extension path", () => {
    const ext = tmp("dx-find-dist-");
    const distBin = join(ext, "dist", BIN);
    mkdirSync(join(ext, "dist"), { recursive: true });
    writeFileSync(distBin, "binary");
    expect(findLspBinary(ext)).toBe(distBin);
  });

  it("returns undefined when no binary exists anywhere reachable", () => {
    const ext = tmp("dx-find-none-");
    expect(findLspBinary(ext)).toBeUndefined();
  });
});

describe("copyLspBinaryToTemp", () => {
  it("returns undefined and does not throw when the source binary is missing", () => {
    const ext = tmp("dx-copy-none-");
    expect(copyLspBinaryToTemp(ext)).toBeUndefined();
  });

  it("produces a usable binary path that is a faithful copy of the source", () => {
    const ext = tmp("dx-copy-");
    const debugDir = join(ext, "target", "debug");
    mkdirSync(debugDir, { recursive: true });
    const source = join(debugDir, BIN);
    writeFileSync(source, "the-real-binary-bytes");

    const used = copyLspBinaryToTemp(ext);
    expect(used).toBeDefined();
    expect(existsSync(used!)).toBe(true);
    // The returned path must contain the same bytes as the source binary,
    // whether it is the source itself (POSIX) or a temp copy (Windows).
    expect(readFileSync(used!, "utf-8")).toBe("the-real-binary-bytes");
    if (used !== source) tmps.push(join(used!, ".."));
  });

  it("on Windows copies off the source path so a running .exe cannot lock the rebuild", () => {
    if (process.platform !== "win32") return;
    const ext = tmp("dx-copy-win-");
    const debugDir = join(ext, "target", "debug");
    mkdirSync(debugDir, { recursive: true });
    const source = join(debugDir, BIN);
    writeFileSync(source, "x");
    const used = copyLspBinaryToTemp(ext);
    // Negative: the used path must NOT be the source path on Windows.
    expect(used).not.toBe(source);
    if (used) tmps.push(join(used, ".."));
  });
});

describe("applyWindowsCliPathFix", () => {
  it("rewrites Code.exe to the bin/code.cmd CLI entry point when it exists", () => {
    const base = "C:\\vscode\\Code.exe";
    const fixed = applyWindowsCliPathFix(base, () => true);
    expect(fixed.endsWith("code.cmd")).toBe(true);
    expect(fixed).not.toBe(base);
  });

  it("leaves the path untouched when the CLI entry point is absent", () => {
    const base = "C:\\vscode\\Code.exe";
    expect(applyWindowsCliPathFix(base, () => false)).toBe(base);
  });
});

describe("resolveVscodeExecutablePath", () => {
  it("uses a validated explicit host path without invoking the downloader", async () => {
    const explicit = "C:\\vscode\\Code - Insiders.exe";
    let downloads = 0;
    const resolved = await resolveVscodeExecutablePath("insiders", {
      explicitExecutablePath: explicit,
      download: async () => {
        downloads++;
        return "C:\\downloaded\\Code.exe";
      },
      existsSync: (candidate) => candidate === explicit,
    });

    expect(resolved).toBe(explicit);
    expect(downloads).toBe(0);
  });

  it("rejects an explicit host path that does not exist", async () => {
    await expect(
      resolveVscodeExecutablePath("insiders", {
        explicitExecutablePath: "C:\\missing\\Code.exe",
        existsSync: () => false,
      }),
    ).rejects.toThrow("Configured VS Code executable does not exist");
  });

  it("returns the downloaded path verbatim on non-Windows platforms", async () => {
    const downloaded = "/opt/vscode/code";
    const resolved = await resolveVscodeExecutablePath("stable", {
      download: async () => downloaded,
      platform: "linux",
      existsSync: () => true,
    });
    expect(resolved).toBe(downloaded);
  });

  it("returns the downloaded host executable on Windows", async () => {
    const downloaded = "C:\\vscode\\Code.exe";
    const resolved = await resolveVscodeExecutablePath("stable", {
      download: async () => downloaded,
      platform: "win32",
      existsSync: () => true,
    });
    expect(resolved).toBe(downloaded);
  });

  it("passes the requested version through to the injected downloader", async () => {
    let seen: string | undefined;
    await resolveVscodeExecutablePath("1.95.0", {
      download: async (v) => {
        seen = v;
        return "/opt/vscode/code";
      },
      platform: "linux",
      existsSync: () => false,
    });
    expect(seen).toBe("1.95.0");
  });

  it("retries a failed VS Code acquisition a bounded number of times before giving up", async () => {
    // Acquisition is a network download; a transient timeout there is not a
    // product failure and must not fail the route. Assertions never retry.
    let attempts = 0;
    const waits: number[] = [];
    const resolved = await resolveVscodeExecutablePath("stable", {
      download: async () => {
        attempts++;
        if (attempts < 3)
          throw Object.assign(new Error("connect ETIMEDOUT"), { code: "ETIMEDOUT" });
        return "/opt/vscode/code";
      },
      platform: "linux",
      existsSync: () => true,
      retry: { attempts: 3, delayMs: 10, sleep: async (ms) => void waits.push(ms) },
    });
    expect(resolved).toBe("/opt/vscode/code");
    expect(attempts).toBe(3);
    expect(waits).toEqual([10, 20]);
  });

  it("surfaces the last acquisition error once the retry budget is exhausted", async () => {
    let attempts = 0;
    await expect(
      resolveVscodeExecutablePath("stable", {
        download: async () => {
          attempts++;
          throw new Error(`download failed #${attempts}`);
        },
        platform: "linux",
        existsSync: () => true,
        retry: { attempts: 3, delayMs: 1, sleep: async () => undefined },
      }),
    ).rejects.toThrow(
      /Could not acquire VS Code stable after 3 attempt\(s\)\..*download failed #3/,
    );
    expect(attempts).toBe(3);
  });

  it("keeps the retry bounded when the configured attempt count is not finite", async () => {
    let attempts = 0;
    await expect(
      resolveVscodeExecutablePath("stable", {
        download: async () => {
          attempts++;
          throw new Error("never");
        },
        platform: "linux",
        existsSync: () => true,
        retry: { attempts: Number.POSITIVE_INFINITY, delayMs: 1, sleep: async () => undefined },
      }),
    ).rejects.toThrow("never");
    expect(attempts).toBe(VSCODE_ACQUISITION_RETRY.attempts);
  });
});

describe("acquireVscodeInChildProcess", () => {
  function script(body: string): string {
    const file = join(tmp("dx-acquire-"), "acquire.js");
    writeFileSync(file, body);
    return file;
  }

  it("returns the executable the acquisition process reports", async () => {
    const scriptPath = script(
      `console.log("progress");\nconsole.log(${JSON.stringify(VSCODE_EXECUTABLE_MARKER)} + "/opt/vscode/code-" + process.argv[2]);`,
    );
    await expect(acquireVscodeInChildProcess("1.135.0", { scriptPath })).resolves.toBe(
      "/opt/vscode/code-1.135.0",
    );
  });

  it("turns a download that crashes its process into an ordinary, retryable rejection", async () => {
    // test-electron 2.5.x leaks a rejected promise when the archive stream is
    // reset (ECONNRESET), which Node turns into an uncaught exception. In-process
    // that killed the runner; out of process it is just a failed attempt.
    const scriptPath = script(
      `Promise.reject(Object.assign(new Error("aborted"), { code: "ECONNRESET" }));`,
    );
    await expect(acquireVscodeInChildProcess("1.135.0", { scriptPath })).rejects.toThrow(
      "VS Code 1.135.0 acquisition process exited with code 1",
    );
  });

  it("rejects a process that succeeds without reporting an executable", async () => {
    const scriptPath = script(`console.log("nothing to see");`);
    await expect(acquireVscodeInChildProcess("stable", { scriptPath })).rejects.toThrow(
      "exited without reporting an executable",
    );
  });

  it("kills and rejects a process that outlives its time cap", async () => {
    const scriptPath = script(`setTimeout(() => {}, 60_000);`);
    await expect(
      acquireVscodeInChildProcess("stable", { scriptPath, timeoutMs: 200 }),
    ).rejects.toThrow("timed out after 200ms");
  });
});
