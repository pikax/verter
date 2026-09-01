/**
 * @ai-generated - Verifies the owned declaration-only fixture dependency materializer.
 * @vitest-environment node
 */
import { existsSync, lstatSync, readFileSync, readdirSync, realpathSync, statSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "typescript";
import { afterEach, describe, expect, it } from "vitest";

import {
  stageEnduranceFixtureDependencies,
  type EnduranceFixtureFramework,
} from "../src/endurance/fixtureDependencies.js";
import { disposeWorkspace, materializeWorkspace } from "../src/endurance/workspace.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "..", "..", "..");
const SOURCE_SVELTE = realpathSync(
  path.join(REPO_ROOT, "packages", "svelte-jsx", "node_modules", "svelte"),
);
const SVELTE_DECLARATION_CLOSURE = [
  "svelte",
  "esrap",
  "clsx",
  "magic-string",
  "@types/estree",
  "locate-character",
  "@jridgewell/sourcemap-codec",
] as const;

const workspaces: string[] = [];
afterEach(() => {
  for (const workspace of workspaces.splice(0)) disposeWorkspace(workspace);
});

function workspace(): string {
  const root = materializeWorkspace({
    "src/probe.ts": [
      'import type { Component, Snippet } from "svelte";',
      'import type { HTMLAttributes } from "svelte/elements";',
      "export type Surface = [Component, Snippet, HTMLAttributes<HTMLElement>];",
      "",
    ].join("\n"),
    "tsconfig.json": JSON.stringify({
      compilerOptions: {
        target: "ES2022",
        module: "ESNext",
        moduleResolution: "Bundler",
        lib: ["ES2022", "DOM", "DOM.Iterable"],
        strict: true,
        skipLibCheck: false,
        types: [],
      },
      include: ["src/**/*.ts"],
    }),
  });
  workspaces.push(root);
  return root;
}

function declarationEntries(root: string): string[] {
  const entries: string[] = [];
  const visit = (directory: string) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      expect(lstatSync(absolute).isSymbolicLink(), absolute).toBe(false);
      if (entry.isDirectory()) visit(absolute);
      else entries.push(absolute);
    }
  };
  visit(root);
  return entries;
}

function resolveFromOwnedRoot(root: string, specifier: string): string | undefined {
  const withinRoot = (candidate: string) => {
    const relative = path.relative(root, path.resolve(candidate));
    return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
  };
  const host: ts.ModuleResolutionHost = {
    fileExists: (candidate) => withinRoot(candidate) && ts.sys.fileExists(candidate),
    readFile: (candidate) => (withinRoot(candidate) ? ts.sys.readFile(candidate) : undefined),
    directoryExists: (candidate) =>
      withinRoot(candidate) && (ts.sys.directoryExists?.(candidate) ?? false),
    getDirectories: (candidate) =>
      withinRoot(candidate) ? (ts.sys.getDirectories?.(candidate) ?? []) : [],
    realpath: (candidate) => candidate,
    getCurrentDirectory: () => root,
  };
  return ts.resolveModuleName(
    specifier,
    path.join(root, "src", "probe.ts"),
    {
      module: ts.ModuleKind.ESNext,
      moduleResolution: ts.ModuleResolutionKind.Bundler,
      target: ts.ScriptTarget.ES2022,
    },
    host,
  ).resolvedModule?.resolvedFileName;
}

describe("endurance fixture dependencies", () => {
  /**
   * A synthetic Svelte fixture must resolve from bytes it owns, not from the
   * checkout above it. The restricted resolution host makes ancestor lookup
   * impossible; byte equality and recursive link checks prevent regressions to
   * ambient symlinks or a partial hand-written shim.
   */
  it("copies the installed Svelte declaration closure into an owned Svelte fixture", () => {
    const root = workspace();

    expect(resolveFromOwnedRoot(root, "svelte")).toBeUndefined();
    expect(resolveFromOwnedRoot(root, "svelte/elements")).toBeUndefined();

    stageEnduranceFixtureDependencies(root, "svelte");

    const destination = path.join(root, "node_modules", "svelte");
    expect(path.normalize(resolveFromOwnedRoot(root, "svelte")!)).toBe(
      path.join(destination, "types", "index.d.ts"),
    );
    expect(path.normalize(resolveFromOwnedRoot(root, "svelte/elements")!)).toBe(
      path.join(destination, "elements.d.ts"),
    );
    expect(readFileSync(path.join(destination, "package.json"))).toEqual(
      readFileSync(path.join(SOURCE_SVELTE, "package.json")),
    );

    const entries = declarationEntries(destination);
    expect(entries.length).toBeGreaterThan(2);
    expect(
      entries.every((entry) => {
        const relative = path.relative(destination, entry).replaceAll("\\", "/");
        return relative === "package.json" || /\.d\.(?:ts|mts|cts)$/.test(relative);
      }),
    ).toBe(true);
    expect(existsSync(path.join(destination, "node_modules"))).toBe(false);
    expect(entries.some((entry) => /\.(?:c|m)?js$/.test(entry))).toBe(false);

    for (const packageName of SVELTE_DECLARATION_CLOSURE) {
      const packageRoot = path.join(root, "node_modules", ...packageName.split("/"));
      expect(existsSync(path.join(packageRoot, "package.json")), packageName).toBe(true);
      const packageEntries = declarationEntries(packageRoot);
      expect(
        packageEntries.every((entry) => {
          const relative = path.relative(packageRoot, entry).replaceAll("\\", "/");
          return relative === "package.json" || /\.d\.(?:ts|mts|cts)$/.test(relative);
        }),
        packageName,
      ).toBe(true);
    }

    const tsc = path.join(REPO_ROOT, "node_modules", "typescript", "bin", "tsc");
    const compile = spawnSync(
      process.execPath,
      [tsc, "--noEmit", "--pretty", "false", "--project", root],
      {
        cwd: root,
        encoding: "utf8",
        stdio: "pipe",
      },
    );
    expect(compile.status, `${compile.stdout}\n${compile.stderr}`).toBe(0);
  });

  it.each<readonly [EnduranceFixtureFramework]>([["vue"]])(
    "stages no framework package for a %s fixture",
    (framework) => {
      const root = workspace();

      stageEnduranceFixtureDependencies(root, framework);

      expect(existsSync(path.join(root, "node_modules"))).toBe(false);
    },
  );

  it("refuses to stage into a root the endurance harness does not own", () => {
    expect(() => stageEnduranceFixtureDependencies(REPO_ROOT, "svelte")).toThrow(
      /owned synthetic endurance root/,
    );
    expect(statSync(REPO_ROOT).isDirectory()).toBe(true);
  });
});
