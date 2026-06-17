import { describe, expect, it } from "vitest";

import type { Hover } from "../src/normalize/index.js";
import { normalizeHover } from "../src/normalize/index.js";

describe("normalizeHover — no-hover vs empty-hover", () => {
  it("represents a null / undefined response as `null` (no hover), and does NOT throw", () => {
    expect(normalizeHover(null)).toBeNull();
    expect(normalizeHover(undefined)).toBeNull();
  });

  it("represents an empty-contents hover DISTINCTLY from no-hover", () => {
    // A real Hover whose contents are empty is NOT the same as the absence of a
    // hover — a hover over a synthetic region is not automatically a failure.
    const emptyMarkup: Hover = { contents: { kind: "markdown", value: "" } };
    const out = normalizeHover(emptyMarkup);
    expect(out).not.toBeNull();
    expect(out?.contents).toBe("");

    const emptyArray: Hover = { contents: [] };
    const outArray = normalizeHover(emptyArray);
    expect(outArray).not.toBeNull();
    expect(outArray?.contents).toBe("");
  });
});

describe("normalizeHover — clean comparable Vue-surface label", () => {
  it("surfaces a `@click` handler label as `@click`, never `onClick`", () => {
    const hover: Hover = {
      contents: {
        kind: "markdown",
        value: "```ts\n(property) @click: (e: MouseEvent) => void\n```",
      },
      range: { start: { line: 3, character: 4 }, end: { line: 3, character: 10 } },
    };
    const out = normalizeHover(hover);
    expect(out?.contents).toContain("@click");
    expect(out?.contents).not.toContain("onClick");
    // The range is carried for downstream position assertions.
    expect(out?.range).toEqual(hover.range);
  });

  it("surfaces an event-modifier label such as `@touchmove.stop`", () => {
    const hover: Hover = { contents: { kind: "plaintext", value: "@touchmove.stop" } };
    expect(normalizeHover(hover)?.contents).toBe("@touchmove.stop");
  });

  it("joins a MarkedString[] hover and extracts the string values (dropping language fences)", () => {
    const hover: Hover = {
      contents: [
        { language: "typescript", value: "const drawerRef: Ref<HTMLElement | null>" },
        "the template ref",
      ],
    };
    const out = normalizeHover(hover);
    expect(out?.contents).toContain("drawerRef");
    expect(out?.contents).toContain("the template ref");
    // Negative: the `language` tag is metadata, not surfaced content.
    expect(out?.contents).not.toContain("typescript");
  });

  it("normalizes CRLF/CR line endings to LF for cross-platform comparability", () => {
    const hover: Hover = { contents: { kind: "plaintext", value: "line1\r\nline2\rline3" } };
    const out = normalizeHover(hover);
    expect(out?.contents).toBe("line1\nline2\nline3");
    expect(out?.contents).not.toContain("\r");
  });

  it("accepts a bare-string hover (deprecated MarkedString form)", () => {
    expect(normalizeHover({ contents: "ref" })?.contents).toBe("ref");
  });
});

describe("normalizeHover — totality over a malformed contents body", () => {
  it("returns no-hover (null) for a null or missing `contents`, never throwing", () => {
    // A raw provider response is untyped (`any` from the client); a body whose
    // `contents` is null or absent folds to no-hover instead of throwing.
    expect(normalizeHover({ contents: null } as unknown as Hover)).toBeNull();
    expect(normalizeHover({} as unknown as Hover)).toBeNull();
  });

  it("folds a `contents` array containing a null entry to empty contents (markedStringValue null guard)", () => {
    // A null array element must not dereference `.value`; the deep guard folds it
    // to "" rather than throwing. `contents` is present, so this is empty-hover
    // (`{ contents: "" }`), NOT no-hover (null).
    const out = normalizeHover({ contents: [null] } as unknown as Hover);
    expect(out).not.toBeNull();
    expect(out?.contents).toBe("");
  });

  it("folds a non-object, non-string `contents` (a number) to empty contents (extractContents non-object guard)", () => {
    const out = normalizeHover({ contents: 42 } as unknown as Hover);
    expect(out).not.toBeNull();
    expect(out?.contents).toBe("");
  });

  it("drops malformed array entries while keeping the valid MarkedString values", () => {
    const out = normalizeHover({
      contents: [null, 42, "keep", { value: "v" }, { value: 7 }],
    } as unknown as Hover);
    expect(out?.contents).toBe("keep\nv");
  });
});
