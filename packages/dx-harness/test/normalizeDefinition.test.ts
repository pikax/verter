import { describe, expect, it } from "vitest";

import type { Location, LocationLink, Range } from "../src/normalize/index.js";
import {
  definitionMatchesExpected,
  isDefinitionGeneratedOnly,
  isUnmappedGeneratedOnly,
  normalizeDefinition,
} from "../src/normalize/index.js";

const range = (sl: number, sc: number, el: number, ec: number): Range => ({
  start: { line: sl, character: sc },
  end: { line: el, character: ec },
});

describe("normalizeDefinition — input shape coverage", () => {
  it("maps null / undefined to an empty array (no throw)", () => {
    expect(normalizeDefinition(null)).toEqual([]);
    expect(normalizeDefinition(undefined)).toEqual([]);
  });

  it("normalizes a single Location, a Location[], and a LocationLink[] to one canonical shape", () => {
    const loc: Location = { uri: "file:///src/foo.ts", range: range(2, 4, 2, 7) };
    expect(normalizeDefinition(loc)).toEqual([
      { uri: "file:///src/foo.ts", range: range(2, 4, 2, 7) },
    ]);

    expect(normalizeDefinition([loc])).toEqual([
      { uri: "file:///src/foo.ts", range: range(2, 4, 2, 7) },
    ]);

    const link: LocationLink = {
      targetUri: "file:///src/foo.ts",
      targetRange: range(2, 0, 4, 1),
      targetSelectionRange: range(2, 4, 2, 7),
    };
    // A LocationLink projects its targetUri/targetSelectionRange to the canonical form.
    expect(normalizeDefinition([link])).toEqual([
      { uri: "file:///src/foo.ts", range: range(2, 4, 2, 7) },
    ]);
  });

  it("marks a target in a generated artifact as `fromGenerated`", () => {
    const gen: Location = { uri: "file:///src/App.vue.tsx", range: range(10, 2, 10, 8) };
    const authored: Location = { uri: "file:///src/foo.ts", range: range(1, 0, 1, 3) };
    const [g, a] = normalizeDefinition([gen, authored]);
    expect(g.fromGenerated).toBe(true);
    expect(a.fromGenerated).toBeUndefined();
  });
});

describe("normalizeDefinition — a precise line-0 target is VALID", () => {
  it("does NOT flag a precise line-0 authored target as invalid (regression vs a naive `line===0`)", () => {
    // A real declaration at the very top of an authored source file. A naive
    // predicate that rejects `range.start.line === 0` would WRONGLY fail this.
    const target: Location = { uri: "file:///src/foo.ts", range: range(0, 9, 0, 12) };
    const [t] = normalizeDefinition(target);
    expect(t.range.start.line).toBe(0); // precise line-0 range — preserved as-is
    expect(t.fromGenerated).toBeUndefined(); // authored, not generated
    // It is NOT a generated-only-unmapped failure…
    expect(isUnmappedGeneratedOnly(t)).toBe(false);
    expect(isDefinitionGeneratedOnly(normalizeDefinition(target))).toBe(false);
    // …and it MATCHES the expected symbol by file + range, line-0 notwithstanding.
    expect(
      definitionMatchesExpected(normalizeDefinition(target), {
        uri: "file:///src/foo.ts",
        range: range(0, 9, 0, 12),
      }),
    ).toBe(true);
  });

  it("fails a genuinely wrong target by SYMBOL IDENTITY (file / range), never by line number", () => {
    // Same precise line-0 range, but a different source file → not the expected symbol.
    const wrongFile: Location = { uri: "file:///src/other.ts", range: range(0, 9, 0, 12) };
    expect(
      definitionMatchesExpected(normalizeDefinition(wrongFile), {
        uri: "file:///src/foo.ts",
        range: range(0, 9, 0, 12),
      }),
    ).toBe(false);

    // Correct file, wrong range → not the expected symbol.
    const wrongRange: Location = { uri: "file:///src/foo.ts", range: range(7, 0, 7, 3) };
    expect(
      definitionMatchesExpected(normalizeDefinition(wrongRange), {
        uri: "file:///src/foo.ts",
        range: range(0, 9, 0, 12),
      }),
    ).toBe(false);
  });

  it("flags a generated-only target set as the generated-only-unmapped failure", () => {
    const gen: Location = { uri: "file:///src/App.vue.tsx", range: range(0, 0, 0, 5) };
    const targets = normalizeDefinition(gen);
    expect(isUnmappedGeneratedOnly(targets[0])).toBe(true);
    // The whole set is generated-only (no authored target was produced).
    expect(isDefinitionGeneratedOnly(targets)).toBe(true);
    // An EMPTY set is not "generated-only" — there is no generated target at all.
    expect(isDefinitionGeneratedOnly([])).toBe(false);
  });

  it("is NOT generated-only when at least one authored target is present", () => {
    const gen: Location = { uri: "file:///src/App.vue.tsx", range: range(0, 0, 0, 5) };
    const authored: Location = { uri: "file:///src/foo.ts", range: range(0, 9, 0, 12) };
    expect(isDefinitionGeneratedOnly(normalizeDefinition([gen, authored]))).toBe(false);
  });
});

