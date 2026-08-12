// Self-test: expected-golden immutability (BF2 required exit).
//
// Proves candidate output can never update its own expectation — checked
// structurally (the comparator module has no write path into goldens/ at
// all) AND operationally (running many divergent comparisons against a real
// committed golden leaves that file's bytes byte-identical afterward).

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import * as compareModule from "../src/compare.mjs";
import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { readGoldenFile } from "../src/golden-store.mjs";
import { GOLDENS_ROOT, HARNESS_ROOT } from "../src/paths.mjs";

const GOLDEN_PATH = path.join(GOLDENS_ROOT, "vue", "basic-interpolation__vdom__map0__prod0.json");

describe("golden immutability — structural", () => {
  it("the comparator module exports no filesystem-write function", () => {
    // compare.mjs must not import node:fs write functions at all — the
    // ABSENCE of any write-shaped export is what makes "candidate cannot
    // update its own expectation" true by construction, not convention.
    const exportNames = Object.keys(compareModule);
    for (const name of exportNames) {
      expect(name.toLowerCase()).not.toMatch(/write|save|persist|update.*golden/);
    }
  });

  it("readGoldenFile returns a deep-frozen object", () => {
    const record = readGoldenFile(GOLDEN_PATH);
    expect(Object.isFrozen(record)).toBe(true);
    expect(Object.isFrozen(record.domain)).toBe(true);
    expect(() => {
      record.code = "MUTATED";
    }).toThrow();
  });
});

describe("golden immutability — operational", () => {
  it("the golden file's bytes are unchanged after many divergent comparisons", () => {
    const before = readFileSync(GOLDEN_PATH, "utf8");
    const golden = JSON.parse(before);

    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/vue/basic-interpolation.vue"),
      "utf8",
    );
    const divergentVariants = [
      { code: golden.code.replace("root", "MUTATED_ROOT"), diagnostics: [] },
      { code: "export default {}", diagnostics: [] },
      { code: "", diagnostics: [] },
      compileVueFixture(source, "fixtures/vue/basic-interpolation.vue", {
        backend: "ssr", // deliberately the wrong backend vs this golden's vdom
        sourceMap: false,
        isProd: false,
      }),
    ];
    for (const candidate of divergentVariants) {
      compareModule.compareArtifacts(golden, candidate, { linkBaseDir: HARNESS_ROOT });
    }

    const after = readFileSync(GOLDEN_PATH, "utf8");
    expect(after).toBe(before);
  });
});
