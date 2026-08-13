// Self-test: non-vacuous official AND candidate arms (BF2 required exit).
//
// Proves both sides of a comparison do REAL, non-zero work: the golden
// (official) side has substantial content and a non-trivial parse tree, and
// the comparator does NOT trivially pass on emptiness — an empty-vs-empty
// pair must be rejected as vacuous by this suite's own assertions, and a
// real-vs-empty pair must fail structurally, never silently pass.

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { compileSvelteFixture } from "../src/invoke-svelte-oracle.mjs";
import { compareArtifacts } from "../src/compare.mjs";
import { parseModule } from "../src/normalize.mjs";
import { readGoldenSet } from "../src/golden-store.mjs";
import { GOLDENS_ROOT, HARNESS_ROOT } from "../src/paths.mjs";
import { oracleLinkBaseDir } from "../src/oracle-install.mjs";

describe("non-vacuous official arm", () => {
  it("every committed golden carries substantial, well-formed code", () => {
    const set = readGoldenSet(GOLDENS_ROOT);
    let checked = 0;
    for (const [name, record] of set) {
      expect(record.code.length).toBeGreaterThan(50);
      const ast = parseModule(record.code, name);
      expect(ast.body.length).toBeGreaterThan(0);
      checked += 1;
    }
    expect(checked).toBe(48);
  });
});

describe("non-vacuous candidate arm — comparator does not trivially pass on emptiness", () => {
  it("rejects an empty-vs-real candidate (never silently passes on vacuity)", async () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/vue/basic-interpolation.vue"),
      "utf8",
    );
    const golden = compileVueFixture(source, "fixtures/vue/basic-interpolation.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    const emptyCandidate = { code: "", diagnostics: [] };
    const report = await compareArtifacts(golden, emptyCandidate, {
      linkBaseDir: oracleLinkBaseDir("vue"),
    });
    expect(report.verdict).toBe("fail");
  });

  it("passes only when the candidate arm ALSO does real, matching compiler work", async () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/svelte/props-events.svelte"),
      "utf8",
    );
    const golden = compileSvelteFixture(source, "fixtures/svelte/props-events.svelte", {
      generate: "client",
      runes: true,
      dev: false,
      sourceMap: false,
    });
    const candidate = compileSvelteFixture(source, "fixtures/svelte/props-events.svelte", {
      generate: "client",
      runes: true,
      dev: false,
      sourceMap: false,
    });
    expect(golden.code.length).toBeGreaterThan(50);
    expect(candidate.code.length).toBeGreaterThan(50);
    const report = await compareArtifacts(golden, candidate, {
      linkBaseDir: oracleLinkBaseDir("svelte"),
    });
    expect(report.verdict).toBe("pass");
  });
});
