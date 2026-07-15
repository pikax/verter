import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import {
  applyWindowsCliPathFix,
  copyLspBinaryToTemp,
  findLspBinary,
  resolveVscodeExecutablePath,
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
});
