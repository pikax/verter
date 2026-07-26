/**
 * Runtime resolution guards for the `verter-lsp` launcher.
 *
 * The launcher's job is to hand an editor client the absolute path of the
 * native server binary for the host it is running on. Everything semantic
 * about that decision — which npm platform package serves a given
 * platform/arch/libc, what the binary is called there, and the order the
 * candidate locations are tried in — is asserted here against the canonical
 * matrix, not against the resolver's own opinion.
 *
 * Expected values are derived from `platforms.js`'s decomposition of the rust
 * targets, so an assertion cannot be satisfied by the resolver agreeing with
 * itself.
 */

import { describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve as resolvePath, sep } from "node:path";
import { fileURLToPath } from "node:url";

import {
  isMusl,
  platformPackageName,
  resolveServerBinary,
  resolveSuffix,
  serverBinaryCandidates,
  serverBinaryPath,
} from "./index.js";
import { PLATFORM_MATRIX, SUPPORTED_TARGETS } from "./platforms.js";

const PACKAGE_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolvePath(PACKAGE_DIR, "..", "..");

/** The (platform, arch, musl) host triple each matrix row must serve. */
function hostFor(row: (typeof PLATFORM_MATRIX)[number]) {
  return { platform: row.os, arch: row.cpu, musl: row.libc === "musl" };
}

describe("resolveSuffix", () => {
  it.each(PLATFORM_MATRIX.map((row) => [row.npmSuffix, row] as const))(
    "%s is selected for its own platform/arch/libc host",
    (suffix, row) => {
      const host = hostFor(row);
      expect(resolveSuffix(host.platform, host.arch, host.musl)).toBe(suffix);
    },
  );

  it("splits linux by libc rather than collapsing to one variant", () => {
    expect(resolveSuffix("linux", "x64", false)).toBe("linux-x64-gnu");
    expect(resolveSuffix("linux", "x64", true)).toBe("linux-x64-musl");
    expect(resolveSuffix("linux", "arm64", false)).toBe("linux-arm64-gnu");
    expect(resolveSuffix("linux", "arm64", true)).toBe("linux-arm64-musl");

    // Negative: a musl host must never be served the glibc package.
    expect(resolveSuffix("linux", "x64", true)).not.toBe("linux-x64-gnu");
    expect(resolveSuffix("linux", "arm64", true)).not.toBe("linux-arm64-gnu");
  });

  it("ignores the libc flag where there is no libc split", () => {
    expect(resolveSuffix("darwin", "arm64", true)).toBe("darwin-arm64");
    expect(resolveSuffix("darwin", "x64", true)).toBe("darwin-x64");
    expect(resolveSuffix("win32", "x64", true)).toBe("win32-x64-msvc");
  });

  it("returns null for combinations no platform package covers", () => {
    expect(resolveSuffix("linux", "ia32", false)).toBeNull();
    expect(resolveSuffix("win32", "arm64", false)).toBeNull();
    expect(resolveSuffix("darwin", "ia32", false)).toBeNull();
    expect(resolveSuffix("freebsd", "x64", false)).toBeNull();
    expect(resolveSuffix("aix", "ppc64", false)).toBeNull();
  });

  it("isMusl is false off linux regardless of host", () => {
    // The probe is only meaningful on linux; on other platforms Node reports a
    // single libc and the matrix has no split to make.
    if (process.platform !== "linux") expect(isMusl()).toBe(false);
  });
});

describe("platformPackageName", () => {
  it.each(PLATFORM_MATRIX.map((row) => [row.npmSuffix, row.packageName] as const))(
    "%s maps to %s",
    (suffix, packageName) => {
      expect(platformPackageName(suffix)).toBe(packageName);
    },
  );
});

