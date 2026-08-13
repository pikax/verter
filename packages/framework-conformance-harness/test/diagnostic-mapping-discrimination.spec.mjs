// Self-test: diagnostic/mapping discrimination (BF2 required exit).
//
// EVERY contract-observable diagnostic field discriminates INDEPENDENTLY:
// category/kind, code, the FULL message chain, source/file identity, start
// AND end spans, related information, and order/count. Likewise every
// contractual source-map field, including sourcesContent and sourceRoot. A
// diagnostic or map that matches on every field but one must be caught —
// each case below differs from its baseline in EXACTLY one field, so a
// comparison that ignored that field would falsely pass and fail the test.

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import { compareDiagnostics, compareMappings, CONTRACTUAL_MAP_FIELDS } from "../src/compare.mjs";
import { compileSvelteFixture } from "../src/invoke-svelte-oracle.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";

/** A fully-populated baseline diagnostic; each test perturbs ONE field. */
function baseline() {
  return {
    kind: "warning",
    code: "a11y_x",
    message: ["outer message", "nested detail"],
    source: "fixtures/vue/basic-interpolation.vue",
    start: { line: 3, column: 5 },
    end: { line: 3, column: 12 },
    related: [
      {
        message: "related here",
        source: "fixtures/vue/basic-interpolation.vue",
        start: { line: 1, column: 1 },
        end: { line: 1, column: 4 },
      },
    ],
  };
}

describe("diagnostic discrimination — every field independently", () => {
  it("treats identical fully-populated diagnostics as equal", () => {
    const result = compareDiagnostics([baseline()], [baseline()]);
    expect(result.equal).toBe(true);
    expect(result.firstMismatch).toBeNull();
  });

  const singleFieldCases = [
    ["kind", (d) => ({ ...d, kind: "error" })],
    ["code", (d) => ({ ...d, code: "a11y_y" })],
    ["message (head of chain)", (d) => ({ ...d, message: ["OTHER message", "nested detail"] })],
    ["message (nested chain link)", (d) => ({ ...d, message: ["outer message", "OTHER detail"] })],
    [
      "message (chain truncated — presence, not just head)",
      (d) => ({ ...d, message: ["outer message"] }),
    ],
    ["source", (d) => ({ ...d, source: "fixtures/vue/props-emit.vue" })],
    ["start.line", (d) => ({ ...d, start: { line: 4, column: 5 } })],
    ["start.column", (d) => ({ ...d, start: { line: 3, column: 6 } })],
    ["end.line", (d) => ({ ...d, end: { line: 4, column: 12 } })],
    ["end.column", (d) => ({ ...d, end: { line: 3, column: 13 } })],
    ["end presence", (d) => ({ ...d, end: null })],
    [
      "related (message)",
      (d) => ({ ...d, related: [{ ...d.related[0], message: "OTHER related" }] }),
    ],
    [
      "related (span)",
      (d) => ({ ...d, related: [{ ...d.related[0], start: { line: 2, column: 1 } }] }),
    ],
    ["related (presence)", (d) => ({ ...d, related: [] })],
  ];

  for (const [field, mutate] of singleFieldCases) {
    it(`distinguishes diagnostics differing ONLY by ${field}`, () => {
      const golden = baseline();
      const candidate = mutate(baseline());
      // Proof the perturbation applied and is confined to one field.
      expect(JSON.stringify(candidate)).not.toBe(JSON.stringify(golden));
      const result = compareDiagnostics([golden], [candidate]);
      expect(result.equal).toBe(false);
      expect(result.firstMismatch).not.toBeNull();
    });
  }

  it("distinguishes diagnostic sequences by count", () => {
    const result = compareDiagnostics([baseline()], [baseline(), baseline()]);
    expect(result.equal).toBe(false);
    expect(result.firstMismatch.fields).toEqual(["count"]);
  });

  it("distinguishes diagnostic sequences by order", () => {
    const a = baseline();
    const b = { ...baseline(), code: "other_code" };
    const result = compareDiagnostics([a, b], [b, a]);
    expect(result.equal).toBe(false);
  });

  it("real official Svelte warnings carry code, message, start AND end spans", () => {
    // A fixture Svelte itself warns about (a11y): proves the producer side
    // captures the full field set the comparator discriminates on.
    const warned = compileSvelteFixture(
      '<script>let x = 1;</script>\n<img src="x.png" />\n',
      "selftest-warned.svelte",
      { generate: "client", runes: true, dev: false, sourceMap: false },
    );
    const warnings = warned.diagnostics.filter((d) => d.kind === "warning");
    expect(warnings.length).toBeGreaterThan(0);
    for (const warning of warnings) {
      expect(warning.code).toBeTruthy();
      expect(warning.message).toBeTruthy();
      expect(warning.source).toBe("selftest-warned.svelte");
      expect(warning.start).not.toBeNull();
      expect(warning.end).not.toBeNull();
    }
  });
});

