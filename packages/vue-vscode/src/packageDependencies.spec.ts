import { execFileSync } from "node:child_process";
import {
  existsSync,
  lstatSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

// The packaging helper is JavaScript because it is executed directly by Node
// from package.mjs. TypeScript validates its public shape at this call site.
// @ts-expect-error -- stage-deps.mjs intentionally has no generated declaration file.
import {
  discoverWorkspacePackages,
  patchWorkspaceRanges,
  stageRuntimeDependencies,
} from "../stage-deps.mjs";

const extensionDir = path.resolve(import.meta.dirname, "..");
const workspaceRoot = path.resolve(extensionDir, "..", "..");
const scratchDirs: string[] = [];
// This row copies and validates a real npm dependency graph. The root test
// command runs every workspace package in parallel, so Vitest's 5-second unit
// default is not a stable integration bound under CI I/O contention. Retain a
// finite ceiling that still fails a wedged copy or npm subprocess.
const PACKAGE_GRAPH_INTEGRATION_TIMEOUT_MS = 20_000;

/**
 * Every directory named `name` anywhere in a staged `node_modules` tree,
 * relative to `nodeModulesDir`.
 *
 * Recursive by necessity: `stage-deps.mjs` nests each dependency under its
 * owner, so a package can appear at any depth. Scoped directories (`@verter/…`)
 * are descended into, as is each package's own nested `node_modules`.
 */
function findNestedPackage(nodeModulesDir: string, name: string): string[] {
  if (!existsSync(nodeModulesDir)) return [];
  const found: string[] = [];
  for (const entry of readdirSync(nodeModulesDir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const entryPath = path.join(nodeModulesDir, entry.name);
    if (entry.name.startsWith("@")) {
      for (const scoped of readdirSync(entryPath, { withFileTypes: true })) {
        if (!scoped.isDirectory()) continue;
        const scopedPath = path.join(entryPath, scoped.name);
        if (`${entry.name}/${scoped.name}` === name) found.push(scopedPath);
        found.push(...findNestedPackage(path.join(scopedPath, "node_modules"), name));
      }
      continue;
    }
    if (entry.name === name) found.push(entryPath);
    found.push(...findNestedPackage(path.join(entryPath, "node_modules"), name));
  }
  return found;
}

afterEach(() => {
  for (const dir of scratchDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("VSIX runtime dependency staging", () => {
  it(
    "materializes a complete npm-valid production tree without pnpm links",
    () => {
      const stageDir = mkdtempSync(path.join(tmpdir(), "verter-vsix-deps-"));
      scratchDirs.push(stageDir);

      const extensionManifest = JSON.parse(
        readFileSync(path.join(extensionDir, "package.json"), "utf8"),
      );
      // TypeScript is NOT a runtime dependency: the extension serves from the
      // workspace's own TypeScript and the LSP discovers installs itself, so no
      // `typescript` package may enter the production (VSIX) dependency graph.
      // It stays a devDependency for types and for the headless service specs.
      expect(extensionManifest.dependencies).not.toHaveProperty("typescript");
      expect(extensionManifest.devDependencies.typescript).toBe("^6.0.3");
      patchWorkspaceRanges(
        extensionManifest,
        extensionManifest.version,
        discoverWorkspacePackages(workspaceRoot),
      );
      delete extensionManifest.devDependencies;
      writeFileSync(
        path.join(stageDir, "package.json"),
        `${JSON.stringify(extensionManifest, null, 2)}\n`,
      );

      stageRuntimeDependencies({
        packageDir: extensionDir,
        workspaceRoot,
        destinationNodeModules: path.join(stageDir, "node_modules"),
        packageVersion: extensionManifest.version,
      });

      const pluginDir = path.join(stageDir, "node_modules", "@verter", "typescript-plugin");
      const pluginModules = path.join(pluginDir, "node_modules");
      const requiredPackages = [
        pluginDir,
        path.join(pluginModules, "@verter", "language-shared"),
        path.join(pluginModules, "@verter", "native"),
        path.join(pluginModules, "@verter", "svelte-jsx"),
      ];

      for (const packagePath of requiredPackages) {
        expect(lstatSync(packagePath).isSymbolicLink(), packagePath).toBe(false);
        expect(lstatSync(path.join(packagePath, "package.json")).isFile(), packagePath).toBe(true);
      }
      // Negative: the production tree must NOT stage a TypeScript compiler at
      // ANY level. Staging nests each dependency below its owner, so a runtime
      // `typescript` introduced under any descendant lands at an arbitrary
      // depth — checking only the two known locations would miss it, and the
      // esbuild bundle guard cannot see it either (the plugin is externalized).
      // So walk the whole staged tree.
      const stagedTypeScriptDirs = findNestedPackage(
        path.join(stageDir, "node_modules"),
        "typescript",
      );
      expect(stagedTypeScriptDirs, "no TypeScript compiler anywhere in the staged tree").toEqual(
        [],
      );
      expect(existsSync(path.join(pluginModules, "typescript"))).toBe(false);
      expect(existsSync(path.join(pluginModules, "svelte"))).toBe(false);
      expect(
        existsSync(path.join(pluginModules, "@verter", "svelte-jsx", "jsx-runtime.d.ts")),
      ).toBe(true);

      const stagedPluginManifest = JSON.parse(
        readFileSync(path.join(pluginDir, "package.json"), "utf8"),
      );
      expect(Object.values(stagedPluginManifest.dependencies)).not.toContainEqual(
        expect.stringMatching(/^workspace:/),
      );

      const npm =
        process.platform === "win32"
          ? {
              file: process.env.ComSpec ?? "cmd.exe",
              args: ["/d", "/s", "/c", "npm list --omit=dev --depth=99999 --loglevel=error"],
            }
          : {
              file: "npm",
              args: ["list", "--omit=dev", "--depth=99999", "--loglevel=error"],
            };
      expect(() =>
        execFileSync(npm.file, npm.args, { cwd: stageDir, stdio: "pipe" }),
      ).not.toThrow();
    },
    PACKAGE_GRAPH_INTEGRATION_TIMEOUT_MS,
  );
});
