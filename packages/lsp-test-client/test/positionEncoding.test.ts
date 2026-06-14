import { describe, expect, it } from "vitest";

import {
  adoptServerEncoding,
  byteOffsetToPosition,
  defaultClientPositionEncodings,
  DocumentPositions,
  isPositionEncoding,
  positionToByteOffset,
  withPositionEncodings,
  type PositionEncoding,
} from "../src/index.js";

const ALL_ENCODINGS: PositionEncoding[] = ["utf-8", "utf-16", "utf-32"];

describe("position-encoding negotiation helpers", () => {
  it("adopts a valid server encoding and defaults to utf-16 otherwise", () => {
    expect(adoptServerEncoding("utf-8")).toBe("utf-8");
    expect(adoptServerEncoding("utf-16")).toBe("utf-16");
    expect(adoptServerEncoding("utf-32")).toBe("utf-32");
    // Missing / invalid → spec default.
    expect(adoptServerEncoding(undefined)).toBe("utf-16");
    expect(adoptServerEncoding(null)).toBe("utf-16");
    expect(adoptServerEncoding("latin1")).toBe("utf-16");
  });

  it("validates position-encoding strings", () => {
    expect(isPositionEncoding("utf-8")).toBe(true);
    expect(isPositionEncoding("utf-16")).toBe(true);
    expect(isPositionEncoding("utf-32")).toBe(true);
    expect(isPositionEncoding("utf-7")).toBe(false);
    expect(isPositionEncoding(42)).toBe(false);
  });

  it("advertises utf-16 then utf-8 by default", () => {
    expect(defaultClientPositionEncodings()).toEqual(["utf-16", "utf-8"]);
  });

  it("injects general.positionEncodings without clobbering or mutating input", () => {
    const params = {
      processId: 1,
      capabilities: {
        textDocument: { hover: {} },
        general: { markdown: { parser: "marked" } },
      },
    };
    const out = withPositionEncodings(params, ["utf-16", "utf-8"]);
    expect(out.capabilities.general!.positionEncodings).toEqual(["utf-16", "utf-8"]);
    expect((out.capabilities.general as any).markdown).toEqual({ parser: "marked" });
    expect((out.capabilities as any).textDocument).toEqual({ hover: {} });
    expect(out.processId).toBe(1);
    // Input object is not mutated.
    expect((params.capabilities.general as any).positionEncodings).toBeUndefined();
  });
});

describe("DocumentPositions ASCII baseline", () => {
  it("maps ASCII positions identically across all encodings", () => {
    const text = "const x = 1\nconst y = 2\n";
    const doc = new DocumentPositions(text);
    const off = text.indexOf("x");
    for (const enc of ALL_ENCODINGS) {
      const pos = doc.utf16ToPosition(off, enc);
      expect(pos).toEqual({ line: 0, character: 6 });
      expect(doc.positionToUtf16(pos, enc)).toBe(off);
    }
    // ASCII byte offset equals the UTF-16 offset.
    expect(doc.utf16ToByte(off)).toBe(off);
  });
});

