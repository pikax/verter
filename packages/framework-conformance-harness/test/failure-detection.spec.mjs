// Self-test: parse/runtime failure detection (BF2 required exit). The full
// linking-surface failure detection lives in test/link-surface.spec.mjs.

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { compileSvelteFixture } from "../src/invoke-svelte-oracle.mjs";
import { compareArtifacts, checkParseValidity } from "../src/compare.mjs";
import { executeVueSsr, cleanupScratch } from "../src/execute-vue-runtime.mjs";
import {
  executeSvelteSsr,
  cleanupScratch as cleanupSvelteScratch,
} from "../src/execute-svelte-runtime.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";
import { oracleLinkBaseDir } from "../src/oracle-install.mjs";

describe("parse failure detection", () => {
  it("flags syntactically broken candidate code", () => {
    const result = checkParseValidity("const x = {{{ this is not js", "candidate");
    expect(result.ok).toBe(false);
    expect(result.error).toBeTruthy();
  });

  it("compareArtifacts reports a parse failure without computing structural equality", async () => {
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
    const report = await compareArtifacts(golden, brokenCandidate, {
      linkBaseDir: oracleLinkBaseDir("vue"),
    });
    expect(report.verdict).toBe("fail");
    expect(report.candidateParse.ok).toBe(false);
    expect(report.structural).toBeNull();
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
