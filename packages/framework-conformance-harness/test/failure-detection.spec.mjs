// Self-test: parse/link/runtime failure detection (BF2 required exit).

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { compareArtifacts, checkParseValidity, checkLinkValidity } from "../src/compare.mjs";
import { parseModule } from "../src/normalize.mjs";
import { executeVueSsr, cleanupScratch } from "../src/execute-vue-runtime.mjs";
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
});
