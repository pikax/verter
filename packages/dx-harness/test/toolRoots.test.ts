import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { afterEach, describe, expect, it } from "vitest";

import { canonicalizePath, joinCanonical } from "../src/paths.js";
import { readTypescriptVersionFromDisk, resolveToolRoots } from "../src/toolRoots.js";

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

function tmp(): string {
  const d = mkdtempSync(join(tmpdir(), "dx-toolroots-"));
  tmps.push(d);
  return d;
}

/**
 * The real repository root — three levels up from this test file
 * (`packages/dx-harness/test/` → repo root). Used to exercise the DEFAULT tsdk
 * resolution against the actual pnpm checkout (no injected version reader), so the
 * test proves `expectedTsserverJs` points at a tsserver.js that EXISTS on disk.
 */
const REPO_ROOT = canonicalizePath(fileURLToPath(new URL("../../..", import.meta.url)));

describe("resolveToolRoots", () => {
  it("defaults the tsdk to the extension's bundled TypeScript lib and derives tsserver.js", () => {
    const roots = resolveToolRoots("D:/wt/dx-harness", {
      readTypescriptVersion: () => "6.0.3",
    });
    expect(roots.repoRoot).toBe("d:/wt/dx-harness");
    // The shipped extension's bundled `--tsdk` is `<extensionPath>/node_modules/typescript/lib`,
    // with `extensionPath` = the `packages/vue-vscode` package (extension.ts).
    expect(roots.tsserverTsdk).toBe(
      "d:/wt/dx-harness/packages/vue-vscode/node_modules/typescript/lib",
    );
    expect(roots.expectedTsserverJs).toBe(
      "d:/wt/dx-harness/packages/vue-vscode/node_modules/typescript/lib/tsserver.js",
    );
    expect(roots.expectedTsserverJs).toBe(`${roots.tsserverTsdk}/tsserver.js`);
    // Negative: NOT the bare repo-root node_modules, which a pnpm workspace leaves
    // without a hoisted typescript.
    expect(roots.tsserverTsdk).not.toBe("d:/wt/dx-harness/node_modules/typescript/lib");
    expect(roots.tsserverVersion).toBe("6.0.3");
  });

  it("normalises Windows backslashes and drive casing in every path it emits", () => {
    const roots = resolveToolRoots("D:\\wt\\dx-harness", {
      readTypescriptVersion: () => "6.0.3",
    });
    for (const p of [roots.repoRoot, roots.tsserverTsdk, roots.expectedTsserverJs]) {
      expect(p).not.toContain("\\");
      expect(p.startsWith("d:/")).toBe(true);
    }
  });

  it("keeps a canonical UNC // prefix through the bundled-extension tsdk join", () => {
    const roots = resolveToolRoots("//server/share/repo", {
      readTypescriptVersion: () => "6.0.3",
    });
    // A UNC base must survive the join — posix.join would collapse `//` to `/`,
    // diverging from verter_span's canonical UNC identity on Windows.
    expect(roots.repoRoot).toBe("//server/share/repo");
    expect(roots.tsserverTsdk).toBe(
      "//server/share/repo/packages/vue-vscode/node_modules/typescript/lib",
    );
    expect(roots.expectedTsserverJs).toBe(
      "//server/share/repo/packages/vue-vscode/node_modules/typescript/lib/tsserver.js",
    );
    // Discrimination: the leading double slash is NOT collapsed to a single one.
    expect(roots.tsserverTsdk).not.toBe(
      "/server/share/repo/packages/vue-vscode/node_modules/typescript/lib",
    );
    expect(roots.expectedTsserverJs).not.toBe(
      "/server/share/repo/packages/vue-vscode/node_modules/typescript/lib/tsserver.js",
    );
  });

  it("lets an explicit user tsdk override the default and derives tsserver.js from it", () => {
    const roots = resolveToolRoots("/home/me/repo", {
      userTsdk: "/opt/ts/lib",
      readTypescriptVersion: () => "6.0.3",
    });
    expect(roots.tsserverTsdk).toBe("/opt/ts/lib");
    expect(roots.expectedTsserverJs).toBe("/opt/ts/lib/tsserver.js");
    // Negative: the override is NOT placed under the repo's node_modules.
    expect(roots.expectedTsserverJs).not.toContain("/repo/node_modules/");
  });

  it("reads the pinned TypeScript version against the RESOLVED tsdk", () => {
    const seen: string[] = [];
    const roots = resolveToolRoots("/home/me/repo", {
      userTsdk: "/opt/ts/lib",
      readTypescriptVersion: (tsdk) => {
        seen.push(tsdk);
        return "5.9.9";
      },
    });
    expect(roots.tsserverVersion).toBe("5.9.9");
    // The version is sourced from the tsdk actually in use, not a fixed path.
    expect(seen).toEqual(["/opt/ts/lib"]);
  });

  it("surfaces an optional tsgo binary pin, canonicalised, and omits it when absent", () => {
    const withTsgo = resolveToolRoots("/repo", {
      tsgoBin: "C:\\tools\\tsgo.exe",
      readTypescriptVersion: () => "6.0.3",
    });
    expect(withTsgo.tsgoBin).toBe("c:/tools/tsgo.exe");

    const withoutTsgo = resolveToolRoots("/repo", { readTypescriptVersion: () => "6.0.3" });
    expect(withoutTsgo.tsgoBin).toBeUndefined();
  });

  it("leaves tsserverVersion undefined when the reader finds no version", () => {
    const roots = resolveToolRoots("/repo", { readTypescriptVersion: () => undefined });
    expect(roots.tsserverVersion).toBeUndefined();
  });
});

