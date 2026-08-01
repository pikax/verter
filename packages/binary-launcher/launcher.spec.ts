/**
 * Unit guards for the shared launcher substrate's own contracts.
 *
 * The per-family suites exercise the happy paths through two real families.
 * What they cannot reach is the substrate's fail-loud behaviour: a rust target
 * the decomposition does not understand, a naming collision, or a launcher
 * constructed without the inputs it needs. Those paths exist precisely so a
 * new platform cannot be half-added, so they need to be proven to fire.
 */

import { afterEach, describe, expect, it } from "vitest";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";

import { buildPlatformMatrix, createLauncher, packageDirResolver } from "./index.js";

const NAMING = { packagePrefix: "@verter/test-", binaryStem: "verter-test" };
const localRequire = createRequire(import.meta.url);

/**
 * Scratch directories, removed after each case.
 *
 * What is on disk is the whole subject of the resolution cases below, so every
 * probed location must be one the test creates and owns — including the
 * workspace whose `target/` holds a development build. The per-family suites
 * cannot own that one: their launcher is constructed against the real
 * repository root, where a contributor who has run `pnpm run build:lsp` has a
 * real binary and a fresh clone has none. An assertion about resolution ORDER
 * made there is decided by whether the developer happened to build, not by the
 * resolver. Constructing a launcher here, over directories this file created,
 * is what makes the order assertable at all.
 */
