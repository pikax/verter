/**
 * Unit guards for the shared launcher substrate's own contracts.
 *
 * The per-family suites exercise the happy paths through two real families.
 * What they cannot reach is the substrate's fail-loud behaviour: a rust target
 * the decomposition does not understand, a naming collision, or a launcher
 * constructed without the inputs it needs. Those paths exist precisely so a
 * new platform cannot be half-added, so they need to be proven to fire.
 */

import { describe, expect, it } from "vitest";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { basename, join } from "node:path";

import { buildPlatformMatrix, createLauncher, packageDirResolver } from "./index.js";

const NAMING = { packagePrefix: "@verter/test-", binaryStem: "verter-test" };
const localRequire = createRequire(import.meta.url);

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