describe("normalizeDefinition — totality over malformed entries and nested ranges", () => {
  // Each entry is an `any` from the raw client; null / non-object entries skip,
  // and a malformed nested range must fold to the (0,0) default rather than leak a
  // broken range that throws in a downstream `rangesEqual`.
  it("skips null / undefined / non-object / unrecognized entries without throwing", () => {
    expect(() =>
      normalizeDefinition([null, undefined, 42, "x", {}] as unknown as readonly Location[]),
    ).not.toThrow();
    expect(
      normalizeDefinition([null, undefined, 42, "x", {}] as unknown as readonly Location[]),
    ).toEqual([]);
  });

  it("coerces a malformed LocationLink range to the (0,0) default, keeping the uri", () => {
    const link = { targetUri: "file:///src/foo.ts", targetSelectionRange: { start: null } };
    const out = normalizeDefinition(link as unknown as LocationLink);
    expect(out).toHaveLength(1);
    expect(out[0].uri).toBe("file:///src/foo.ts");
    expect(out[0].range).toEqual(range(0, 0, 0, 0));
    // The coerced target is safe for the downstream symbol-identity predicates.
    expect(() => definitionMatchesExpected(out, { uri: "file:///src/foo.ts" })).not.toThrow();
    expect(definitionMatchesExpected(out, { uri: "file:///src/foo.ts" })).toBe(true);
  });

  it("coerces a malformed Location range (non-object) to the (0,0) default, keeping the uri", () => {
    const loc = { uri: "file:///src/bar.ts", range: 42 };
    const out = normalizeDefinition(loc as unknown as Location);
    expect(out).toHaveLength(1);
    expect(out[0].uri).toBe("file:///src/bar.ts");
    expect(out[0].range).toEqual(range(0, 0, 0, 0));
  });

  it("falls back to targetRange when targetSelectionRange is malformed (non-object)", () => {
    const link = {
      targetUri: "file:///src/foo.ts",
      targetSelectionRange: 42,
      targetRange: range(2, 0, 4, 1),
    };
    const out = normalizeDefinition(link as unknown as LocationLink);
    expect(out[0].range).toEqual(range(2, 0, 4, 1));
  });

  it("does not change a well-formed Location / LocationLink", () => {
    const loc: Location = { uri: "file:///src/foo.ts", range: range(2, 4, 2, 7) };
    expect(normalizeDefinition(loc)).toEqual([
      { uri: "file:///src/foo.ts", range: range(2, 4, 2, 7) },
    ]);
    const link: LocationLink = {
      targetUri: "file:///src/foo.ts",
      targetRange: range(2, 0, 4, 1),
      targetSelectionRange: range(2, 4, 2, 7),
    };
    expect(normalizeDefinition([link])).toEqual([
      { uri: "file:///src/foo.ts", range: range(2, 4, 2, 7) },
    ]);
  });
});

describe("definitionMatchesExpected — an empty expectation never matches", () => {
  it("does NOT match any target when the expectation declares neither uri nor range", () => {
    const target: Location = { uri: "file:///src/foo.ts", range: range(2, 4, 2, 7) };
    const targets = normalizeDefinition(target);
    // An empty expectation is not a wildcard — it must fail, not vacuously pass.
    expect(definitionMatchesExpected(targets, {})).toBe(false);
    // A full file+range expectation still matches the real target.
    expect(
      definitionMatchesExpected(targets, {
        uri: "file:///src/foo.ts",
        range: range(2, 4, 2, 7),
      }),
    ).toBe(true);
    // A uri-only expectation matches by file alone; a wrong file does not.
    expect(definitionMatchesExpected(targets, { uri: "file:///src/foo.ts" })).toBe(true);
    expect(definitionMatchesExpected(targets, { uri: "file:///src/other.ts" })).toBe(false);
  });
});
