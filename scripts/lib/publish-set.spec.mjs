#!/usr/bin/env node

// Tests for the derived npm publish set. Run: node --test scripts/lib/publish-set.spec.mjs

import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, resolve } from "node:path";
import test from "node:test";

import {
  MARKETPLACE_ONLY,
  PRODUCT_ROOTS,
  computePublishSet,
  scanWorkspacePackages,
} from "./publish-set.mjs";

const ROOT = resolve(import.meta.dirname, "../..");

const EXPECTED_NPM = [
  "@verter/component-meta",
  "@verter/language-shared",
  "@verter/native",
  "@verter/nuxt",
  "@verter/proto",
  "@verter/svelte-jsx",
  "@verter/type-ir",
  "@verter/typeinfo",
  "@verter/types",
  "@verter/typescript-plugin",
  "@verter/unplugin",
  "@verter/wasm",
  "verter-lsp",
  "verter-tsc",
];

test("derived npm set equals the expected product closure minus marketplace-only", () => {
  const set = computePublishSet();
  assert.deepEqual([...set.npm].sort(), EXPECTED_NPM);
  assert.deepEqual(PRODUCT_ROOTS, [
    "@verter/typeinfo",
    "@verter/component-meta",
    "@verter/unplugin",
    "@verter/nuxt",
    "verter-lsp",
    "verter-tsc",
    "verter-vscode",
  ]);
  assert.deepEqual(MARKETPLACE_ONLY, ["verter-vscode"]);
});

// The binary families ship their servers/bindings as per-platform packages the
// launcher resolves at runtime. A family whose npm/<platform> dirs are missing
// from the platform set installs a launcher with no binary behind it.
test("every binary family's platform dirs are in the platform publish set", () => {
  const set = computePublishSet();
  const dirs = set.platform.map((dir) => dir.split("\\").join("/"));
  for (const family of ["native", "verter-lsp", "verter-tsc"]) {
    const familyDirs = dirs.filter((dir) => dir.startsWith(`packages/${family}/npm/`));
    assert.equal(
      familyDirs.length,
      7,
      `expected 7 platform dirs for ${family}, got ${familyDirs.length}: ${familyDirs.join(", ")}`,
    );
  }
});

// @verter/oxc-bindings is published on npm from an earlier release but is
// deliberately NOT a product root: it is an internal binding package, not part
// of the shipped surface, so it stops being republished from here on.
test("oxc-bindings is not published", () => {
  const set = computePublishSet();
  assert.ok(!set.npm.includes("@verter/oxc-bindings"));
  assert.ok(!set.order.includes("@verter/oxc-bindings"));
});

test("every runtime workspace dependency of every published package is itself published", () => {
  const set = computePublishSet();
  const workspace = scanWorkspacePackages(join(ROOT, "packages"));
  const published = new Set(set.npm);
  for (const platformPath of set.platform) {
    for (const [name, entry] of workspace) {
      if (relative(ROOT, entry.dir) === platformPath) published.add(name);
    }
  }

  for (const name of set.npm) {
    const entry = workspace.get(name);
    for (const field of ["dependencies", "optionalDependencies", "peerDependencies"]) {
      for (const depName of Object.keys(entry.pkg[field] ?? {})) {
        if (!workspace.has(depName)) continue; // external dep — npm resolves it
        assert.ok(
          published.has(depName),
          `${name} has runtime workspace dep ${depName} which is not in the publish set`,
        );
      }
    }
  }
});

test("verter-vscode is not in the npm set", () => {
  const set = computePublishSet();
  assert.ok(!set.npm.includes("verter-vscode"));
  assert.ok(!set.order.includes("verter-vscode"));
  assert.ok(set.marketplaceOnly.includes("verter-vscode"));
});

test("a private package in the closure throws", () => {
  const fixture = mkdtempSync(join(tmpdir(), "publish-set-"));
  const writePkg = (dir, pkg) => {
    mkdirSync(join(fixture, "packages", dir), { recursive: true });
    writeFileSync(join(fixture, "packages", dir, "package.json"), JSON.stringify(pkg));
  };
  writePkg("root", {
    name: "@test/root",
    version: "1.0.0",
    dependencies: { "@test/dep": "workspace:*" },
  });
  writePkg("dep", { name: "@test/dep", version: "1.0.0", private: true });

  assert.throws(
    () => computePublishSet({ rootDir: fixture, roots: ["@test/root"] }),
    /@test\/dep.*private/s,
  );
});