describe("mapping discrimination — every contractual field independently", () => {
  /** Fully-populated baseline map; each case perturbs ONE contractual field. */
  function baselineMap() {
    return {
      version: 3,
      mappings: "AAAA,CAAC",
      sources: ["x.vue"],
      sourceRoot: "src/",
      sourcesContent: ["<template>original</template>"],
      names: ["count"],
      file: "out.js",
    };
  }

  it("classifies exactly the contractual field set (sourcesContent and sourceRoot included)", () => {
    expect(CONTRACTUAL_MAP_FIELDS).toEqual([
      "version",
      "mappings",
      "sources",
      "sourceRoot",
      "sourcesContent",
      "names",
    ]);
  });

  it("treats identical fully-populated maps as equal", () => {
    expect(compareMappings(baselineMap(), baselineMap()).equal).toBe(true);
  });

  const mapCases = [
    ["version", (m) => ({ ...m, version: 4 })],
    ["mappings", (m) => ({ ...m, mappings: "AAAA,CAAD" })],
    ["sources", (m) => ({ ...m, sources: ["y.vue"] })],
    ["sourceRoot", (m) => ({ ...m, sourceRoot: "lib/" })],
    ["sourceRoot presence", (m) => ({ ...m, sourceRoot: undefined })],
    ["sourcesContent", (m) => ({ ...m, sourcesContent: ["<template>MUTATED</template>"] })],
    ["sourcesContent presence", (m) => ({ ...m, sourcesContent: undefined })],
    ["names", (m) => ({ ...m, names: ["renamed"] })],
  ];

  for (const [field, mutate] of mapCases) {
    it(`distinguishes maps differing ONLY in ${field}`, () => {
      const golden = baselineMap();
      const candidate = mutate(baselineMap());
      expect(JSON.stringify(candidate)).not.toBe(JSON.stringify(golden));
      const result = compareMappings(golden, candidate);
      expect(result.equal).toBe(false);
      const fieldKey = field.split(" ")[0];
      expect(result.fields[fieldKey]).toBe(false);
      // Every OTHER contractual field still matches — the catch is
      // attributable to exactly the perturbed field.
      for (const other of CONTRACTUAL_MAP_FIELDS) {
        if (other !== fieldKey) expect(result.fields[other]).toBe(true);
      }
    });
  }

  it("the incidental `file` field alone does NOT flag a divergence (classified, excluded by decision)", () => {
    const golden = baselineMap();
    const candidate = { ...baselineMap(), file: "different-out.js" };
    expect(compareMappings(golden, candidate).equal).toBe(true);
  });

  it("map presence itself discriminates", () => {
    expect(compareMappings(baselineMap(), null).equal).toBe(false);
    expect(compareMappings(null, baselineMap()).equal).toBe(false);
    expect(compareMappings(null, null).equal).toBe(true);
  });

  it("a golden generated with sourceMap:true differs from one generated with sourceMap:false in mapPresent", () => {
    const source = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/svelte/basic-runes.svelte"),
      "utf8",
    );
    const withMap = compileSvelteFixture(source, "fixtures/svelte/basic-runes.svelte", {
      generate: "client",
      runes: true,
      dev: false,
      sourceMap: true,
    });
    const withoutMap = compileSvelteFixture(source, "fixtures/svelte/basic-runes.svelte", {
      generate: "client",
      runes: true,
      dev: false,
      sourceMap: false,
    });
    expect(withMap.map).not.toBeNull();
    expect(withoutMap.map).toBeNull();
    // Real official maps carry the full contractual field set the
    // comparator discriminates on.
    expect(withMap.map.mappings).toBeTruthy();
    expect(withMap.map.sources).toBeTruthy();
    expect(withMap.map.names).toBeDefined();
  });
});
