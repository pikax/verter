import { describe, expect, it } from "vitest";

import {
  AnchorError,
  addFileAnchors,
  requireAnchor,
  stripAnchors,
  type AnchorMap,
} from "../src/anchors.js";

describe("stripAnchors", () => {
  it("removes a template `<!-- @dx-anchor id -->` comment and records its position", () => {
    const { stripped, anchors } = stripAnchors("const x = <!-- @dx-anchor here -->42");
    expect(stripped).toBe("const x = 42");
    // Negative: no anchor-comment syntax survives in the stripped output.
    expect(stripped).not.toContain("@dx-anchor");
    expect(stripped).not.toContain("<!--");
    expect(anchors.get("here")).toEqual({ line: 0, character: 10, encoding: "utf-16" });
  });

  it("removes a script `// @dx-anchor id` line comment and records its position", () => {
    const src = "<script>\nconst v = 1\n// @dx-anchor lc\n</script>";
    const { stripped, anchors } = stripAnchors(src);
    expect(stripped).not.toContain("@dx-anchor");
    expect(stripped).not.toContain("//");
    // The line-comment anchor occupied its own line; the now-empty line remains
    // so the anchor lands at column 0 of line 2.
    expect(anchors.get("lc")).toEqual({ line: 2, character: 0, encoding: "utf-16" });
  });

  it("does NOT treat the old `<|name|>` delimiter syntax as an anchor", () => {
    const src = "const x = <|here|>42";
    const { stripped, anchors } = stripAnchors(src);
    // The retired grammar is left verbatim and records nothing.
    expect(stripped).toBe(src);
    expect(anchors.size).toBe(0);
  });

  it("shifts a same-line position left by exactly the stripped comment length", () => {
    const comment = "<!-- @dx-anchor x -->";
    const src = `<div>${comment}after</div>`;
    const { stripped, anchors } = stripAnchors(src);
    expect(stripped).toBe("<div>after</div>");
    // The anchor lands where the comment began; "after" now begins there too.
    expect(anchors.get("x")).toEqual({ line: 0, character: 5, encoding: "utf-16" });
    // The raw column of "after" (after "<div>" + the whole comment) shifted left
    // by EXACTLY the comment length — not by some other amount.
    const rawAfterCol = "<div>".length + comment.length;
    expect(stripped.indexOf("after")).toBe(rawAfterCol - comment.length);
    expect(stripped.indexOf("after")).toBe(5);
  });

  it("recomputes later positions on the same line after an earlier comment is removed", () => {
    // Two template anchor comments on one line: removing the first shifts the
    // second left by the first comment's full length.
    const { stripped, anchors } = stripAnchors("ab<!-- @dx-anchor a -->cd<!-- @dx-anchor b -->ef");
    expect(stripped).toBe("abcdef");
    expect(anchors.get("a")).toEqual({ line: 0, character: 2, encoding: "utf-16" });
    // 'b' sits after "abcd" → char 4, NOT its raw offset (25) before strip.
    expect(anchors.get("b")).toEqual({ line: 0, character: 4, encoding: "utf-16" });
    expect(anchors.get("b")!.character).not.toBe(25);
  });

  it("throws on a duplicate anchor name within a file", () => {
    const dup = "<!-- @dx-anchor dup --> ... <!-- @dx-anchor dup -->";
    expect(() => stripAnchors(dup)).toThrow(AnchorError);
    expect(() => stripAnchors(dup)).toThrow(/dup/);
  });

  it("strips a template HTML-comment anchor and keeps surrounding markup", () => {
    const src = "<template>\n  <!-- @dx-anchor tpl --><div/>\n</template>";
    const { stripped, anchors } = stripAnchors(src);
    expect(stripped).toBe("<template>\n  <div/>\n</template>");
    expect(stripped).not.toContain("@dx-anchor");
    // The anchor lands where `<div/>` now begins, after the two-space indent.
    expect(anchors.get("tpl")).toEqual({ line: 1, character: 2, encoding: "utf-16" });
  });

  it("recomputes per-region positions across a multi-region SFC", () => {
    const src = [
      "<template>",
      "  <!-- @dx-anchor t --><div/>",
      "</template>",
      "<script>// @dx-anchor s",
      "</script>",
    ].join("\n");
    const { stripped, anchors } = stripAnchors(src);
    expect(stripped).not.toContain("@dx-anchor");
    // Template anchor on line 1, after the two-space indent.
    expect(anchors.get("t")).toEqual({ line: 1, character: 2, encoding: "utf-16" });
    // Script line-comment anchor on line 3, after "<script>" (8 chars). Its line
    // accounts for the template region above it.
    expect(anchors.get("s")).toEqual({ line: 3, character: 8, encoding: "utf-16" });
  });

  it("yields identical positions for CRLF and LF inputs", () => {
    const lf = "a\n<!-- @dx-anchor m -->b";
    const crlf = "a\r\n<!-- @dx-anchor m -->b";
    expect(stripAnchors(lf).anchors.get("m")).toEqual({
      line: 1,
      character: 0,
      encoding: "utf-16",
    });
    expect(stripAnchors(crlf).anchors.get("m")).toEqual({
      line: 1,
      character: 0,
      encoding: "utf-16",
    });
    // The stripped CRLF text preserves the CRLF terminator (anchors do not
    // rewrite line endings).
    expect(stripAnchors(crlf).stripped).toBe("a\r\nb");
  });

  it("ignores malformed `@dx-anchor` comments that carry no id", () => {
    // `<!-- @dx-anchor -->` has no id before the close; `// @dx-anchor` at EOL has
    // no id on the same line. Neither matches the well-formed grammar.
    const src = "<!-- @dx-anchor --> // @dx-anchor\nx";
    const { stripped, anchors } = stripAnchors(src);
    expect(stripped).toBe(src);
    expect(anchors.size).toBe(0);
  });

  it("strips a line-comment anchor with trailing whitespace, leaving NO residue", () => {
    // A line comment runs to EOL: `// @dx-anchor mark   ` is the whole comment, so
    // the trailing spaces after the id are part of it and must not survive.
    const { stripped, anchors } = stripAnchors("// @dx-anchor mark   ");
    expect(stripped).toBe("");
    // Negative: not the id, not the comment marker, and no trailing-space residue.
    expect(stripped).not.toContain("@dx-anchor");
    expect(stripped).not.toContain("mark");
    expect(stripped).not.toMatch(/\s/);
    expect(anchors.get("mark")).toMatchObject({ line: 0, character: 0 });
  });

  it("strips a line-comment anchor followed by trailing words (the WHOLE line comment)", () => {
    // `// @dx-anchor mark some words` is one line comment — only `mark` is the id;
    // `some words` is comment prose and must be removed with it, not left live.
    const { stripped, anchors } = stripAnchors("// @dx-anchor mark some words");
    expect(stripped).toBe("");
    expect(stripped).not.toContain("some words");
    expect(stripped).not.toContain("mark");
    expect(anchors.get("mark")).toMatchObject({ line: 0, character: 0 });
  });

  it("strips a code-trailing anchor comment to EOL, leaving only the code (no trailing space)", () => {
    // `const x = 1 // @dx-anchor id` → the comment (and the space that separated it
    // from the code) is removed; the code stays, with no now-trailing whitespace.
    const { stripped, anchors } = stripAnchors("const x = 1 // @dx-anchor id");
    expect(stripped).toBe("const x = 1");
    expect(stripped).not.toMatch(/ $/);
    expect(stripped).not.toContain("@dx-anchor");
    // The anchor records where the comment WAS — immediately after the code.
    expect(anchors.get("id")).toMatchObject({ line: 0, character: 11 });
  });

  it("recomputes positions for the FULL removed span (comment + trailing words)", () => {
    // The removed span is the entire line comment incl. its trailing prose; the
    // following line and the recorded position must reflect that full length.
    const { stripped, anchors } = stripAnchors(
      "const a = 1 // @dx-anchor first extra trailing\nconst b = 2",
    );
    expect(stripped).toBe("const a = 1\nconst b = 2");
    expect(stripped).not.toContain("extra trailing");
    expect(stripped).not.toContain("@dx-anchor");
    expect(anchors.get("first")).toMatchObject({ line: 0, character: 11 });
  });

  it("leaves the template HTML comment (delimited) fully stripped even with trailing markup", () => {
    // The delimited `<!-- ... -->` form already stops at `-->`; trailing words on
    // the same template line stay live source — only the line-comment form runs to
    // EOL. This pins the asymmetry so the EOL rule never bleeds into templates.
    const { stripped } = stripAnchors("<div><!-- @dx-anchor t -->live tail</div>");
    expect(stripped).toBe("<div>live tail</div>");
    expect(stripped).not.toContain("@dx-anchor");
    expect(stripped).toContain("live tail");
  });
});

