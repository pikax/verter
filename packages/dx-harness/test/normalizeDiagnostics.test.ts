import { describe, expect, it } from "vitest";

import type { Diagnostic, Range } from "../src/normalize/index.js";
import {
  isDefaultDiagnosticRange,
  isImpossibleDefaultDiagnostic,
  normalizeDiagnostics,
} from "../src/normalize/index.js";

const range = (sl: number, sc: number, el: number, ec: number): Range => ({
  start: { line: sl, character: sc },
  end: { line: el, character: ec },
});

describe("normalizeDiagnostics — canonical shape", () => {
  it("maps null / undefined / empty to an empty array (no throw)", () => {
    expect(normalizeDiagnostics(null)).toEqual([]);
    expect(normalizeDiagnostics(undefined)).toEqual([]);
    expect(normalizeDiagnostics([])).toEqual([]);
  });

  it("maps numeric severity to a stable name and stringifies the code", () => {
    const diags: Diagnostic[] = [
      {
        range: range(2, 1, 2, 5),
        severity: 1,
        code: 2304,
        source: "ts",
        message: "Cannot find name",
      },
    ];
    const [d] = normalizeDiagnostics(diags);
    expect(d.severity).toBe("Error");
    expect(d.code).toBe("2304");
    expect(d.source).toBe("ts");
    expect(d.message).toBe("Cannot find name");
  });

  it("sorts order-insensitively by range then content", () => {
    const diags: Diagnostic[] = [
      { range: range(5, 0, 5, 3), severity: 1, message: "b" },
      { range: range(1, 0, 1, 3), severity: 2, message: "a" },
    ];
    const reversed: Diagnostic[] = [diags[1], diags[0]];
    expect(normalizeDiagnostics(diags)).toEqual(normalizeDiagnostics(reversed));
    expect(normalizeDiagnostics(diags).map((d) => d.message)).toEqual(["a", "b"]);
  });

  it("normalizes CRLF in the message to LF for cross-platform comparability", () => {
    const diags: Diagnostic[] = [
      { range: range(0, 0, 0, 1), severity: 1, message: "line1\r\nline2" },
    ];
    expect(normalizeDiagnostics(diags)[0].message).toBe("line1\nline2");
  });
});

describe("normalizeDiagnostics — default (0,0) predicate", () => {
  it("recognizes the default (0,0)-(0,0) sentinel range", () => {
    expect(isDefaultDiagnosticRange(range(0, 0, 0, 0))).toBe(true);
    // A precise positive-width line-0 range is NOT the default sentinel.
    expect(isDefaultDiagnosticRange(range(0, 0, 0, 4))).toBe(false);
    expect(isDefaultDiagnosticRange(range(3, 1, 3, 4))).toBe(false);
  });

  it("does NOT flag a precise line-0 diagnostic whose known source span is also at (0,0)", () => {
    // A genuine positive-width diagnostic at the top of the file, correctly mapped.
    // A naive `line===0` rule would WRONGLY flag this.
    const diag: Diagnostic = {
      range: range(0, 0, 0, 4),
      severity: 1,
      message: "real top-of-file error",
    };
    const knownSpan = range(0, 0, 0, 4);
    expect(isImpossibleDefaultDiagnostic(diag, knownSpan)).toBe(false);
  });

  it("flags a (0,0) diagnostic whose known source span is ELSEWHERE (mapping collapsed)", () => {
    // The component-diagnostic fallback collapses to (0,0) on offset-mapping failure;
    // the real error is at (5,2) → this IS a suspected default-(0,0) fallback.
    const sentinelDiag: Diagnostic = { range: range(0, 0, 0, 0), severity: 1, message: "x" };
    const collapsedDiag: Diagnostic = { range: range(0, 0, 0, 3), severity: 1, message: "x" };
    const knownSpan = range(5, 2, 5, 8);
    expect(isImpossibleDefaultDiagnostic(sentinelDiag, knownSpan)).toBe(true);
    expect(isImpossibleDefaultDiagnostic(collapsedDiag, knownSpan)).toBe(true);
  });

  it("flags the zero-width default sentinel even when the known source span is at (0,0)", () => {
    // (0,0)-(0,0) is the offset-mapping-failure fallback by construction; a real
    // diagnostic has an extent, so the zero-width sentinel is always impossible.
    const sentinelDiag: Diagnostic = { range: range(0, 0, 0, 0), severity: 1, message: "x" };
    expect(isImpossibleDefaultDiagnostic(sentinelDiag, range(0, 0, 0, 0))).toBe(true);
  });

  it("does NOT flag a normal diagnostic that does not start at the origin", () => {
    const diag: Diagnostic = { range: range(7, 3, 7, 9), severity: 1, message: "elsewhere" };
    expect(isImpossibleDefaultDiagnostic(diag, range(7, 3, 7, 9))).toBe(false);
    // Even with an unrelated known span, a non-origin diagnostic is not a (0,0) fallback.
    expect(isImpossibleDefaultDiagnostic(diag, range(2, 0, 2, 4))).toBe(false);
  });
});

describe("normalizeDiagnostics — totality over a non-array body", () => {
  it("returns [] for a non-array response (malformed), never throwing", () => {
    // The raw client hands over an untyped value; anything that is not an array
    // folds to the empty set rather than throwing on `.map`.
    expect(normalizeDiagnostics({ junk: 1 } as unknown as readonly Diagnostic[])).toEqual([]);
  });
});

