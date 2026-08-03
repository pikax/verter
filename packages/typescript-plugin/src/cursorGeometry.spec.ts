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
