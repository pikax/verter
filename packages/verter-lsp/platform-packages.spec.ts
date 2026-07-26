/**
 * Shape guards for the `verter-lsp` distribution: the launcher package and its
 * per-platform optional-dependency packages
 * (`packages/verter-lsp/npm/<suffix>/package.json`).
 *
 * The launcher resolves `@verter/lsp-<suffix>` and then loads that package's
 * binary. If a template's `name`/`files`/`os`/`cpu`/`libc`/`version` is wrong,
 * the package manager either refuses to install it on the right host (wrong
 * `os`/`cpu`/`libc`), installs it but the launcher can't find the binary
 * (wrong `files`), or installs a version-mismatched server (drifted
 * `version`). Both directions are pinned — every matrix row has a template,
 * and every template on disk is a matrix row — so a dropped platform cannot
 * leave an orphan behind.
 *
 * Expected values come from the matrix derived in `platforms.js` from the rust
 * targets, never from the templates under test.
 */

import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve as resolvePath } from "node:path";
import { fileURLToPath } from "node:url";

import { PLATFORM_MATRIX, buildPlatformMatrix, SUPPORTED_RUST_TARGETS } from "./platforms.js";

const PACKAGE_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolvePath(PACKAGE_DIR, "..", "..");

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
  optionalDependencies?: Record<string, string>;
}

function readJson<T>(path: string): T {
  return JSON.parse(readFileSync(path, "utf8")) as T;
}

const launcher = readJson<LauncherPackageJson>(join(PACKAGE_DIR, "package.json"));

function readTemplate(npmSuffix: string): PlatformPackageJson {
  return readJson<PlatformPackageJson>(join(PACKAGE_DIR, "npm", npmSuffix, "package.json"));
}

function templateDirsOnDisk(): string[] {
  return readdirSync(join(PACKAGE_DIR, "npm"), { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
}

describe("verter-lsp platform packages", () => {
  it("covers the full rust-target matrix", () => {
    expect(PLATFORM_MATRIX.length).toBe(SUPPORTED_RUST_TARGETS.length);
    expect(PLATFORM_MATRIX.length).toBe(7);
  });

  it.each(PLATFORM_MATRIX.map((row) => [row.npmSuffix, row] as const))(
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
      // server binary from a different release than the client expects.
      expect(tpl.version).toBe(launcher.version);
      expect(tpl.license).toBe("MIT");
    },
  );

  it("has no template directory outside the matrix", () => {
    const expected = PLATFORM_MATRIX.map((row) => row.npmSuffix).sort();
    expect(templateDirsOnDisk()).toEqual(expected);
  });

  it("declares every platform package as an optional dependency, and nothing else", () => {
    const expected = PLATFORM_MATRIX.map((row) => row.packageName).sort();
    expect(Object.keys(launcher.optionalDependencies ?? {}).sort()).toEqual(expected);
    for (const range of Object.values(launcher.optionalDependencies ?? {})) {
      expect(range).toBe("workspace:*");
    }
  });

  it("publishes the launcher runtime and no test material", () => {
    expect(launcher.name).toBe("verter-lsp");
    expect(launcher.bin).toEqual({ "verter-lsp": "./bin/run.js" });
    expect(launcher.main).toBe("index.js");

    const files = launcher.files ?? [];
    // The runtime surface the published launcher needs: the CLI shim, the
    // resolver, the matrix it resolves through, and the type declarations.
    expect(files).toContain("bin");
    expect(files).toContain("index.js");
    expect(files).toContain("index.d.ts");
    expect(files).toContain("platforms.js");
    expect(files).toContain("platforms.d.ts");

    // Negative: specs and test helpers must never enter the tarball.
    for (const entry of files) {
      expect(entry).not.toMatch(/\.spec\./);
      expect(entry).not.toContain("test-helpers");
    }
  });

  it("is a workspace package family, so the launcher links its platform packages", () => {
    const workspaceYaml = readFileSync(join(REPO_ROOT, "pnpm-workspace.yaml"), "utf8");
    expect(workspaceYaml).toContain("packages/verter-lsp/npm/*");
  });

  it("keeps the staged binaries out of git", () => {
    const gitignore = readFileSync(join(REPO_ROOT, ".gitignore"), "utf8");
    // CI stages the built binaries into the template dirs at publish time;
    // committing one would ship a stale server.
    expect(gitignore).toContain("packages/verter-lsp/npm/*/verter-lsp");
    expect(gitignore).toContain("packages/verter-lsp/npm/*/verter-lsp.exe");
  });

  // ---- Discrimination self-proof -------------------------------------------
  // Build a VARIANT matrix from a rust-target list with one target dropped.
  // The real enumerations must then DISAGREE with the variant, proving each
  // arm above genuinely cross-checks the matrix rather than comparing a thing
  // to itself.
  describe("(discrimination) dropping a target desynchronises every enumeration", () => {
    const DROPPED = "x86_64-pc-windows-msvc";
    const variantMatrix = buildPlatformMatrix(
      SUPPORTED_RUST_TARGETS.filter((target) => target !== DROPPED),
    );

    it("the variant really is smaller and lacks the dropped platform", () => {
      expect(variantMatrix.length).toBe(PLATFORM_MATRIX.length - 1);
      expect(variantMatrix.some((row) => row.npmSuffix === "win32-x64-msvc")).toBe(false);
    });

    it("template dirs on disk no longer match the variant", () => {
      const variantSuffixes = variantMatrix.map((row) => row.npmSuffix).sort();
      expect(templateDirsOnDisk()).not.toEqual(variantSuffixes);
      expect(templateDirsOnDisk()).toContain("win32-x64-msvc");
    });

    it("optionalDependencies no longer match the variant", () => {
      const variantPackages = variantMatrix.map((row) => row.packageName).sort();
      const actual = Object.keys(launcher.optionalDependencies ?? {}).sort();
      expect(actual).not.toEqual(variantPackages);
      expect(actual).toContain("@verter/lsp-win32-x64-msvc");
    });
  });
});
