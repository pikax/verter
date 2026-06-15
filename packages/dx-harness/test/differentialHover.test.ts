import { describe, expect, it } from "vitest";

import { GeneratedDocument, compareHover, stripUnstableDocs } from "../src/differential/index.js";
import type { CanonicalHover, NormalizedHover } from "../src/index.js";

describe("stripUnstableDocs — keep the type signature, drop unstable documentation", () => {
  it("drops a fenced code block's fences and the trailing prose", () => {
    const raw = "```typescript\nconst x: string\n```\nSome documentation that changes.";
    expect(stripUnstableDocs(raw)).toBe("const x: string");
  });

  it("drops @-tag JSDoc lines and collapses a multi-line signature", () => {
    const raw = "function useX(): Ref<\n  number\n>\n\n@returns a ref\n@since 1.0";
    expect(stripUnstableDocs(raw)).toBe("function useX(): Ref< number >");
  });

  it("an empty body and an empty fenced block both strip to the empty string", () => {
    expect(stripUnstableDocs("")).toBe("");
    expect(stripUnstableDocs("```typescript\n```")).toBe("");
  });
});

describe("compareHover — type label parity with docs stripped", () => {
  it("equal type labels after stripping docs -> agreement", () => {
    const v: CanonicalHover = { contents: "function useX(): Ref<number>\n\n@returns a ref" };
    const b: NormalizedHover = { contents: "```typescript\nfunction useX(): Ref<number>\n```" };
    expect(compareHover(v, b)).toEqual([]);
  });

  it("a difference only in the unstable docs -> agreement (stripped away)", () => {
    const v: CanonicalHover = { contents: "const x: string\n\nverter doc prose" };
    const b: NormalizedHover = { contents: "const x: string\n\ntsserver doc prose" };
    expect(compareHover(v, b)).toEqual([]);
  });

  it("a genuinely different type label -> typeLabelMismatch", () => {
    const v: CanonicalHover = { contents: "const x: string" };
    const b: NormalizedHover = { contents: "const x: number" };
    const out = compareHover(v, b);
    expect(out.map((d) => d.class)).toEqual(["typeLabelMismatch"]);
  });

  it("a required snippet absent from verter's type label -> missingSnippet", () => {
    const v: CanonicalHover = { contents: "const x: string" };
    const b: NormalizedHover = { contents: "const x: string" };
    const out = compareHover(v, b, { requiredSnippets: ["Ref<"] });
    expect(out.map((d) => d.class)).toEqual(["missingSnippet"]);
  });
});

describe("compareHover — no-hover vs empty-hover is handled, never a false failure", () => {
  it("both contentless (null/empty) -> agreement", () => {
    expect(compareHover(null, null)).toEqual([]);
    // verter empty-hover vs baseline no-hover: both contentless, not a failure.
    expect(compareHover({ contents: "" }, null)).toEqual([]);
  });

  it("content on exactly one side -> hoverPresenceMismatch", () => {
    const real: CanonicalHover = { contents: "const x: string" };
    expect(compareHover(null, { contents: "const x: string" }).map((d) => d.class)).toEqual([
      "hoverPresenceMismatch",
    ]);
    expect(compareHover(real, { contents: "" }).map((d) => d.class)).toEqual([
      "hoverPresenceMismatch",
    ]);
  });
});

describe("compareHover — optional range parity in generated space", () => {
  const doc = new GeneratedDocument("const x: string = 'hi';\n");

  it("a baseline byte range matching verter's LSP range -> no range divergence", () => {
    // bytes 6..7 are `x` on line 0 -> {line:0,character:6}..{line:0,character:7}.
    const v: CanonicalHover = {
      contents: "const x: string",
      range: { start: { line: 0, character: 6 }, end: { line: 0, character: 7 } },
    };
    const b: NormalizedHover = { contents: "const x: string", rangeStart: 6, rangeEnd: 7 };
    expect(compareHover(v, b, { document: doc })).toEqual([]);
  });

  it("a baseline byte range that maps elsewhere -> rangeMismatch", () => {
    const v: CanonicalHover = {
      contents: "const x: string",
      range: { start: { line: 0, character: 6 }, end: { line: 0, character: 7 } },
    };
    const b: NormalizedHover = { contents: "const x: string", rangeStart: 0, rangeEnd: 5 };
    const out = compareHover(v, b, { document: doc });
    expect(out.map((d) => d.class)).toEqual(["rangeMismatch"]);
  });
});