describe("resolveToolRoots default tsdk mirrors the shipped extension", () => {
  it("defaults to the extension's bundled TypeScript and points at an EXISTING tsserver.js", () => {
    // Real resolution against this checkout: no injected version reader, no override.
    const roots = resolveToolRoots(REPO_ROOT);

    // The default tsdk is the bundled TS under the shipped extension package
    // (`packages/vue-vscode`) — exactly the lib `extension.ts` passes via `--tsdk`
    // (`join(extensionPath, "node_modules", "typescript", "lib")`).
    const bundled = joinCanonical(
      REPO_ROOT,
      "packages",
      "vue-vscode",
      "node_modules",
      "typescript",
      "lib",
    );
    expect(roots.tsserverTsdk).toBe(bundled);
    expect(roots.expectedTsserverJs).toBe(joinCanonical(bundled, "tsserver.js"));

    // The pinned tsserver.js EXISTS in this pnpm checkout, so C's strict existence
    // gate (verter_dx_baseline provider.rs) passes. This is the assertion that fails
    // against the old bare-repo-root default, whose typescript dir a pnpm workspace
    // never hoists to the root.
    expect(existsSync(roots.expectedTsserverJs)).toBe(true);

    // Negative: NOT the bare `<repoRoot>/node_modules/typescript/lib` default.
    const bareRoot = joinCanonical(REPO_ROOT, "node_modules", "typescript", "lib", "tsserver.js");
    expect(roots.expectedTsserverJs).not.toBe(bareRoot);

    // The version is read from the RESOLVED tsdk's real package.json — not injected
    // — so the test exercises real on-disk resolution end to end.
    const pkg = JSON.parse(readFileSync(joinCanonical(bundled, "..", "package.json"), "utf-8")) as {
      version?: unknown;
    };
    expect(typeof pkg.version).toBe("string");
    expect(roots.tsserverVersion).toBe(pkg.version);
  });

  it("still lets an explicit user tsdk override the bundled default", () => {
    const roots = resolveToolRoots(REPO_ROOT, { userTsdk: "/opt/ts/lib" });
    expect(roots.tsserverTsdk).toBe("/opt/ts/lib");
    expect(roots.expectedTsserverJs).toBe("/opt/ts/lib/tsserver.js");
    // Negative: the override is NOT redirected under the extension package.
    expect(roots.tsserverTsdk).not.toContain("/vue-vscode/");
  });
});

describe("readTypescriptVersionFromDisk", () => {
  it("reads the version from <tsdk>/../package.json", () => {
    const root = tmp();
    const lib = join(root, "node_modules", "typescript", "lib");
    mkdirSync(lib, { recursive: true });
    writeFileSync(
      join(root, "node_modules", "typescript", "package.json"),
      JSON.stringify({ name: "typescript", version: "6.0.3" }),
    );
    expect(readTypescriptVersionFromDisk(lib)).toBe("6.0.3");
  });

  it("returns undefined when the package.json is absent", () => {
    const root = tmp();
    expect(readTypescriptVersionFromDisk(join(root, "missing", "lib"))).toBeUndefined();
  });
});
