// Self-test: diagnostic/mapping discrimination (BF2 required exit).
//
// Two diagnostics that differ only in code, or only in position, must never
// be treated as equal — and a golden/candidate pair that differ only in
// whether a source map was produced must be distinguishable too.

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import { compareDiagnostics } from "../src/compare.mjs";
import { compileSvelteFixture } from "../src/invoke-svelte-oracle.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";

describe("diagnostic discrimination", () => {
  it("treats identical diagnostic sequences as equal", () => {
    const a = [{ kind: "warning", code: "a11y_x", start: { line: 1, column: 2 } }];
    const b = [{ kind: "warning", code: "a11y_x", start: { line: 1, column: 2 } }];
    expect(compareDiagnostics(a, b).equal).toBe(true);
  });

  it("distinguishes diagnostics differing only by code", () => {
    const a = [{ kind: "warning", code: "a11y_x", start: { line: 1, column: 2 } }];
    const b = [{ kind: "warning", code: "a11y_y", start: { line: 1, column: 2 } }];
    expect(compareDiagnostics(a, b).equal).toBe(false);
  });

  it("distinguishes diagnostics differing only by position (span drift)", () => {
    const a = [{ kind: "warning", code: "a11y_x", start: { line: 1, column: 2 } }];
    const b = [{ kind: "warning", code: "a11y_x", start: { line: 1, column: 9 } }];
    expect(compareDiagnostics(a, b).equal).toBe(false);
  });

  it("distinguishes diagnostic sequences by count/order", () => {
    const a = [
      { kind: "warning", code: "x", start: { line: 1, column: 1 } },
      { kind: "warning", code: "y", start: { line: 2, column: 1 } },
    ];
    const b = [
      { kind: "warning", code: "y", start: { line: 2, column: 1 } },
      { kind: "warning", code: "x", start: { line: 1, column: 1 } },
    ];
    expect(compareDiagnostics(a, b).equal).toBe(false);
  });
});

describe("mapping presence discrimination", () => {
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
  });
});
