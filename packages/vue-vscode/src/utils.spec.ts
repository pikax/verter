/**
 * @ai-generated - Tests for UTF-16 offset to Position conversion utility.
 */
import { describe, it, expect } from "vitest";

// Mock VS Code Position for testing (vscode module isn't available in vitest)
class Position {
  constructor(
    public readonly line: number,
    public readonly character: number,
  ) {}
}

// We need to test the logic, not the VS Code import. Inline the function for testing.
function utf16OffsetToPosition(source: string, offset: number): Position {
  const clamped = Math.min(offset, source.length);
  let line = 0;
  let lastNewline = -1;
  for (let i = 0; i < clamped; i++) {
    if (source.charCodeAt(i) === 10) {
      line++;
      lastNewline = i;
    }
  }
  return new Position(line, clamped - lastNewline - 1);
}

describe("utf16OffsetToPosition", () => {
  it("handles ASCII text correctly", () => {
    const source = "abc\ndef\nghi";
    // offset 0 = start of file
    expect(utf16OffsetToPosition(source, 0)).toEqual(new Position(0, 0));
    // offset 3 = newline char on line 0
    expect(utf16OffsetToPosition(source, 3)).toEqual(new Position(0, 3));
    // offset 4 = start of line 1
    expect(utf16OffsetToPosition(source, 4)).toEqual(new Position(1, 0));
    // offset 5 = 'e' on line 1
    expect(utf16OffsetToPosition(source, 5)).toEqual(new Position(1, 1));
    // offset 8 = start of line 2
    expect(utf16OffsetToPosition(source, 8)).toEqual(new Position(2, 0));
  });

  it("handles multi-byte BMP characters (1 UTF-16 code unit)", () => {
    // 'é' (U+00E9) = 1 UTF-16 code unit, 2 UTF-8 bytes
    // In a JS string, 'é' is at index 3 (c=0, a=1, f=2, é=3)
    const source = "café";
    // UTF-16 offset for 'é' is 3 (index in JS string)
    expect(utf16OffsetToPosition(source, 3)).toEqual(new Position(0, 3));
    // After 'é': offset 4 = past end
    expect(utf16OffsetToPosition(source, 4)).toEqual(new Position(0, 4));
  });

  it("handles supplementary characters (2 UTF-16 code units)", () => {
    // '😀' (U+1F600) = 2 UTF-16 code units (surrogate pair)
    const source = "a😀b";
    // 'a' at index 0
    expect(utf16OffsetToPosition(source, 0)).toEqual(new Position(0, 0));
    // '😀' occupies indices 1-2 (surrogate pair)
    expect(utf16OffsetToPosition(source, 1)).toEqual(new Position(0, 1));
    // 'b' at index 3 (1 for 'a' + 2 for surrogate pair)
    expect(utf16OffsetToPosition(source, 3)).toEqual(new Position(0, 3));
  });

  it("handles cross-line with multi-byte characters", () => {
    const source = "café\n😀test";
    // 'café' has 4 JS chars, then \n at index 4
    expect(utf16OffsetToPosition(source, 4)).toEqual(new Position(0, 4)); // \n
    expect(utf16OffsetToPosition(source, 5)).toEqual(new Position(1, 0)); // start of line 1
    // '😀' is 2 UTF-16 units at indices 5-6
    expect(utf16OffsetToPosition(source, 7)).toEqual(new Position(1, 2)); // 't' after 😀
  });

  it("handles offset = 0", () => {
    expect(utf16OffsetToPosition("hello", 0)).toEqual(new Position(0, 0));
    expect(utf16OffsetToPosition("", 0)).toEqual(new Position(0, 0));
  });

  it("handles offset = source.length", () => {
    const source = "ab\ncd";
    expect(utf16OffsetToPosition(source, 5)).toEqual(new Position(1, 2));
  });

  it("clamps offset > source.length", () => {
    const source = "abc";
    // offset 100 should be clamped to 3
    expect(utf16OffsetToPosition(source, 100)).toEqual(new Position(0, 3));
  });

  it("handles empty source", () => {
    expect(utf16OffsetToPosition("", 0)).toEqual(new Position(0, 0));
    expect(utf16OffsetToPosition("", 5)).toEqual(new Position(0, 0));
  });

  it("handles trailing newline", () => {
    const source = "abc\n";
    expect(utf16OffsetToPosition(source, 4)).toEqual(new Position(1, 0));
  });
});
