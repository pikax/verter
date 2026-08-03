import { describe, expect, it } from "vitest";
import { isFrameworkAttributeNamePosition } from "./cursorGeometry";

describe("isFrameworkAttributeNamePosition (parser-supplied opening spans)", () => {
  it("classifies inside a parser-identified opening tag with a decoy '<' in an attribute string", () => {
    // `title="a<b"` contains a decoy `<`. A raw lastIndexOf("<") self-scan
    // anchors on the decoy and mis-tracks quotes; the parser-supplied opening
    // span anchors on the real tag start, and the bounded lexer inside it
    // tracks the quoted value correctly.
    const source = '<div title="a<b" cl';
    expect(isFrameworkAttributeNamePosition(source, source.length, [[0, source.length]])).toBe(
      true,
    );
  });

  it("fails closed without a parser-identified opening span", () => {
    // No stamped opening span covering the position → no classification,
    // never a raw-source `<` rediscovery.
    const source = "<div cl";
    expect(isFrameworkAttributeNamePosition(source, source.length, [])).toBe(false);
    expect(isFrameworkAttributeNamePosition(source, source.length, undefined)).toBe(false);
  });

  it("stays inside the 256-byte lookback bound within the span", () => {
    const filler = "data-x".padEnd(300, "y");
    const source = `<div ${filler} cl`;
    // The tag start is beyond 256 bytes of lookback: the bounded lexer cannot
    // reach the parser-identified tag start, so classification fails closed.
    expect(isFrameworkAttributeNamePosition(source, source.length, [[0, source.length]])).toBe(
      false,
    );
  });

  it("converts UTF-8 byte spans before comparing against UTF-16 positions (R2-C-02)", () => {
    // The stamped opening ranges are UTF-8 BYTE offsets from the Rust
    // producer; tsserver positions are UTF-16 code units. Four astral chars
    // before the tag: 16 bytes but only 8 UTF-16 units — an unconverted
    // compare places the tag start beyond the caret and fails closed on a
    // genuine attribute-name position.
    const source = "\u{1F600}\u{1F600}\u{1F600}\u{1F600}<div cl";
    // Byte span of `<div cl`: [16, 23]. UTF-16 span: [8, 15].
    expect(isFrameworkAttributeNamePosition(source, source.length, [[16, 23]])).toBe(true);
  });

  it("does not misclassify text content owned by a byte-shifted span (R2-C-02)", () => {
    // `<b>` byte span is [8, 11]; its UTF-16 span is [4, 7]. Position 9 sits
    // in the TEXT content after `hi` — an unconverted compare puts it inside
    // the shifted tag span and fabricates an attribute-name classification.
    const source = "\u{1F600}\u{1F600}<b>hi";
    expect(isFrameworkAttributeNamePosition(source, 9, [[8, 11]])).toBe(false);
  });

  it("stays correct across CRLF lines with astral content (R2-C-02)", () => {
    // "💚\r\n<div cl": CRLF is width-1-per-unit in both encodings; the astral
    // heart shifts bytes by 2. Byte span of `<div cl` is [6, 13]; UTF-16
    // span is [4, 11].
    const source = "\u{1F49A}\r\n<div cl";
    expect(isFrameworkAttributeNamePosition(source, source.length, [[6, 13]])).toBe(true);
  });

  it("rejects positions inside closing or declaration tags", () => {
    const source = "</div cl";
    expect(isFrameworkAttributeNamePosition(source, source.length, [[0, source.length]])).toBe(
      false,
    );
  });

  it("rejects positions inside an unbalanced brace or quote", () => {
    const source = '<Widget prop={count < 3 ? "a" : "b"';
    expect(isFrameworkAttributeNamePosition(source, source.length, [[0, source.length]])).toBe(
      false,
    );
  });
});
