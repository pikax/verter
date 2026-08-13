// Self-test: FULL linking-surface failure detection (BF2 required exit:
// "import/export and exact-package linking").
//
// One discriminating test per linking category — static imports (named,
// default, namespace, side-effect), module-load failure, re-export sources
// (`export … from`, `export * from`), named re-exports, local
// undeclared-name exports, default-export existence, and exact-package
// identity (the resolved module must be the exact pinned package version,
// not merely "a module that resolves"). Negative controls use REAL scratch
// packages installed into a real node_modules tree — never mocks — and
// every planted failure is asserted to surface in its OWN category, so
// deleting any one category branch fails its test.

import { describe, expect, it, afterAll } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import {
  checkLinkValidity,
  checkParseValidity,
  cleanupLinkScratch,
  compareArtifacts,
} from "../src/compare.mjs";
import { parseModule } from "../src/normalize.mjs";
import { oracleLinkBaseDir } from "../src/oracle-install.mjs";

// Link validity for conformance artifacts resolves against the ISOLATED
// per-domain oracle installs realized from the committed locks — the same
// closure the compilers themselves load from — never the workspace store.
const VUE_BASE = oracleLinkBaseDir("vue");
const SVELTE_BASE = oracleLinkBaseDir("svelte");

// A real scratch node_modules tree holding the negative-control packages.
const SCRATCH_BASE = mkdtempSync(path.join(tmpdir(), "bf2-link-"));
function scratchPackage(name, files, manifest = {}) {
  const dir = path.join(SCRATCH_BASE, "node_modules", name);
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    path.join(dir, "package.json"),
    JSON.stringify({ name, version: "1.0.0", type: "module", main: "index.mjs", ...manifest }),
  );
  for (const [file, content] of Object.entries(files)) {
    writeFileSync(path.join(dir, file), content);
  }
}
scratchPackage("bf2-broken-pkg", { "index.mjs": 'throw new Error("bf2 load boom");\n' });
scratchPackage("bf2-no-default-pkg", { "index.mjs": "export const named = 1;\n" });
scratchPackage("bf2-with-default-pkg", {
  "index.mjs": "export default 42;\nexport const named = 1;\n",
});
// A package that RESOLVES as "vue" but is the WRONG version — the exact
// "a module resolves" vs "the exact pinned package resolves" distinction.
const WRONG_VUE_BASE = mkdtempSync(path.join(tmpdir(), "bf2-wrongvue-"));
{
  const dir = path.join(WRONG_VUE_BASE, "node_modules", "vue");
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    path.join(dir, "package.json"),
    JSON.stringify({ name: "vue", version: "3.5.0", type: "module", main: "index.mjs" }),
  );
  writeFileSync(path.join(dir, "index.mjs"), "export const ref = () => {};\n");
}

afterAll(() => {
  cleanupLinkScratch(SCRATCH_BASE);
  cleanupLinkScratch(WRONG_VUE_BASE);
  cleanupLinkScratch(VUE_BASE);
  cleanupLinkScratch(SVELTE_BASE);
  rmSync(SCRATCH_BASE, { recursive: true, force: true });
  rmSync(WRONG_VUE_BASE, { recursive: true, force: true });
});

describe("static import resolution", () => {
  it("flags an import specifier that does not resolve at all", async () => {
    const ast = parseModule(
      'import { thing } from "this-package-does-not-exist-bf2-selftest";\nexport default thing;',
    );
    const result = await checkLinkValidity(ast, VUE_BASE);
    expect(result.ok).toBe(false);
    expect(result.unresolved).toContain("this-package-does-not-exist-bf2-selftest");
  });

  it("accepts a named import that exists on the real pinned package", async () => {
    const ast = parseModule('import { ref } from "vue";\nexport default ref;');
    const result = await checkLinkValidity(ast, VUE_BASE);
    expect(result.ok).toBe(true);
    expect(result.resolved).toContain("vue");
    expect(result.missingExports).toEqual([]);
  });

  it("flags a named import whose module resolves but does NOT export that name", async () => {
    const ast = parseModule(
      'import { thisNamedExportDoesNotExistOnVueBf2Selftest } from "vue";\nexport default thisNamedExportDoesNotExistOnVueBf2Selftest;',
    );
    const result = await checkLinkValidity(ast, VUE_BASE);
    expect(result.ok).toBe(false);
    expect(result.resolved).toContain("vue");
    expect(result.missingExports).toEqual(["vue#thisNamedExportDoesNotExistOnVueBf2Selftest"]);
  });

  it("accepts a default import from a module WITH a default export", async () => {
    const ast = parseModule('import X from "bf2-with-default-pkg";\nexport default X;');
    const result = await checkLinkValidity(ast, SCRATCH_BASE);
    expect(result.missingDefaults).toEqual([]);
    expect(result.resolved).toContain("bf2-with-default-pkg");
  });

  it("flags a default import from a module WITHOUT a default export", async () => {
    const ast = parseModule('import X from "bf2-no-default-pkg";\nexport default X;');
    const result = await checkLinkValidity(ast, SCRATCH_BASE);
    expect(result.ok).toBe(false);
    expect(result.missingDefaults).toEqual(["bf2-no-default-pkg"]);
  });

  it("accepts a namespace import of a loadable module", async () => {
    const ast = parseModule('import * as ns from "vue";\nexport default ns;');
    const result = await checkLinkValidity(ast, VUE_BASE);
    expect(result.ok).toBe(true);
    expect(result.resolved).toContain("vue");
  });

  it("accepts a side-effect import of a loadable pinned module", async () => {
    const ast = parseModule('import "svelte/internal/disclose-version";\nexport default 1;');
    const result = await checkLinkValidity(ast, SVELTE_BASE);
    expect(result.ok).toBe(true);
    expect(result.resolved).toContain("svelte/internal/disclose-version");
  });
});