describe("serverBinaryCandidates", () => {
  /** Stub the platform-package lookup so ordering is host-independent. */
  const stubDir = (dir: string | null) => () => dir;

  it.each(PLATFORM_MATRIX.map((row) => [row.npmSuffix, row] as const))(
    "%s puts the installed platform package first and PATH last",
    (_suffix, row) => {
      const host = hostFor(row);
      const candidates = serverBinaryCandidates({
        ...host,
        platformPackageDir: stubDir("/stub/pkg"),
      });

      expect(candidates.length).toBeGreaterThanOrEqual(2);
      expect(candidates[0]).toEqual({
        path: join("/stub/pkg", row.binaryName),
        source: "platform-package",
      });

      const last = candidates[candidates.length - 1];
      expect(last).toEqual({ path: row.binaryName, source: "path" });

      // Every candidate carries this row's binary name — a `.exe` only on
      // Windows, and never a `.exe` anywhere else.
      for (const candidate of candidates) {
        expect(candidate.path.endsWith(row.binaryName)).toBe(true);
      }
      expect(row.binaryName.endsWith(".exe")).toBe(row.os === "win32");
    },
  );

  it("omits the platform-package candidate when the package is not installed", () => {
    const candidates = serverBinaryCandidates({
      platform: "linux",
      arch: "x64",
      musl: false,
      platformPackageDir: stubDir(null),
    });
    expect(candidates.some((c) => c.source === "platform-package")).toBe(false);
    expect(candidates.some((c) => c.source === "dev-build")).toBe(true);
  });

  it("points dev-build candidates at the workspace target dir, not above the repo", () => {
    const candidates = serverBinaryCandidates({
      platform: "linux",
      arch: "x64",
      musl: false,
      platformPackageDir: stubDir(null),
    });
    const devPaths = candidates.filter((c) => c.source === "dev-build").map((c) => c.path);

    expect(devPaths).toEqual([
      join(REPO_ROOT, "target", "debug", "verter-lsp"),
      join(REPO_ROOT, "target", "release", "verter-lsp"),
    ]);

    // Negative: an off-by-one in the walk up from the package dir would land
    // in the repository's PARENT, which is outside the workspace entirely.
    const aboveRepo = resolvePath(REPO_ROOT, "..");
    for (const devPath of devPaths) {
      expect(devPath.startsWith(REPO_ROOT + sep)).toBe(true);
      expect(devPath.startsWith(join(aboveRepo, "target") + sep)).toBe(false);
    }
  });

  it("is empty for an unsupported host", () => {
    expect(serverBinaryCandidates({ platform: "freebsd", arch: "x64", musl: false })).toEqual([]);
  });
});

describe("resolveServerBinary", () => {
  it("returns the platform package binary when it exists on disk", () => {
    const row = PLATFORM_MATRIX[0];
    const host = hostFor(row);
    const dir = mkdtempSync(join(tmpdir(), "verter-lsp-pkg-"));
    const binary = join(dir, row.binaryName);
    writeFileSync(binary, "#!/bin/sh\n");

    const resolved = resolveServerBinary({ ...host, platformPackageDir: () => dir });
    expect(resolved).toEqual({ path: binary, source: "platform-package" });
    expect(serverBinaryPath({ ...host, platformPackageDir: () => dir })).toBe(binary);
  });

  it("falls back to the bare binary name (PATH) when nothing is on disk", () => {
    const row = PLATFORM_MATRIX[0];
    const host = hostFor(row);
    const empty = mkdtempSync(join(tmpdir(), "verter-lsp-empty-"));

    const resolved = resolveServerBinary({ ...host, platformPackageDir: () => empty });
    expect(resolved.source).toBe("path");
    expect(resolved.path).toBe(row.binaryName);
  });

  it("exposes the resolved path to bare-command editors via the CLI shim", () => {
    // Helix, Neovim and any editor that launches a bare command cannot resolve
    // a Node module; they ask the shim for the native binary path instead. The
    // flag must answer WITHOUT starting a server, so this is safe to run.
    const stdout = execFileSync(process.execPath, [join("bin", "run.js"), "--print-server-path"], {
      cwd: PACKAGE_DIR,
      encoding: "utf8",
      // Closed stdin plus a hard bound: if the flag branch ever regresses, the
      // shim would forward the flag to a real stdio server, and this must fail
      // fast instead of hanging the suite.
      input: "",
      timeout: 30_000,
    });

    const lines = stdout.trim().split(/\r?\n/);
    expect(lines).toHaveLength(1);

    const expectedName = process.platform === "win32" ? "verter-lsp.exe" : "verter-lsp";
    expect(basename(lines[0])).toBe(expectedName);
  });

  it("throws a supported-target list for an unsupported host", () => {
    expect(() => resolveServerBinary({ platform: "freebsd", arch: "x64", musl: false })).toThrow(
      /freebsd\/x64/,
    );
    expect(() => resolveServerBinary({ platform: "freebsd", arch: "x64", musl: false })).toThrow(
      new RegExp(SUPPORTED_TARGETS.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
    );
  });
});
