// Self-test: parse/link/runtime failure detection (BF2 required exit).

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { compileSvelteFixture } from "../src/invoke-svelte-oracle.mjs";
import { compareArtifacts, checkParseValidity, checkLinkValidity } from "../src/compare.mjs";
import { parseModule } from "../src/normalize.mjs";
import { executeVueSsr, cleanupScratch } from "../src/execute-vue-runtime.mjs";
import {
  executeSvelteSsr,
  cleanupScratch as cleanupSvelteScratch,
} from "../src/execute-svelte-runtime.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";

describe("parse failure detection", () => {
  it("flags syntactically broken candidate code", () => {
    const result = checkParseValidity("const x = {{{ this is not js", "candidate");
    expect(result.ok).toBe(false);
    expect(result.error).toBeTruthy();
  });

  it("compareArtifacts reports a parse failure without computing structural equality", () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/vue/basic-interpolation.vue"),
      "utf8",
    );
    const golden = compileVueFixture(source, "fixtures/vue/basic-interpolation.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    const brokenCandidate = { code: "export default function( {", diagnostics: [] };
    const report = compareArtifacts(golden, brokenCandidate, { linkBaseDir: HARNESS_ROOT });
    expect(report.verdict).toBe("fail");
    expect(report.candidateParse.ok).toBe(false);
    expect(report.structural).toBeNull();
  });
});

describe("link (import resolution) failure detection", () => {
  it("flags an import specifier that does not resolve against the real packages", () => {
    const ast = parseModule(
      'import { thing } from "this-package-does-not-exist-bf2-selftest";\nexport default thing;',
    );
    const result = checkLinkValidity(ast, HARNESS_ROOT);
    expect(result.ok).toBe(false);
    expect(result.unresolved).toContain("this-package-does-not-exist-bf2-selftest");
  });

  it("accepts an import that resolves against the real pinned packages", () => {
    const ast = parseModule('import { ref } from "vue";\nexport default ref;');
    const result = checkLinkValidity(ast, HARNESS_ROOT);
    expect(result.ok).toBe(true);
    expect(result.resolved).toContain("vue");
    expect(result.missingExports).toEqual([]);
  });

  it("flags a named import whose module resolves but does NOT export that name (require.resolve() alone would pass this)", () => {
    // "vue" genuinely resolves as a module, but never exports a binding
    // named this — this is exactly the class require.resolve() cannot
    // catch on its own.
    const ast = parseModule(
      'import { thisNamedExportDoesNotExistOnVueBf2Selftest } from "vue";\nexport default thisNamedExportDoesNotExistOnVueBf2Selftest;',
    );
    const result = checkLinkValidity(ast, HARNESS_ROOT);
    expect(result.ok).toBe(false);
    expect(result.resolved).toContain("vue");
    expect(result.missingExports).toEqual(["vue#thisNamedExportDoesNotExistOnVueBf2Selftest"]);
  });

  it("compareArtifacts fails a candidate whose named import is missing from a real, resolvable module", () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/vue/basic-interpolation.vue"),
      "utf8",
    );
    const golden = compileVueFixture(source, "fixtures/vue/basic-interpolation.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    const brokenCandidate = {
      code:
        'import { thisNamedExportDoesNotExistOnVueBf2Selftest } from "vue";\n' +
        "export default thisNamedExportDoesNotExistOnVueBf2Selftest;",
      diagnostics: [],
    };
    const report = compareArtifacts(golden, brokenCandidate, { linkBaseDir: HARNESS_ROOT });
    expect(report.verdict).toBe("fail");
    expect(report.link.ok).toBe(false);
    expect(report.link.missingExports).toContain("vue#thisNamedExportDoesNotExistOnVueBf2Selftest");
    expect(report.reasons.some((r) => r.includes("missing named exports"))).toBe(true);
  });
});

describe("runtime failure detection", () => {
  it("flags code that throws when executed against the official runtime", async () => {
    const result = await executeVueSsr('export default { render() { throw new Error("boom"); } }');
    expect(result.ok).toBe(false);
    expect(result.error).toContain("boom");
  });

  it("succeeds for real, correct compiled SSR output", async () => {
    const source = readFileSync(path.join(HARNESS_ROOT, "fixtures/vue/slots.vue"), "utf8");
    const ssr = compileVueFixture(source, "fixtures/vue/slots.vue", {
      backend: "ssr",
      sourceMap: false,
      isProd: false,
    });
    const result = await executeVueSsr(ssr.code);
    expect(result.ok).toBe(true);
    expect(result.html).toContain("panel");
    cleanupScratch();
  });

  it("Svelte: flags code that throws when executed against the official server runtime", async () => {
    const result = await executeSvelteSsr(
      'export default function() { throw new Error("svelte boom"); }',
    );
    expect(result.ok).toBe(false);
    expect(result.error).toContain("svelte boom");
    cleanupSvelteScratch();
  });

  it("Svelte: succeeds for real, correct compiled server output", async () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/svelte/legacy-slots.svelte"),
      "utf8",
    );
    const server = compileSvelteFixture(source, "fixtures/svelte/legacy-slots.svelte", {
      generate: "server",
      runes: false,
      dev: false,
      sourceMap: false,
    });
    expect(server.diagnostics.filter((d) => d.kind === "compile-error")).toEqual([]);
    const result = await executeSvelteSsr(server.code, { title: "Hello BF2" });
    expect(result.ok).toBe(true);
    expect(result.html).toContain("panel");
    expect(result.html).toContain("Hello BF2");
    cleanupSvelteScratch();
  });
});
