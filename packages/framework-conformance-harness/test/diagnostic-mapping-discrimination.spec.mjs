// Self-test: diagnostic/mapping discrimination (BF2 required exit).
//
// EVERY contract-observable diagnostic field discriminates INDEPENDENTLY:
// category/kind, code, the FULL message chain, source/file identity, start
// AND end spans, related information, and order/count. A
// diagnostic that matches on every field but one must be caught — each case
// below differs from its baseline in EXACTLY one field, so a comparison that
// ignored that field would falsely pass and fail the test.

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import { compareDiagnostics } from "../src/compare.mjs";
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

describe("map PRESENCE is a recorded, discriminating property of a golden", () => {
  // Field-by-field comparison against the OFFICIAL compiler's map is gone:
  // a `mappings` field describes ONE generated document, and Verter's
  // generated JS is legitimately not byte-identical to official's, so the
  // two maps are never in the same coordinate space. The mapping axis is
  // now self-referential (src/mapping-oracle.mjs, exercised by
  // test/mapping-oracle*.spec.mjs). What remains observable HERE is that a
  // golden records whether its compilation produced a map at all.
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
    // A produced map is a real, populated v3 map.
    expect(withMap.map.mappings).toBeTruthy();
    expect(withMap.map.sources).toBeTruthy();
    expect(withMap.map.names).toBeDefined();
  });
});
