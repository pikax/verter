// The EXTENSION type provider must preserve TypeScript's PER-IMPORT module
// resolution mode.
//
// Under `moduleResolution: "nodenext"` the condition set a specifier resolves
// against is decided per import site, not per project: an `import` in an ESM
// file resolves through the `import` condition, the same specifier in a CommonJS
// file resolves through `require`. A resolution override that answers every
// specifier with one mode picks the wrong half of a package's conditional
// exports — the user gets the CJS types (or a spurious error) on a correctly
// written ESM import.
//
// These tests assert on the RESOLVED TYPE of a dual-published package whose two
// halves declare distinguishable literal types, so a wrong condition is visible
// as a wrong type rather than as a missing diagnostic.

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { ExtensionTsService } from "./extensionTsService.js";
import { materializeWorkspaceTypeScript } from "./extensionTsService.testUtils.js";

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

/**
 * A `"type": "module"` project configured for NodeNext resolution — the layout
 * where per-import mode is load-bearing.
 */
function makeNodeNextWorkspace(): string {
  const root = mkdtempSync(join(tmpdir(), "ext-ts-nodenext-"));
  tmps.push(root);
  writeFileSync(
    join(root, "package.json"),
    JSON.stringify({ name: "fixture", private: true, type: "module" }),
  );
  writeFileSync(
    join(root, "tsconfig.json"),
    JSON.stringify({
      compilerOptions: {
        module: "nodenext",
        moduleResolution: "nodenext",
        target: "esnext",
        strict: true,
      },
      include: ["*.ts", "*.cts"],
    }),
  );
  materializeWorkspaceTypeScript(root);
  return root;
}

/**
 * A dual-published dependency whose `import` and `require` conditions declare
 * DIFFERENT types. Which half answers is the observable under test.
 */
function materializeDualPublishedPackage(root: string): void {
  const pkgDir = join(root, "node_modules", "dual-pkg");
  mkdirSync(pkgDir, { recursive: true });
  writeFileSync(
    join(pkgDir, "package.json"),
    JSON.stringify({
      name: "dual-pkg",
      version: "0.0.0-fixture",
      type: "module",
      exports: {
        ".": {
          import: { types: "./index.d.mts", default: "./index.mjs" },
          require: { types: "./index.d.cts", default: "./index.cjs" },
        },
      },
    }),
  );
  writeFileSync(join(pkgDir, "index.d.mts"), 'export declare const flavour: "esm";\n');
  writeFileSync(join(pkgDir, "index.d.cts"), 'export declare const flavour: "cjs";\n');
  writeFileSync(join(pkgDir, "index.mjs"), "export const flavour = 'esm';\n");
  writeFileSync(join(pkgDir, "index.cjs"), "exports.flavour = 'cjs';\n");
}

/** 1-based `{ line, offset }` of `needle`'s first occurrence, as tsserver counts. */
function positionOf(source: string, needle: string): { line: number; offset: number } {
  const index = source.indexOf(needle);
  if (index < 0) throw new Error(`fixture does not contain ${needle}`);
  const before = source.slice(0, index);
  return {
    line: before.split("\n").length,
    offset: index - (before.lastIndexOf("\n") + 1) + 1,
  };
}

const PROBE_SOURCE = [
  'import { flavour } from "dual-pkg";',
  "",
  "export const probe = flavour;",
  "",
].join("\n");

function probeFlavour(root: string, fileName: string): string {
  const filePath = join(root, fileName);
  writeFileSync(filePath, PROBE_SOURCE);

  const unavailable: string[] = [];
  const svc = new ExtensionTsService(root, (message) => unavailable.push(message));
  svc.handleQuery("open", { file: filePath, fileContent: PROBE_SOURCE });
  const info = svc.handleQuery("quickinfo", {
    file: filePath,
    ...positionOf(PROBE_SOURCE, "probe = flavour"),
  }) as { displayString: string } | undefined;
  expect(
    unavailable,
    "the fixture workspace has a real TypeScript; nothing may fail closed",
  ).toEqual([]);
  expect(info, "quickinfo must answer for the probe binding").toBeDefined();
  return info!.displayString;
}

describe("ExtensionTsService — per-import NodeNext resolution mode", () => {
  // The defect: the resolution override dropped `resolveModuleName`'s
  // resolution-mode argument, so TypeScript fell back to the `require`
  // condition and an ESM import resolved to the package's CommonJS half.
  it("resolves an ESM import through the `import` condition", () => {
    const root = makeNodeNextWorkspace();
    materializeDualPublishedPackage(root);

    // `entry.ts` inside a `"type": "module"` package IS an ES module, so the
    // import site's mode is ESNext.
    const display = probeFlavour(root, "entry.ts");

    expect(display).toContain('"esm"');
    expect(display, "the `require` condition answered an ESM import").not.toContain('"cjs"');
  });

  // The complement, and the reason the fix must forward the REAL per-site mode
  // rather than hardcode ESM: the same specifier in a CommonJS file must still
  // resolve through `require`.
  it("resolves a CommonJS import through the `require` condition", () => {
    const root = makeNodeNextWorkspace();
    materializeDualPublishedPackage(root);

    // `.cts` is CommonJS regardless of the package's `type` field.
    const display = probeFlavour(root, "legacy.cts");

    expect(display).toContain('"cjs"');
    expect(display, "the `import` condition answered a CommonJS import").not.toContain('"esm"');
  });
});