describe("DocumentPositions non-ASCII round trip (go/no-go)", () => {
  it("round-trips a non-ASCII cursor: source pos -> byte offset -> LSP position, encoding-sensitive", () => {
    const text = "ab café 😀 cd";
    const doc = new DocumentPositions(text);

    // Cursor just after the astral emoji (surrogate pair occupies indices 8..9).
    const cursorUtf16 = text.indexOf("😀") + 2;
    expect(cursorUtf16).toBe(10);

    // Source (editor / UTF-16) position.
    const sourcePos = doc.utf16ToPosition(cursorUtf16, "utf-16");
    expect(sourcePos).toEqual({ line: 0, character: 10 });

    // -> UTF-8 byte offset (verter's contract).
    const byte = doc.sourceToByte(sourcePos);
    expect(byte).toBe(13);

    // -> LSP position, which depends on the negotiated encoding.
    expect(doc.byteToPosition(byte, "utf-8")).toEqual({ line: 0, character: 13 });
    expect(doc.byteToPosition(byte, "utf-16")).toEqual({ line: 0, character: 10 });
    expect(doc.byteToPosition(byte, "utf-32")).toEqual({ line: 0, character: 9 });

    // Discriminating: utf-16 and utf-8 MUST differ for this non-ASCII case.
    expect(doc.byteToPosition(byte, "utf-8").character).not.toBe(
      doc.byteToPosition(byte, "utf-16").character,
    );

    // Each LSP position round-trips back to the same byte offset.
    expect(doc.positionToByte({ line: 0, character: 13 }, "utf-8")).toBe(13);
    expect(doc.positionToByte({ line: 0, character: 10 }, "utf-16")).toBe(13);
    expect(doc.positionToByte({ line: 0, character: 9 }, "utf-32")).toBe(13);
  });

  it("computes line starts and byte offsets across LF and CRLF with non-ASCII", () => {
    const text = "café\r\n😀x\nlast";
    const doc = new DocumentPositions(text);

    // Line 1 starts at the emoji.
    expect(doc.utf16ToPosition(text.indexOf("😀"), "utf-8")).toEqual({ line: 1, character: 0 });
    // "café" is 5 bytes + CRLF (2) = byte 7.
    expect(doc.utf16ToByte(text.indexOf("😀"))).toBe(7);

    // 'x' sits after the 4-byte emoji on line 1.
    const xUtf16 = text.indexOf("x");
    expect(doc.utf16ToPosition(xUtf16, "utf-8")).toEqual({ line: 1, character: 4 });
    expect(doc.utf16ToPosition(xUtf16, "utf-16")).toEqual({ line: 1, character: 2 });

    // Line 2 ("last") starts at byte 13.
    expect(doc.utf16ToByte(text.indexOf("last"))).toBe(13);
    expect(doc.utf16ToPosition(text.indexOf("last"), "utf-8")).toEqual({ line: 2, character: 0 });
  });

  it("inverts utf16<->byte and clamps mid-character byte offsets to a boundary", () => {
    const text = "é😀";
    const doc = new DocumentPositions(text);
    // é = bytes 0..1, 😀 = bytes 2..5.
    expect(doc.utf16ToByte(0)).toBe(0);
    expect(doc.utf16ToByte(1)).toBe(2);
    expect(doc.utf16ToByte(3)).toBe(6);
    expect(doc.byteToUtf16(0)).toBe(0);
    expect(doc.byteToUtf16(2)).toBe(1);
    expect(doc.byteToUtf16(6)).toBe(3);
    // A byte offset inside the emoji clamps back to the character start.
    expect(doc.byteToUtf16(4)).toBe(1);
  });

  it("exposes one-shot convenience helpers that honour the encoding", () => {
    const text = "ab café 😀 cd";
    expect(byteOffsetToPosition(text, 13, "utf-8")).toEqual({ line: 0, character: 13 });
    expect(byteOffsetToPosition(text, 13, "utf-16")).toEqual({ line: 0, character: 10 });
    expect(positionToByteOffset(text, { line: 0, character: 10 }, "utf-16")).toBe(13);
  });
});

describe("DocumentPositions out-of-range character clamping (LSP 3.17)", () => {
  it("clamps an over-long character to end-of-line instead of crossing the newline", () => {
    // Line 0 content is "ab" (utf-16 offsets 0..2); the '\n' is at offset 2 and
    // line 1 starts at offset 3. Per LSP 3.17 an out-of-range `character` clamps
    // to the line length, excluding the terminator — it must NOT walk into line 1.
    const doc = new DocumentPositions("ab\ncd");
    expect(doc.lineCount).toBe(2);
    expect(doc.positionToUtf16({ line: 0, character: 999 }, "utf-16")).toBe(2);
    // Negative: it must not land at the start of line 1 (offset 3) or beyond.
    expect(doc.positionToUtf16({ line: 0, character: 999 }, "utf-16")).not.toBe(3);
    // And the byte projection clamps to the same end-of-line content boundary.
    expect(doc.positionToByte({ line: 0, character: 999 }, "utf-16")).toBe(2);
    expect(doc.positionToByte({ line: 0, character: 999 }, "utf-16")).not.toBe(3);
  });

  it("excludes a CRLF terminator (both \\r and \\n) from the clamped line end", () => {
    // "ab\r\ncd": line 0 content "ab" ends at utf-16 offset 2; CR=2, LF=3, line 1 at 4.
    const doc = new DocumentPositions("ab\r\ncd");
    expect(doc.positionToUtf16({ line: 0, character: 999 }, "utf-16")).toBe(2);
    expect(doc.positionToUtf16({ line: 0, character: 999 }, "utf-16")).not.toBe(3);
    expect(doc.positionToUtf16({ line: 0, character: 999 }, "utf-16")).not.toBe(4);
  });

  it("clamps an over-long non-ASCII line to end-of-content under each encoding", () => {
    // Line 0 is "café" (4 code points, 5 utf-8 bytes, 4 utf-16 units) then '\n'.
    const doc = new DocumentPositions("café\nx");
    const eol16 = "café".length; // 4
    expect(doc.positionToUtf16({ line: 0, character: 999 }, "utf-8")).toBe(eol16);
    expect(doc.positionToUtf16({ line: 0, character: 999 }, "utf-16")).toBe(eol16);
    expect(doc.positionToUtf16({ line: 0, character: 999 }, "utf-32")).toBe(eol16);
    // Negative: never past end-of-line-0 content into the '\n' (offset 4) or line 1.
    expect(doc.positionToUtf16({ line: 0, character: 999 }, "utf-8")).not.toBe(5);
  });
});