describe("module-load failure", () => {
  it("flags a module that resolves but THROWS while loading", async () => {
    const ast = parseModule('import * as ns from "bf2-broken-pkg";\nexport default ns;');
    const result = await checkLinkValidity(ast, SCRATCH_BASE);
    expect(result.ok).toBe(false);
    expect(result.unresolved).toEqual([]); // it resolves — the failure is a LOAD failure
    expect(result.loadFailures.some((f) => f.startsWith("bf2-broken-pkg:"))).toBe(true);
    expect(result.loadFailures.join(" ")).toContain("bf2 load boom");
  });
});

describe("export/re-export sources", () => {
  it("accepts a named re-export that exists on the source module", async () => {
    const ast = parseModule('export { ref } from "vue";');
    const result = await checkLinkValidity(ast, VUE_BASE);
    expect(result.ok).toBe(true);
    expect(result.resolved).toContain("vue");
  });

  it("flags a named re-export missing from the source module", async () => {
    const ast = parseModule('export { bf2NoSuchReExport } from "vue";');
    const result = await checkLinkValidity(ast, VUE_BASE);
    expect(result.ok).toBe(false);
    expect(result.missingExports).toEqual(["vue#bf2NoSuchReExport"]);
  });

  it("accepts `export * from` a loadable module and flags an unresolvable one", async () => {
    const ok = await checkLinkValidity(parseModule('export * from "vue";'), VUE_BASE);
    expect(ok.ok).toBe(true);
    const bad = await checkLinkValidity(
      parseModule('export * from "bf2-does-not-exist-star";'),
      VUE_BASE,
    );
    expect(bad.ok).toBe(false);
    expect(bad.unresolved).toContain("bf2-does-not-exist-star");
  });

  it("a local `export { name }` whose name is never declared is a module early error the PARSE oracle raises", () => {
    // acorn performs export-reference checking for sourceType:module, so
    // this category is caught upstream of link validation, by the parse
    // oracle itself — asserted here so the category's coverage is explicit,
    // not assumed.
    const result = checkParseValidity(
      "const a = 1;\nexport { a, bf2UndeclaredName };",
      "candidate",
    );
    expect(result.ok).toBe(false);
    expect(result.error).toContain("bf2UndeclaredName");
  });
});

describe("exact-package identity", () => {
  it("accepts the exact pinned vue version from the real install", async () => {
    const ast = parseModule('import { ref } from "vue";\nexport default ref;');
    const result = await checkLinkValidity(ast, VUE_BASE);
    expect(result.packageIdentityViolations).toEqual([]);
  });

  it("flags a resolvable module that is the WRONG package version (not just 'a module resolves')", async () => {
    const ast = parseModule('import { ref } from "vue";\nexport default ref;');
    const result = await checkLinkValidity(ast, WRONG_VUE_BASE);
    expect(result.ok).toBe(false);
    expect(result.resolved).toContain("vue"); // it resolves and loads fine…
    expect(result.packageIdentityViolations.some((v) => v.includes("vue@3.5.0"))).toBe(true); // …but it is not the exact pinned package
  });

  it("flags an import of a package outside the pinned closures", async () => {
    const ast = parseModule('import { named } from "bf2-with-default-pkg";\nexport default named;');
    const result = await checkLinkValidity(ast, SCRATCH_BASE);
    expect(result.ok).toBe(false);
    expect(result.unpinnedPackages).toEqual(["bf2-with-default-pkg"]);
  });
});

describe("comparator integration", () => {
  it("compareArtifacts fails a candidate whose named import is missing from a real, resolvable module", async () => {
    const golden = { code: 'import { ref } from "vue";\nexport default ref;', diagnostics: [] };
    const candidate = {
      code:
        'import { thisNamedExportDoesNotExistOnVueBf2Selftest } from "vue";\n' +
        "export default thisNamedExportDoesNotExistOnVueBf2Selftest;",
      diagnostics: [],
    };
    const report = await compareArtifacts(golden, candidate, { linkBaseDir: VUE_BASE });
    expect(report.verdict).toBe("fail");
    expect(report.link.ok).toBe(false);
    expect(report.link.missingExports).toContain("vue#thisNamedExportDoesNotExistOnVueBf2Selftest");
    expect(report.reasons.some((r) => r.includes("missing named exports"))).toBe(true);
  });
});