const scratchDirs: string[] = [];
afterEach(() => {
  for (const dir of scratchDirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function scratchDir(name: string): string {
  const dir = mkdtempSync(join(tmpdir(), name));
  scratchDirs.push(dir);
  return dir;
}

/** Plant an executable stub at `path`, creating its parents. */
function plantBinary(path: string): string {
  mkdirSync(join(path, ".."), { recursive: true });
  writeFileSync(path, "#!/bin/sh\n");
  return path;
}

describe("buildPlatformMatrix", () => {
  it("derives every field from the rust target itself", () => {
    const [row] = buildPlatformMatrix(["aarch64-unknown-linux-musl"], NAMING);
    expect(row).toEqual({
      rustTarget: "aarch64-unknown-linux-musl",
      npmSuffix: "linux-arm64-musl",
      packageName: "@verter/test-linux-arm64-musl",
      os: "linux",
      cpu: "arm64",
      libc: "musl",
      binaryName: "verter-test",
    });
  });

  it("suffixes the binary with .exe only on Windows", () => {
    const [win] = buildPlatformMatrix(["x86_64-pc-windows-msvc"], NAMING);
    const [mac] = buildPlatformMatrix(["aarch64-apple-darwin"], NAMING);
    expect(win.binaryName).toBe("verter-test.exe");
    expect(mac.binaryName).toBe("verter-test");
    expect(mac.libc).toBeNull();
  });

  it("refuses a rust target the decomposition does not cover", () => {
    // Adding a target without teaching the decomposition must fail loudly
    // rather than produce a half-formed row.
    expect(() => buildPlatformMatrix(["riscv64gc-unknown-linux-gnu"], NAMING)).toThrow(
      /unknown rust arch "riscv64gc"/,
    );
    expect(() => buildPlatformMatrix(["x86_64-unknown-freebsd"], NAMING)).toThrow(
      /unknown rust os\/abi "unknown-freebsd"/,
    );
    expect(() => buildPlatformMatrix(["nodashes"], NAMING)).toThrow(/malformed rust target/);
  });

  it("refuses two targets that would claim the same platform package", () => {
    expect(() =>
      buildPlatformMatrix(["x86_64-apple-darwin", "x86_64-apple-darwin"], NAMING),
    ).toThrow(/duplicate npm suffix "darwin-x64"/);
  });

  it("refuses to build a matrix without family naming", () => {
    expect(() => buildPlatformMatrix(["x86_64-apple-darwin"], { packagePrefix: "@x/" })).toThrow(
      /needs a packagePrefix and binaryStem/,
    );
    expect(() => buildPlatformMatrix(["x86_64-apple-darwin"], { binaryStem: "x" })).toThrow(
      /needs a packagePrefix and binaryStem/,
    );
  });
});

describe("createLauncher", () => {
  const matrix = buildPlatformMatrix(["x86_64-unknown-linux-gnu"], NAMING);

  it("refuses to construct without every input", () => {
    const complete = {
      toolName: "verter-test",
      matrix,
      workspaceRoot: "/repo",
      resolvePackageDir: () => null,
    };
    for (const missing of ["toolName", "matrix", "workspaceRoot", "resolvePackageDir"] as const) {
      const options = { ...complete, [missing]: undefined };
      expect(() => createLauncher(options as never), `missing ${missing}`).toThrow(
        /needs toolName, matrix, workspaceRoot and resolvePackageDir/,
      );
    }
  });

  it("names the tool, not the substrate, when a host is unsupported", () => {
    const launcher = createLauncher({
      toolName: "verter-test",
      matrix,
      workspaceRoot: "/repo",
      resolvePackageDir: () => null,
    });
    expect(() => launcher.resolveBinary({ platform: "sunos", arch: "x64", musl: false })).toThrow(
      /^verter-test: unsupported platform 'sunos\/x64'/,
    );
  });

  it("returns null for a suffix outside its own matrix", () => {
    const launcher = createLauncher({
      toolName: "verter-test",
      matrix,
      workspaceRoot: "/repo",
      resolvePackageDir: () => null,
    });
    expect(launcher.platformPackageName("linux-x64-gnu")).toBe("@verter/test-linux-x64-gnu");
    expect(launcher.platformPackageName("darwin-arm64")).toBeNull();
  });
});

describe("resolveBinary — on-disk search order", () => {
  const matrix = buildPlatformMatrix(["x86_64-unknown-linux-gnu"], NAMING);
  const [row] = matrix;
  const host = { platform: row.os, arch: row.cpu, musl: row.libc === "musl" };

  /** A launcher whose every probed location is a directory this test owns. */
  function launcherOver(workspaceRoot: string, packageDir: string | null) {
    return createLauncher({
      toolName: "verter-test",
      matrix,
      workspaceRoot,
      resolvePackageDir: () => packageDir,
    });
  }

  it("falls back to the bare binary name (PATH) when nothing is on disk", () => {
    const launcher = launcherOver(scratchDir("launcher-no-build-"), scratchDir("launcher-pkg-"));

    expect(launcher.resolveBinary(host)).toEqual({ path: row.binaryName, source: "path" });
    expect(launcher.binaryPath(host)).toBe(row.binaryName);
  });

  // The mirror of the case above, and the proof that pointing the launcher at a
  // workspace actually takes effect: with a development build present under the
  // SAME owned workspace, resolution must return that exact path. A
  // `workspaceRoot` the launcher ignored would answer with the bare PATH name
  // here too — so the empty-workspace case above cannot be passing merely
  // because the redirection silently did nothing.
  it("returns the development build under the workspace it was pointed at", () => {
    const workspace = scratchDir("launcher-built-");
    const binary = plantBinary(join(workspace, "target", "debug", row.binaryName));
    const launcher = launcherOver(workspace, scratchDir("launcher-pkgless-"));

    expect(launcher.resolveBinary(host)).toEqual({ path: binary, source: "dev-build" });
  });

  it("prefers a debug build over a release build of the same workspace", () => {
    const workspace = scratchDir("launcher-both-");
    const debug = plantBinary(join(workspace, "target", "debug", row.binaryName));
    const release = plantBinary(join(workspace, "target", "release", row.binaryName));
    const launcher = launcherOver(workspace, scratchDir("launcher-pkgless-"));

    expect(launcher.resolveBinary(host)).toEqual({ path: debug, source: "dev-build" });
    expect(launcher.binaryPath(host)).not.toBe(release);
  });

  it("resolves a release build when only that half of the workspace is built", () => {
    const workspace = scratchDir("launcher-release-");
    const release = plantBinary(join(workspace, "target", "release", row.binaryName));
    const launcher = launcherOver(workspace, scratchDir("launcher-pkgless-"));

    expect(launcher.resolveBinary(host)).toEqual({ path: release, source: "dev-build" });
  });

  // An installed platform package is the published install's binary; a
  // development build is a contributor's. Both present is exactly a contributor
  // with the package installed, and the order between them is the reason the
  // candidate list is ordered rather than a set.
  it("prefers an installed platform package over a development build", () => {
    const workspace = scratchDir("launcher-dev-");
    const devBuild = plantBinary(join(workspace, "target", "debug", row.binaryName));
    const packageDir = scratchDir("launcher-installed-");
    const packaged = plantBinary(join(packageDir, row.binaryName));
    const launcher = launcherOver(workspace, packageDir);

    expect(launcher.resolveBinary(host)).toEqual({ path: packaged, source: "platform-package" });
    expect(launcher.binaryPath(host)).not.toBe(devBuild);
  });

  it("skips a platform package directory that holds no binary", () => {
    const workspace = scratchDir("launcher-dev-only-");
    const devBuild = plantBinary(join(workspace, "target", "debug", row.binaryName));
    const launcher = launcherOver(workspace, scratchDir("launcher-empty-pkg-"));

    expect(launcher.resolveBinary(host)).toEqual({ path: devBuild, source: "dev-build" });
  });
});

describe("packageDirResolver", () => {
  it("resolves an installed package to its directory", () => {
    const resolve = packageDirResolver(localRequire);
    // `vitest` is a dependency of THIS package, so it resolves only through
    // the caller's context — which is the whole point of taking a `require`.
    const dir = resolve("vitest");
    expect(dir).not.toBeNull();
    expect(basename(dir!)).toBe("vitest");
    expect(existsSync(join(dir!, "package.json"))).toBe(true);
  });

  it("returns null rather than throwing when a package is absent", () => {
    const resolve = packageDirResolver(localRequire);
    expect(resolve("@verter/definitely-not-installed-abcxyz")).toBeNull();
  });
});