describe("normalizeDiagnostics — totality over malformed array entries", () => {
  // The raw client hands over an `any` per entry, so every entry — including a
  // bare `{}`, a junk object, `null`, or a non-object — must fold to a safe
  // canonical diagnostic instead of dereferencing `diag.range` / `diag.message`.
  it("folds an empty-object entry to the (0,0) default range, unknown severity, empty message", () => {
    const out = normalizeDiagnostics([{}] as unknown as readonly Diagnostic[]);
    expect(out).toHaveLength(1);
    expect(out[0].range).toEqual(range(0, 0, 0, 0));
    expect(out[0].severity).toBe("unknown");
    expect(out[0].message).toBe("");
    expect(out[0].code).toBeUndefined();
    expect(out[0].source).toBeUndefined();
  });

  it("does not throw on a junk-property entry, a null entry, or a non-object entry", () => {
    expect(() =>
      normalizeDiagnostics([{ junk: 1 }] as unknown as readonly Diagnostic[]),
    ).not.toThrow();
    expect(() =>
      normalizeDiagnostics([null, undefined, 42, "x"] as unknown as readonly Diagnostic[]),
    ).not.toThrow();
    const out = normalizeDiagnostics([{ junk: 1 }] as unknown as readonly Diagnostic[]);
    expect(out[0].range).toEqual(range(0, 0, 0, 0));
  });

  it("coerces a malformed nested range to (0,0) and a non-string message to the empty string", () => {
    const out = normalizeDiagnostics([
      { range: { start: null }, message: 5 },
    ] as unknown as readonly Diagnostic[]);
    expect(out[0].range).toEqual(range(0, 0, 0, 0));
    expect(out[0].message).toBe("");
  });

  it("omits a non-string source and a non-scalar code, treats a non-number severity as unknown", () => {
    const out = normalizeDiagnostics([
      { range: range(1, 0, 1, 1), severity: {}, message: "m", source: 5, code: {} },
    ] as unknown as readonly Diagnostic[]);
    expect(out[0].source).toBeUndefined();
    expect(out[0].code).toBeUndefined();
    expect(out[0].severity).toBe("unknown");
  });

  it("sorts a mix of malformed and well-formed entries without throwing", () => {
    const out = normalizeDiagnostics([
      { range: range(5, 0, 5, 3), severity: 1, message: "b" },
      {},
      { range: range(1, 0, 1, 3), severity: 2, message: "a" },
    ] as unknown as readonly Diagnostic[]);
    expect(out).toHaveLength(3);
    // The (0,0)-coerced junk entry sorts first (lowest range).
    expect(out.map((d) => d.message)).toEqual(["", "a", "b"]);
  });

  it("leaves a well-formed diagnostic's canonical output unchanged", () => {
    const wellFormed: Diagnostic = {
      range: range(2, 1, 2, 5),
      severity: 1,
      code: 2304,
      source: "ts",
      message: "Cannot find name",
    };
    expect(normalizeDiagnostics([wellFormed])).toEqual([
      {
        range: range(2, 1, 2, 5),
        severity: "Error",
        code: "2304",
        source: "ts",
        message: "Cannot find name",
        // An untagged diagnostic carries an empty tag set (always present).
        tags: [],
      },
    ]);
  });
});

describe("normalizeDiagnostics — editor tags (gray-out / strikethrough contract)", () => {
  // The user-visible end-to-end contract: a published `.vue` unused-import
  // diagnostic must carry the `Unnecessary` LSP tag (1) so the editor FADES it.
  // Without this the gray-out is silently lost. The dx-harness guards it here.
  it("carries the Unnecessary tag (1) through normalization — the unused-import fade", () => {
    const unused: Diagnostic = {
      range: range(0, 9, 0, 15),
      severity: 4,
      code: 6133,
      source: "ts",
      message: "'unused' is declared but its value is never read.",
      tags: [1],
    };
    const [d] = normalizeDiagnostics([unused]);
    expect(d.tags).toEqual([1]);
    // Negative: the tag is NOT silently dropped (the pre-fix shape had no tags).
    expect(d.tags).not.toEqual([]);
  });

  it("carries BOTH Unnecessary (1) and Deprecated (2) tags, sorted, on one diagnostic", () => {
    const both: Diagnostic = {
      range: range(0, 0, 0, 9),
      severity: 4,
      message: "'oldUnused' is declared but its value is never read.",
      // Out-of-order on the wire; normalization sorts for set comparison.
      tags: [2, 1],
    };
    const [d] = normalizeDiagnostics([both]);
    expect(d.tags).toEqual([1, 2]);
  });

  it("an untagged diagnostic normalizes to an empty tag set, never undefined", () => {
    const plain: Diagnostic = {
      range: range(1, 0, 1, 1),
      severity: 1,
      message: "Type error",
    };
    const [d] = normalizeDiagnostics([plain]);
    expect(d.tags).toEqual([]);
  });

  it("drops a junk (non-number) tag entry rather than throwing", () => {
    const out = normalizeDiagnostics([
      { range: range(0, 0, 0, 1), severity: 4, message: "m", tags: [1, "x", null] },
    ] as unknown as readonly Diagnostic[]);
    expect(out[0].tags).toEqual([1]);
  });

  it("two diagnostics identical except for tags sort deterministically (not deduped)", () => {
    const tagged: Diagnostic = {
      range: range(3, 0, 3, 4),
      severity: 4,
      message: "same",
      tags: [1],
    };
    const untagged: Diagnostic = { range: range(3, 0, 3, 4), severity: 4, message: "same" };
    const out = normalizeDiagnostics([untagged, tagged]);
    expect(out).toHaveLength(2);
    // The empty-tag one sorts before the tagged one (tag tiebreaker).
    expect(out.map((d) => d.tags)).toEqual([[], [1]]);
  });
});
