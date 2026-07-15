import { describe, expect, it } from "vitest";

import { GeneratedDocument, compareDiagnostics } from "../src/differential/index.js";
import type { CanonicalDiagnostic, NormalizedDiagnostic } from "../src/index.js";

// `const x = y;` — `y` is bytes 10..11 -> {line:0,character:10}..{line:0,character:11}.
const tsx = "const x = y;\n";
// The emitted-TSX converter is a REQUIRED input — built once per artifact, queried per probe.
const doc = new GeneratedDocument(tsx);

function vDiag(over: Partial<CanonicalDiagnostic> = {}): CanonicalDiagnostic {
  return {
    range: { start: { line: 0, character: 10 }, end: { line: 0, character: 11 } },
    severity: "Error",
    code: "2304",
    message: "Cannot find name 'y'.",
    ...over,
  };
}

function bDiag(over: Partial<NormalizedDiagnostic> = {}): NormalizedDiagnostic {
  return {
    message: "Cannot find name 'y'.",
    severity: "error",
    start: 10,
    end: 11,
    code: "2304",
    ...over,
  };
}

describe("compareDiagnostics — code+range parity in generated space", () => {
  it("same code and same projected range -> agreement", () => {
    expect(compareDiagnostics([vDiag()], [bDiag()], doc)).toEqual([]);
  });

  it("a positive-width LINE-0 diagnostic that matches the baseline is NOT a default-range failure", () => {
    const v = vDiag({
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 5 } },
    });
    const b = bDiag({ start: 0, end: 5 });
    expect(compareDiagnostics([v], [b], doc)).toEqual([]);
  });

  it("same code+range but a different severity/category -> severityMismatch (not agreement)", () => {
    // verter says Error, baseline says warning, at the same code 2304 and same range.
    const out = compareDiagnostics([vDiag()], [bDiag({ severity: "warning" })], doc);
    expect(out.map((d) => d.class)).toEqual(["severityMismatch"]);
    expect(out.map((d) => d.class)).not.toContain("rangeMismatch");
  });
});

describe("compareDiagnostics — verter-only / baseline-only / default-range are distinct classes", () => {
  it("a verter diagnostic the baseline lacks -> verterOnly", () => {
    const out = compareDiagnostics([vDiag()], [], doc);
    expect(out.map((d) => d.class)).toEqual(["verterOnly"]);
  });

  it("a baseline diagnostic verter lacks -> baselineOnly", () => {
    const out = compareDiagnostics([], [bDiag()], doc);
    expect(out.map((d) => d.class)).toEqual(["baselineOnly"]);
  });

  it("a matched diagnostic verter collapsed to the (0,0) default -> defaultRange, NOT rangeMismatch", () => {
    const v = vDiag({
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
    });
    const out = compareDiagnostics([v], [bDiag()], doc);
    expect(out.map((d) => d.class)).toEqual(["defaultRange"]);
    expect(out.map((d) => d.class)).not.toContain("rangeMismatch");
  });

  it("a matched diagnostic at two distinct real ranges -> rangeMismatch (not default-range)", () => {
    const v = vDiag({
      range: { start: { line: 2, character: 2 }, end: { line: 2, character: 5 } },
    });
    const out = compareDiagnostics([v], [bDiag()], doc);
    expect(out.map((d) => d.class)).toEqual(["rangeMismatch"]);
  });

  it("does not collapse the three classes when all occur together", () => {
    const v = [vDiag({ code: "2304" })]; // matched-but-collapsed below via range
    const collapsed = vDiag({
      code: "2552",
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
    });
    const verterOnlyDiag = vDiag({ code: "7006", message: "implicit any" });
    const baseline = [
      bDiag({ code: "2304" }),
      bDiag({ code: "2552", message: "did you mean" }),
      bDiag({ code: "1005", message: "';' expected" }), // baseline-only
    ];
    const out = compareDiagnostics([...v, collapsed, verterOnlyDiag], baseline, doc);
    const classes = out.map((d) => d.class).sort();
    expect(classes).toEqual(["baselineOnly", "defaultRange", "verterOnly"]);
  });
});

describe("compareDiagnostics — the emitted-TSX converter is a required input", () => {
  it("cannot be called without a prepared document (no silent (0,0) collapse)", () => {
    // @ts-expect-error the GeneratedDocument argument is required; there is no
    // emitted-TSX-absent path that would collapse baseline ranges to the (0,0) default.
    const call = (): unknown[] => compareDiagnostics([vDiag()], [bDiag()]);
    expect(call).toBeTypeOf("function");
  });
});
