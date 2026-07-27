/**
 * The shared guard suite every Verter binary-family launcher must satisfy.
 *
 * A family (`verter-lsp`, `verter-mcp`) is a launcher package plus one
 * per-platform binary package each. The same things can go wrong in every one
 * of them: a template whose `os`/`cpu`/`libc` gates install to the wrong host,
 * an `optionalDependencies` list that has drifted from the matrix, an orphaned
 * platform directory, a resolver that hands back the glibc binary on a musl
 * host, a staged binary nobody keeps out of git.
 *
 * Asserting that once, parameterised by family, is the same rule the runtime
 * follows: one implementation, no per-family fork that can quietly diverge.
 * Expected values are derived from the family's own matrix — which is computed
 * from its rust targets, never read from the templates under test.
 */

import { describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve as resolvePath, sep } from "node:path";

import type { PlatformEntry } from "../index.js";

/** The runtime surface a launcher package exposes. */
export interface FamilyModule {
  readonly PLATFORM_MATRIX: readonly PlatformEntry[];
  readonly SUPPORTED_TARGETS: string;
  isMusl(): boolean;
  platformPackageName(npmSuffix: string): string | null;
  resolveSuffix(platform: string, arch: string, musl: boolean): string | null;
  serverBinaryCandidates(options?: Record<string, unknown>): readonly {
    path: string;
    source: string;
  }[];
  resolveServerBinary(options?: Record<string, unknown>): { path: string; source: string };
  serverBinaryPath(options?: Record<string, unknown>): string;
}

/** The matrix module a launcher package exposes. */
export interface FamilyPlatformsModule {
  readonly SUPPORTED_RUST_TARGETS: readonly string[];
  buildPlatformMatrix(rustTargets: readonly string[]): readonly PlatformEntry[];
}

export interface BinaryFamily {
  /** Absolute path of the launcher package directory. */
  readonly packageDir: string;
  /** Published launcher package name, e.g. `verter-lsp`. */
  readonly packageName: string;
  /** `require`d launcher module. */
  readonly module: FamilyModule;
  /** `require`d matrix module. */
  readonly platforms: FamilyPlatformsModule;
}

interface PlatformPackageJson {
  name?: string;
  version?: string;
  files?: string[];
  os?: string[];
  cpu?: string[];
  libc?: string[];
  license?: string;
}

interface LauncherPackageJson {
  name?: string;
  version?: string;
  bin?: Record<string, string>;
  files?: string[];
  main?: string;
  dependencies?: Record<string, string>;
  optionalDependencies?: Record<string, string>;
}

function readJson<T>(path: string): T {
  return JSON.parse(readFileSync(path, "utf8")) as T;
}

