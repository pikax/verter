import { describe, expect, it } from "vitest";

import {
  compareBaselineRanking,
  compareCompletion,
  type BaselineCompletion,
} from "../src/differential/index.js";
import type { CanonicalCompletionList } from "../src/index.js";

function verter(
  items: CanonicalCompletionList["items"],
  extra: Partial<CanonicalCompletionList> = {},
): CanonicalCompletionList {
  return {
    items,
    isIncomplete: false,
    noSuggestionsCollapse: items.length === 0,
    ...extra,
  };
}

function baseline(items: BaselineCompletion["items"]): BaselineCompletion {
  return { items, isIncomplete: false };
}

describe("compareCompletion — order-insensitive set parity", () => {
  it("identical label sets modulo order -> agreement (no divergence)", () => {
    const v = verter([
      { label: "ref", kind: "Function", insertText: "ref" },
      { label: "computed", kind: "Function", insertText: "computed" },
    ]);
    const b = baseline([
      { label: "computed", kind: "Function", insertText: "computed" },
      { label: "ref", kind: "Function", insertText: "ref" },
    ]);
    expect(compareCompletion(v, b)).toEqual([]);
  });

  it("a baseline label verter lacks is a missingLabel gap", () => {
    const v = verter([{ label: "ref" }]);
    const b = baseline([{ label: "ref" }, { label: "computed" }]);
    const out = compareCompletion(v, b);
    expect(out).toHaveLength(1);
    expect(out[0].class).toBe("missingLabel");
    expect(out[0].detail).toContain("computed");
  });

  it("a scenario-required label missing from verter -> missingLabel", () => {
    const v = verter([{ label: "ref" }]);
    const b = baseline([{ label: "ref" }]);
    const out = compareCompletion(v, b, { requiredLabels: ["useTemplateRef"] });
    expect(out.map((d) => d.class)).toEqual(["missingLabel"]);
    expect(out[0].detail).toContain("useTemplateRef");
  });

  it("verter empty where baseline is non-empty -> noSuggestionsCollapse (not a label storm)", () => {
    const v = verter([]);
    const b = baseline([{ label: "ref" }, { label: "computed" }, { label: "watch" }]);
    const out = compareCompletion(v, b);
    expect(out).toHaveLength(1);
    expect(out[0].class).toBe("noSuggestionsCollapse");
  });

  it("a verter-only label absent from the baseline -> extraLabel (set parity is bidirectional)", () => {
    const v = verter([{ label: "a" }, { label: "b" }]);
    const b = baseline([{ label: "a" }]);
    const out = compareCompletion(v, b);
    expect(out.map((d) => d.class)).toEqual(["extraLabel"]);
    expect(out[0].detail).toContain("b");
  });

  it("a baseline-only label -> missingLabel (the other direction still holds)", () => {
    const v = verter([{ label: "a" }]);
    const b = baseline([{ label: "a" }, { label: "b" }]);
    const out = compareCompletion(v, b);
    expect(out.map((d) => d.class)).toEqual(["missingLabel"]);
    expect(out[0].detail).toContain("b");
  });
});

describe("compareCompletion — order alone never diverges (ranking is asserted elsewhere)", () => {
  it("an order-only difference in the shared label set is agreement", () => {
    const v = verter([{ label: "a" }, { label: "b" }]);
    const b = baseline([{ label: "b" }, { label: "a" }]);
    expect(compareCompletion(v, b)).toEqual([]);
  });
});

describe("compareBaselineRanking — a baseline-side order assertion, never a verter divergence", () => {
  it("a baseline order that violates the asserted ranking -> rankingMismatch", () => {
    const b = baseline([{ label: "b" }, { label: "a" }]);
    const out = compareBaselineRanking(b, ["a", "b"]);
    expect(out.map((d) => d.class)).toEqual(["rankingMismatch"]);
    expect(out[0].baselineValue).toEqual(["b", "a"]);
  });

  it("a baseline order matching the asserted ranking -> agreement", () => {
    const b = baseline([{ label: "a" }, { label: "b" }, { label: "c" }]);
    expect(compareBaselineRanking(b, ["a", "b"])).toEqual([]);
  });
});

describe("compareCompletion — kind and insert/edit shape", () => {
  it("a kind mismatch on a shared label -> wrongKind", () => {
    const v = verter([{ label: "x", kind: "Variable" }]);
    const b = baseline([{ label: "x", kind: "Property" }]);
    const out = compareCompletion(v, b);
    expect(out.map((d) => d.class)).toEqual(["wrongKind"]);
  });

  it("kind compare is case-insensitive (provider vocab casing is not a divergence)", () => {
    const v = verter([{ label: "x", kind: "Method" }]);
    const b = baseline([{ label: "x", kind: "method" }]);
    expect(compareCompletion(v, b)).toEqual([]);
  });

  it("a kind present on exactly one side of a shared label -> wrongKind", () => {
    const v = verter([{ label: "x", kind: "Function" }]);
    const b = baseline([{ label: "x" }]);
    const out = compareCompletion(v, b);
    expect(out.map((d) => d.class)).toEqual(["wrongKind"]);
  });

  it("an insert/edit-shape mismatch on a shared label -> insertEditShape", () => {
    const v = verter([{ label: "x", insertText: "x" }]);
    const b = baseline([{ label: "x", insertText: "x()" }]);
    const out = compareCompletion(v, b);
    expect(out.map((d) => d.class)).toEqual(["insertEditShape"]);
  });

  it("a verter textEdit is honored as the effective insert", () => {
    const v = verter([
      {
        label: "x",
        textEdit: {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
          newText: "x()",
        },
      },
    ]);
    const b = baseline([{ label: "x", insertText: "x()" }]);
    expect(compareCompletion(v, b)).toEqual([]);
  });
});