describe("stripAnchors — position encoding metadata", () => {
  it("tags EVERY stripped position with the utf-16 column encoding", () => {
    // Authored anchor columns are UTF-16 code units (LSP's default). Raw-LSP /
    // extension consumers must be able to tell that apart from a negotiated byte
    // offset, so the encoding is DTO metadata, not a doc-comment promise.
    const { anchors } = stripAnchors("<!-- @dx-anchor tpl -->x\n// @dx-anchor lc");
    expect(anchors.size).toBe(2);
    for (const [, pos] of anchors) {
      expect(pos.encoding).toBe("utf-16");
      expect(pos.encoding).not.toBeUndefined();
    }
  });
});

describe("addFileAnchors / requireAnchor", () => {
  it("attaches the file to each anchor and merges across files", () => {
    const map: AnchorMap = new Map();
    addFileAnchors(map, "a.vue", stripAnchors("<!-- @dx-anchor x -->1"));
    addFileAnchors(map, "b.vue", stripAnchors("<!-- @dx-anchor y -->2"));
    expect(requireAnchor(map, "x")).toEqual({
      file: "a.vue",
      line: 0,
      character: 0,
      encoding: "utf-16",
    });
    expect(requireAnchor(map, "y")).toEqual({
      file: "b.vue",
      line: 0,
      character: 0,
      encoding: "utf-16",
    });
  });

  it("carries the utf-16 encoding metadata through onto the resolved Anchor", () => {
    const map: AnchorMap = new Map();
    addFileAnchors(map, "a.vue", stripAnchors("<!-- @dx-anchor x -->1"));
    const a = requireAnchor(map, "x");
    expect(a.encoding).toBe("utf-16");
    expect(a.encoding).not.toBeUndefined();
  });

  it("throws on a duplicate anchor name ACROSS files", () => {
    const map: AnchorMap = new Map();
    addFileAnchors(map, "a.vue", stripAnchors("<!-- @dx-anchor dup -->"));
    expect(() => addFileAnchors(map, "b.vue", stripAnchors("<!-- @dx-anchor dup -->"))).toThrow(
      AnchorError,
    );
    expect(() => addFileAnchors(map, "b.vue", stripAnchors("<!-- @dx-anchor dup -->"))).toThrow(
      /dup/,
    );
  });

  it("throws a clear error when a required anchor is missing", () => {
    const map: AnchorMap = new Map();
    addFileAnchors(map, "a.vue", stripAnchors("<!-- @dx-anchor present -->"));
    expect(() => requireAnchor(map, "absent")).toThrow(AnchorError);
    expect(() => requireAnchor(map, "absent")).toThrow(/absent/);
    // Negative: a present anchor does not throw.
    expect(() => requireAnchor(map, "present")).not.toThrow();
  });
});