/** Run the full family guard suite. */
export function describeBinaryFamily(family: BinaryFamily): void {
  const { packageDir, packageName, module: mod, platforms } = family;

  const dirName = basename(packageDir);
  const repoRoot = resolvePath(packageDir, "..", "..");
  const matrix = mod.PLATFORM_MATRIX;
  const launcher = readJson<LauncherPackageJson>(join(packageDir, "package.json"));

  const hostFor = (row: PlatformEntry) => ({
    platform: row.os,
    arch: row.cpu,
    musl: row.libc === "musl",
  });

  const readTemplate = (npmSuffix: string) =>
    readJson<PlatformPackageJson>(join(packageDir, "npm", npmSuffix, "package.json"));

  const templateDirsOnDisk = () =>
    readdirSync(join(packageDir, "npm"), { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => entry.name)
      .sort();

  describe(`${packageName} — host resolution`, () => {
    it.each(matrix.map((row) => [row.npmSuffix, row] as const))(
      "%s is selected for its own platform/arch/libc host",
      (suffix, row) => {
        const host = hostFor(row);
        expect(mod.resolveSuffix(host.platform, host.arch, host.musl)).toBe(suffix);
      },
    );

    it("splits linux by libc rather than collapsing to one variant", () => {
      expect(mod.resolveSuffix("linux", "x64", false)).toBe("linux-x64-gnu");
      expect(mod.resolveSuffix("linux", "x64", true)).toBe("linux-x64-musl");
      expect(mod.resolveSuffix("linux", "arm64", false)).toBe("linux-arm64-gnu");
      expect(mod.resolveSuffix("linux", "arm64", true)).toBe("linux-arm64-musl");

      // Negative: a musl host must never be served the glibc package.
      expect(mod.resolveSuffix("linux", "x64", true)).not.toBe("linux-x64-gnu");
      expect(mod.resolveSuffix("linux", "arm64", true)).not.toBe("linux-arm64-gnu");
    });

    it("ignores the libc flag where there is no libc split", () => {
      expect(mod.resolveSuffix("darwin", "arm64", true)).toBe("darwin-arm64");
      expect(mod.resolveSuffix("darwin", "x64", true)).toBe("darwin-x64");
      expect(mod.resolveSuffix("win32", "x64", true)).toBe("win32-x64-msvc");
    });

    it("returns null for combinations no platform package covers", () => {
      expect(mod.resolveSuffix("linux", "ia32", false)).toBeNull();
      expect(mod.resolveSuffix("win32", "arm64", false)).toBeNull();
      expect(mod.resolveSuffix("darwin", "ia32", false)).toBeNull();
      expect(mod.resolveSuffix("freebsd", "x64", false)).toBeNull();
      expect(mod.resolveSuffix("aix", "ppc64", false)).toBeNull();
    });

    it("reports a non-musl libc off linux", () => {
      if (process.platform !== "linux") expect(mod.isMusl()).toBe(false);
    });

    it.each(matrix.map((row) => [row.npmSuffix, row.packageName] as const))(
      "%s maps to %s",
      (suffix, expected) => {
        expect(mod.platformPackageName(suffix)).toBe(expected);
      },
    );
  });

  describe(`${packageName} — binary candidates`, () => {
    const stubDir = (dir: string | null) => () => dir;

    it.each(matrix.map((row) => [row.npmSuffix, row] as const))(
      "%s puts the installed platform package first and PATH last",
      (_suffix, row) => {
        const candidates = mod.serverBinaryCandidates({
          ...hostFor(row),
          platformPackageDir: stubDir("/stub/pkg"),
        });

        expect(candidates.length).toBeGreaterThanOrEqual(2);
        expect(candidates[0]).toEqual({
          path: join("/stub/pkg", row.binaryName),
          source: "platform-package",
        });
        expect(candidates[candidates.length - 1]).toEqual({
          path: row.binaryName,
          source: "path",
        });

        for (const candidate of candidates) {
          expect(candidate.path.endsWith(row.binaryName)).toBe(true);
        }
        expect(row.binaryName.endsWith(".exe")).toBe(row.os === "win32");
      },
    );

    it("omits the platform-package candidate when the package is not installed", () => {
      const candidates = mod.serverBinaryCandidates({
        platform: "linux",
        arch: "x64",
        musl: false,
        platformPackageDir: stubDir(null),
      });
      expect(candidates.some((c) => c.source === "platform-package")).toBe(false);
      expect(candidates.some((c) => c.source === "dev-build")).toBe(true);
    });

    it("points dev-build candidates at the workspace target dir, not above the repo", () => {
      const row = matrix.find((entry) => entry.os === "linux" && entry.libc === "glibc")!;
      const devPaths = mod
        .serverBinaryCandidates({
          ...hostFor(row),
          platformPackageDir: stubDir(null),
        })
        .filter((c) => c.source === "dev-build")
        .map((c) => c.path);

      expect(devPaths).toEqual([
        join(repoRoot, "target", "debug", row.binaryName),
        join(repoRoot, "target", "release", row.binaryName),
      ]);

      // Negative: an off-by-one in the walk up from the package dir would land
      // in the repository's PARENT, outside the workspace entirely.
      const aboveRepo = resolvePath(repoRoot, "..");
      for (const devPath of devPaths) {
        expect(devPath.startsWith(repoRoot + sep)).toBe(true);
        expect(devPath.startsWith(join(aboveRepo, "target") + sep)).toBe(false);
      }
    });

    it("is empty for an unsupported host", () => {
      expect(mod.serverBinaryCandidates({ platform: "freebsd", arch: "x64", musl: false })).toEqual(
        [],
      );
    });
  });

  describe(`${packageName} — binary resolution`, () => {
    it("returns the platform package binary when it exists on disk", () => {
      const row = matrix[0];
      const dir = mkdtempSync(join(tmpdir(), `${packageName}-pkg-`));
      const binary = join(dir, row.binaryName);
      writeFileSync(binary, "#!/bin/sh\n");

      const options = { ...hostFor(row), platformPackageDir: () => dir };
      expect(mod.resolveServerBinary(options)).toEqual({
        path: binary,
        source: "platform-package",
      });
      expect(mod.serverBinaryPath(options)).toBe(binary);
    });

    // The rest of the search order — a development build, and the bare-name
    // PATH fallback when nothing is on disk — is asserted in
    // `packages/binary-launcher/launcher.spec.ts`, over a launcher built on
    // directories that suite owns. It cannot be asserted from here: this
    // family's launcher probes the REAL repository's `target/`, which holds a
    // binary on a contributor's machine and none on a CI runner that never
    // compiles Rust, so the outcome would be decided by whether the developer
    // happened to build rather than by the resolver. What IS this family's to
    // prove is that its exported entry points are the shared launcher's — the
    // case above, and the candidate list asserted in full further up.

    it("throws a supported-target list for an unsupported host", () => {
      const unsupported = { platform: "freebsd", arch: "x64", musl: false };
      expect(() => mod.resolveServerBinary(unsupported)).toThrow(/freebsd\/x64/);
      expect(() => mod.resolveServerBinary(unsupported)).toThrow(
        new RegExp(mod.SUPPORTED_TARGETS.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
      );
    });

    it("exposes the resolved path to bare-command clients via the CLI shim", () => {
      // Editors and MCP hosts that cannot resolve a Node module ask the shim
      // for the native binary path. The flag must answer WITHOUT starting a
      // server, so this is safe to run.
      const stdout = execFileSync(
        process.execPath,
        [join("bin", "run.js"), "--print-server-path"],
        {
          cwd: packageDir,
          encoding: "utf8",
          // Closed stdin plus a hard bound: if the flag branch ever regresses,
          // the shim would forward the flag to a real stdio server, and this
          // must fail fast instead of hanging the suite.
          input: "",
          timeout: 30_000,
        },
      );

      const lines = stdout.trim().split(/\r?\n/);
      expect(lines).toHaveLength(1);

      const stem = matrix.find((row) => row.os === process.platform)?.binaryName ?? packageName;
      expect(basename(lines[0])).toBe(stem);
    });
  });

  describe(`${packageName} — platform packages`, () => {
    it("covers the full rust-target matrix", () => {
      expect(matrix.length).toBe(platforms.SUPPORTED_RUST_TARGETS.length);
      expect(matrix.length).toBe(7);
    });

    it.each(matrix.map((row) => [row.npmSuffix, row] as const))(
      "npm/%s/package.json matches the canonical matrix row",
      (_suffix, row) => {
        const tpl = readTemplate(row.npmSuffix);

        expect(tpl.name).toBe(row.packageName);

        // `files` ships EXACTLY the one platform binary — nothing else belongs
        // in a package whose only purpose is to carry it.
        expect(tpl.files).toEqual([row.binaryName]);

        // Platform constraints gate install to the right host.
        expect(tpl.os).toEqual([row.os]);
        expect(tpl.cpu).toEqual([row.cpu]);
        if (row.libc) {
          expect(tpl.libc).toEqual([row.libc]);
        } else {
          expect(tpl.libc).toBeUndefined();
        }

        // Lock-step with the launcher: a drifted platform package would serve a
        // binary from a different release than the client expects.
        expect(tpl.version).toBe(launcher.version);
        expect(tpl.license).toBe("MIT");
      },
    );

    it("has no template directory outside the matrix", () => {
      expect(templateDirsOnDisk()).toEqual(matrix.map((row) => row.npmSuffix).sort());
    });

    it("declares every platform package as an optional dependency, and nothing else", () => {
      const expected = matrix.map((row) => row.packageName).sort();
      expect(Object.keys(launcher.optionalDependencies ?? {}).sort()).toEqual(expected);
      for (const range of Object.values(launcher.optionalDependencies ?? {})) {
        expect(range).toBe("workspace:*");
      }
    });

    it("publishes the launcher runtime and no test material", () => {
      expect(launcher.name).toBe(packageName);
      expect(launcher.bin).toEqual({ [packageName]: "./bin/run.js" });
      expect(launcher.main).toBe("index.js");

      // Resolution is shared, so the launcher carries it as a runtime dep.
      expect(launcher.dependencies?.["@verter/binary-launcher"]).toBe("workspace:^");

      const files = launcher.files ?? [];
      for (const entry of ["bin", "index.js", "index.d.ts", "platforms.js", "platforms.d.ts"]) {
        expect(files).toContain(entry);
      }
      for (const entry of files) {
        expect(entry).not.toMatch(/\.spec\./);
        expect(entry).not.toContain("test-support");
      }
    });

    it("is a workspace package family, so the launcher links its platform packages", () => {
      const workspaceYaml = readFileSync(join(repoRoot, "pnpm-workspace.yaml"), "utf8");
      expect(workspaceYaml).toContain(`packages/${dirName}/npm/*`);
    });

    it("keeps the staged binaries out of git", () => {
      const gitignore = readFileSync(join(repoRoot, ".gitignore"), "utf8");
      const stem = matrix.find((row) => row.os !== "win32")!.binaryName;
      // CI stages the built binaries into the template dirs at publish time;
      // committing one would ship a stale binary.
      expect(gitignore).toContain(`packages/${dirName}/npm/*/${stem}`);
      expect(gitignore).toContain(`packages/${dirName}/npm/*/${stem}.exe`);
    });

    // ---- Discrimination self-proof ---------------------------------------
    // Build a VARIANT matrix from a rust-target list with one target dropped.
    // The real enumerations must then DISAGREE with the variant, proving each
    // arm above genuinely cross-checks the matrix rather than comparing a
    // thing to itself.
    describe("(discrimination) dropping a target desynchronises every enumeration", () => {
      const DROPPED = "x86_64-pc-windows-msvc";
      const variantMatrix = platforms.buildPlatformMatrix(
        platforms.SUPPORTED_RUST_TARGETS.filter((target) => target !== DROPPED),
      );

      it("the variant really is smaller and lacks the dropped platform", () => {
        expect(variantMatrix.length).toBe(matrix.length - 1);
        expect(variantMatrix.some((row) => row.npmSuffix === "win32-x64-msvc")).toBe(false);
      });

      it("template dirs on disk no longer match the variant", () => {
        expect(templateDirsOnDisk()).not.toEqual(variantMatrix.map((r) => r.npmSuffix).sort());
        expect(templateDirsOnDisk()).toContain("win32-x64-msvc");
      });

      it("optionalDependencies no longer match the variant", () => {
        const actual = Object.keys(launcher.optionalDependencies ?? {}).sort();
        expect(actual).not.toEqual(variantMatrix.map((r) => r.packageName).sort());
        expect(actual).toContain(
          matrix.find((row) => row.npmSuffix === "win32-x64-msvc")!.packageName,
        );
      });
    });
  });
}
